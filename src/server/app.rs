use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::DefaultBodyLimit;
use axum::extract::{Multipart, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::core::config::AppConfig;
use crate::core::midi::MidiParser;
use crate::core::musescore::MusescoreImporter;
use crate::core::sheet::SheetParser;
use crate::core::song::Song;
use crate::core::transposition::TranspositionOptimizer;
use crate::hub::{HubMidiItem, LocalMidiFile, MidiHubClient};
use crate::player::engine::{PlaybackStatus, PlayerEngine};
use crate::synth::audio_output::AudioOutputManager;
use crate::synth::{discover_system_soundfonts, DiscoveredSoundFont, SoundFontPresetInfo};

#[derive(Clone)]
pub struct AppState {
    pub player: Arc<PlayerEngine>,
    pub config: Arc<Mutex<AppConfig>>,
    pub hub: Arc<tokio::sync::Mutex<MidiHubClient>>,
    pub status_receiver: Arc<Mutex<broadcast::Receiver<PlaybackStatus>>>,
    pub current_song: Arc<Mutex<Option<Song>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ClientAction {
    #[serde(rename = "play")]
    Play,
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "seek")]
    Seek { time_ms: f64 },
    #[serde(rename = "set_speed")]
    SetSpeed { speed: f64 },
    #[serde(rename = "set_bpm")]
    SetBpm { bpm: f64 },
    #[serde(rename = "set_transpose")]
    SetTranspose { transpose: i8 },
    #[serde(rename = "load_soundfont")]
    LoadSoundFont {
        path: String,
        bank: Option<i32>,
        patch: Option<i32>,
    },
    #[serde(rename = "unload_soundfont")]
    UnloadSoundFont,
    #[serde(rename = "set_soundfont_preset")]
    SetSoundFontPreset { bank: i32, patch: i32 },
    #[serde(rename = "get_soundfont_presets")]
    GetSoundFontPresets,
    #[serde(rename = "list_soundfonts")]
    ListSoundFonts,
    #[serde(rename = "set_synth_mode")]
    SetSynthMode { mode: String },
    #[serde(rename = "load_file")]
    LoadFile { path: String },
    #[serde(rename = "load_sheet")]
    LoadSheet {
        sheet_text: String,
        title: Option<String>,
    },
    #[serde(rename = "import_musescore")]
    ImportMusescore { url: String },
    #[serde(rename = "update_config")]
    UpdateConfig { config: AppConfig },
    #[serde(rename = "hub_search")]
    HubSearch { query: String },
    #[serde(rename = "hub_download")]
    HubDownload { midi_filename: String },
    #[serde(rename = "list_local_midis")]
    ListLocalMidis,
    #[serde(rename = "get_state")]
    GetState,
    #[serde(rename = "get_current_song")]
    GetCurrentSong,
    #[serde(rename = "save_song")]
    SaveSong { song: Song },
    #[serde(rename = "quantize_song")]
    QuantizeSong { grid_ms: f64 },
    #[serde(rename = "transpose_song")]
    TransposeSong { semitones: i8 },
    #[serde(rename = "set_effects")]
    SetEffects {
        eq_low: f32,
        eq_mid: f32,
        eq_high: f32,
        delay_enabled: bool,
        delay_time_ms: f32,
        delay_feedback: f32,
        delay_mix: f32,
    },
    #[serde(rename = "render_wav")]
    RenderWav { output_path: String },
    #[serde(rename = "trigger_note")]
    TriggerNote { note: u8, velocity: u8, is_on: bool },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerMessage {
    #[serde(rename = "status")]
    Status(PlaybackStatus),
    #[serde(rename = "config")]
    Config(AppConfig),
    #[serde(rename = "current_song")]
    CurrentSong(Option<Song>),
    #[serde(rename = "hub_results")]
    HubResults(Vec<HubMidiItem>),
    #[serde(rename = "local_midis")]
    LocalMidis(Vec<LocalMidiFile>),
    #[serde(rename = "soundfont_presets")]
    SoundFontPresets {
        presets: Vec<SoundFontPresetInfo>,
        current_bank: i32,
        current_patch: i32,
        soundfont_path: String,
    },
    #[serde(rename = "available_soundfonts")]
    AvailableSoundFonts(Vec<DiscoveredSoundFont>),
    #[serde(rename = "notification")]
    Notification { level: String, message: String },
}

const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const MAX_RENDER_DURATION_MS: f64 = 2.0 * 60.0 * 60.0 * 1000.0;
const MAX_RENDER_NOTES: usize = 500_000;
const MAX_SAFE_FILENAME_STEM: usize = 96;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_desktop_html))
        .route("/desktop.html", get(serve_desktop_html))
        .route("/index.html", get(serve_desktop_html))
        .route("/vitl-brand-logo.svg", get(serve_logo_svg))
        .route("/vitl-brand-logo.png", get(serve_logo_png))
        .route("/favicon.ico", get(serve_logo_png))
        .route("/ws", get(ws_handler))
        .route("/api/status", get(get_status_api))
        .route("/api/config", get(get_config_api))
        .route("/api/action", post(post_action_api))
        .route("/api/local_midis", get(get_local_midis_api))
        .route("/api/hub_songs", get(get_hub_songs_api))
        .route("/api/upload_midi", post(upload_midi_api))
        .route("/api/import_musescore", post(import_musescore_api))
        .route("/api/current_song", get(get_current_song_api))
        .route("/api/save_midi", post(save_midi_api))
        .route(
            "/api/export_midi",
            get(export_midi_get_api).post(export_midi_post_api),
        )
        .route(
            "/api/export_sheet",
            get(export_sheet_get_api).post(export_sheet_post_api),
        )
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "error": message.into(),
        })),
    )
        .into_response()
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

