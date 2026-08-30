use super::dsp::{soft_limit, Reverb, StereoDelay, ThreeBandEqualizer};
use std::f32::consts::PI;

const MAX_POLYPHONY: usize = 128;
const MAX_HARMONICS: usize = 12;

#[derive(Debug, Clone)]
struct Harmonic {
    freq: f32,
    amplitude: f32,
    decay_rate: f32,
    phase: f32,
}

#[derive(Debug, Clone)]
struct Voice {
    note: u8,
    velocity: u8,
    is_active: bool,
    is_releasing: bool,
    sustained: bool,
    time_active: f32,
    release_time: f32,
    pan_left: f32,
    pan_right: f32,
    harmonics: Vec<Harmonic>,
    hammer_noise_state: u32,
    hammer_noise_previous: f32,
    hammer_noise_amplitude: f32,
    smoothed_energy: f32,
}

impl Voice {
    fn new_inactive() -> Self {
        Self {
            note: 0,
            velocity: 0,
            is_active: false,
            is_releasing: false,
            sustained: false,
            time_active: 0.0,
            release_time: 0.0,
            pan_left: 0.5,
            pan_right: 0.5,
            harmonics: Vec::new(),
            hammer_noise_state: 1,
            hammer_noise_previous: 0.0,
            hammer_noise_amplitude: 0.0,
            smoothed_energy: 0.0,
        }
    }

    fn init(&mut self, note: u8, velocity: u8, sample_rate: f32) {
        self.note = note;
        self.velocity = velocity;
        self.is_active = true;
        self.is_releasing = false;
        self.sustained = false;
        self.time_active = 0.0;
        self.release_time = 0.0;
        self.smoothed_energy = 0.0;

        // Panning based on keyboard pitch: 21 (A0, far left) to 108 (C8, far right)
        let pan = ((note as f32 - 21.0) / 87.0).clamp(0.0, 1.0);
        self.pan_left = (1.0 - pan * 0.6).sqrt();
        self.pan_right = (0.4 + pan * 0.6).sqrt();

        let norm_vel = (velocity as f32 / 127.0).clamp(0.01, 1.0);
        let vel_gain = norm_vel.powf(1.8);

        // Fundamental frequency
        let f0 = 440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0);

        // String inharmonicity coefficient (stiffness): higher for short bass strings
        let b = 0.0001 + 0.0003 * (1.0 - (note as f32 / 127.0));

        // Base decay: lower notes ring longer (up to 8s), high notes decay fast (0.5s)
        let note_decay_base = (1.0 - (note as f32 / 127.0)).powf(1.5) * 5.0 + 0.6;

        self.harmonics.clear();
        for k in 1..=MAX_HARMONICS {
            let k_f = k as f32;
            let harmonic_freq = k_f * f0 * (1.0 + b * k_f * k_f).sqrt();

            if harmonic_freq >= sample_rate * 0.45 {
                break;
            }

            // High harmonics decay much faster than low harmonics
            let decay = (k_f.powf(1.4) / note_decay_base) * 1.2;

            // Velocity brightness: high velocity introduces stronger upper harmonics
            let spectral_tilt = (1.0 / k_f.powf(1.1 - norm_vel * 0.4)) * vel_gain;

            self.harmonics.push(Harmonic {
                freq: harmonic_freq,
                amplitude: spectral_tilt * 0.35,
                decay_rate: decay,
                phase: (k as f32 * 0.123) * PI * 2.0,
            });
        }

