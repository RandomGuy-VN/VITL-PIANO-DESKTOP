use std::sync::Mutex;
use tauri::State;

struct BackendPort(Mutex<u16>);

#[tauri::command]
fn get_backend_port(port: State<'_, BackendPort>) -> u16 {
    *port.0.lock().unwrap()
}

#[tauri::command]
fn window_action(window: tauri::Window, action: String) {
    match action.as_str() {
        "close" => { let _ = window.close(); }
        "minimize" => { let _ = window.minimize(); }
        "maximize" => {
            if let Ok(is_max) = window.is_maximized() {
                let _ = if is_max { window.unmaximize() } else { window.maximize() };
            }
        }
        "drag" => { let _ = window.start_dragging(); }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<u16>();
    
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            if let Err(e) = vitl_piano_desktop::run_backend(4242, None, ready_tx).await {
                eprintln!("Backend error: {:?}", e);
            }
        });
    });

    let port = ready_rx.recv().unwrap();

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .manage(BackendPort(Mutex::new(port)))
        .invoke_handler(tauri::generate_handler![get_backend_port, window_action])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

