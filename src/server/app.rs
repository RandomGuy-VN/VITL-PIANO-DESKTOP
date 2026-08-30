use axum::extract::DefaultBodyLimit;
use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Multipart, State};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
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

#[derive(Debug, Deserialize)]
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
    LoadSoundFont { path: String, bank: Option<i32>, patch: Option<i32> },
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
    LoadSheet { sheet_text: String, title: Option<String> },
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
    #[serde(rename = "get_transcriber_status")]
    GetTranscriberStatus,
    #[serde(rename = "install_transkun")]
    InstallTranskun,
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
    #[serde(rename = "transcriber_status")]
    TranscriberStatusMsg(crate::core::transcriber::TranscriberStatus),
    #[serde(rename = "transcribe_progress")]
    TranscribeProgress { step: String, percent: f32, log: String },
    #[serde(rename = "notification")]
    Notification { level: String, message: String },
}

pub fn create_router(state: AppState) -> Router {
    let serve_dir = ServeDir::new(".");

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
        .route("/api/export_midi", get(export_midi_get_api).post(export_midi_post_api))
        .route("/api/export_sheet", get(export_sheet_get_api).post(export_sheet_post_api))
        .route("/api/transcribe", post(transcribe_audio_api))
        .route("/api/transcriber/status", get(get_transcriber_status_api))
        .route("/api/transcriber/install", post(post_transcriber_install_api))
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive())
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}

async fn serve_desktop_html() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-store, no-cache, must-revalidate, max-age=0"),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        include_str!("../../desktop.html"),
    )
}

async fn serve_logo_svg() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (axum::http::header::CACHE_CONTROL, "no-store, no-cache, must-revalidate, max-age=0"),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        include_str!("../../vitl-brand-logo.svg"),
    )
}

