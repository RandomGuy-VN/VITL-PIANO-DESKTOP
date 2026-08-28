use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{error, info};

use crate::core::config::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubMidiItem {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub name: String,
    #[serde(default)]
    pub artists: Option<String>,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(rename = "midiFilename")]
    pub midi_filename: String,
    #[serde(rename = "imageFilename", default)]
    pub image_filename: Option<String>,
    #[serde(default)]
    pub downloads: Option<u64>,
    #[serde(default)]
    pub views: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalMidiFile {
    pub name: String,
    pub file_path: String,
    pub size_bytes: u64,
}

pub struct MidiHubClient {
    client: Client,
    pub cached_hub_data: Vec<HubMidiItem>,
}

impl MidiHubClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .build()
                .unwrap_or_default(),
            cached_hub_data: Vec::new(),
        }
    }

    /// Fetch latest songs from nanoMIDI API
    pub async fn fetch_hub_data(&mut self) -> Result<Vec<HubMidiItem>> {
        info!("Fetching nanoMIDI Hub database...");
        let url = "https://api.nanomidi.net/api/midiData";
        let res = self.client.get(url).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("Failed to fetch nanoMIDI hub data: HTTP {}", res.status());
        }

        let items: Vec<HubMidiItem> = res.json().await?;
        self.cached_hub_data = items.clone();
        info!("Fetched {} songs from nanoMIDI Hub", items.len());
        Ok(items)
    }

    /// Search cached hub items
    pub fn search(&self, query: &str) -> Vec<HubMidiItem> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.cached_hub_data.iter().take(60).cloned().collect();
        }

        self.cached_hub_data
            .iter()
            .filter(|item| {
                item.name.to_lowercase().contains(&q)
                    || item
                        .artists
                        .as_deref()
                        .map(|a| a.to_lowercase().contains(&q))
                        .unwrap_or(false)
                    || item
                        .uploader
                        .as_deref()
                        .map(|u| u.to_lowercase().contains(&q))
                        .unwrap_or(false)
            })
            .take(60)
            .cloned()
            .collect()
    }

    /// Download a MIDI file directly into the local MIDI folder
    pub async fn download_song(&self, midi_filename: &str) -> Result<PathBuf> {
        let url = format!("https://api.nanomidi.net/api/midis/{}", midi_filename);
        info!("Downloading MIDI file from {}", url);

        let res = self.client.get(&url).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("Download failed: HTTP {}", res.status());
        }

        let bytes = res.bytes().await?;
        let midis_dir = AppConfig::midis_dir();
        fs::create_dir_all(&midis_dir)?;
        let target_path = midis_dir.join(midi_filename);

        fs::write(&target_path, bytes).with_context(|| format!("Failed to save MIDI to {:?}", target_path))?;
        info!("Successfully saved MIDI to {:?}", target_path);

        Ok(target_path)
    }

    /// List all local MIDI files in the MIDI library
    pub fn list_local_midis() -> Vec<LocalMidiFile> {
        let dir = AppConfig::midis_dir();
        let mut list = Vec::new();

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext.eq_ignore_ascii_case("mid") || ext.eq_ignore_ascii_case("midi") || ext.eq_ignore_ascii_case("txt") {
                        if let Ok(meta) = entry.metadata() {
                            list.push(LocalMidiFile {
                                name: path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string(),
                                file_path: path.to_string_lossy().to_string(),
                                size_bytes: meta.len(),
                            });
                        }
                    }
                }
            }
        }

        list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        list
    }
}
