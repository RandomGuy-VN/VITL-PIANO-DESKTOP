use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardLayoutType {
    QwertyUS,
    AzertyFR,
    QwertzDE,
    Dvorak,
}

impl Default for KeyboardLayoutType {
    fn default() -> Self {
        KeyboardLayoutType::QwertyUS
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub play_pause: String,
    pub pause: String,
    pub stop: String,
    pub speed_up: String,
    pub slow_down: String,
    pub transpose_up: String,
    pub transpose_down: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            play_pause: "F1".to_string(),
            pause: "F2".to_string(),
            stop: "F3".to_string(),
            speed_up: "F4".to_string(),
            slow_down: "F5".to_string(),
            transpose_up: "Ctrl+Up".to_string(),
            transpose_down: "Ctrl+Down".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynthSoundMode {
    PhysicalModeling,
    SoundFont,
}

impl Default for SynthSoundMode {
    fn default() -> Self {
        SynthSoundMode::PhysicalModeling
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthConfig {
    pub enabled: bool,
    #[serde(default)]
    pub mode: SynthSoundMode,
    #[serde(default)]
    pub soundfont_path: Option<String>,
    #[serde(default)]
    pub soundfont_bank: i32,
    #[serde(default)]
    pub soundfont_patch: i32,
    #[serde(default)]
    pub soundfont_preset_name: Option<String>,
    pub volume: f32,
    pub reverb_mix: f32,
    pub reverb_room_size: f32,
    pub metronome_enabled: bool,
    pub metronome_volume: f32,
}

impl Default for SynthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: SynthSoundMode::PhysicalModeling,
            soundfont_path: None,
            soundfont_bank: 0,
            soundfont_patch: 0,
            soundfont_preset_name: None,
            volume: 0.8,
            reverb_mix: 0.3,
            reverb_room_size: 0.7,
            metronome_enabled: false,
            metronome_volume: 0.6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanizeConfig {
    pub chord_delay_ms: f64,
    pub jitter_ms: f64,
    pub mistake_rate: f64,
    pub velocity_variation: f64,
    pub finger_limit: usize,
}

impl Default for HumanizeConfig {
    fn default() -> Self {
        Self {
            chord_delay_ms: 0.0,
            jitter_ms: 0.0,
            mistake_rate: 0.0,
            velocity_variation: 0.0,
            finger_limit: 11,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub active_theme: String,
    pub custom_css: String,
    pub background_mode: String, // "gradient", "solid", "image", "matrix", "stars", "waves"
    pub background_url: Option<String>,
    pub background_blur: f32,
    pub background_opacity: f32,
    pub accent_color: Option<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            active_theme: "dark-obsidian".to_string(),
            custom_css: String::new(),
            background_mode: "gradient".to_string(),
            background_url: None,
            background_blur: 0.0,
            background_opacity: 1.0,
            accent_color: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerConfig {
    pub enabled: bool,
    pub waterfall_speed: f64,
    pub palette: String, // "neon-cyber", "classic-gold", "sakura", "ocean-blue", "emerald", "synthwave"
    pub particle_effects: bool,
    pub note_glow: bool,
    pub show_piano_roll: bool,
    pub tail_rounding: f32,
    pub split_hands_color: bool,
    pub show_falling_notes: bool,
}

impl Default for VisualizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            waterfall_speed: 1.0,
            palette: "neon-cyber".to_string(),
            particle_effects: true,
            note_glow: true,
            show_piano_roll: true,
            tail_rounding: 4.0,
            split_hands_color: true,
            show_falling_notes: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectsConfig {
    pub eq_low: f32,  // -12.0 to +12.0 dB
    pub eq_mid: f32,  // -12.0 to +12.0 dB
    pub eq_high: f32, // -12.0 to +12.0 dB
    pub delay_enabled: bool,
    pub delay_time_ms: f32,
    pub delay_feedback: f32,
    pub delay_mix: f32,
    pub chorus_enabled: bool,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
}

impl Default for EffectsConfig {
    fn default() -> Self {
        Self {
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
            delay_enabled: false,
            delay_time_ms: 250.0,
            delay_feedback: 0.35,
            delay_mix: 0.25,
            chorus_enabled: false,
            chorus_rate: 1.5,
            chorus_depth: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub playback_speed: f64,
    pub transpose_offset: i8,
    pub pitch_offset: i8,
    pub loop_song: bool,
    pub release_on_pause: bool,
    pub macro_enabled: bool,
    pub use_midi_output: bool,
    pub velocity: bool,
    #[serde(default = "default_velocity_multiplier")]
    pub velocity_multiplier: f64,
    #[serde(default = "default_fixed_velocity")]
    pub fixed_velocity: u8,
    #[serde(default = "default_min_velocity")]
    pub min_velocity: u8,
    #[serde(default = "default_max_velocity")]
    pub max_velocity: u8,
    pub sustain: bool,
    pub no_doubles: bool,
    pub allow_88_keys: bool,
    #[serde(default = "default_true")]
    pub note_lengths: bool,
    #[serde(default = "default_min_note_length")]
    pub min_note_length_ms: f64,
    #[serde(default = "default_max_note_length")]
    pub max_note_length_ms: f64,
    pub sustain_cutoff: u8,
    pub auto_focus_window: bool,
    pub target_window_title: String,
    pub keyboard_layout: KeyboardLayoutType,
    pub hotkeys: HotkeyConfig,
    pub synth: SynthConfig,
    pub humanize: HumanizeConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub visualizer: VisualizerConfig,
    #[serde(default)]
    pub effects: EffectsConfig,
    pub custom_mappings_61: HashMap<String, String>,
    pub custom_mappings_low: HashMap<String, String>,
    pub custom_mappings_high: HashMap<String, String>,
    pub recent_files: Vec<String>,
    pub queue_files: Vec<String>,
    pub current_file: String,
}

fn default_true() -> bool {
    true
}

fn default_min_note_length() -> f64 {
    30.0
}

fn default_max_note_length() -> f64 {
    5000.0
}

fn default_velocity_multiplier() -> f64 {
    1.0
}

fn default_fixed_velocity() -> u8 {
    100
}

fn default_min_velocity() -> u8 {
    1
}

fn default_max_velocity() -> u8 {
    127
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            playback_speed: 1.0,
            transpose_offset: 0,
            pitch_offset: 0,
            loop_song: false,
            release_on_pause: true,
            macro_enabled: true,
            use_midi_output: false,
            velocity: false,
            velocity_multiplier: 1.0,
            fixed_velocity: 100,
            min_velocity: 1,
            max_velocity: 127,
            sustain: true,
            no_doubles: true,
            allow_88_keys: false,
            note_lengths: true,
            min_note_length_ms: 30.0,
            max_note_length_ms: 5000.0,
            sustain_cutoff: 63,
            auto_focus_window: true,
            target_window_title: "Roblox".to_string(),
            keyboard_layout: KeyboardLayoutType::QwertyUS,
            hotkeys: HotkeyConfig::default(),
            synth: SynthConfig::default(),
            humanize: HumanizeConfig::default(),
            theme: ThemeConfig::default(),
            visualizer: VisualizerConfig::default(),
            effects: EffectsConfig::default(),
            custom_mappings_61: HashMap::new(),
            custom_mappings_low: HashMap::new(),
            custom_mappings_high: HashMap::new(),
            recent_files: Vec::new(),
            queue_files: Vec::new(),
            current_file: String::new(),
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            let app_dir = config_dir.join("vitl-piano");
            let _ = fs::create_dir_all(&app_dir);
            app_dir.join("config.json")
        } else {
            PathBuf::from("config.json")
        }
    }

    pub fn midis_dir() -> PathBuf {
        if let Some(doc_dir) = dirs::document_dir() {
            let midis = doc_dir.join("VITL-Piano").join("Midis");
            let _ = fs::create_dir_all(&midis);
            midis
        } else {
            let midis = PathBuf::from("midis");
            let _ = fs::create_dir_all(&midis);
            midis
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }
        let config = AppConfig::default();
        let _ = config.save();
        config
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        let json_str = serde_json::to_string_pretty(self)?;
        fs::write(path, json_str)?;
        Ok(())
    }
}
