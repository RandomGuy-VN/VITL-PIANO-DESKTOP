#![allow(dead_code, unused_imports, unused_variables)]

pub mod core;
pub mod hub;
pub mod input;
pub mod midi_io;
pub mod player;
pub mod server;
pub mod synth;
pub mod webview;

use anyhow::Result;
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::core::config::AppConfig;
use crate::core::midi::MidiParser;
use crate::core::song::Song;
use crate::hub::MidiHubClient;
use crate::input::hotkeys::{HotkeyAction, HotkeyManager};
use crate::player::engine::{PlaybackStatus, PlayerEngine, PlayerState};
use crate::server::app::{create_router, AppState};
use crate::synth::audio_output::AudioOutputManager;

pub async fn run_backend(
    start_port: u16,
    initial_file: Option<String>,
    ready_tx: std::sync::mpsc::Sender<u16>,
) -> Result<()> {
    let config = Arc::new(Mutex::new(AppConfig::load()));
    info!("Loaded configuration from {:?}", AppConfig::config_path());

    // In headless mode, we explicitly disable the audio synthesizer
    let mut use_synth = true;
    if std::env::args().any(|a| a == "--headless") {
        let mut cfg = config.lock();
        cfg.synth.enabled = false;
        use_synth = false;
        info!("Headless mode: Audio synthesizer disabled.");
    }

    // Initialize Real-Time Sound Synthesis Output Engine (cpal)
    let audio_manager = if use_synth {
        match AudioOutputManager::new() {
            Ok(mgr) => {
                info!("Audio synthesizer initialized at {} Hz", mgr.sample_rate());
                Some(mgr)
            }
            Err(e) => {
                error!(
                    "Audio device warning: {:?}. Running in silent macro mode.",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let synth_engine = if let Some(ref mgr) = audio_manager {
        mgr.engine()
    } else {
        Arc::new(Mutex::new(synth::engine::PianoSynthEngine::new(44100.0)))
    };

    // Load configured SoundFont on startup if set
    {
        let cfg = config.lock().clone();
        if cfg.synth.mode == crate::core::config::SynthSoundMode::SoundFont {
            if let Some(ref sf_path) = cfg.synth.soundfont_path {
                let _ = synth_engine.lock().load_soundfont(sf_path);
            }
        }
    }

    // Status broadcast channel
    let (status_tx, status_rx) = broadcast::channel::<PlaybackStatus>(128);

    // Initialize Player Engine
    let player = Arc::new(PlayerEngine::new(
        synth_engine,
        Arc::clone(&config),
        status_tx.clone(),
    ));

    // Initialize Global Hotkeys Listener
    let (hotkey_mgr, mut hotkey_rx) = HotkeyManager::new();
    let hotkey_config = config.lock().hotkeys.clone();
    hotkey_mgr.start_listener(hotkey_config);

    let player_hotkey = Arc::clone(&player);
    tokio::spawn(async move {
        while let Some(action) = hotkey_rx.recv().await {
            match action {
                HotkeyAction::PlayPause => {
                    if player_hotkey.state() == PlayerState::Playing {
                        player_hotkey.pause();
                    } else {
                        player_hotkey.play();
                    }
                }
                HotkeyAction::Pause => {
                    player_hotkey.pause();
                }
                HotkeyAction::Stop => {
                    player_hotkey.stop();
                }
                HotkeyAction::SpeedUp => {
                    player_hotkey.adjust_speed(0.1);
                    info!("Speed increased: {:.1}x", player_hotkey.get_speed());
                }
                HotkeyAction::SlowDown => {
                    player_hotkey.adjust_speed(-0.1);
                    info!("Speed decreased: {:.1}x", player_hotkey.get_speed());
                }
                HotkeyAction::TransposeUp => {
                    player_hotkey.adjust_transpose(1);
                    info!("Transpose increased: {}", player_hotkey.get_transpose());
                }
                HotkeyAction::TransposeDown => {
                    player_hotkey.adjust_transpose(-1);
                    info!("Transpose decreased: {}", player_hotkey.get_transpose());
                }
            }
        }
    });

    // Hub Client
    let hub_client = Arc::new(tokio::sync::Mutex::new(MidiHubClient::new()));

    // Load initial song if passed, or fallback to configured song / bundled sample
    let current_song: Arc<Mutex<Option<Song>>> = Arc::new(Mutex::new(None));
    let initial_path = initial_file.or_else(|| {
        let cfg_file = config.lock().current_file.clone();
        if !cfg_file.is_empty() && std::path::Path::new(&cfg_file).exists() {
            Some(cfg_file)
        } else {
            let samples = [
                "samples/fur_elise.mid",
                "samples/canon_in_d.mid",
                "samples/rush_e.mid",
            ];
            samples
                .iter()
                .find(|s| std::path::Path::new(s).exists())
                .map(|s| s.to_string())
        }
    });

    if let Some(ref file_path) = initial_path {
        info!("Loading initial song: {}", file_path);
        let path_buf = std::path::PathBuf::from(file_path);
        let res = if file_path.to_lowercase().ends_with(".txt") {
            if let Ok(content) = std::fs::read_to_string(&path_buf) {
                let title = path_buf
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                crate::core::sheet::SheetParser::parse_sheet(&content, title, None)
            } else {
                Err(anyhow::anyhow!("Failed to read sheet file"))
            }
        } else {
            MidiParser::parse_file(file_path)
        };

        if let Ok(song) = res {
            *current_song.lock() = Some(song.clone());
            player.load_song(song);
        }
    }

    // Launch Web Server & WebSocket IPC Bridge with automatic port fallback
    let state = AppState {
        player: Arc::clone(&player),
        config: Arc::clone(&config),
        hub: Arc::clone(&hub_client),
        status_receiver: Arc::new(Mutex::new(status_rx)),
        current_song,
    };

    let router = create_router(state);

    // Try binding starting from start_port up to start_port + 50
    let mut bound_listener = None;
    let mut bound_port = start_port;

    for p in start_port..(start_port + 50) {
        let addr = SocketAddr::from(([127, 0, 0, 1], p));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => {
                bound_port = p;
                bound_listener = Some(l);
                break;
            }
            Err(e) => {
                warn!("Port {} unavailable ({:?}), trying next port...", p, e);
            }
        }
    }

    let listener = bound_listener.ok_or_else(|| {
        anyhow::anyhow!(
            "Could not bind to any port in range {}..{}",
            start_port,
            start_port + 50
        )
    })?;
    let local_addr = listener.local_addr()?;
    info!("Server listening on http://{}", local_addr);

    // Notify main thread of actual port
    let _ = ready_tx.send(bound_port);

    axum::serve(listener, router).await?;

    Ok(())
}
