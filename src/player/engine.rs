use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::humanizer::HumanizerEngine;
use crate::core::config::AppConfig;
use crate::core::song::{NoteEvent, Song};
use crate::input::mapping::KeyMappingEngine;
use crate::input::simulator::InputSimulator;
use crate::synth::engine::PianoSynthEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaybackStatus {
    pub state: PlayerState,
    pub current_time_ms: f64,
    pub total_duration_ms: f64,
    pub formatted_current: String,
    pub formatted_total: String,
    pub progress: f64,
    pub speed: f64,
    pub transpose: i8,
    pub bpm: f64,
    pub active_notes: Vec<u8>,
    pub song_title: String,
    /// True when playback finished naturally (song reached end), false on manual stop
    pub finished_naturally: bool,
}

pub struct PlayerEngine {
    current_song: Arc<Mutex<Option<Song>>>,
    state: Arc<Mutex<PlayerState>>,
    playback_speed: Arc<Mutex<f64>>,
    transpose_offset: Arc<Mutex<i8>>,
    seek_request_ms: Arc<Mutex<Option<f64>>>,
    should_stop: Arc<AtomicBool>,
    /// Generation counter to prevent concurrent playback loops
    playback_generation: Arc<AtomicU64>,
    status_sender: broadcast::Sender<PlaybackStatus>,
    synth: Arc<Mutex<PianoSynthEngine>>,
    simulator: Arc<InputSimulator>,
    mapping: Arc<Mutex<KeyMappingEngine>>,
    humanizer: Arc<Mutex<HumanizerEngine>>,
    config: Arc<Mutex<AppConfig>>,
}

