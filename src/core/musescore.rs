use anyhow::{Context, Result};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use super::config::AppConfig;
use super::midi::MidiParser;
use super::song::Song;

#[derive(Debug, Deserialize)]
struct JMuseInfo {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JMuseResponse {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    info: Option<JMuseInfo>,
}

pub struct MusescoreImporter {
    client: Client,
}

impl MusescoreImporter {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
            ),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/html, */*"),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("en-US,en;q=0.9"),
        );

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .default_headers(headers)
            .build()
            .unwrap_or_default();

        Self { client }
    }

    /// Extract numeric MuseScore Score ID from input URL or raw ID string.
    pub fn parse_score_id(input: &str) -> Option<u64> {
        let trimmed = input.trim();
        if let Ok(id) = trimmed.parse::<u64>() {
            return Some(id);
        }

        let re = Regex::new(r"(?:scores|score)/(\d+)").ok()?;
        if let Some(caps) = re.captures(trimmed) {
            if let Some(m) = caps.get(1) {
                return m.as_str().parse::<u64>().ok();
            }
        }

        let re_digits = Regex::new(r"/(\d+)(?:[/?#]|$)").ok()?;
        if let Some(caps) = re_digits.captures(trimmed) {
            if let Some(m) = caps.get(1) {
                return m.as_str().parse::<u64>().ok();
            }
        }

        None
    }

    /// Compute LibreScore MD5 authorization token for JMuse API
    pub fn compute_auth_token(score_id: u64, suffix: &str) -> String {
        let raw = format!("{}midi0{}", score_id, suffix);
        let digest = format!("{:x}", md5::compute(raw.as_bytes()));
        digest.chars().take(4).collect()
    }

    /// Fetch HTML to extract score title and JS bundle suffix
    async fn fetch_page_metadata(&self, score_id: u64, raw_input: &str) -> (Option<String>, Option<String>) {
        let target_url = if raw_input.starts_with("http://") || raw_input.starts_with("https://") {
            raw_input.to_string()
        } else {
            format!("https://musescore.com/score/{}", score_id)
        };

        let resp = match self.client.get(&target_url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => return (None, None),
        };

        let html = match resp.text().await {
            Ok(t) => t,
            _ => return (None, None),
        };

        // 1. Title extraction
        let title_re = Regex::new(r#"<meta\s+property=["']og:title["']\s+content=["'](.*?)["']"#).ok();
        let title = title_re.and_then(|re| {
            re.captures(&html).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        });

        // 2. JS build suffix extraction (LibreScore algorithm)
        let js_re = Regex::new(r#"href=["'](https://musescore\.com/static/public/build/musescore.*?(?:_es6)?/20.+?\.js)["']"#).ok();
        let mut js_suffix = None;

        if let Some(re) = js_re {
            for caps in re.captures_iter(&html) {
                if let Some(js_url) = caps.get(1) {
                    if let Ok(js_resp) = self.client.get(js_url.as_str()).send().await {
                        if let Ok(js_code) = js_resp.text().await {
                            let suffix_re = Regex::new(r#""([^"]+)"\)\.substr\(0,4\)"#).unwrap();
                            if let Some(s_caps) = suffix_re.captures(&js_code) {
                                if let Some(s) = s_caps.get(1) {
                                    js_suffix = Some(s.as_str().to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        (title, js_suffix)
    }

    /// Try LibreScore JMuse API with calculated auth tokens
    async fn try_jmuse_api(&self, score_id: u64, suffix_opt: Option<&str>) -> Result<Vec<u8>> {
        let mut suffixes = vec!["9654,4e", "8c41", "6c4d", "9a2f", "4b12"];
        if let Some(s) = suffix_opt {
            suffixes.insert(0, s);
        }

        for suffix in suffixes {
            let auth = Self::compute_auth_token(score_id, suffix);
            let api_url = format!("https://musescore.com/api/jmuse?id={}&type=midi&index=0", score_id);

            debug!("Trying JMuse API with auth={}: {}", auth, api_url);

            let res = match self
                .client
                .get(&api_url)
                .header(AUTHORIZATION, &auth)
                .header("Referer", format!("https://musescore.com/score/{}", score_id))
                .header("Origin", "https://musescore.com")
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => r,
                _ => continue,
            };

            if let Ok(json_res) = res.json::<JMuseResponse>().await {
                let download_url_opt = json_res.info.and_then(|i| i.url).or(json_res.url);
                if let Some(download_url) = download_url_opt {
                    if !download_url.is_empty() {
                        info!("Found MuseScore MIDI download URL: {}", download_url);
                        let file_resp = self.client.get(&download_url).send().await?;
                        if file_resp.status().is_success() {
                            let bytes = file_resp.bytes().await?;
                            if bytes.starts_with(b"MThd") {
                                return Ok(bytes.to_vec());
                            }
                        }
                    }
                }
            }
        }

        anyhow::bail!("JMuse API download failed for score {}", score_id)
    }

    /// Fallback to LibreScore & community converter mirror endpoints
    async fn try_mirror_proxies(&self, score_id: u64) -> Result<Vec<u8>> {
        let mirror_urls = [
            format!("https://api.nanomidi.net/api/musescore/{}", score_id),
            format!("https://webmscore-api.librescore.org/score/{}/midi", score_id),
            format!("https://api.librescore.org/api/score/{}/midi", score_id),
        ];

        for url in mirror_urls {
            debug!("Attempting mirror proxy: {}", url);
            if let Ok(resp) = self.client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        if bytes.starts_with(b"MThd") {
                            info!("Successfully fetched MIDI from mirror: {}", url);
                            return Ok(bytes.to_vec());
                        }
                    }
                }
            }
        }

        anyhow::bail!("All mirror proxies failed for score {}", score_id)
    }

    /// Main entry point: Import MuseScore score by link or ID, save to Library, and return Song
    pub async fn import_score(&self, input: &str) -> Result<(Song, PathBuf)> {
        let score_id = Self::parse_score_id(input)
            .ok_or_else(|| anyhow::anyhow!("Invalid MuseScore URL or score ID: {}", input))?;

        info!("Starting import for MuseScore Score ID: {}", score_id);

        let (title_opt, suffix_opt) = self.fetch_page_metadata(score_id, input).await;

        // Try JMuse API first, then fallback to mirror proxies
        let midi_bytes = match self.try_jmuse_api(score_id, suffix_opt.as_deref()).await {
            Ok(bytes) => bytes,
            Err(jmuse_err) => {
                warn!("Direct JMuse API failed ({:?}), trying mirror proxies...", jmuse_err);
                self.try_mirror_proxies(score_id).await.context(
                    "MuseScore Cloudflare security blocked direct downloading for this score. Please download the .mid/.mscz using LibreScore userscript and drag-and-drop into VITL Piano.",
                )?
            }
        };

        if !midi_bytes.starts_with(b"MThd") {
            anyhow::bail!("Downloaded payload is not a valid MIDI file (missing MThd header)");
        }

        // Determine clean filename
        let raw_title = title_opt.unwrap_or_else(|| format!("MuseScore_{}", score_id));
        let sanitized_title: String = raw_title
            .chars()
            .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>()
            .trim()
            .to_string();

        let filename = format!("{}.mid", sanitized_title);
        let midis_dir = AppConfig::midis_dir();
        fs::create_dir_all(&midis_dir)?;
        let target_path = midis_dir.join(&filename);

        fs::write(&target_path, &midi_bytes)
            .with_context(|| format!("Failed to write MIDI to {:?}", target_path))?;

        info!("Saved MuseScore MIDI to {:?}", target_path);

        let song = MidiParser::parse_file(&target_path)
            .with_context(|| format!("Failed to parse imported MuseScore MIDI {:?}", target_path))?;

        Ok((song, target_path))
    }
}
