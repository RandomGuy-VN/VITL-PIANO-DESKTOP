mod install;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::os::windows::process::CommandExt;
use std::time::Duration;
use tauri::{Emitter, Window};
use install::{
    get_install_info, check_latest_update, download_and_extract_online_update,
    close_running_instances, extract_payload, create_shortcut, register_uninstaller,
    launch_application, InstallInfo, InstallOptions, OnlineUpdateInfo,
};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[tauri::command]
fn get_info() -> InstallInfo {
    get_install_info()
}

#[tauri::command]
async fn check_update() -> OnlineUpdateInfo {
    tokio::task::spawn_blocking(|| {
        check_latest_update()
    }).await.unwrap_or_else(|_| OnlineUpdateInfo {
        available: false,
        tag: "v1.0.0".to_string(),
        name: "Embedded Bundle".to_string(),
        download_url: None,
    })
}

#[tauri::command]
fn select_folder(current: String) -> Option<String> {
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $f = New-Object System.Windows.Forms.FolderBrowserDialog; \
         $f.Description = 'Select VITL Piano Installation Folder'; \
         $f.SelectedPath = '{}'; \
         $f.ShowNewFolderButton = $true; \
         if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ \
             [Console]::Write($f.SelectedPath) \
         }}",
        current.replace('\'', "''")
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !path_str.is_empty() {
        Some(path_str)
    } else {
        None
    }
}

#[tauri::command]
async fn perform_install(window: Window, options: InstallOptions) -> Result<String, String> {
    let target_dir = PathBuf::from(&options.target_dir);
    let target_exe = target_dir.join("vitl-piano.exe");

    let win = window.clone();
    let emit_progress = move |percent: f32, msg: &str| {
        let _ = win.emit("install-progress", serde_json::json!({
            "percent": (percent * 100.0).round() as u32,
            "status": msg
        }));
    };

    emit_progress(0.05, "Closing any active VITL Piano instances...");
    close_running_instances();
    tokio::time::sleep(Duration::from_millis(300)).await;

    emit_progress(0.1, "Preparing installation directory...");
    
    let mut installed_successfully = false;

    // Check if user chose to claim latest online release from GitHub
    if options.use_online_latest {
        let download_url = options.online_download_url.clone().unwrap_or_else(|| {
            "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-windows-portable.zip".to_string()
        });

        let cb_win = window.clone();
        let target_clone = target_dir.clone();
        let url_clone = download_url.clone();

        let res = tokio::task::spawn_blocking(move || {
            download_and_extract_online_update(&url_clone, &target_clone, |pct, status| {
                let _ = cb_win.emit("install-progress", serde_json::json!({
                    "percent": (pct * 100.0).round() as u32,
                    "status": status
                }));
            })
        }).await;

        if let Ok(Ok(())) = res {
            installed_successfully = true;
        } else {
            let _ = window.emit("install-progress", serde_json::json!({
                "percent": 30,
                "status": "Online package unavailable, unpacking offline bundle..."
            }));
        }
    }

    // If not installed online or offline fallback, use embedded bundle
    if !installed_successfully {
        let cb_win = window.clone();
        let target_clone = target_dir.clone();
        tokio::task::spawn_blocking(move || {
            extract_payload(&target_clone, |pct, status| {
                let _ = cb_win.emit("install-progress", serde_json::json!({
                    "percent": (pct * 100.0).round() as u32,
                    "status": status
                }));
            })
        }).await.map_err(|e| format!("Extraction task error: {}", e))??;
    }

    if options.create_desktop_shortcut {
        let _ = window.emit("install-progress", serde_json::json!({
            "percent": 88,
            "status": "Creating Desktop shortcut..."
        }));
        if let Some(desktop) = dirs::desktop_dir() {
            let shortcut_path = desktop.join("VITL Piano.lnk");
            create_shortcut(&target_exe, &shortcut_path, &target_dir);
        }
    }

    if options.create_start_menu_shortcut {
        let _ = window.emit("install-progress", serde_json::json!({
            "percent": 92,
            "status": "Creating Start Menu shortcut..."
        }));
        if let Some(mut start_menu) = dirs::data_dir() {
            start_menu.push("Microsoft");
            start_menu.push("Windows");
            start_menu.push("Start Menu");
            start_menu.push("Programs");
            let shortcut_path = start_menu.join("VITL Piano.lnk");
            create_shortcut(&target_exe, &shortcut_path, &target_dir);
        }
    }

    let _ = window.emit("install-progress", serde_json::json!({
        "percent": 96,
        "status": "Registering Windows uninstaller..."
    }));
    register_uninstaller(&target_dir, &target_exe);

    let _ = window.emit("install-progress", serde_json::json!({
        "percent": 100,
        "status": "Installation Complete!"
    }));

    if options.launch_after {
        tokio::time::sleep(Duration::from_millis(400)).await;
        launch_application(&target_exe);
    }

    Ok(target_exe.to_string_lossy().to_string())
}

#[tauri::command]
fn launch_app(target_exe: String) {
    launch_application(Path::new(&target_exe));
}

#[tauri::command]
fn window_action(window: Window, action: String) {
    match action.as_str() {
        "close" => { let _ = window.close(); }
        "minimize" => { let _ = window.minimize(); }
        "drag" => { let _ = window.start_dragging(); }
        _ => {}
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_info,
            check_update,
            select_folder,
            perform_install,
            launch_app,
            window_action
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri setup application");
}
