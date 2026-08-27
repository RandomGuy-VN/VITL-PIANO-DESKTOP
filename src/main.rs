#![allow(dead_code, unused_imports, unused_variables)]

mod core;
mod hub;
mod input;
mod midi_io;
mod player;
mod server;
mod synth;
mod webview;

use anyhow::Result;
use clap::Parser;
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
use crate::webview::DesktopWindow;

#[derive(Parser, Debug)]
#[command(
    name = "vitl-piano-desktop",
    version = "1.0.0",
    about = "VITL Piano Native Desktop Autoplayer & Synthesizer"
)]
struct CliArgs {
    /// Port for desktop server and IPC
    #[arg(short, long, default_value = "4242")]
    port: u16,

    /// Automatically open the native desktop window
    #[arg(long, default_value = "true")]
    window: bool,

    /// MIDI or Sheet file to load immediately on startup
    #[arg(short, long)]
    file: Option<String>,

    /// Run in headless mode without native desktop window
    #[arg(long)]
    headless: bool,
}

fn main() -> Result<()> {
    // Make bundled runtime library shims (./lib, e.g. libjxl.so.0.12 required
    // by libwebkit2gtk) visible to child processes spawned by this app —
    // notably WebKit's WebProcess/NetworkProcess helpers. Our own RPATH
    // covers the main binary only; helpers inherit the environment instead.
    let mut bundled_lib_dir: Option<String> = None;
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let libdir = dir.join("lib");
            if libdir.is_dir() {
                let cur = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
                let new_val = if cur.is_empty() {
                    libdir.display().to_string()
                } else {
                    format!("{}:{}", libdir.display(), cur)
                };
                std::env::set_var("LD_LIBRARY_PATH", &new_val);
                bundled_lib_dir = Some(new_val);
            }
        }
    }

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vitl_piano_desktop=info,tower_http=info".into()),
        )
        .init();

    info!("==================================================");
    info!("    VITL Piano Native Desktop & Synthesizer       ");
    info!("             High-Performance v1.0.0              ");
    info!("==================================================");
    if let Some(dir) = bundled_lib_dir {
        info!(
            "Bundled libs exported to helper processes: LD_LIBRARY_PATH={}",
            dir
        );
    }

    let args = CliArgs::parse();
    let port = args.port;
    let file_arg = args.file.clone();
    let file_arg_headless = file_arg.clone();
    let headless = args.headless;

    // Create channel to notify main thread when backend server is ready with actual port
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<u16>();

    // Start background Tokio runtime for audio synthesizer, scheduler, and web server
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to initialize Tokio runtime");
        rt.block_on(async move {
            if let Err(e) = run_backend(port, file_arg, ready_tx).await {
                error!("Backend runtime error: {:?}", e);
            }
        });
    });

    // Wait for server to bind and receive actual port
    let actual_port = ready_rx.recv().unwrap_or(port);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let server_url = format!("http://127.0.0.1:{}/desktop.html?_t={}", actual_port, ts);

    if !headless && args.window {
        DesktopWindow::run(server_url, "VITL Piano Desktop", 1120.0, 760.0)?;
    } else {
        info!("Running in headless daemon mode on http://127.0.0.1:{}", actual_port);
        
        if file_arg_headless.is_some() {
            info!("Headless mode: Auto-playing file in 3 seconds... Switch to Roblox now!");
            std::thread::sleep(std::time::Duration::from_secs(3));
            
            // Trigger playback via HTTP request to the local API
            let _ = std::process::Command::new("curl")
                .args(["-X", "POST", "-H", "Content-Type: application/json", "-d", "{\"action\":\"play\"}", &format!("http://127.0.0.1:{}/api/action", actual_port)])
                .output();
        }

        // Keep main thread alive
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    Ok(())
}

async fn run_backend(
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

    // 2. Initialize Real-Time Sound Synthesis Output Engine (cpal)
    let audio_manager = if use_synth {
        match AudioOutputManager::new() {
            Ok(mgr) => {
                info!("Audio synthesizer initialized at {} Hz", mgr.sample_rate());
                Some(mgr)
            }
            Err(e) => {
                error!("Audio device warning: {:?}. Running in silent macro mode.", e);
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

    // 3. Status broadcast channel
    let (status_tx, status_rx) = broadcast::channel::<PlaybackStatus>(128);

    // 4. Initialize Player Engine
    let player = Arc::new(PlayerEngine::new(
        synth_engine,
        Arc::clone(&config),
        status_tx.clone(),
    ));

    // 5. Initialize Global Hotkeys Listener
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

    // 6. Hub Client
    let hub_client = Arc::new(tokio::sync::Mutex::new(MidiHubClient::new()));

    // 7. Load initial song if passed
    let current_song: Arc<Mutex<Option<Song>>> = Arc::new(Mutex::new(None));
    if let Some(file_path) = initial_file {
        info!("Loading requested initial file: {}", file_path);
        if let Ok(song) = MidiParser::parse_file(&file_path) {
            *current_song.lock() = Some(song.clone());
            player.load_song(song);
        }
    }

    // 8. Launch Web Server & WebSocket IPC Bridge with automatic port fallback
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

    let listener = bound_listener.ok_or_else(|| anyhow::anyhow!("Could not bind to any port in range {}..{}", start_port, start_port + 50))?;
    let local_addr = listener.local_addr()?;
    info!("Server listening on http://{}", local_addr);

    // Notify main thread of actual port
    let _ = ready_tx.send(bound_port);

    axum::serve(listener, router).await?;

    Ok(())
}