async fn serve_logo_png() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            (axum::http::header::CACHE_CONTROL, "no-store, no-cache, must-revalidate, max-age=0"),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        include_bytes!("../../vitl-brand-logo.png").as_slice(),
    )
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
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
                        if ws_send_1.lock().await.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket client lagged by {} status messages, continuing", n);
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
                let (b, p) = synth.get_soundfont_active_preset().unwrap_or((initial_bank, initial_patch));
                (r, presets, b, p)
            };
            match res {
                Ok(()) => {
                    state.config.lock().synth.mode = crate::core::config::SynthSoundMode::SoundFont;
                    state.config.lock().synth.soundfont_path = Some(path.clone());
                    state.config.lock().synth.soundfont_bank = cur_bank;
                    state.config.lock().synth.soundfont_patch = cur_patch;
                    let _ = state.config.lock().save();
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "success".to_string(),
                        message: format!("SoundFont loaded: {}", path),
                    }).await;
                    send_ws_message(ws, &ServerMessage::SoundFontPresets {
                        presets,
                        current_bank: cur_bank,
                        current_patch: cur_patch,
                        soundfont_path: path,
                    }).await;
                }
                Err(e) => {
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: format!("SoundFont error: {}", e),
                    }).await;
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
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "info".to_string(),
                        message: format!("Switched SoundFont instrument (Bank {}, Patch {})", bank, patch),
                    }).await;
                }
                Err(e) => {
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: format!("Failed to set preset: {}", e),
                    }).await;
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
            send_ws_message(ws, &ServerMessage::SoundFontPresets {
                presets,
                current_bank: cur_bank,
                current_patch: cur_patch,
                soundfont_path: sf_path,
            }).await;
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
            send_ws_message(ws, &ServerMessage::Notification {
                level: "info".to_string(),
                message: "Switched to built-in physical grand piano synth".to_string(),
            }).await;
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
                state.config.lock().synth.mode = crate::core::config::SynthSoundMode::PhysicalModeling;
            }
            let _ = state.config.lock().save();
        }
        ClientAction::LoadFile { path } => {
            info!("Loading song file: {}", path);
            let path_buf = PathBuf::from(&path);
            let res = if path.ends_with(".txt") {
                if let Ok(content) = std::fs::read_to_string(&path_buf) {
                    let title = path_buf.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    SheetParser::parse_sheet(&content, title, None)
                } else {
                    Err(anyhow::anyhow!("Failed to read sheet text file"))
                }
            } else {
                MidiParser::parse_file(&path_buf)
            };

            match res {
                Ok(song) => {
                    info!("Successfully parsed song '{}' ({} notes)", song.title, song.total_notes);
                    state.config.lock().current_file = path.clone();
                    let _ = state.config.lock().save();
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song);
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "success".to_string(),
                        message: "Song loaded successfully".to_string(),
                    }).await;
                }
                Err(e) => {
                    error!("Error parsing MIDI file: {:?}", e);
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: format!("Failed to parse file: {}", e),
                    }).await;
                }
            }
        }
        ClientAction::LoadSheet { sheet_text, title } => {
            let song_title = title.unwrap_or_else(|| "Virtual Piano Sheet".to_string());
            match SheetParser::parse_sheet(&sheet_text, song_title, None) {
                Ok(song) => {
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song);
                }
                Err(e) => {
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: format!("Failed to parse sheet: {}", e),
                    }).await;
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

                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "success".to_string(),
                        message: format!("Imported MuseScore: {}", song.title),
                    }).await;

                    let list = MidiHubClient::list_local_midis();
                    send_ws_message(ws, &ServerMessage::LocalMidis(list)).await;
                }
                Err(e) => {
                    error!("Failed to import MuseScore {}: {:?}", url, e);
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: format!("MuseScore import failed: {}", e),
                    }).await;
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
                        state.player.load_song(song);
                        state.player.play();
                        send_ws_message(ws, &ServerMessage::Notification {
                            level: "success".to_string(),
                            message: format!("Now playing: {}", midi_filename),
                        }).await;
                    } else {
                        send_ws_message(ws, &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("Downloaded but failed to parse: {}", midi_filename),
                        }).await;
                    }
                }
                Err(e) => {
                    error!("Failed to download hub song {}: {:?}", midi_filename, e);
                    send_ws_message(ws, &ServerMessage::Notification {
                        level: "error".to_string(),
                        message: format!("Download failed: {}", e),
                    }).await;
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
            let sanitized_title = song.title.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>();
            let filename = format!("{}.mid", sanitized_title);
            let midi_path = AppConfig::midis_dir().join(&filename);
            if let Ok(bytes) = song.to_midi_bytes() {
                let _ = std::fs::write(&midi_path, bytes);
            }

            send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
            send_ws_message(ws, &ServerMessage::Notification {
                level: "success".to_string(),
                message: format!("Song '{}' saved successfully", song.title),
            }).await;

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
                send_ws_message(ws, &ServerMessage::Notification {
                    level: "info".to_string(),
                    message: format!("Quantized to {:.0}ms grid", grid_ms),
                }).await;
            }
        }
        ClientAction::TransposeSong { semitones } => {
            let mut song_opt = state.current_song.lock().clone();
            if let Some(ref mut song) = song_opt {
                song.transpose(semitones);
                *state.current_song.lock() = Some(song.clone());
                state.player.load_song(song.clone());
                send_ws_message(ws, &ServerMessage::CurrentSong(Some(song.clone()))).await;
                send_ws_message(ws, &ServerMessage::Notification {
                    level: "info".to_string(),
                    message: format!("Transposed song by {} semitones", semitones),
                }).await;
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
        ClientAction::GetTranscriberStatus => {
            let status = crate::core::transcriber::AudioTranscriber::check_status();
            send_ws_message(ws, &ServerMessage::TranscriberStatusMsg(status)).await;
        }
        ClientAction::InstallTranskun => {
            send_ws_message(ws, &ServerMessage::Notification {
                level: "info".to_string(),
                message: "Installing Transkun in background...".to_string(),
            }).await;
            let ws_clone = Arc::clone(ws);
            tokio::task::spawn_blocking(move || {
                let res = crate::core::transcriber::AudioTranscriber::install_transkun_sync();
                tokio::spawn(async move {
                    match res {
                        Ok(msg) => {
                            send_ws_message(&ws_clone, &ServerMessage::Notification {
                                level: "success".to_string(),
                                message: msg,
                            }).await;
                            let status = crate::core::transcriber::AudioTranscriber::check_status();
                            send_ws_message(&ws_clone, &ServerMessage::TranscriberStatusMsg(status)).await;
                        }
                        Err(err) => {
                            send_ws_message(&ws_clone, &ServerMessage::Notification {
                                level: "error".to_string(),
                                message: err,
                            }).await;
                        }
                    }
                });
            });
        }
        ClientAction::RenderWav { output_path } => {
            let song_opt = state.current_song.lock().clone();
            if let Some(song) = song_opt.as_ref() {
                match AudioOutputManager::render_song_to_wav(song, &output_path) {
                    Ok(()) => {
                        send_ws_message(ws, &ServerMessage::Notification {
                            level: "success".to_string(),
                            message: format!("WAV rendered to {}", output_path),
                        }).await;
                    }
                    Err(e) => {
                        send_ws_message(ws, &ServerMessage::Notification {
                            level: "error".to_string(),
                            message: format!("WAV render failed: {}", e),
                        }).await;
                    }
                }
            } else {
                send_ws_message(ws, &ServerMessage::Notification {
                    level: "error".to_string(),
                    message: "No song loaded to render".to_string(),
                }).await;
            }
        }
        ClientAction::TriggerNote { note, velocity, is_on } => {
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
    let song_title = state.current_song.lock().as_ref()
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

async fn post_action_api(State(state): State<AppState>, Json(action): Json<ClientAction>) -> Response {
    match action {
        ClientAction::Play => state.player.play(),
        ClientAction::Pause => state.player.pause(),
        ClientAction::Stop => state.player.stop(),
        ClientAction::Seek { time_ms } => state.player.seek(time_ms),
        ClientAction::SetSpeed { speed } => state.player.set_speed(speed),
        ClientAction::SetTranspose { transpose } => state.player.set_transpose(transpose),
        _ => {} // Ignore complex actions in basic REST for now
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

            let res = if file_name.ends_with(".txt") {
                let text = String::from_utf8_lossy(&bytes);
                SheetParser::parse_sheet(&text, file_name.clone(), None)
            } else {
                MidiParser::parse_bytes(&bytes, file_name.clone())
            };

            match res {
                Ok(song) => {
                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song);
                    return Json(serde_json::json!({ "success": true, "filename": file_name })).into_response();
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
        Err(e) => {
            (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("{}", e)
                })),
            )
                .into_response()
        }
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

    let sanitized_title = song.title.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>();
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
    })).into_response()
}