        // Initial hammer strike noise transient. Seeded per note/velocity for reproducible output.
        let seed = 0xA3C5_9AC3
            ^ (note as u32).wrapping_mul(0x9E37_79B9)
            ^ (velocity as u32).wrapping_mul(0x85EB_CA6B);
        self.hammer_noise_state = if seed == 0 { 1 } else { seed };
        self.hammer_noise_previous = 0.0;
        self.hammer_noise_amplitude = vel_gain * 0.15;
    }

    fn release(&mut self) {
        if self.is_active && !self.is_releasing {
            self.is_releasing = true;
            self.release_time = 0.0;
        }
    }

    fn next_hammer_noise(&mut self) -> f32 {
        let mut state = self.hammer_noise_state;
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        self.hammer_noise_state = state;

        let raw = ((state as f64 / u32::MAX as f64) * 2.0 - 1.0) as f32;
        // Differencing removes DC bias. The 0.5 scale keeps the result in [-1, 1].
        let noise = (raw - self.hammer_noise_previous) * 0.5;
        self.hammer_noise_previous = raw;
        noise
    }

    fn next_sample(&mut self, dt: f32) -> (f32, f32) {
        if !self.is_active {
            return (0.0, 0.0);
        }

        self.time_active += dt;
        let mut sample = 0.0f32;
        let mut envelope_energy = 0.0f32;

        for h in &mut self.harmonics {
            let env = (-h.decay_rate * self.time_active).exp();
            let amplitude = h.amplitude * env;
            let phase_inc = 2.0 * PI * h.freq * dt;
            h.phase = (h.phase + phase_inc) % (2.0 * PI);
            sample += h.phase.sin() * amplitude;
            envelope_energy += amplitude * amplitude;
        }

        // Hammer transient noise (decays within 15ms)
        if self.time_active < 0.015 && self.hammer_noise_amplitude > 0.001 {
            let noise = self.next_hammer_noise();
            let noise_env = (1.0 - self.time_active / 0.015).max(0.0);
            sample += noise * self.hammer_noise_amplitude * noise_env;
        }

        // Release damping envelope
        if self.is_releasing {
            self.release_time += dt;
            let damp_time = 0.12f32; // 120ms smooth key release
            let damp_env = (1.0 - self.release_time / damp_time).max(0.0);
            sample *= damp_env;

            if self.release_time >= damp_time {
                self.is_active = false;
                return (0.0, 0.0);
            }
        }

        // Smooth actual energy over roughly 50ms so oscillator zero crossings cannot kill a voice.
        let smoothing = (dt * 20.0).clamp(0.0, 1.0);
        self.smoothed_energy += (sample * sample - self.smoothed_energy) * smoothing;

        // Pedal-sustained voices remain alive until pedal release, subject to the hard safety limit.
        let inaudible_threshold = 0.00005f32;
        let naturally_decayed = self.time_active > 2.0
            && !self.sustained
            && envelope_energy.sqrt() < inaudible_threshold
            && self.smoothed_energy.sqrt() < inaudible_threshold;
        if self.time_active > 12.0 || naturally_decayed {
            self.is_active = false;
            return (0.0, 0.0);
        }

        (sample * self.pan_left, sample * self.pan_right)
    }
}

/// Metronome Click Voice
#[derive(Debug, Clone)]
struct MetronomeVoice {
    is_active: bool,
    freq: f32,
    time: f32,
    duration: f32,
    volume: f32,
}

impl MetronomeVoice {
    fn new() -> Self {
        Self {
            is_active: false,
            freq: 880.0,
            time: 0.0,
            duration: 0.04,
            volume: 0.5,
        }
    }

    fn trigger(&mut self, is_accent: bool, volume: f32) {
        self.is_active = true;
        self.time = 0.0;
        self.freq = if is_accent { 1200.0 } else { 800.0 };
        self.duration = if is_accent { 0.05 } else { 0.035 };
        self.volume = volume;
    }

    fn reset(&mut self) {
        self.is_active = false;
        self.time = 0.0;
    }

    fn next_sample(&mut self, dt: f32) -> (f32, f32) {
        if !self.is_active {
            return (0.0, 0.0);
        }
        self.time += dt;
        if self.time >= self.duration {
            self.is_active = false;
            return (0.0, 0.0);
        }
        let env = (1.0 - self.time / self.duration).powf(2.0);
        let sample = (self.time * self.freq * 2.0 * PI).sin() * self.volume * env;
        (sample, sample)
    }
}

