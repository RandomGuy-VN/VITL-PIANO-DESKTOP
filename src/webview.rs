use anyhow::Result;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Icon, WindowBuilder};
use tracing::info;
use wry::WebViewBuilder;

#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    Close,
    Minimize,
    Maximize,
    Drag,
}

pub struct DesktopWindow;

impl DesktopWindow {
    /// Launches the modern frameless native desktop GUI window with embedded WebView
    pub fn run(url: String, title: &str, width: f64, height: f64) -> Result<()> {
        info!("Launching frameless native Desktop WebView window for {}", url);

        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
        let proxy = event_loop.create_proxy();

        let icon = if let Ok(icon_bytes) = std::fs::read("vitl-brand-logo.png") {
            image_from_bytes(&icon_bytes)
        } else {
            None
        };

        let mut window_builder = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(LogicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(800.0, 550.0))
            .with_decorations(false) // Frameless: remove ugly OS topbar
            .with_resizable(true);

        if let Some(i) = icon {
            window_builder = window_builder.with_window_icon(Some(i));
        }

        let window = window_builder.build(&event_loop)?;

        let proxy_ipc = proxy.clone();
        let ipc_handler = move |req: wry::http::Request<String>| {
            let body = req.body().trim();
            match body {
                "close" => {
                    let _ = proxy_ipc.send_event(UserEvent::Close);
                }
                "minimize" => {
                    let _ = proxy_ipc.send_event(UserEvent::Minimize);
                }
                "maximize" => {
                    let _ = proxy_ipc.send_event(UserEvent::Maximize);
                }
                "drag" => {
                    let _ = proxy_ipc.send_event(UserEvent::Drag);
                }
                _ => {}
            }
        };

        #[cfg(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "ios",
            target_os = "android"
        ))]
        let _webview = WebViewBuilder::new()
            .with_url(&url)
            .with_ipc_handler(ipc_handler)
            .with_devtools(cfg!(debug_assertions))
            .build(&window)?;

        #[cfg(not(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "ios",
            target_os = "android"
        )))]
        let _webview = {
            use tao::platform::unix::WindowExtUnix;
            use wry::WebViewBuilderExtUnix;
            let vbox = window.default_vbox().expect("Failed to get GTK vbox from window");
            WebViewBuilder::new()
                .with_url(&url)
                .with_ipc_handler(ipc_handler)
                .with_devtools(cfg!(debug_assertions))
                .build_gtk(vbox)?
        };

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::UserEvent(UserEvent::Close) => {
                    *control_flow = ControlFlow::Exit;
                }
                Event::UserEvent(UserEvent::Minimize) => {
                    window.set_minimized(true);
                }
                Event::UserEvent(UserEvent::Maximize) => {
                    window.set_maximized(!window.is_maximized());
                }
                Event::UserEvent(UserEvent::Drag) => {
                    let _ = window.drag_window();
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            }
        });
    }
}

fn image_from_bytes(_bytes: &[u8]) -> Option<Icon> {
    let width = 32u32;
    let height = 32u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let is_border = x == 0 || x == width - 1 || y == 0 || y == height - 1;
            if is_border {
                rgba.extend_from_slice(&[74, 107, 77, 255]); // Moss green
            } else {
                rgba.extend_from_slice(&[246, 244, 239, 255]); // Paper
            }
        }
    }
    Icon::from_rgba(rgba, width, height).ok()
}
