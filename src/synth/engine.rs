use std::f32::consts::PI;
use super::dsp::{soft_limit, Reverb};

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
    hammer_noise_phase: f32,
    hammer_noise_amplitude: f32,
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
            hammer_noise_phase: 0.0,
            hammer_noise_amplitude: 0.0,
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

        // Initial hammer strike noise transient
        self.hammer_noise_phase = 0.0;
        self.hammer_noise_amplitude = vel_gain * 0.15;
    }

    fn release(&mut self) {
        if self.is_active && !self.is_releasing {
            self.is_releasing = true;
            self.release_time = 0.0;
        }
    }

    fn next_sample(&mut self, dt: f32) -> (f32, f32) {
        if !self.is_active {
            return (0.0, 0.0);
        }

        self.time_active += dt;
        let mut sample = 0.0f32;

        for h in &mut self.harmonics {
            let env = (-h.decay_rate * self.time_active).exp();
            let phase_inc = 2.0 * PI * h.freq * dt;
            h.phase = (h.phase + phase_inc) % (2.0 * PI);
            sample += h.phase.sin() * h.amplitude * env;
        }

        // Hammer transient noise (decays within 15ms)
        if self.time_active < 0.015 && self.hammer_noise_amplitude > 0.001 {
            let noise = ((self.time_active * 12345.67).sin() * 43758.5453).fract() * 2.0 - 1.0;
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

        // Auto-terminate voice if energy has dropped below audible threshold
        if self.time_active > 12.0 || (self.time_active > 1.0 && sample.abs() < 0.00005) {
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

use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::sync::Arc;
use std::fs::File;
use crate::core::config::SynthSoundMode;
use tracing::{info, warn, error};

pub struct SoundFontEngine {
    synthesizer: Synthesizer,
    pub sample_rate: f32,
}

impl SoundFontEngine {
    pub fn load_file(path: &str, sample_rate: f32) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("Failed to open SoundFont file '{}': {}", path, e))?;
        let sound_font = Arc::new(SoundFont::new(&mut file).map_err(|e| format!("Failed to parse SoundFont: {:?}", e))?);
        let settings = SynthesizerSettings::new(sample_rate as i32);
        let synthesizer = Synthesizer::new(&sound_font, &settings).map_err(|e| format!("Failed to initialize SoundFont synth: {:?}", e))?;
        Ok(Self { synthesizer, sample_rate })
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        self.synthesizer.note_on(0, note as i32, velocity as i32);
    }

    pub fn note_off(&mut self, note: u8) {
        self.synthesizer.note_off(0, note as i32);
    }

    pub fn set_sustain(&mut self, down: bool) {
        self.synthesizer.process_midi_message(0, 0xB0, 64, if down { 127 } else { 0 });
    }

    pub fn all_notes_off(&mut self) {
        self.synthesizer.note_off_all(false);
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
    pub volume: f32,
    pub sustain_pedal: bool,
    pub enabled: bool,
    pub metronome_enabled: bool,
    pub metronome_volume: f32,
    pub mode: SynthSoundMode,
    soundfont_engine: Option<SoundFontEngine>,
    pub soundfont_path: Option<String>,
}

impl PianoSynthEngine {
    pub fn new(sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let voices = (0..MAX_POLYPHONY).map(|_| Voice::new_inactive()).collect();
        let reverb = Reverb::new(sample_rate);

        Self {
            sample_rate,
            dt,
            voices,
            metronome: MetronomeVoice::new(),
            reverb,
            volume: 0.8,
            sustain_pedal: false,
            enabled: true,
            metronome_enabled: false,
            metronome_volume: 0.6,
            mode: SynthSoundMode::PhysicalModeling,
            soundfont_engine: None,
            soundfont_path: None,
        }
    }

    /// Load an optional custom SoundFont (.sf2 / .sf3)
    pub fn load_soundfont(&mut self, path: &str) -> Result<(), String> {
        info!("Loading SoundFont from: {}", path);
        match SoundFontEngine::load_file(path, self.sample_rate) {
            Ok(sf) => {
                self.soundfont_engine = Some(sf);
                self.soundfont_path = Some(path.to_string());
                self.mode = SynthSoundMode::SoundFont;
                info!("SoundFont loaded successfully: {}", path);
                Ok(())
            }
            Err(e) => {
                warn!("SoundFont load failed ({}); falling back to built-in physical synthesizer", e);
                self.mode = SynthSoundMode::PhysicalModeling;
                Err(e)
            }
        }
    }

    /// Unload SoundFont and return to built-in physical modeling
    pub fn unload_soundfont(&mut self) {
        self.soundfont_engine = None;
        self.soundfont_path = None;
        self.mode = SynthSoundMode::PhysicalModeling;
        info!("SoundFont unloaded. Active synth mode: Physical Modeling Grand");
    }

    /// Switch active synthesis mode
    pub fn set_mode(&mut self, mode: SynthSoundMode) {
        if mode == SynthSoundMode::SoundFont && self.soundfont_engine.is_none() {
            warn!("Cannot switch to SoundFont mode: no SoundFont file loaded. Reverting to Physical Modeling.");
            self.mode = SynthSoundMode::PhysicalModeling;
        } else {
            self.mode = mode;
        }
    }

    /// Trigger a note on event
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        if !self.enabled || velocity == 0 {
            return;
        }

        if self.mode == SynthSoundMode::SoundFont {
            if let Some(ref mut sf) = self.soundfont_engine {
                sf.note_on(note, velocity);
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
        if self.mode == SynthSoundMode::SoundFont {
            if let Some(ref mut sf) = self.soundfont_engine {
                sf.note_off(note);
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

    /// Stop all active voices immediately
    pub fn all_notes_off(&mut self) {
        if let Some(ref mut sf) = self.soundfont_engine {
            sf.all_notes_off();
        }
        for v in &mut self.voices {
            v.is_active = false;
        }
        self.reverb.reset();
    }

    /// Set reverb properties
    pub fn set_reverb_params(&mut self, mix: f32, room_size: f32) {
        self.reverb.wet_mix = mix.clamp(0.0, 1.0);
        self.reverb.room_size = room_size.clamp(0.0, 0.98);
    }

    /// Compute next stereo audio sample pair
    pub fn next_sample(&mut self) -> (f32, f32) {
        if !self.enabled {
            return (0.0, 0.0);
        }

        let dt = self.dt;
        let (mut mix_l, mut mix_r) = if self.mode == SynthSoundMode::SoundFont && self.soundfont_engine.is_some() {
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

        // Apply reverb DSP
        let (rev_l, rev_r) = self.reverb.process(mix_l, mix_r);

        // Apply master volume and soft-knee limiter
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
