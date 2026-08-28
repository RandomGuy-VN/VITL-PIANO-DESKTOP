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
use crate::core::sheet::SheetParser;
use crate::core::song::Song;
use crate::core::transposition::TranspositionOptimizer;
use crate::hub::{HubMidiItem, LocalMidiFile, MidiHubClient};
use crate::player::engine::{PlaybackStatus, PlayerEngine};
use crate::synth::audio_output::AudioOutputManager;

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
    #[serde(rename = "set_transpose")]
    SetTranspose { transpose: i8 },
    #[serde(rename = "load_file")]
    LoadFile { path: String },
    #[serde(rename = "load_sheet")]
    LoadSheet { sheet_text: String, title: Option<String> },
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
    #[serde(rename = "hub_results")]
    HubResults(Vec<HubMidiItem>),
    #[serde(rename = "local_midis")]
    LocalMidis(Vec<LocalMidiFile>),
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
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_str!("../../vitl-brand-logo.svg"),
    )
}

async fn serve_logo_png() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
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
        ClientAction::SetTranspose { transpose } => {
            state.player.set_transpose(transpose);
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
