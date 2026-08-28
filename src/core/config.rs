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
            chord_delay_ms: 15.0,
            jitter_ms: 5.0,
            mistake_rate: 0.0,
            velocity_variation: 10.0,
            finger_limit: 11,
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
    pub sustain: bool,
    pub no_doubles: bool,
    pub allow_88_keys: bool,
    pub sustain_cutoff: u8,
    pub auto_focus_window: bool,
    pub target_window_title: String,
    pub keyboard_layout: KeyboardLayoutType,
    pub hotkeys: HotkeyConfig,
    pub synth: SynthConfig,
    pub humanize: HumanizeConfig,
    pub custom_mappings_61: HashMap<String, String>,
    pub custom_mappings_low: HashMap<String, String>,
    pub custom_mappings_high: HashMap<String, String>,
    pub recent_files: Vec<String>,
    pub queue_files: Vec<String>,
    pub current_file: String,
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
            sustain: true,
            no_doubles: true,
            allow_88_keys: false,
            sustain_cutoff: 63,
            auto_focus_window: true,
            target_window_title: "Roblox".to_string(),
            keyboard_layout: KeyboardLayoutType::QwertyUS,
            hotkeys: HotkeyConfig::default(),
            synth: SynthConfig::default(),
            humanize: HumanizeConfig::default(),
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