use crate::core::config::SynthSoundMode;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SoundFontPresetInfo {
    pub bank: i32,
    pub patch: i32,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredSoundFont {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

/// Discover SoundFonts available on the system and in the application directory
pub fn discover_system_soundfonts() -> Vec<DiscoveredSoundFont> {
    let mut results = Vec::new();
    let mut search_paths = Vec::new();

    // 1. Current directory and ./soundfonts
    search_paths.push(PathBuf::from("./soundfonts"));
    search_paths.push(PathBuf::from("."));

    // 2. User local data / config directory
    if let Some(config_dir) = dirs::config_dir() {
        search_paths.push(config_dir.join("vitl-piano-desktop").join("soundfonts"));
    }
    if let Some(data_dir) = dirs::data_local_dir() {
        search_paths.push(data_dir.join("soundfonts"));
    }

    // 3. Linux standard system soundfont paths
    #[cfg(target_os = "linux")]
    {
        search_paths.push(PathBuf::from("/usr/share/sounds/sf2"));
        search_paths.push(PathBuf::from("/usr/share/sounds/sf3"));
        search_paths.push(PathBuf::from("/usr/share/soundfonts"));
        search_paths.push(PathBuf::from("/usr/share/midi"));
    }

    // 4. Windows common soundfont locations
    #[cfg(target_os = "windows")]
    {
        search_paths.push(PathBuf::from("C:\\soundfonts"));
        if let Ok(appdata) = std::env::var("APPDATA") {
            search_paths.push(PathBuf::from(appdata).join("vitl-piano").join("soundfonts"));
        }
    }

    let mut seen_paths = std::collections::HashSet::new();

    for dir in search_paths {
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if is_supported_soundfont_path(&path) {
                        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                        if seen_paths.insert(canonical) {
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("SoundFont")
                                .to_string();
                            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            results.push(DiscoveredSoundFont {
                                name,
                                path: path.to_string_lossy().to_string(),
                                size_bytes,
                            });
                        }
                    }
                }
            }
        }
    }

    results
}

fn is_supported_soundfont_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("sf2"))
        .unwrap_or(false)
}

pub fn resolve_soundfont_path(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.exists() {
        return p.to_path_buf();
    }

    let filename = p.file_name().unwrap_or_default();

    // 1. Check relative to current executable directory
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let c1 = exe_dir.join(path);
            if c1.exists() {
                return c1;
            }
            let c2 = exe_dir.join("soundfonts").join(filename);
            if c2.exists() {
                return c2;
            }
            let c3 = exe_dir.join(filename);
            if c3.exists() {
                return c3;
            }
        }
    }

    // 2. Check current working directory
    let cwd_c1 = PathBuf::from("soundfonts").join(filename);
    if cwd_c1.exists() {
        return cwd_c1;
    }

    // 3. Check standard user data / install directory (~/.local/share/vitl-piano/soundfonts/)
    if let Some(home) = dirs::home_dir() {
        let c1 = home
            .join(".local/share/vitl-piano/soundfonts")
            .join(filename);
        if c1.exists() {
            return c1;
        }
        let c2 = home.join(".local/share/vitl-piano").join(path);
        if c2.exists() {
            return c2;
        }
    }

    // 4. Check Config / AppData directory
    if let Some(config_dir) = dirs::config_dir() {
        let c = config_dir
            .join("vitl-piano-desktop/soundfonts")
            .join(filename);
        if c.exists() {
            return c;
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        let c = data_dir.join("vitl-piano/soundfonts").join(filename);
        if c.exists() {
            return c;
        }
    }

    // 5. Check system soundfont directories
    let sys_candidates = [
        PathBuf::from("/usr/share/sounds/sf2").join(filename),
        PathBuf::from("/usr/share/sounds/sf3").join(filename),
        PathBuf::from("/usr/share/soundfonts").join(filename),
        PathBuf::from("C:\\soundfonts").join(filename),
    ];
    for sc in sys_candidates {
        if sc.exists() {
            return sc;
        }
    }

    p.to_path_buf()
}

pub struct SoundFontEngine {
    synthesizer: Synthesizer,
    pub sample_rate: f32,
    pub presets: Vec<SoundFontPresetInfo>,
    pub current_bank: i32,
    pub current_patch: i32,
}