impl PlayerEngine {
    pub fn new(
        synth: Arc<Mutex<PianoSynthEngine>>,
        config: Arc<Mutex<AppConfig>>,
        status_sender: broadcast::Sender<PlaybackStatus>,
    ) -> Self {
        let cfg = config.lock().clone();
        let mapping = Arc::new(Mutex::new(KeyMappingEngine::new(cfg.keyboard_layout)));
        let simulator = Arc::new(InputSimulator::new());
        let humanizer = Arc::new(Mutex::new(HumanizerEngine::new(cfg.humanize)));

        Self {
            current_song: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(PlayerState::Stopped)),
            playback_speed: Arc::new(Mutex::new(cfg.playback_speed)),
            transpose_offset: Arc::new(Mutex::new(cfg.transpose_offset)),
            seek_request_ms: Arc::new(Mutex::new(None)),
            should_stop: Arc::new(AtomicBool::new(false)),
            playback_generation: Arc::new(AtomicU64::new(0)),
            status_sender,
            synth,
            simulator,
            mapping,
            humanizer,
            config,
        }
    }

    pub fn load_song(&self, song: Song) {
        self.stop();
        *self.current_song.lock() = Some(song);
        self.broadcast_status(0.0, &[]);
    }

    pub fn play(&self) {
        let current_state = *self.state.lock();
        if current_state == PlayerState::Playing {
            return;
        }

        if current_state == PlayerState::Paused {
            *self.state.lock() = PlayerState::Playing;
            return;
        }

        let song_opt = self.current_song.lock().clone();
        let Some(song) = song_opt else {
            warn!("No song loaded to play");
            return;
        };

        // Window auto-focus if enabled
        let cfg = self.config.lock().clone();
        if cfg.auto_focus_window && cfg.macro_enabled {
            InputSimulator::auto_focus_window(&cfg.target_window_title);
            std::thread::sleep(Duration::from_millis(150));
        }

        *self.state.lock() = PlayerState::Playing;
        self.should_stop.store(false, Ordering::SeqCst);

        // Increment generation — any older playback loop will detect its generation is stale and exit
        let my_generation = self.playback_generation.fetch_add(1, Ordering::SeqCst) + 1;

        let song_arc = Arc::new(song);
        let state_arc = Arc::clone(&self.state);
        let speed_arc = Arc::clone(&self.playback_speed);
        let transpose_arc = Arc::clone(&self.transpose_offset);
        let seek_arc = Arc::clone(&self.seek_request_ms);
        let stop_flag = Arc::clone(&self.should_stop);
        let gen_arc = Arc::clone(&self.playback_generation);
        let synth_arc = Arc::clone(&self.synth);
        let sim_arc = Arc::clone(&self.simulator);
        let map_arc = Arc::clone(&self.mapping);
        let hum_arc = Arc::clone(&self.humanizer);
        let config_arc = Arc::clone(&self.config);
        let sender = self.status_sender.clone();

        tokio::spawn(async move {
            info!("Playback loop started for '{}' (gen={})", song_arc.title, my_generation);

            let all_notes = song_arc.all_notes_flattened();
            let total_duration = song_arc.duration_ms;

            let mut note_cursor = 0;
            let mut current_time_ms = 0.0;
            let mut last_tick = Instant::now();
            let mut active_visual_notes: Vec<u8> = Vec::new();
            let mut last_status_emit = Instant::now();

            while !stop_flag.load(Ordering::Relaxed) {
                // Check generation — if a newer play() was called, this loop should exit
                if gen_arc.load(Ordering::Relaxed) != my_generation {
                    info!("Playback loop gen={} superseded, exiting", my_generation);
                    return;
                }

                // Handle Seek
                if let Some(target_ms) = seek_arc.lock().take() {
                    current_time_ms = target_ms.clamp(0.0, total_duration);
                    // Find new note cursor
                    note_cursor = all_notes
                        .iter()
                        .position(|n| n.start_ms >= current_time_ms)
                        .unwrap_or(all_notes.len());
                    synth_arc.lock().all_notes_off();
                    sim_arc.release_all();
                    active_visual_notes.clear();
                    last_tick = Instant::now();
                }

                let current_state = *state_arc.lock();
                if current_state == PlayerState::Stopped {
                    break;
                }

                if current_state == PlayerState::Paused {
                    synth_arc.lock().all_notes_off();
                    sim_arc.release_all();
                    active_visual_notes.clear();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    last_tick = Instant::now();
                    continue;
                }

                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f64() * 1000.0;
                last_tick = now;

                let speed = *speed_arc.lock();
                current_time_ms += dt * speed;

                if current_time_ms > total_duration + 500.0 {
                    // Song finished naturally
                    let loop_enabled = config_arc.lock().loop_song;
                    if loop_enabled {
                        current_time_ms = 0.0;
                        note_cursor = 0;
                        active_visual_notes.clear();
                        synth_arc.lock().all_notes_off();
                        sim_arc.release_all();
                        continue;
                    } else {
                        *state_arc.lock() = PlayerState::Stopped;
                        synth_arc.lock().all_notes_off();
                        sim_arc.release_all();
                        active_visual_notes.clear();
                        // Send a final status with finished_naturally = true
                        let status = PlaybackStatus {
                            state: PlayerState::Stopped,
                            current_time_ms: total_duration,
                            total_duration_ms: total_duration,
                            formatted_current: format_duration(total_duration),
                            formatted_total: format_duration(total_duration),
                            progress: 1.0,
                            speed,
                            transpose: *transpose_arc.lock(),
                            bpm: song_arc.bpm,
                            active_notes: vec![],
                            song_title: song_arc.title.clone(),
                            finished_naturally: true,
                        };
                        let _ = sender.send(status);
                        break;
                    }
                }

                let transpose = *transpose_arc.lock();
                let cfg = config_arc.lock().clone();

                // Process all notes whose start_time has arrived
                let mut chord_batch: Vec<NoteEvent> = Vec::new();
                while note_cursor < all_notes.len() && all_notes[note_cursor].start_ms <= current_time_ms {
                    chord_batch.push(all_notes[note_cursor].clone());
                    note_cursor += 1;
                }

                if !chord_batch.is_empty() {
                    // Humanize chord notes
                    let humanized = hum_arc.lock().process_chord_notes(&chord_batch);
                    
                    let mut macro_keys = Vec::new();

                    for note_event in humanized {
                        let final_note = ((note_event.note as i16) + (transpose as i16)).clamp(21, 108) as u8;

                        // 1. Audio synthesis
                        if cfg.synth.enabled {
                            synth_arc.lock().note_on(final_note, note_event.velocity);
                        }

                        // 2. Visualizer key active state
                        if !active_visual_notes.contains(&final_note) {
                            active_visual_notes.push(final_note);
                        }

                        // 3. Macro keyboard simulation collection
                        if cfg.macro_enabled {
                            if cfg.velocity {
                                let vel_char = map_arc.lock().get_velocity_key(note_event.velocity);
                                sim_arc.send_velocity(vel_char);
                            }

                            if let Some(key_map) = map_arc.lock().get_piano_key(final_note, cfg.allow_88_keys) {
                                macro_keys.push((key_map.key_char, key_map.is_shift, key_map.is_ctrl));
                            }
                        }
                    }
                    
                    // 4. Send the chord to the OS macro simulation in a single atomic batch
                    if !macro_keys.is_empty() {
                        let sim = Arc::clone(&sim_arc);
                        tokio::task::spawn_blocking(move || {
                            sim.tap_chord(macro_keys, 10);
                        });
                    }
                }

                // Clean visualizer notes that finished duration
                active_visual_notes.retain(|&n| {
                    all_notes.iter().any(|note| {
                        let final_n = ((note.note as i16) + (transpose as i16)).clamp(21, 108) as u8;
                        final_n == n
                            && note.start_ms <= current_time_ms
                            && (note.start_ms + note.duration_ms.min(400.0)) >= current_time_ms
                    })
                });

                // Periodic status broadcast (~30 fps)
                if last_status_emit.elapsed() >= Duration::from_millis(33) {
                    let formatted_curr = format_duration(current_time_ms);
                    let formatted_tot = format_duration(total_duration);
                    let progress = (current_time_ms / total_duration.max(1.0)).clamp(0.0, 1.0);

                    let status = PlaybackStatus {
                        state: *state_arc.lock(),
                        current_time_ms,
                        total_duration_ms: total_duration,
                        formatted_current: formatted_curr,
                        formatted_total: formatted_tot,
                        progress,
                        speed,
                        transpose,
                        bpm: song_arc.bpm,
                        active_notes: active_visual_notes.clone(),
                        song_title: song_arc.title.clone(),
                        finished_naturally: false,
                    };

                    let _ = sender.send(status);
                    last_status_emit = Instant::now();
                }

                tokio::time::sleep(Duration::from_millis(4)).await;
            }

            info!("Playback loop ended (gen={})", my_generation);
        });
    }

    pub fn pause(&self) {
        *self.state.lock() = PlayerState::Paused;
        self.synth.lock().all_notes_off();
        self.simulator.release_all();
    }

    pub fn stop(&self) {
        *self.state.lock() = PlayerState::Stopped;
        self.should_stop.store(true, Ordering::SeqCst);
        self.synth.lock().all_notes_off();
        self.simulator.release_all();
        self.broadcast_status(0.0, &[]);
    }

    pub fn seek(&self, time_ms: f64) {
        *self.seek_request_ms.lock() = Some(time_ms);
    }

    pub fn set_speed(&self, speed: f64) {
        let clamped = speed.clamp(0.1, 3.0);
        *self.playback_speed.lock() = clamped;
        self.config.lock().playback_speed = clamped;
    }

    pub fn get_speed(&self) -> f64 {
        *self.playback_speed.lock()
    }

    pub fn adjust_speed(&self, delta: f64) {
        let cur = self.get_speed();
        self.set_speed(cur + delta);
    }

    pub fn set_transpose(&self, transpose: i8) {
        let clamped = transpose.clamp(-24, 24);
        *self.transpose_offset.lock() = clamped;
        self.config.lock().transpose_offset = clamped;
    }

    pub fn get_transpose(&self) -> i8 {
        *self.transpose_offset.lock()
    }

    pub fn adjust_transpose(&self, delta: i8) {
        let cur = self.get_transpose();
        self.set_transpose(cur + delta);
    }

    pub fn state(&self) -> PlayerState {
        *self.state.lock()
    }

    fn broadcast_status(&self, current_time_ms: f64, active_notes: &[u8]) {
        let song_guard = self.current_song.lock();
        let (title, total_ms, bpm) = if let Some(s) = song_guard.as_ref() {
            (s.title.clone(), s.duration_ms, s.bpm)
        } else {
            ("No song loaded".to_string(), 0.0, 120.0)
        };

        let formatted_curr = format_duration(current_time_ms);
        let formatted_tot = format_duration(total_ms);
        let progress = if total_ms > 0.0 {
            (current_time_ms / total_ms).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let status = PlaybackStatus {
            state: *self.state.lock(),
            current_time_ms,
            total_duration_ms: total_ms,
            formatted_current: formatted_curr,
            formatted_total: formatted_tot,
            progress,
            speed: *self.playback_speed.lock(),
            transpose: *self.transpose_offset.lock(),
            bpm,
            active_notes: active_notes.to_vec(),
            song_title: title,
            finished_naturally: false,
        };

        let _ = self.status_sender.send(status);
    }

    pub fn synth(&self) -> Arc<Mutex<PianoSynthEngine>> {
        Arc::clone(&self.synth)
    }

    pub fn update_config(&self, cfg: AppConfig) {
        *self.config.lock() = cfg.clone();
        *self.humanizer.lock() = HumanizerEngine::new(cfg.humanize);
        *self.mapping.lock() = KeyMappingEngine::new(cfg.keyboard_layout);

        let mut synth = self.synth.lock();
        synth.enabled = cfg.synth.enabled;
        synth.volume = cfg.synth.volume;
        synth.set_reverb_params(cfg.synth.reverb_mix, cfg.synth.reverb_room_size);
    }
}

fn format_duration(ms: f64) -> String {
    let total_seconds = (ms / 1000.0).round() as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}