fn websocket_origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin_header) = headers.get(header::ORIGIN) else {
        // Non-browser local clients may omit Origin. Browsers always send it.
        return true;
    };
    let Ok(origin) = origin_header
        .to_str()
        .ok()
        .and_then(|value| reqwest::Url::parse(value).ok())
        .ok_or(())
    else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https") {
        return false;
    }
    let Some(origin_host) = origin.host_str() else {
        return false;
    };
    if !is_loopback_host(origin_host) {
        return false;
    }

    let Some(host_header) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(request_url) = reqwest::Url::parse(&format!("http://{}", host_header)) else {
        return false;
    };
    let Some(request_host) = request_url.host_str() else {
        return false;
    };

    is_loopback_host(request_host)
        && origin_host.eq_ignore_ascii_case(request_host)
        && origin.port_or_known_default() == request_url.port_or_known_default()
}

fn sanitized_stem(value: &str, fallback: &str) -> String {
    let sanitized: String = value
        .chars()
        .take(MAX_SAFE_FILENAME_STEM)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches('_');
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized.to_string()
    }
}

fn basename_from_untrusted(value: &str) -> Option<&str> {
    value
        .trim()
        .rsplit(|character| character == '/' || character == '\\')
        .next()
        .filter(|basename| !basename.is_empty() && *basename != "." && *basename != "..")
}

fn sanitize_upload_filename(value: &str) -> std::result::Result<String, String> {
    let basename = basename_from_untrusted(value)
        .ok_or_else(|| "Upload filename is missing or invalid".to_string())?;
    let path = Path::new(basename);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "Uploaded file must have a .mid, .midi, or .txt extension".to_string())?;
    if !matches!(extension.as_str(), "mid" | "midi" | "txt") {
        return Err("Uploaded file must have a .mid, .midi, or .txt extension".to_string());
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "Upload filename is not valid UTF-8".to_string())?;
    if stem.is_empty() {
        return Err("Upload filename must include a name".to_string());
    }
    Ok(format!("{}.{}", sanitized_stem(stem, "upload"), extension))
}

fn sanitize_song_filename(title: &str) -> String {
    format!("{}.mid", sanitized_stem(title, "song"))
}

fn sanitize_wav_filename(requested_path: &str, song_title: &str) -> String {
    let requested_stem = basename_from_untrusted(requested_path)
        .and_then(|basename| Path::new(basename).file_stem())
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or(song_title);
    format!("{}.wav", sanitized_stem(requested_stem, "song"))
}

fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_unique_temp_file(path: &Path) -> std::io::Result<(PathBuf, fs::File)> {
    let parent = usable_parent(path);
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for attempt in 0..16u8 {
        let candidate = parent.join(format!(
            ".{}.{}.{}.{}.tmp",
            filename,
            std::process::id(),
            timestamp,
            attempt
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique temporary output file",
    ))
}

fn atomic_write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let (temp_path, mut temp_file) = create_unique_temp_file(path)?;
    let result = (|| {
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);
        fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn persist_song_midi(song: &Song) -> std::result::Result<PathBuf, String> {
    let bytes = song.to_midi_bytes()?;
    let path = AppConfig::midis_dir().join(sanitize_song_filename(&song.title));
    atomic_write_file(&path, &bytes)
        .map_err(|error| format!("Failed to save MIDI to {:?}: {}", path, error))?;
    Ok(path)
}

fn validate_render_song(song: &Song) -> std::result::Result<usize, String> {
    if !song.duration_ms.is_finite()
        || song.duration_ms < 0.0
        || song.duration_ms > MAX_RENDER_DURATION_MS
    {
        return Err("Song duration is invalid or exceeds the two-hour render limit".to_string());
    }

    let mut note_count = 0usize;
    let mut maximum_note_end = 0.0f64;
    for track in &song.tracks {
        note_count = note_count
            .checked_add(track.notes.len())
            .ok_or_else(|| "Song note count overflowed".to_string())?;
        if note_count > MAX_RENDER_NOTES {
            return Err(format!(
                "Song exceeds the render limit of {} notes",
                MAX_RENDER_NOTES
            ));
        }
        for note in &track.notes {
            if !note.start_ms.is_finite()
                || !note.duration_ms.is_finite()
                || note.start_ms < 0.0
                || note.duration_ms < 0.0
            {
                return Err("Song contains an invalid note timestamp or duration".to_string());
            }
            let note_end = note.start_ms + note.duration_ms;
            if !note_end.is_finite() || note_end > MAX_RENDER_DURATION_MS {
                return Err("Song contains a note beyond the two-hour render limit".to_string());
            }
            maximum_note_end = maximum_note_end.max(note_end);
        }
    }
    if maximum_note_end > song.duration_ms + 1.0 {
        return Err("Song duration metadata does not contain all note events".to_string());
    }
    for control in &song.control_events {
        if !control.time_ms.is_finite()
            || control.time_ms < 0.0
            || control.time_ms > MAX_RENDER_DURATION_MS
        {
            return Err("Song contains an invalid control-event timestamp".to_string());
        }
    }

    Ok(note_count)
}

fn render_song_atomically(song: &Song, output_path: &Path) -> Result<()> {
    let (temp_path, placeholder) = create_unique_temp_file(output_path)
        .with_context(|| format!("Failed to create render file near {:?}", output_path))?;
    drop(placeholder);
    let result = (|| -> Result<()> {
        AudioOutputManager::render_song_to_wav(song, &temp_path)?;
        fs::rename(&temp_path, output_path).with_context(|| {
            format!(
                "Failed to atomically move rendered WAV from {:?} to {:?}",
                temp_path, output_path
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn validate_musescore_input(input: &str) -> std::result::Result<(), String> {
    let input = input.trim();
    if input.parse::<u64>().is_ok() {
        return Ok(());
    }
    let url = reqwest::Url::parse(input)
        .map_err(|_| "MuseScore input must be a score ID or HTTPS URL".to_string())?;
    if url.scheme() != "https" {
        return Err("MuseScore URLs must use HTTPS".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "MuseScore URL is missing a host".to_string())?;
    if host != "musescore.com" && !host.ends_with(".musescore.com") {
        return Err("MuseScore URL host is not allowed".to_string());
    }
    if MusescoreImporter::parse_score_id(input).is_none() {
        return Err("MuseScore URL does not contain a score ID".to_string());
    }
    Ok(())
}

async fn serve_desktop_html() -> impl IntoResponse {
    let content = if let Ok(c) = tokio::fs::read_to_string("desktop.html").await {
        c
    } else if let Ok(local_path) = std::env::var("HOME").map(|h| format!("{}/.local/share/vitl-piano/desktop.html", h)) {
        tokio::fs::read_to_string(&local_path).await.unwrap_or_else(|_| include_str!("../../desktop.html").to_string())
    } else {
        include_str!("../../desktop.html").to_string()
    };

    (
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                axum::http::header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, max-age=0",
            ),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        content,
    )
}

async fn serve_logo_svg() -> impl IntoResponse {
    let content = if let Ok(c) = tokio::fs::read_to_string("vitl-brand-logo.svg").await {
        c
    } else if let Ok(local_path) = std::env::var("HOME").map(|h| format!("{}/.local/share/vitl-piano/vitl-brand-logo.svg", h)) {
        tokio::fs::read_to_string(&local_path).await.unwrap_or_else(|_| include_str!("../../vitl-brand-logo.svg").to_string())
    } else {
        include_str!("../../vitl-brand-logo.svg").to_string()
    };

    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (
                axum::http::header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, max-age=0",
            ),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        content,
    )
}

async fn serve_logo_png() -> impl IntoResponse {
    let bytes = if let Ok(b) = tokio::fs::read("vitl-brand-logo.png").await {
        b
    } else if let Ok(local_path) = std::env::var("HOME").map(|h| format!("{}/.local/share/vitl-piano/vitl-brand-logo.png", h)) {
        tokio::fs::read(&local_path).await.unwrap_or_else(|_| include_bytes!("../../vitl-brand-logo.png").to_vec())
    } else {
        include_bytes!("../../vitl-brand-logo.png").to_vec()
    };

    (
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            (
                axum::http::header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, max-age=0",
            ),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        bytes,
    )
}

async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    if !websocket_origin_allowed(&headers) {
        return json_error(StatusCode::FORBIDDEN, "WebSocket origin is not allowed");
    }
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial config
    let current_cfg = state.config.lock().clone();
    let initial_msg = ServerMessage::Config(current_cfg);
    if let Ok(json) = serde_json::to_string(&initial_msg) {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Send initial current song if available
    let cur_song = state.current_song.lock().clone();
    let song_msg = ServerMessage::CurrentSong(cur_song);
    if let Ok(json) = serde_json::to_string(&song_msg) {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Send initial playback status
    let initial_status = ServerMessage::Status(state.player.get_status());
    if let Ok(json) = serde_json::to_string(&initial_status) {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Shared sender for both tasks
    let ws_sender = Arc::new(tokio::sync::Mutex::new(sender));

    // Task 1: Forward playback status to websocket client
    let mut status_subscriber = {
        let rx_lock = state.status_receiver.lock();
        rx_lock.resubscribe()
    };

    let ws_send_1 = Arc::clone(&ws_sender);
    let send_task = tokio::spawn(async move {
        loop {
            match status_subscriber.recv().await {
                Ok(status) => {
                    let msg = ServerMessage::Status(status);
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_send_1
                            .lock()
                            .await
                            .send(Message::Text(json))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        "WebSocket client lagged by {} status messages, continuing",
                        n
                    );
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Task 2: Receive and handle client actions
    let state_clone = state.clone();
    let ws_send_2 = Arc::clone(&ws_sender);
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(action) = serde_json::from_str::<ClientAction>(&text) {
                    handle_client_action(action, &state_clone, &ws_send_2).await;
                }
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

type WsSender = Arc<tokio::sync::Mutex<futures_util::stream::SplitSink<WebSocket, Message>>>;

async fn send_ws_message(ws: &WsSender, msg: &ServerMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = ws.lock().await.send(Message::Text(json)).await;
    }
}

async fn handle_client_action(action: ClientAction, state: &AppState, ws: &WsSender) {
    match action {
        ClientAction::Play => {
            state.player.play();
        }
        ClientAction::Pause => {
            state.player.pause();
        }
        ClientAction::Stop => {
            state.player.stop();
        }
        ClientAction::Seek { time_ms } => {
            state.player.seek(time_ms);
        }
        ClientAction::SetSpeed { speed } => {
            state.player.set_speed(speed);
        }
        ClientAction::SetBpm { bpm } => {
            state.player.set_bpm(bpm);
        }
        ClientAction::SetTranspose { transpose } => {
            state.player.set_transpose(transpose);
        }
        ClientAction::LoadSoundFont { path, bank, patch } => {
            let initial_bank = bank.unwrap_or_else(|| state.config.lock().synth.soundfont_bank);
            let initial_patch = patch.unwrap_or_else(|| state.config.lock().synth.soundfont_patch);
            let (res, presets, cur_bank, cur_patch) = {
                let synth_arc = state.player.synth();
                let mut synth = synth_arc.lock();
                let r = synth.load_soundfont_preset(&path, initial_bank, initial_patch);
                let presets = synth.get_soundfont_presets();
                let (b, p) = synth
                    .get_soundfont_active_preset()
                    .unwrap_or((initial_bank, initial_patch));
                (r, presets, b, p)
            };
            match res {
                Ok(()) => {
                    state.config.lock().synth.mode = crate::core::config::SynthSoundMode::SoundFont;
                    state.config.lock().synth.soundfont_path = Some(path.clone());
                    state.config.lock().synth.soundfont_bank = cur_bank;
                    state.config.lock().synth.soundfont_patch = cur_patch;
                    let _ = state.config.lock().save();
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "success".to_string(),
                            message: format!("SoundFont loaded: {}", path),
                        },
                    )
                    .await;
                    send_ws_message(
                        ws,
                        &ServerMessage::SoundFontPresets {
                            presets,
                            current_bank: cur_bank,
                            current_patch: cur_patch,
                            soundfont_path: path,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("SoundFont error: {}", e),
                        },
                    )
                    .await;
                }
            }
        }
        ClientAction::SetSoundFontPreset { bank, patch } => {
            let res = {
                let synth_arc = state.player.synth();
                let mut synth = synth_arc.lock();
                synth.set_soundfont_preset(bank, patch)
            };
            match res {
                Ok(()) => {
                    state.config.lock().synth.soundfont_bank = bank;
                    state.config.lock().synth.soundfont_patch = patch;
                    let _ = state.config.lock().save();
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "info".to_string(),
                            message: format!(
                                "Switched SoundFont instrument (Bank {}, Patch {})",
                                bank, patch
                            ),
                        },
                    )
                    .await;
                }
                Err(e) => {
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("Failed to set preset: {}", e),
                        },
                    )
                    .await;
                }
            }
        }
        ClientAction::GetSoundFontPresets => {
            let (presets, cur_bank, cur_patch, sf_path) = {
                let synth_arc = state.player.synth();
                let synth = synth_arc.lock();
                let presets = synth.get_soundfont_presets();
                let (b, p) = synth.get_soundfont_active_preset().unwrap_or((0, 0));
                let path = synth.soundfont_path.clone().unwrap_or_default();
                (presets, b, p, path)
            };
            send_ws_message(
                ws,
                &ServerMessage::SoundFontPresets {
                    presets,
                    current_bank: cur_bank,
                    current_patch: cur_patch,
                    soundfont_path: sf_path,
                },
            )
            .await;
        }
        ClientAction::ListSoundFonts => {
            let soundfonts = discover_system_soundfonts();
            send_ws_message(ws, &ServerMessage::AvailableSoundFonts(soundfonts)).await;
        }
        ClientAction::UnloadSoundFont => {
            {
                let synth_arc = state.player.synth();
                let mut synth = synth_arc.lock();
                synth.unload_soundfont();
            }
            state.config.lock().synth.mode = crate::core::config::SynthSoundMode::PhysicalModeling;
            state.config.lock().synth.soundfont_path = None;
            let _ = state.config.lock().save();
            send_ws_message(
                ws,
                &ServerMessage::Notification {
                    level: "info".to_string(),
                    message: "Switched to built-in physical grand piano synth".to_string(),
                },
            )
            .await;
        }
        ClientAction::SetSynthMode { mode } => {
            {
                let synth_arc = state.player.synth();
                let mut synth = synth_arc.lock();
                if mode == "soundfont" {
                    synth.set_mode(crate::core::config::SynthSoundMode::SoundFont);
                } else {
                    synth.set_mode(crate::core::config::SynthSoundMode::PhysicalModeling);
                }
            }
            if mode == "soundfont" {
                state.config.lock().synth.mode = crate::core::config::SynthSoundMode::SoundFont;
            } else {
                state.config.lock().synth.mode =
                    crate::core::config::SynthSoundMode::PhysicalModeling;
            }
            let _ = state.config.lock().save();
        }
        ClientAction::LoadFile { path } => {
            info!("Loading song file: {}", path);
            let path_buf = PathBuf::from(&path);
            let res = if path.to_lowercase().ends_with(".txt") {
                if let Ok(content) = std::fs::read_to_string(&path_buf) {
                    let title = path_buf
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    SheetParser::parse_sheet(&content, title, None)
                } else {
                    Err(anyhow::anyhow!("Failed to read sheet text file"))
                }
            } else {
                MidiParser::parse_file(&path_buf)
            };

            match res {
                Ok(song) => {
                    info!(
                        "Successfully parsed song '{}' ({} notes)",
                        song.title, song.total_notes
                    );
                    state.config.lock().current_file = path.clone();
                    let _ = state.config.lock().save();
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song.clone());
                    send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "success".to_string(),
                            message: "Song loaded successfully".to_string(),
                        },
                    )
                    .await;
                }
                Err(e) => {
                    error!("Error parsing MIDI file: {:?}", e);
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("Failed to parse file: {}", e),
                        },
                    )
                    .await;
                }
            }
        }
        ClientAction::LoadSheet { sheet_text, title } => {
            let song_title = title.unwrap_or_else(|| "Virtual Piano Sheet".to_string());
            match SheetParser::parse_sheet(&sheet_text, song_title, None) {
                Ok(song) => {
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song.clone());
                    send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                }
                Err(e) => {
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("Failed to parse sheet: {}", e),
                        },
                    )
                    .await;
                }
            }
        }
        ClientAction::ImportMusescore { url } => {
            let importer = MusescoreImporter::new();
            match importer.import_score(&url).await {
                Ok((song, path)) => {
                    info!("Successfully imported MuseScore MIDI: {}", song.title);
                    let path_str = path.to_string_lossy().to_string();
                    state.config.lock().current_file = path_str.clone();
                    let _ = state.config.lock().save();
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song.clone());

                    send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "success".to_string(),
                            message: format!("Imported MuseScore: {}", song.title),
                        },
                    )
                    .await;

                    let list = MidiHubClient::list_local_midis();
                    send_ws_message(ws, &ServerMessage::LocalMidis(list)).await;
                }
                Err(e) => {
                    error!("Failed to import MuseScore {}: {:?}", url, e);
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("MuseScore import failed: {}", e),
                        },
                    )
                    .await;
                }
            }
        }
        ClientAction::UpdateConfig { config } => {
            *state.config.lock() = config.clone();
            state.player.update_config(config.clone());
            let _ = config.save();
        }
        ClientAction::HubSearch { query } => {
            let mut hub = state.hub.lock().await;
            if hub.cached_hub_data.is_empty() {
                let _ = hub.fetch_hub_data().await;
            }
            let results = hub.search(&query);
            send_ws_message(ws, &ServerMessage::HubResults(results)).await;
        }
        ClientAction::HubDownload { midi_filename } => {
            let hub = state.hub.lock().await;
            match hub.download_song(&midi_filename).await {
                Ok(path) => {
                    if let Ok(song) = MidiParser::parse_file(&path) {
                        info!("Hub song downloaded and loaded: {}", song.title);
                        *state.current_song.lock() = Some(song.clone());
                        state.player.load_song(song.clone());
                        state.player.play();
                        send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                        send_ws_message(
                            ws,
                            &ServerMessage::Notification {
                                level: "success".to_string(),
                                message: format!("Now playing: {}", midi_filename),
                            },
                        )
                        .await;
                    } else {
                        send_ws_message(
                            ws,
                            &ServerMessage::Notification {
                                level: "error".to_string(),
                                message: format!(
                                    "Downloaded but failed to parse: {}",
                                    midi_filename
                                ),
                            },
                        )
                        .await;
                    }
                }
                Err(e) => {
                    error!("Failed to download hub song {}: {:?}", midi_filename, e);
                    send_ws_message(
                        ws,
                        &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("Download failed: {}", e),
                        },
                    )
                    .await;
                }
            }
        }
        ClientAction::ListLocalMidis => {
            let list = MidiHubClient::list_local_midis();
            send_ws_message(ws, &ServerMessage::LocalMidis(list)).await;
        }
        ClientAction::GetState => {
            let cfg = state.config.lock().clone();
            send_ws_message(ws, &ServerMessage::Config(cfg)).await;
        }
        ClientAction::GetCurrentSong => {
            let cur_song = state.current_song.lock().clone();
            send_ws_message(ws, &ServerMessage::CurrentSong(cur_song)).await;
        }
        ClientAction::SaveSong { mut song } => {
            song.finalize();
            *state.current_song.lock() = Some(song.clone());
            state.player.load_song(song.clone());

            // Save to Midis directory
            let sanitized_title = song
                .title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let filename = format!("{}.mid", sanitized_title);
            let midi_path = AppConfig::midis_dir().join(&filename);
            if let Ok(bytes) = song.to_midi_bytes() {
                let _ = std::fs::write(&midi_path, bytes);
            }

            send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
            send_ws_message(
                ws,
                &ServerMessage::Notification {
                    level: "success".to_string(),
                    message: format!("Song '{}' saved successfully", song.title),
                },
            )
            .await;

            let list = MidiHubClient::list_local_midis();
            send_ws_message(ws, &ServerMessage::LocalMidis(list)).await;
        }
        ClientAction::QuantizeSong { grid_ms } => {
            let mut song_opt = state.current_song.lock().clone();
            if let Some(ref mut song) = song_opt {
                song.quantize(grid_ms);
                *state.current_song.lock() = Some(song.clone());
                state.player.load_song(song.clone());
                send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                send_ws_message(
                    ws,
                    &ServerMessage::Notification {
                        level: "info".to_string(),
                        message: format!("Quantized to {:.0}ms grid", grid_ms),
                    },
                )
                .await;
            }
        }
        ClientAction::TransposeSong { semitones } => {
            let mut song_opt = state.current_song.lock().clone();
            if let Some(ref mut song) = song_opt {
                song.transpose(semitones);
                *state.current_song.lock() = Some(song.clone());
                state.player.load_song(song.clone());
                send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                send_ws_message(
                    ws,
                    &ServerMessage::Notification {
                        level: "info".to_string(),
                        message: format!("Transposed song by {} semitones", semitones),
                    },
                )
                .await;
            }
        }
        ClientAction::SetEffects {
            eq_low,
            eq_mid,
            eq_high,
            delay_enabled,
            delay_time_ms,
            delay_feedback,
            delay_mix,
        } => {
            {
                let synth_arc = state.player.synth();
                let mut synth = synth_arc.lock();
                synth.set_eq_params(eq_low, eq_mid, eq_high);
                synth.set_delay_params(delay_enabled, delay_time_ms, delay_feedback, delay_mix);
            }
            {
                let mut cfg = state.config.lock();
                cfg.effects.eq_low = eq_low;
                cfg.effects.eq_mid = eq_mid;
                cfg.effects.eq_high = eq_high;
                cfg.effects.delay_enabled = delay_enabled;
                cfg.effects.delay_time_ms = delay_time_ms;
                cfg.effects.delay_feedback = delay_feedback;
                cfg.effects.delay_mix = delay_mix;
                let _ = cfg.save();
            }
        }
        ClientAction::RenderWav { output_path } => {
            let song_opt = state.current_song.lock().clone();
            if let Some(song) = song_opt.as_ref() {
                match AudioOutputManager::render_song_to_wav(song, &output_path) {
                    Ok(()) => {
                        send_ws_message(
                            ws,
                            &ServerMessage::Notification {
                                level: "success".to_string(),
                                message: format!("WAV rendered to {}", output_path),
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ws_message(
                            ws,
                            &ServerMessage::Notification {
                                level: "error".to_string(),
                                message: format!("WAV render failed: {}", e),
                            },
                        )
                        .await;
                    }
                }
            } else {
                send_ws_message(
                    ws,
                    &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: "No song loaded to render".to_string(),
                    },
                )
                .await;
            }
        }
        ClientAction::TriggerNote {
            note,
            velocity,
            is_on,
        } => {
            if is_on {
                state.player.synth().lock().note_on(note, velocity);
            } else {
                state.player.synth().lock().note_off(note);
            }
        }
    }
}