impl SoundFontEngine {
    pub fn load_file(
        path: &str,
        sample_rate: f32,
        initial_bank: i32,
        initial_patch: i32,
    ) -> Result<Self, String> {
        let resolved_path = resolve_soundfont_path(path);
        let mut file = File::open(&resolved_path).map_err(|e| {
            format!(
                "Failed to open SoundFont file '{}' (searched: '{}'): {}",
                path,
                resolved_path.display(),
                e
            )
        })?;
        let sound_font_raw =
            SoundFont::new(&mut file).map_err(|e| format!("Failed to parse SoundFont: {:?}", e))?;

        let mut presets = Vec::new();
        for p in sound_font_raw.get_presets() {
            let name = p.get_name().trim().to_string();
            presets.push(SoundFontPresetInfo {
                bank: p.get_bank_number(),
                patch: p.get_patch_number(),
                name: if name.is_empty() {
                    format!("Preset {}:{}", p.get_bank_number(), p.get_patch_number())
                } else {
                    name
                },
            });
        }
        presets.sort_by_key(|p| (p.bank, p.patch));

        let sound_font = Arc::new(sound_font_raw);
        let mut settings = SynthesizerSettings::new(sample_rate as i32);
        settings.enable_reverb_and_chorus = true;
        settings.maximum_polyphony = 256;
        let mut synthesizer = Synthesizer::new(&sound_font, &settings)
            .map_err(|e| format!("Failed to initialize SoundFont synth: {:?}", e))?;

        // Resolve active preset
        let (bank, patch) = if !presets.is_empty() {
            if presets
                .iter()
                .any(|p| p.bank == initial_bank && p.patch == initial_patch)
            {
                (initial_bank, initial_patch)
            } else {
                (presets[0].bank, presets[0].patch)
            }
        } else {
            (initial_bank, initial_patch)
        };

        for ch in 0..Synthesizer::CHANNEL_COUNT as i32 {
            if ch != Synthesizer::PERCUSSION_CHANNEL as i32 {
                synthesizer.process_midi_message(ch, 0xB0, 0x00, bank >> 7);
                synthesizer.process_midi_message(ch, 0xB0, 0x20, bank & 0x7F);
                synthesizer.process_midi_message(ch, 0xC0, patch, 0);
            }
        }

        Ok(Self {
            synthesizer,
            sample_rate,
            presets,
            current_bank: bank,
            current_patch: patch,
        })
    }

