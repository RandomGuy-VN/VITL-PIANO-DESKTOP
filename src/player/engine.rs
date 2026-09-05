use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{info, warn};

use rdev::Key;

use super::humanizer::HumanizerEngine;
use crate::core::config::AppConfig;
use crate::core::song::{NoteEvent, Song};
use crate::input::mapping::KeyMappingEngine;
use crate::input::simulator::InputSimulator;
use crate::synth::engine::PianoSynthEngine;

#[derive(Debug, Clone)]
struct ActiveNoteInFlight {
    final_note: u8,
    channel: u8,
    end_time_ms: f64,
    macro_key: Option<(Key, bool, bool)>,
}

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
    last_playback_time_ms: Arc<Mutex<f64>>,
    should_stop: Arc<AtomicBool>,
    is_loop_running: Arc<AtomicBool>,
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
            last_playback_time_ms: Arc::new(Mutex::new(0.0)),
            should_stop: Arc::new(AtomicBool::new(false)),
            is_loop_running: Arc::new(AtomicBool::new(false)),
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
        if current_state == PlayerState::Playing && self.is_loop_running.load(Ordering::SeqCst) {
            return;
        }

        if current_state == PlayerState::Paused && self.is_loop_running.load(Ordering::SeqCst) {
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
        self.is_loop_running.store(true, Ordering::SeqCst);

        // Increment generation — any older playback loop will detect its generation is stale and exit
        let my_generation = self.playback_generation.fetch_add(1, Ordering::SeqCst) + 1;

        let song_arc = Arc::new(song);
        let state_arc = Arc::clone(&self.state);
        let speed_arc = Arc::clone(&self.playback_speed);
        let transpose_arc = Arc::clone(&self.transpose_offset);
        let seek_arc = Arc::clone(&self.seek_request_ms);
        let stop_flag = Arc::clone(&self.should_stop);
        let is_running_flag = Arc::clone(&self.is_loop_running);
        let gen_arc = Arc::clone(&self.playback_generation);
        let synth_arc = Arc::clone(&self.synth);
        let sim_arc = Arc::clone(&self.simulator);
        let map_arc = Arc::clone(&self.mapping);
        let hum_arc = Arc::clone(&self.humanizer);
        let config_arc = Arc::clone(&self.config);
        let last_time_arc = Arc::clone(&self.last_playback_time_ms);
        let sender = self.status_sender.clone();

        tokio::spawn(async move {
            info!(
                "Playback loop started for '{}' (gen={})",
                song_arc.title, my_generation
            );

            struct LoopGuard(Arc<AtomicBool>, Arc<AtomicU64>, u64);
            impl Drop for LoopGuard {
                fn drop(&mut self) {
                    if self.1.load(Ordering::Relaxed) == self.2 {
                        self.0.store(false, Ordering::SeqCst);
                    }
                }
            }
            let _guard = LoopGuard(
                Arc::clone(&is_running_flag),
                Arc::clone(&gen_arc),
                my_generation,
            );

            let all_notes = song_arc.all_notes_flattened();
            let total_duration = song_arc.duration_ms;

            let is_black_midi = song_arc.is_black_midi()
                || (config_arc.lock().black_midi.enabled && song_arc.total_notes > 10_000);
            if is_black_midi {
                info!(
                    "Black MIDI mode active for '{}': total_notes={}, density={:.1} notes/sec",
                    song_arc.title,
                    song_arc.total_notes,
                    song_arc.note_density()
                );
            }

            let mut note_cursor = 0;
            let mut current_time_ms = 0.0;
            let mut anchor_time = Instant::now();
            let mut anchor_offset_ms = 0.0;
            let mut prev_speed = *speed_arc.lock();
            let mut active_visual_notes: Vec<u8> = Vec::new();
            let mut active_notes_in_flight: Vec<ActiveNoteInFlight> =
                Vec::with_capacity(if is_black_midi { 2048 } else { 256 });
            let mut active_shift_count: usize = 0;
            let mut active_ctrl_count: usize = 0;
            let mut pitch_ref_counts = [0u16; 128];
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
                    *last_time_arc.lock() = current_time_ms;
                    anchor_offset_ms = current_time_ms;
                    anchor_time = Instant::now();
                    // Find new note cursor
                    note_cursor = all_notes
                        .iter()
                        .position(|n| n.start_ms >= current_time_ms)
                        .unwrap_or(all_notes.len());
                    synth_arc.lock().all_notes_off();
                    sim_arc.release_all();
                    active_visual_notes.clear();
                    active_notes_in_flight.clear();
                    active_shift_count = 0;
                    active_ctrl_count = 0;
                    pitch_ref_counts.fill(0);
                }

                let current_state = *state_arc.lock();
                if current_state == PlayerState::Stopped {
                    break;
                }

                if current_state == PlayerState::Paused {
                    synth_arc.lock().all_notes_off();
                    sim_arc.release_all();
                    active_visual_notes.clear();
                    active_notes_in_flight.clear();
                    active_shift_count = 0;
                    active_ctrl_count = 0;
                    pitch_ref_counts.fill(0);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    anchor_time = Instant::now();
                    anchor_offset_ms = current_time_ms;
                    continue;
                }

                let speed = *speed_arc.lock();
                if (speed - prev_speed).abs() > 0.0001 {
                    anchor_offset_ms = current_time_ms;
                    anchor_time = Instant::now();
                    prev_speed = speed;
                }

                current_time_ms =
                    anchor_offset_ms + anchor_time.elapsed().as_secs_f64() * 1000.0 * speed;
                *last_time_arc.lock() = current_time_ms;

                if current_time_ms > total_duration + 500.0 {
                    // Song finished naturally
                    let loop_enabled = config_arc.lock().loop_song;
                    if loop_enabled {
                        current_time_ms = 0.0;
                        *last_time_arc.lock() = 0.0;
                        anchor_offset_ms = 0.0;
                        anchor_time = Instant::now();
                        note_cursor = 0;
                        active_visual_notes.clear();
                        active_notes_in_flight.clear();
                        active_shift_count = 0;
                        active_ctrl_count = 0;
                        pitch_ref_counts.fill(0);
                        synth_arc.lock().all_notes_off();
                        sim_arc.release_all();
                        continue;
                    } else {
                        *state_arc.lock() = PlayerState::Stopped;
                        *last_time_arc.lock() = 0.0;
                        synth_arc.lock().all_notes_off();
                        sim_arc.release_all();
                        active_visual_notes.clear();
                        active_notes_in_flight.clear();
                        pitch_ref_counts.fill(0);
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
                            bpm: song_arc.get_bpm_at(total_duration) * speed,
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
                while note_cursor < all_notes.len()
                    && all_notes[note_cursor].start_ms <= current_time_ms
                {
                    chord_batch.push(all_notes[note_cursor].clone());
                    note_cursor += 1;
                }

                if !chord_batch.is_empty() {
                    // Black MIDI Optimization: Deduplicate unisons and cull inaudible voices
                    if is_black_midi && chord_batch.len() > 1 {
                        // Keep the highest velocity note for duplicate unisons
                        chord_batch.sort_unstable_by(|a, b| {
                            a.note
                                .cmp(&b.note)
                                .then_with(|| a.channel.cmp(&b.channel))
                                .then_with(|| b.velocity.cmp(&a.velocity))
                        });
                        chord_batch.dedup_by(|a, b| a.note == b.note && a.channel == b.channel);

                        // Cull low velocity ghost notes when chord is dense
                        let min_vel = cfg.black_midi.low_velocity_cull;
                        if min_vel > 0 && chord_batch.len() > 16 {
                            chord_batch.retain(|n| n.velocity >= min_vel);
                        }

                        // Cap simultaneous voices to safe limit (voice stealing)
                        let voice_limit = cfg.black_midi.voice_limit.clamp(32, 256);
                        if chord_batch.len() > voice_limit {
                            chord_batch.sort_unstable_by(|a, b| b.velocity.cmp(&a.velocity));
                            chord_batch.truncate(voice_limit);
                        }
                    }

                    // Humanizer: bypass on dense Black MIDI chords (>24 notes) to prevent latency spikes
                    let notes_to_play = if is_black_midi && chord_batch.len() > 24 {
                        chord_batch
                    } else {
                        let mut hum = hum_arc.lock();
                        if hum.config.chord_delay_ms == 0.0
                            && hum.config.jitter_ms == 0.0
                            && hum.config.mistake_rate == 0.0
                        {
                            chord_batch
                        } else {
                            hum.process_chord_notes(&chord_batch)
                        }
                    };

                    let mut macro_tap_keys = Vec::new();
                    let mut batch_macro_keys_seen = [false; 256];
                    let mut synth_batch = Vec::new();
                    let map_guard = if cfg.macro_enabled {
                        Some(map_arc.lock())
                    } else {
                        None
                    };

                    for note_event in notes_to_play {
                        let final_note =
                            ((note_event.note as i16) + (transpose as i16)).clamp(21, 108) as u8;
                        let note_dur = if cfg.note_lengths {
                            let min_len =
                                cfg.min_note_length_ms.min(cfg.max_note_length_ms).max(10.0);
                            let max_len =
                                cfg.min_note_length_ms.max(cfg.max_note_length_ms).max(10.0);
                            note_event.duration_ms.clamp(min_len, max_len)
                        } else {
                            cfg.min_note_length_ms.max(10.0)
                        };
                        let end_time_ms = current_time_ms + note_dur;

                        // Calculate dynamic or fixed velocity
                        let final_velocity = if !cfg.velocity {
                            cfg.fixed_velocity.clamp(1, 127)
                        } else {
                            let min_v = (cfg.min_velocity as i16)
                                .min(cfg.max_velocity as i16)
                                .clamp(1, 127);
                            let max_v = (cfg.min_velocity as i16)
                                .max(cfg.max_velocity as i16)
                                .clamp(1, 127);
                            let mult = if cfg.velocity_multiplier.is_nan() {
                                1.0
                            } else {
                                cfg.velocity_multiplier.max(0.01)
                            };
                            let scaled = (note_event.velocity as f64) * mult;
                            (scaled.round() as i16).clamp(min_v, max_v) as u8
                        };

                        // 1. Audio synthesis note on queue
                        if cfg.synth.enabled {
                            synth_batch.push((
                                note_event.channel as i32,
                                final_note,
                                final_velocity,
                            ));
                        }

                        // 2. Visualizer key active state (O(1) ref count)
                        pitch_ref_counts[final_note as usize] =
                            pitch_ref_counts[final_note as usize].saturating_add(1);

                        // 3. Macro keyboard simulation collection
                        let mut macro_info = None;
                        if let Some(ref map) = map_guard {
                            if let Some(key_map) = map.get_piano_key(final_note, cfg.allow_88_keys) {
                                let key_idx = key_map.key_char as usize;
                                // In Black MIDI, only trigger each key once per batch
                                if key_idx < 256 && !batch_macro_keys_seen[key_idx] {
                                    batch_macro_keys_seen[key_idx] = true;
                                    if let Some(rk) =
                                        InputSimulator::char_to_rdev_key(key_map.key_char)
                                    {
                                        if cfg.note_lengths {
                                            // If key is already held down by a prior note, pulse key_up first to re-trigger
                                            if sim_arc.is_key_held(rk) {
                                                sim_arc.key_up(rk);
                                            }
                                            if key_map.is_ctrl {
                                                sim_arc.key_down(Key::ControlLeft);
                                                active_ctrl_count += 1;
                                            }
                                            if key_map.is_shift {
                                                sim_arc.key_down(Key::ShiftLeft);
                                                active_shift_count += 1;
                                            }
                                            sim_arc.key_down(rk);
                                            if key_map.is_shift {
                                                sim_arc.key_up(Key::ShiftLeft);
                                            }
                                            if key_map.is_ctrl {
                                                sim_arc.key_up(Key::ControlLeft);
                                            }
                                            macro_info =
                                                Some((rk, key_map.is_shift, key_map.is_ctrl));
                                        } else {
                                            macro_tap_keys.push((
                                                key_map.key_char,
                                                key_map.is_shift,
                                                key_map.is_ctrl,
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        active_notes_in_flight.push(ActiveNoteInFlight {
                            final_note,
                            channel: note_event.channel,
                            end_time_ms,
                            macro_key: macro_info,
                        });
                    }

                    // Release map lock explicitly
                    drop(map_guard);

                    // Batch dispatch audio synthesis notes with single lock acquisition
                    if cfg.synth.enabled && !synth_batch.is_empty() {
                        let mut synth = synth_arc.lock();
                        for (channel, note, velocity) in synth_batch {
                            synth.note_on_channel(channel, note, velocity);
                        }
                    }

                    // 4. Send the chord to OS macro simulation instantaneously if in tap mode
                    if !macro_tap_keys.is_empty() {
                        let sim = Arc::clone(&sim_arc);
                        // In Black MIDI, limit tap chord size to prevent OS input flood
                        if is_black_midi && macro_tap_keys.len() > 32 {
                            macro_tap_keys.truncate(32);
                        }
                        tokio::task::spawn_blocking(move || {
                            sim.tap_chord(macro_tap_keys, 0);
                        });
                    }
                }

                // Clean expired active notes for realistic physical key release and synthesizer damper drop
                let mut expired_notes = Vec::new();
                let mut released_shift = false;
                let mut released_ctrl = false;

                active_notes_in_flight.retain(|item| {
                    if current_time_ms >= item.end_time_ms {
                        expired_notes.push((item.channel, item.final_note));
                        if let Some((rk, is_shift, is_ctrl)) = item.macro_key {
                            sim_arc.key_up(rk);
                            if is_shift && active_shift_count > 0 {
                                active_shift_count -= 1;
                                if active_shift_count == 0 {
                                    released_shift = true;
                                }
                            }
                            if is_ctrl && active_ctrl_count > 0 {
                                active_ctrl_count -= 1;
                                if active_ctrl_count == 0 {
                                    released_ctrl = true;
                                }
                            }
                        }
                        if pitch_ref_counts[item.final_note as usize] > 0 {
                            pitch_ref_counts[item.final_note as usize] -= 1;
                        }
                        false
                    } else {
                        true
                    }
                });

                if released_shift && active_shift_count == 0 {
                    sim_arc.key_up(Key::ShiftLeft);
                }
                if released_ctrl && active_ctrl_count == 0 {
                    sim_arc.key_up(Key::ControlLeft);
                }

                if cfg.synth.enabled && !expired_notes.is_empty() {
                    let mut synth = synth_arc.lock();
                    for (ch, n) in expired_notes {
                        synth.note_off_channel(ch as i32, n);
                    }
                }

                // Sync visualizer active notes directly with active sounding notes in O(88) time
                active_visual_notes.clear();
                for pitch in 21..=108 {
                    if pitch_ref_counts[pitch] > 0 {
                        active_visual_notes.push(pitch as u8);
                    }
                }

                // Periodic status broadcast (~30 fps)
                if last_status_emit.elapsed() >= Duration::from_millis(33) {
                    let formatted_curr = format_duration(current_time_ms);
                    let formatted_tot = format_duration(total_duration);
                    let progress = (current_time_ms / total_duration.max(1.0)).clamp(0.0, 1.0);
                    let dynamic_bpm = song_arc.get_bpm_at(current_time_ms) * speed;

                    let status = PlaybackStatus {
                        state: *state_arc.lock(),
                        current_time_ms,
                        total_duration_ms: total_duration,
                        formatted_current: formatted_curr,
                        formatted_total: formatted_tot,
                        progress,
                        speed,
                        transpose,
                        bpm: dynamic_bpm,
                        active_notes: active_visual_notes.clone(),
                        song_title: song_arc.title.clone(),
                        finished_naturally: false,
                    };

                    let _ = sender.send(status);
                    last_status_emit = Instant::now();
                }

                tokio::time::sleep(Duration::from_millis(1)).await;
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
        *self.last_playback_time_ms.lock() = 0.0;
        self.should_stop.store(true, Ordering::SeqCst);
        self.is_loop_running.store(false, Ordering::SeqCst);
        self.synth.lock().all_notes_off();
        self.simulator.release_all();
        self.broadcast_status(0.0, &[]);
    }

    pub fn seek(&self, time_ms: f64) {
        *self.last_playback_time_ms.lock() = time_ms;
        *self.seek_request_ms.lock() = Some(time_ms);
        self.broadcast_status(time_ms, &[]);
    }

    pub fn set_speed(&self, speed: f64) {
        let clamped = speed.clamp(0.1, 3.0);
        *self.playback_speed.lock() = clamped;
        self.config.lock().playback_speed = clamped;
        let cur_time = *self.last_playback_time_ms.lock();
        self.broadcast_status(cur_time, &[]);
    }

    pub fn set_bpm(&self, target_bpm: f64) {
        if !target_bpm.is_finite() || target_bpm <= 0.0 {
            return;
        }
        let cur_time = *self.last_playback_time_ms.lock();
        let intrinsic_bpm = if let Some(s) = self.current_song.lock().as_ref() {
            s.get_bpm_at(cur_time).max(1.0)
        } else {
            120.0
        };
        let new_speed = (target_bpm / intrinsic_bpm).clamp(0.1, 3.0);
        self.set_speed(new_speed);
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
        let cur_time = *self.last_playback_time_ms.lock();
        self.broadcast_status(cur_time, &[]);
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

    pub fn get_status(&self) -> PlaybackStatus {
        let song_guard = self.current_song.lock();
        let speed = *self.playback_speed.lock();
        let cur_time = *self.last_playback_time_ms.lock();
        let (title, total_ms, bpm) = if let Some(s) = song_guard.as_ref() {
            (s.title.clone(), s.duration_ms, s.get_bpm_at(cur_time) * speed)
        } else {
            ("No song loaded".to_string(), 0.0, 120.0 * speed)
        };

        let formatted_curr = format_duration(cur_time);
        let formatted_tot = format_duration(total_ms);
        let progress = if total_ms > 0.0 {
            (cur_time / total_ms).clamp(0.0, 1.0)
        } else {
            0.0
        };

        PlaybackStatus {
            state: *self.state.lock(),
            current_time_ms: cur_time,
            total_duration_ms: total_ms,
            formatted_current: formatted_curr,
            formatted_total: formatted_tot,
            progress,
            speed,
            transpose: *self.transpose_offset.lock(),
            bpm,
            active_notes: vec![],
            song_title: title,
            finished_naturally: false,
        }
    }

    fn broadcast_status(&self, current_time_ms: f64, active_notes: &[u8]) {
        let song_guard = self.current_song.lock();
        let speed = *self.playback_speed.lock();
        let (title, total_ms, bpm) = if let Some(s) = song_guard.as_ref() {
            (
                s.title.clone(),
                s.duration_ms,
                s.get_bpm_at(current_time_ms) * speed,
            )
        } else {
            ("No song loaded".to_string(), 0.0, 120.0 * speed)
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
            speed,
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
