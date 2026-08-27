use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::core::config::HotkeyConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    PlayPause,
    Pause,
    Stop,
    SpeedUp,
    SlowDown,
    TransposeUp,
    TransposeDown,
}

pub struct HotkeyManager {
    action_sender: mpsc::UnboundedSender<HotkeyAction>,
    is_running: Arc<AtomicBool>,
}

impl HotkeyManager {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<HotkeyAction>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                action_sender: tx,
                is_running: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    pub fn start_listener(&self, _hotkeys: HotkeyConfig) {
        if self.is_running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        let sender = self.action_sender.clone();
        let running_flag = Arc::clone(&self.is_running);

        #[cfg(target_os = "linux")]
        {
            if Self::start_linux_evdev_listener(sender.clone(), Arc::clone(&running_flag)) {
                info!("Hardware global hotkey listener active via Linux evdev");
                return;
            }
        }

        // Fallback to rdev listener (macOS, Windows, or X11)
        thread::spawn(move || {
            info!("Global hotkey listener started via rdev");

            let mut ctrl_pressed = false;

            let callback = move |event: rdev::Event| {
                if !running_flag.load(Ordering::Relaxed) {
                    return;
                }

                match event.event_type {
                    rdev::EventType::KeyPress(key) => match key {
                        rdev::Key::ControlLeft | rdev::Key::ControlRight => {
                            ctrl_pressed = true;
                        }
                        rdev::Key::F1 => {
                            let _ = sender.send(HotkeyAction::PlayPause);
                        }
                        rdev::Key::F2 => {
                            let _ = sender.send(HotkeyAction::Pause);
                        }
                        rdev::Key::F3 => {
                            let _ = sender.send(HotkeyAction::Stop);
                        }
                        rdev::Key::F4 => {
                            let _ = sender.send(HotkeyAction::SpeedUp);
                        }
                        rdev::Key::F5 => {
                            let _ = sender.send(HotkeyAction::SlowDown);
                        }
                        rdev::Key::UpArrow if ctrl_pressed => {
                            let _ = sender.send(HotkeyAction::TransposeUp);
                        }
                        rdev::Key::DownArrow if ctrl_pressed => {
                            let _ = sender.send(HotkeyAction::TransposeDown);
                        }
                        _ => {}
                    },
                    rdev::EventType::KeyRelease(key) => match key {
                        rdev::Key::ControlLeft | rdev::Key::ControlRight => {
                            ctrl_pressed = false;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            };

            if let Err(e) = rdev::listen(callback) {
                warn!("Global hotkey listen error (rdev): {:?}", e);
            }
        });
    }

    #[cfg(target_os = "linux")]
    fn start_linux_evdev_listener(
        sender: mpsc::UnboundedSender<HotkeyAction>,
        running_flag: Arc<AtomicBool>,
    ) -> bool {
        // Standard Linux input-event-codes.h constants
        const KEY_LEFTCTRL: u16 = 29;
        const KEY_RIGHTCTRL: u16 = 97;
        const KEY_SPACE: u16 = 57;
        const KEY_F1: u16 = 59;
        const KEY_F2: u16 = 60;
        const KEY_F3: u16 = 61;
        const KEY_F4: u16 = 62;
        const KEY_F5: u16 = 63;
        const KEY_UP: u16 = 103;
        const KEY_DOWN: u16 = 108;

        let devices = match evdev::enumerate().collect::<Vec<_>>() {
            devices if !devices.is_empty() => devices,
            _ => return false,
        };

        let mut keyboard_devices = Vec::new();
        for (_, device) in devices {
            let name = device.name().unwrap_or("").to_string();
            // Ignore our own virtual macro keyboard
            if name.contains("VITL") {
                continue;
            }
            if device.supported_keys().is_some() {
                keyboard_devices.push(device);
            }
        }

        if keyboard_devices.is_empty() {
            return false;
        }

        info!(
            "Listening for global hotkeys across {} physical keyboard devices",
            keyboard_devices.len()
        );

        for mut device in keyboard_devices {
            let sender = sender.clone();
            let running_flag = Arc::clone(&running_flag);
            thread::spawn(move || {
                let mut ctrl_pressed = false;
                while running_flag.load(Ordering::Relaxed) {
                    match device.fetch_events() {
                        Ok(events) => {
                            for ev in events {
                                if ev.event_type() == evdev::EventType::KEY {
                                    let code = ev.code();
                                    let val = ev.value(); // 1 = press, 0 = release, 2 = repeat
                                    if val == 1 {
                                        match code {
                                            KEY_LEFTCTRL | KEY_RIGHTCTRL => {
                                                ctrl_pressed = true;
                                            }
                                            KEY_F1 => {
                                                let _ = sender.send(HotkeyAction::PlayPause);
                                            }
                                            KEY_F2 => {
                                                let _ = sender.send(HotkeyAction::Pause);
                                            }
                                            KEY_F3 => {
                                                let _ = sender.send(HotkeyAction::Stop);
                                            }
                                            KEY_F4 => {
                                                let _ = sender.send(HotkeyAction::SpeedUp);
                                            }
                                            KEY_F5 => {
                                                let _ = sender.send(HotkeyAction::SlowDown);
                                            }
                                            KEY_UP if ctrl_pressed => {
                                                let _ = sender.send(HotkeyAction::TransposeUp);
                                            }
                                            KEY_DOWN if ctrl_pressed => {
                                                let _ = sender.send(HotkeyAction::TransposeDown);
                                            }
                                            _ => {}
                                        }
                                    } else if val == 0 {
                                        if code == KEY_LEFTCTRL || code == KEY_RIGHTCTRL {
                                            ctrl_pressed = false;
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            thread::sleep(std::time::Duration::from_millis(50));
                        }
                    }
                }
            });
        }

        true
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}