    pub fn set_preset(&mut self, bank: i32, patch: i32) {
        self.current_bank = bank;
        self.current_patch = patch;
        for ch in 0..Synthesizer::CHANNEL_COUNT as i32 {
            if ch != Synthesizer::PERCUSSION_CHANNEL as i32 {
                self.synthesizer
                    .process_midi_message(ch, 0xB0, 0x00, bank >> 7);
                self.synthesizer
                    .process_midi_message(ch, 0xB0, 0x20, bank & 0x7F);
                self.synthesizer.process_midi_message(ch, 0xC0, patch, 0);
            }
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        self.synthesizer.note_on(0, note as i32, velocity as i32);
    }

    pub fn note_on_channel(&mut self, channel: i32, note: u8, velocity: u8) {
        let ch = channel.clamp(0, 15);
        self.synthesizer.note_on(ch, note as i32, velocity as i32);
    }

    pub fn note_off(&mut self, note: u8) {
        self.synthesizer.note_off(0, note as i32);
    }

    pub fn note_off_channel(&mut self, channel: i32, note: u8) {
        let ch = channel.clamp(0, 15);
        self.synthesizer.note_off(ch, note as i32);
    }

    pub fn set_sustain(&mut self, down: bool) {
        for ch in 0..Synthesizer::CHANNEL_COUNT as i32 {
            self.synthesizer
                .process_midi_message(ch, 0xB0, 64, if down { 127 } else { 0 });
        }
    }

    pub fn all_notes_off(&mut self) {
        self.synthesizer.note_off_all(false);
    }

    fn reset(&mut self) {
        let bank = self.current_bank;
        let patch = self.current_patch;
        self.synthesizer.reset();
        self.set_preset(bank, patch);
    }

    pub fn next_sample(&mut self) -> (f32, f32) {
        let mut left = [0.0f32; 1];
        let mut right = [0.0f32; 1];
        self.synthesizer.render(&mut left, &mut right);
        (left[0], right[0])
    }
}

pub struct PianoSynthEngine {
    sample_rate: f32,
    dt: f32,
    voices: Vec<Voice>,
    metronome: MetronomeVoice,
    reverb: Reverb,
    equalizer: ThreeBandEqualizer,
    delay: StereoDelay,
    pub volume: f32,
    pub sustain_pedal: bool,
    pub enabled: bool,
    pub metronome_enabled: bool,
    pub metronome_volume: f32,
    pub mode: SynthSoundMode,
    soundfont_engine: Option<SoundFontEngine>,
    pub soundfont_path: Option<String>,
    was_enabled: bool,
}

impl PianoSynthEngine {
    pub fn new(sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let voices = (0..MAX_POLYPHONY).map(|_| Voice::new_inactive()).collect();
        let reverb = Reverb::new(sample_rate);
        let equalizer = ThreeBandEqualizer::new(sample_rate);
        let delay = StereoDelay::new(sample_rate);

        Self {
            sample_rate,
            dt,
            voices,
            metronome: MetronomeVoice::new(),
            reverb,
            equalizer,
            delay,
            volume: 0.8,
            sustain_pedal: false,
            enabled: true,
            metronome_enabled: false,
            metronome_volume: 0.6,
            mode: SynthSoundMode::PhysicalModeling,
            soundfont_engine: None,
            soundfont_path: None,
            was_enabled: true,
        }
    }

    /// Load an optional custom SoundFont (.sf2)
    pub fn load_soundfont(&mut self, path: &str) -> Result<(), String> {
        self.load_soundfont_preset(path, 0, 0)
    }

    pub fn load_soundfont_preset(
        &mut self,
        path: &str,
        bank: i32,
        patch: i32,
    ) -> Result<(), String> {
        info!(
            "Loading SoundFont from: {} (bank={}, patch={})",
            path, bank, patch
        );
        match SoundFontEngine::load_file(path, self.sample_rate, bank, patch) {
            Ok(sf) => {
                self.reset_audio_state();
                self.soundfont_path = Some(path.to_string());
                self.mode = SynthSoundMode::SoundFont;
                self.soundfont_engine = Some(sf);
                info!("SoundFont loaded successfully: {}", path);
                Ok(())
            }
            Err(e) => {
                warn!(
                    "SoundFont load failed ({}); keeping the current synthesizer",
                    e
                );
                Err(e)
            }
        }
    }

    pub fn get_soundfont_presets(&self) -> Vec<SoundFontPresetInfo> {
        self.soundfont_engine
            .as_ref()
            .map(|sf| sf.presets.clone())
            .unwrap_or_default()
    }

    pub fn get_soundfont_active_preset(&self) -> Option<(i32, i32)> {
        self.soundfont_engine
            .as_ref()
            .map(|sf| (sf.current_bank, sf.current_patch))
    }

    pub fn set_soundfont_preset(&mut self, bank: i32, patch: i32) -> Result<(), String> {
        if let Some(ref mut sf) = self.soundfont_engine {
            sf.set_preset(bank, patch);
            Ok(())
        } else {
            Err("No SoundFont loaded".to_string())
        }
    }

    /// Unload SoundFont and return to built-in physical modeling
    pub fn unload_soundfont(&mut self) {
        self.reset_audio_state();
        self.soundfont_engine = None;
        self.soundfont_path = None;
        self.mode = SynthSoundMode::PhysicalModeling;
        info!("SoundFont unloaded. Active synth mode: Physical Modeling Grand");
    }