async fn get_status_api(State(state): State<AppState>) -> Json<serde_json::Value> {
    let player_state = state.player.state();
    let song_title = state
        .current_song
        .lock()
        .as_ref()
        .map(|s| s.title.clone())
        .unwrap_or_else(|| "No song loaded".to_string());
    Json(serde_json::json!({
        "status": "ok",
        "version": "1.0.0",
        "player_state": format!("{:?}", player_state),
        "song_title": song_title,
        "speed": state.player.get_speed(),
        "transpose": state.player.get_transpose(),
    }))
}

async fn post_action_api(
    State(state): State<AppState>,
    Json(action): Json<ClientAction>,
) -> Response {
    match action {
        ClientAction::Play => state.player.play(),
        ClientAction::Pause => state.player.pause(),
        ClientAction::Stop => state.player.stop(),
        ClientAction::Seek { time_ms } => state.player.seek(time_ms),
        ClientAction::SetSpeed { speed } => state.player.set_speed(speed),
        ClientAction::SetBpm { bpm } => state.player.set_bpm(bpm),
        ClientAction::SetTranspose { transpose } => state.player.set_transpose(transpose),
        ClientAction::TriggerNote {
            note,
            velocity,
            is_on,
        } => {
            if is_on {
                state.player.synth().lock().note_on(note, velocity);
            } else {
                state.player.synth().lock().note_off(note);
            }
        }
        ClientAction::UpdateConfig { config } => {
            *state.config.lock() = config.clone();
            state.player.update_config(config.clone());
            let _ = config.save();
        }
        ClientAction::QuantizeSong { grid_ms } => {
            let mut song_opt = state.current_song.lock().clone();
            if let Some(ref mut song) = song_opt {
                song.quantize(grid_ms);
                *state.current_song.lock() = Some(song.clone());
                state.player.load_song(song.clone());
            }
        }
        ClientAction::TransposeSong { semitones } => {
            let mut song_opt = state.current_song.lock().clone();
            if let Some(ref mut song) = song_opt {
                song.transpose(semitones);
                *state.current_song.lock() = Some(song.clone());
                state.player.load_song(song.clone());
            }
        }
        ClientAction::SetEffects {
            eq_low,
            eq_mid,
            eq_high,
            delay_enabled,
            delay_time_ms,
            delay_feedback,
            delay_mix,
        } => {
            {
                let synth_arc = state.player.synth();
                let mut synth = synth_arc.lock();
                synth.set_eq_params(eq_low, eq_mid, eq_high);
                synth.set_delay_params(delay_enabled, delay_time_ms, delay_feedback, delay_mix);
            }
            {
                let mut cfg = state.config.lock();
                cfg.effects.eq_low = eq_low;
                cfg.effects.eq_mid = eq_mid;
                cfg.effects.eq_high = eq_high;
                cfg.effects.delay_enabled = delay_enabled;
                cfg.effects.delay_time_ms = delay_time_ms;
                cfg.effects.delay_feedback = delay_feedback;
                cfg.effects.delay_mix = delay_mix;
                let _ = cfg.save();
            }
        }
        _ => {}
    }
    Json(serde_json::json!({ "success": true })).into_response()
}