async fn export_midi_get_api(State(state): State<AppState>) -> Response {
    let song_opt = state.current_song.lock().clone();
    if let Some(song) = song_opt {
        match song.to_midi_bytes() {
            Ok(bytes) => {
                let filename = format!("{}.mid", song.title.replace(' ', "_"));
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
            let filename = format!("{}.mid", song.title.replace(' ', "_"));
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
        let filename = format!("{}.txt", song.title.replace(' ', "_"));
        (
            [
                (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8"),
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
    let filename = format!("{}.txt", song.title.replace(' ', "_"));
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        sheet_text,
    )
        .into_response()
}

async fn get_transcriber_status_api() -> Json<crate::core::transcriber::TranscriberStatus> {
    let status = crate::core::transcriber::AudioTranscriber::check_status();
    Json(status)
}

async fn post_transcriber_install_api() -> Response {
    tokio::task::spawn_blocking(|| {
        let _ = crate::core::transcriber::AudioTranscriber::install_transkun_sync();
    });
    Json(serde_json::json!({ "success": true, "message": "Installation started in background" })).into_response()
}

async fn transcribe_audio_api(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    while let Ok(Some(field)) = multipart.next_field().await {
        let original_name = field.file_name().unwrap_or("audio_sample.mp3").to_string();
        if let Ok(bytes) = field.bytes().await {
            let temp_dir = std::env::temp_dir().join("vitl_transcribe");
            let _ = std::fs::create_dir_all(&temp_dir);

            let audio_temp = temp_dir.join(&original_name);
            let stem = PathBuf::from(&original_name).file_stem().unwrap_or_default().to_string_lossy().to_string();
            let midi_temp = temp_dir.join(format!("{}.mid", stem));

            if let Err(e) = std::fs::write(&audio_temp, &bytes) {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "success": false, "error": format!("Failed to write audio: {}", e) }))).into_response();
            }

            match crate::core::transcriber::AudioTranscriber::transcribe_file(&audio_temp, &midi_temp) {
                Ok(song) => {
                    // Copy output MIDI to library
                    let out_path = AppConfig::midis_dir().join(format!("{}.mid", stem));
                    if midi_temp.exists() {
                        let _ = std::fs::copy(&midi_temp, &out_path);
                    }

                    *state.current_song.lock() = Some(song.clone());
                    state.player.load_song(song.clone());

                    return Json(serde_json::json!({
                        "success": true,
                        "title": song.title,
                        "total_notes": song.total_notes,
                        "duration_ms": song.duration_ms,
                        "file_path": out_path.to_string_lossy().to_string()
                    })).into_response();
                }
                Err(e) => {
                    return (axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({ "success": false, "error": e }))).into_response();
                }
            }
        }
    }

    Json(serde_json::json!({ "success": false, "error": "No audio file received" })).into_response()
}