    /// Switch active synthesis mode
    pub fn set_mode(&mut self, mode: SynthSoundMode) {
        let next_mode = if mode == SynthSoundMode::SoundFont && self.soundfont_engine.is_none() {
            warn!("Cannot switch to SoundFont mode: no SoundFont file loaded. Reverting to Physical Modeling.");
            SynthSoundMode::PhysicalModeling
        } else {
            mode
        };

        if self.mode != next_mode {
            self.reset_audio_state();
            self.mode = next_mode;
        }
    }

    /// Trigger a note on event
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        self.note_on_channel(0, note, velocity);
    }

    pub fn note_on_channel(&mut self, channel: i32, note: u8, velocity: u8) {
        if !self.enabled || velocity == 0 {
            return;
        }

        if self.mode == SynthSoundMode::SoundFont {
            if let Some(ref mut sf) = self.soundfont_engine {
                sf.note_on_channel(channel, note, velocity);
                return;
            }
        }

        // Physical Modeling Voice Allocation
        // Check if note is already playing on a voice; if so, re-trigger it
        let mut target_idx = None;
        for (i, v) in self.voices.iter().enumerate() {
            if v.is_active && v.note == note {
                target_idx = Some(i);
                break;
            }
        }

        // Otherwise find first inactive voice
        if target_idx.is_none() {
            for (i, v) in self.voices.iter().enumerate() {
                if !v.is_active {
                    target_idx = Some(i);
                    break;
                }
            }
        }

        // Voice stealing: find oldest releasing voice or longest active voice
        if target_idx.is_none() {
            let mut oldest_idx = 0;
            let mut max_time = -1.0;
            for (i, v) in self.voices.iter().enumerate() {
                let priority = v.time_active + if v.is_releasing { 100.0 } else { 0.0 };
                if priority > max_time {
                    max_time = priority;
                    oldest_idx = i;
                }
            }
            target_idx = Some(oldest_idx);
        }

        if let Some(idx) = target_idx {
            self.voices[idx].init(note, velocity, self.sample_rate);
        }
    }

    /// Trigger a note off event
    pub fn note_off(&mut self, note: u8) {
        self.note_off_channel(0, note);
    }

    pub fn note_off_channel(&mut self, channel: i32, note: u8) {
        if self.mode == SynthSoundMode::SoundFont {
            if let Some(ref mut sf) = self.soundfont_engine {
                sf.note_off_channel(channel, note);
                return;
            }
        }

        for v in &mut self.voices {
            if v.is_active && v.note == note {
                if self.sustain_pedal {
                    v.sustained = true;
                } else {
                    v.release();
                }
            }
        }
    }

    /// Set sustain pedal state (CC64)
    pub fn set_sustain(&mut self, down: bool) {
        self.sustain_pedal = down;
        if let Some(ref mut sf) = self.soundfont_engine {
            sf.set_sustain(down);
        }
        if !down {
            for v in &mut self.voices {
                if v.is_active && v.sustained {
                    v.sustained = false;
                    v.release();
                }
            }
        }
    }

    /// Trigger metronome click
    pub fn trigger_metronome(&mut self, is_accent: bool) {
        if self.metronome_enabled {
            self.metronome.trigger(is_accent, self.metronome_volume);
        }
    }

    fn reset_audio_state(&mut self) {
        if let Some(ref mut sf) = self.soundfont_engine {
            sf.reset();
        }
        for voice in &mut self.voices {
            voice.is_active = false;
            voice.is_releasing = false;
            voice.sustained = false;
            voice.time_active = 0.0;
            voice.release_time = 0.0;
            voice.smoothed_energy = 0.0;
        }
        self.metronome.reset();
        self.reverb.reset();
        self.equalizer.reset();
        self.delay.reset();
        self.sustain_pedal = false;
    }

    /// Stop all active voices immediately and clear effect tails.
    pub fn all_notes_off(&mut self) {
        self.reset_audio_state();
    }

    /// Set reverb properties
    pub fn set_reverb_params(&mut self, mix: f32, room_size: f32) {
        let mix = mix.clamp(0.0, 1.0);
        if mix <= 0.001 && self.reverb.wet_mix > 0.001 {
            self.reverb.reset();
        }
        self.reverb.wet_mix = mix;
        self.reverb.room_size = room_size.clamp(0.0, 0.98);
    }