async fn get_config_api(State(state): State<AppState>) -> Json<AppConfig> {
    let cfg = state.config.lock().clone();
    Json(cfg)
}

async fn get_local_midis_api() -> Json<Vec<LocalMidiFile>> {
    let list = MidiHubClient::list_local_midis();
    Json(list)
}

async fn get_hub_songs_api(State(state): State<AppState>) -> Json<Vec<HubMidiItem>> {
    let mut hub = state.hub.lock().await;
    if hub.cached_hub_data.is_empty() {
        let _ = hub.fetch_hub_data().await;
    }
    Json(hub.cached_hub_data.clone())
}

async fn upload_midi_api(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = field.file_name().unwrap_or("uploaded.mid").to_string();
        if let Ok(bytes) = field.bytes().await {
            let target_path = AppConfig::midis_dir().join(&file_name);
            let _ = std::fs::write(&target_path, &bytes);

            let res = if file_name.to_lowercase().ends_with(".txt") {
                let text = String::from_utf8_lossy(&bytes);
                SheetParser::parse_sheet(&text, file_name.clone(), None)
            } else {
                MidiParser::parse_bytes(&bytes, file_name.clone())
            };

            match res {
                Ok(song) => {
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song);
                    return Json(serde_json::json!({ "success": true, "filename": file_name }))
                        .into_response();
                }
                Err(e) => {
                    return Json(serde_json::json!({ "success": false, "error": format!("Failed to parse {}: {}", file_name, e) })).into_response();
                }
            }
        }
    }

    Json(serde_json::json!({ "success": false, "error": "No file received" })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct MusescoreImportReq {
    pub url: String,
}

async fn import_musescore_api(
    State(state): State<AppState>,
    Json(payload): Json<MusescoreImportReq>,
) -> Response {
    let importer = MusescoreImporter::new();
    match importer.import_score(&payload.url).await {
        Ok((song, path)) => {
            let path_str = path.to_string_lossy().to_string();
            state.config.lock().current_file = path_str.clone();
            let _ = state.config.lock().save();
            *state.current_song.lock() = Some(song.clone());
            state.player.load_song(song.clone());

            Json(serde_json::json!({
                "success": true,
                "title": song.title,
                "duration_ms": song.duration_ms,
                "file_path": path_str
            }))
            .into_response()
        }
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": format!("{}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_current_song_api(State(state): State<AppState>) -> Json<Option<Song>> {
    let song = state.current_song.lock().clone();
    Json(song)
}

async fn save_midi_api(State(state): State<AppState>, Json(mut song): Json<Song>) -> Response {
    song.finalize();
    *state.current_song.lock() = Some(song.clone());
    state.player.load_song(song.clone());

    let sanitized_title = song
        .title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let filename = format!("{}.mid", sanitized_title);
    let midi_path = AppConfig::midis_dir().join(&filename);
    if let Ok(bytes) = song.to_midi_bytes() {
        let _ = std::fs::write(&midi_path, &bytes);
    }

    Json(serde_json::json!({
        "success": true,
        "title": song.title,
        "total_notes": song.total_notes,
        "file_path": midi_path.to_string_lossy().to_string()
    }))
    .into_response()
}

fn sanitize_header_filename(title: &str, ext: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = if clean.is_empty() { "song" } else { &clean };
    format!("{}.{}", base, ext)
}

async fn export_midi_get_api(State(state): State<AppState>) -> Response {
    let song_opt = state.current_song.lock().clone();
    if let Some(song) = song_opt {
        match song.to_midi_bytes() {
            Ok(bytes) => {
                let filename = sanitize_header_filename(&song.title, "mid");
                (
                    [
                        (axum::http::header::CONTENT_TYPE, "audio/midi"),
                        (
                            axum::http::header::CONTENT_DISPOSITION,
                            &format!("attachment; filename=\"{}\"", filename),
                        ),
                    ],
                    bytes,
                )
                    .into_response()
            }
            Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        }
    } else {
        (axum::http::StatusCode::NOT_FOUND, "No active song loaded").into_response()
    }
}

async fn export_midi_post_api(Json(song): Json<Song>) -> Response {
    match song.to_midi_bytes() {
        Ok(bytes) => {
            let filename = sanitize_header_filename(&song.title, "mid");
            (
                [
                    (axum::http::header::CONTENT_TYPE, "audio/midi"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        &format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn export_sheet_get_api(State(state): State<AppState>) -> Response {
    let song_opt = state.current_song.lock().clone();
    if let Some(song) = song_opt {
        let sheet_text = song.to_sheet_text();
        let filename = sanitize_header_filename(&song.title, "txt");
        (
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                ),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    &format!("attachment; filename=\"{}\"", filename),
                ),
            ],
            sheet_text,
        )
            .into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "No active song loaded").into_response()
    }
}

async fn export_sheet_post_api(Json(song): Json<Song>) -> Response {
    let sheet_text = song.to_sheet_text();
    let filename = sanitize_header_filename(&song.title, "txt");
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        sheet_text,
    )
        .into_response()
}