    /// Set 3-Band Equalizer (Low, Mid, High in dB)
    pub fn set_eq_params(&mut self, low_db: f32, mid_db: f32, high_db: f32) {
        self.equalizer.set_gains(low_db, mid_db, high_db);
    }

    /// Set Stereo Delay / Echo parameters
    pub fn set_delay_params(&mut self, enabled: bool, time_ms: f32, feedback: f32, mix: f32) {
        if !enabled && self.delay.enabled {
            self.delay.reset();
        }
        self.delay.enabled = enabled;
        self.delay.delay_time_ms = time_ms.clamp(10.0, 1500.0);
        self.delay.feedback = feedback.clamp(0.0, 0.85);
        self.delay.wet_mix = mix.clamp(0.0, 1.0);
    }

    /// Compute next stereo audio sample pair
    pub fn next_sample(&mut self) -> (f32, f32) {
        if !self.enabled {
            if self.was_enabled {
                self.reset_audio_state();
                self.was_enabled = false;
            }
            return (0.0, 0.0);
        }
        self.was_enabled = true;

        let dt = self.dt;
        let (mut mix_l, mut mix_r) =
            if self.mode == SynthSoundMode::SoundFont && self.soundfont_engine.is_some() {
                if let Some(ref mut sf) = self.soundfont_engine {
                    sf.next_sample()
                } else {
                    (0.0, 0.0)
                }
            } else {
                let mut l = 0.0f32;
                let mut r = 0.0f32;
                for v in &mut self.voices {
                    if v.is_active {
                        let (vl, vr) = v.next_sample(dt);
                        l += vl;
                        r += vr;
                    }
                }
                (l, r)
            };

        // Metronome click
        let (metro_l, metro_r) = self.metronome.next_sample(dt);
        mix_l += metro_l;
        mix_r += metro_r;

        // 1. Apply 3-Band Equalizer DSP
        let (eq_l, eq_r) = self.equalizer.process(mix_l, mix_r);

        // 2. Apply Stereo Delay / Echo DSP
        let (del_l, del_r) = self.delay.process(eq_l, eq_r);

        // 3. Apply Algorithmic Stereo Reverb DSP
        let (rev_l, rev_r) = self.reverb.process(del_l, del_r);

        // 4. Apply master volume and soft-knee limiter
        let out_l = soft_limit(rev_l * self.volume);
        let out_r = soft_limit(rev_r * self.volume);

        (out_l, out_r)
    }

    /// Render a block of stereo interleaved audio samples (L, R, L, R...)
    pub fn process_block(&mut self, output: &mut [f32]) {
        for frame in output.chunks_mut(2) {
            let (l, r) = self.next_sample();
            if frame.len() >= 2 {
                frame[0] = l;
                frame[1] = r;
            } else if !frame.is_empty() {
                frame[0] = (l + r) * 0.5;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hammer_noise_is_deterministic_bounded_and_zero_mean() {
        let mut first = Voice::new_inactive();
        let mut second = Voice::new_inactive();
        first.init(60, 100, 44_100.0);
        second.init(60, 100, 44_100.0);

        let mut sum = 0.0f64;
        let sample_count = 4_096;
        for _ in 0..sample_count {
            let a = first.next_hammer_noise();
            let b = second.next_hammer_noise();
            assert_eq!(a.to_bits(), b.to_bits());
            assert!((-1.0..=1.0).contains(&a));
            sum += a as f64;
        }

        assert!((sum / sample_count as f64).abs() < 0.001);
    }

    #[test]
    fn only_sf2_files_are_advertised_as_supported() {
        assert!(is_supported_soundfont_path(Path::new("Piano.sf2")));
        assert!(is_supported_soundfont_path(Path::new("Piano.SF2")));
        assert!(!is_supported_soundfont_path(Path::new("Piano.sf3")));
        assert!(!is_supported_soundfont_path(Path::new("Piano.dls")));
        assert!(!is_supported_soundfont_path(Path::new("Piano")));
    }
}
