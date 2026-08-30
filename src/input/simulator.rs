use parking_lot::Mutex;
use rdev::{simulate, EventType, Key};
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Backend used for actual OS-level keystroke injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InjectionBackend {
    /// evdev uinput virtual keyboard — works on Wayland and X11 (Linux).
    #[cfg(target_os = "linux")]
    UInput,
    /// rdev XTest simulation — legacy/fallback path.
    RDev,
}

/// Logical vkey constants passed to the native C++ backend
/// (`native/macro_backend.cpp`, enum VitlKey — values must stay in sync).
#[cfg(target_os = "linux")]
mod native_keys {
    pub const KEY_A: i32 = 0;
    pub const KEY_B: i32 = 1;
    pub const KEY_C: i32 = 2;
    pub const KEY_D: i32 = 3;
    pub const KEY_E: i32 = 4;
    pub const KEY_F: i32 = 5;
    pub const KEY_G: i32 = 6;
    pub const KEY_H: i32 = 7;
    pub const KEY_I: i32 = 8;
    pub const KEY_J: i32 = 9;
    pub const KEY_K: i32 = 10;
    pub const KEY_L: i32 = 11;
    pub const KEY_M: i32 = 12;
    pub const KEY_N: i32 = 13;
    pub const KEY_O: i32 = 14;
    pub const KEY_P: i32 = 15;
    pub const KEY_Q: i32 = 16;
    pub const KEY_R: i32 = 17;
    pub const KEY_S: i32 = 18;
    pub const KEY_T: i32 = 19;
    pub const KEY_U: i32 = 20;
    pub const KEY_V: i32 = 21;
    pub const KEY_W: i32 = 22;
    pub const KEY_X: i32 = 23;
    pub const KEY_Y: i32 = 24;
    pub const KEY_Z: i32 = 25;
    pub const KEY_1: i32 = 26;
    pub const KEY_2: i32 = 27;
    pub const KEY_3: i32 = 28;
    pub const KEY_4: i32 = 29;
    pub const KEY_5: i32 = 30;
    pub const KEY_6: i32 = 31;
    pub const KEY_7: i32 = 32;
    pub const KEY_8: i32 = 33;
    pub const KEY_9: i32 = 34;
    pub const KEY_0: i32 = 35;
    pub const KEY_SPACE: i32 = 36;
    pub const KEY_LEFTSHIFT: i32 = 37;
    pub const KEY_LEFTCTRL: i32 = 38;
    pub const KEY_LEFTALT: i32 = 39;
}

/// Lifecycle of the native backend within this process instance.
#[cfg(target_os = "linux")]
const NATIVE_UNINIT: u8 = 0;
#[cfg(target_os = "linux")]
const NATIVE_READY: u8 = 1;
#[cfg(target_os = "linux")]
const NATIVE_FAILED: u8 = 2;

pub struct InputSimulator {
    held_keys: Arc<Mutex<HashSet<Key>>>,
    os_lock: Mutex<()>,
    /// State of the native C++ injection backend (Linux).
    #[cfg(target_os = "linux")]
    native_state: std::sync::atomic::AtomicU8,
}

// Native C++ backend entry points (compiled by build.rs, linked statically).
#[cfg(target_os = "linux")]
#[link(name = "vitl_macro", kind = "static")]
extern "C" {
    fn vitl_macro_init() -> i32;
    fn vitl_macro_write(key: i32, value: i32) -> i32;
}

// Ensures libstdc++ is linked (required by <mutex>/exception tables in
// native/macro_backend.cpp). Must come after the vitl_macro archive.
#[cfg(target_os = "linux")]
#[link(name = "stdc++")]
extern "C" {}

impl InputSimulator {
    pub fn new() -> Self {
        let sim = Self {
            held_keys: Arc::new(Mutex::new(HashSet::new())),
            os_lock: Mutex::new(()),
            #[cfg(target_os = "linux")]
            native_state: std::sync::atomic::AtomicU8::new(NATIVE_UNINIT),
        };
        #[cfg(target_os = "linux")]
        sim.ensure_native_backend();
        sim
    }

    /// Resolves the active injection backend for this platform.
    fn resolve_backend(&self) -> InjectionBackend {
        #[cfg(target_os = "linux")]
        match self.native_state.load(std::sync::atomic::Ordering::Relaxed) {
            NATIVE_READY => InjectionBackend::UInput,
            NATIVE_FAILED => InjectionBackend::RDev,
            _ => {
                if self.ensure_native_backend() {
                    InjectionBackend::UInput
                } else {
                    InjectionBackend::RDev
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        InjectionBackend::RDev
    }

    /// Lazily initialises the native C++ uinput backend (Linux only).
    /// Idempotent; returns true when the backend is ready.
    #[cfg(target_os = "linux")]
    fn ensure_native_backend(&self) -> bool {
        let rc = unsafe { vitl_macro_init() };
        if rc == 0 {
            info!("Keystroke injection backend ready: native C++ uinput virtual keyboard");
            self.native_state
                .store(NATIVE_READY, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            error!(
                "Native macro backend init failed (rc={}) — falling back to rdev XTest \
                 (keystrokes may not reach Wayland-native windows)",
                rc
            );
            self.native_state
                .store(NATIVE_FAILED, std::sync::atomic::Ordering::Relaxed);
            false
        }
    }

    /// Converts a standard character into an `rdev::Key`
    pub fn char_to_rdev_key(c: char) -> Option<Key> {
        let lower = c.to_ascii_lowercase();
        match lower {
            '1' => Some(Key::Num1),
            '2' => Some(Key::Num2),
            '3' => Some(Key::Num3),
            '4' => Some(Key::Num4),
            '5' => Some(Key::Num5),
            '6' => Some(Key::Num6),
            '7' => Some(Key::Num7),
            '8' => Some(Key::Num8),
            '9' => Some(Key::Num9),
            '0' => Some(Key::Num0),
            'a' => Some(Key::KeyA),
            'b' => Some(Key::KeyB),
            'c' => Some(Key::KeyC),
            'd' => Some(Key::KeyD),
            'e' => Some(Key::KeyE),
            'f' => Some(Key::KeyF),
            'g' => Some(Key::KeyG),
            'h' => Some(Key::KeyH),
            'i' => Some(Key::KeyI),
            'j' => Some(Key::KeyJ),
            'k' => Some(Key::KeyK),
            'l' => Some(Key::KeyL),
            'm' => Some(Key::KeyM),
            'n' => Some(Key::KeyN),
            'o' => Some(Key::KeyO),
            'p' => Some(Key::KeyP),
            'q' => Some(Key::KeyQ),
            'r' => Some(Key::KeyR),
            's' => Some(Key::KeyS),
            't' => Some(Key::KeyT),
            'u' => Some(Key::KeyU),
            'v' => Some(Key::KeyV),
            'w' => Some(Key::KeyW),
            'x' => Some(Key::KeyX),
            'y' => Some(Key::KeyY),
            'z' => Some(Key::KeyZ),
            ' ' => Some(Key::Space),
            _ => None,
        }
    }

    /// Maps a logical `rdev::Key` to a native backend vkey constant.
    #[cfg(target_os = "linux")]
    fn native_vkey(key: Key) -> Option<i32> {
        use native_keys::*;
        Some(match key {
            Key::Num1 => KEY_1,
            Key::Num2 => KEY_2,
            Key::Num3 => KEY_3,
            Key::Num4 => KEY_4,
            Key::Num5 => KEY_5,
            Key::Num6 => KEY_6,
            Key::Num7 => KEY_7,
            Key::Num8 => KEY_8,
            Key::Num9 => KEY_9,
            Key::Num0 => KEY_0,
            Key::KeyA => KEY_A,
            Key::KeyB => KEY_B,
            Key::KeyC => KEY_C,
            Key::KeyD => KEY_D,
            Key::KeyE => KEY_E,
            Key::KeyF => KEY_F,
            Key::KeyG => KEY_G,
            Key::KeyH => KEY_H,
            Key::KeyI => KEY_I,
            Key::KeyJ => KEY_J,
            Key::KeyK => KEY_K,
            Key::KeyL => KEY_L,
            Key::KeyM => KEY_M,
            Key::KeyN => KEY_N,
            Key::KeyO => KEY_O,
            Key::KeyP => KEY_P,
            Key::KeyQ => KEY_Q,
            Key::KeyR => KEY_R,
            Key::KeyS => KEY_S,
            Key::KeyT => KEY_T,
            Key::KeyU => KEY_U,
            Key::KeyV => KEY_V,
            Key::KeyW => KEY_W,
            Key::KeyX => KEY_X,
            Key::KeyY => KEY_Y,
            Key::KeyZ => KEY_Z,
            Key::Space => KEY_SPACE,
            Key::ShiftLeft => KEY_LEFTSHIFT,
            Key::ControlLeft => KEY_LEFTCTRL,
            Key::Alt => KEY_LEFTALT,
            _ => return None,
        })
    }

    /// Sends a single KEY event (value: 1 = press, 0 = release) through the
    /// active backend. Returns true on success.
    fn send_key_event(&self, key: Key, value: i32) -> bool {
        match self.resolve_backend() {
            #[cfg(target_os = "linux")]
            InjectionBackend::UInput => self.uinput_emit(key, value),
            #[allow(unreachable_patterns)]
            _ => {
                // Fallback: rdev XTest simulation
                let event_type = if value == 1 {
                    EventType::KeyPress(key)
                } else {
                    EventType::KeyRelease(key)
                };
                match simulate(&event_type) {
                    Ok(()) => true,
                    Err(e) => {
                        error!("Failed to inject key {:?} (value={}): {:?}", key, value, e);
                        false
                    }
                }
            }
        }
    }

    /// Emits one KEY event through the native C++ backend
    /// (presses/releases are SYN_REPORT-synchronised inside C++).
    #[cfg(target_os = "linux")]
    fn uinput_emit(&self, key: Key, value: i32) -> bool {
        let Some(vkey) = Self::native_vkey(key) else {
            error!("No native mapping for key {:?}", key);
            return false;
        };
        let rc = unsafe { vitl_macro_write(vkey, value) };
        if rc != 0 {
            error!(
                "Failed to inject {:?} (value={}) via native backend: rc={}",
                key, value, rc
            );
            return false;
        }
        debug!(?key, value, "injected via native C++ uinput backend");
        true
    }

    /// Low level key down simulation
    pub fn key_down(&self, key: Key) {
        if self.send_key_event(key, 1) {
            self.held_keys.lock().insert(key);
        }
    }

    /// Low level key up simulation
    pub fn key_up(&self, key: Key) {
        if self.send_key_event(key, 0) || self.held_keys.lock().contains(&key) {
            self.held_keys.lock().remove(&key);
        }
    }

    /// Press and release a piano key with modifier handling (Shift, Ctrl)
    pub fn tap_piano_key(
        &self,
        key_char: char,
        is_shift: bool,
        is_ctrl: bool,
        hold_duration_ms: u64,
    ) {
        let _lock = self.os_lock.lock();
        if let Some(rdev_key) = Self::char_to_rdev_key(key_char) {
            if is_shift {
                self.key_down(Key::ShiftLeft);
            }
            if is_ctrl {
                self.key_down(Key::ControlLeft);
            }

            self.key_down(rdev_key);

            if hold_duration_ms > 0 {
                thread::sleep(Duration::from_millis(hold_duration_ms));
            } else {
                thread::sleep(Duration::from_micros(500));
            }

            self.key_up(rdev_key);

            if is_ctrl {
                self.key_up(Key::ControlLeft);
            }
            if is_shift {
                self.key_up(Key::ShiftLeft);
            }
        }
    }

    /// Press and release multiple piano keys accurately grouping by modifier state
    pub fn tap_chord(&self, keys: Vec<(char, bool, bool)>, _hold_duration_ms: u64) {
        let _lock = self.os_lock.lock();
        if keys.is_empty() {
            return;
        }

        let mut unshifted = Vec::new();
        let mut shifted = Vec::new();
        let mut ctrl_only = Vec::new();
        let mut ctrl_shift = Vec::new();

        for (c, shift, ctrl) in keys {
            if let Some(rk) = Self::char_to_rdev_key(c) {
                if ctrl && shift {
                    ctrl_shift.push(rk);
                } else if ctrl {
                    ctrl_only.push(rk);
                } else if shift {
                    shifted.push(rk);
                } else {
                    unshifted.push(rk);
                }
            }
        }

        // 1. Fire unshifted keys with a clean, instant microsecond tap
        if !unshifted.is_empty() {
            for &k in &unshifted {
                self.key_down(k);
            }
            thread::sleep(Duration::from_micros(800));
            for &k in &unshifted {
                self.key_up(k);
            }
        }

        // 2. Fire shifted keys with Shift strictly scoped to this group
        if !shifted.is_empty() {
            self.key_down(Key::ShiftLeft);
            thread::sleep(Duration::from_micros(400));
            for &k in &shifted {
                self.key_down(k);
            }
            thread::sleep(Duration::from_micros(800));
            for &k in &shifted {
                self.key_up(k);
            }
            thread::sleep(Duration::from_micros(400));
            self.key_up(Key::ShiftLeft);
        }

        // 3. Fire Ctrl-only keys with Ctrl strictly scoped to this group
        if !ctrl_only.is_empty() {
            self.key_down(Key::ControlLeft);
            thread::sleep(Duration::from_micros(400));
            for &k in &ctrl_only {
                self.key_down(k);
            }
            thread::sleep(Duration::from_micros(800));
            for &k in &ctrl_only {
                self.key_up(k);
            }
            thread::sleep(Duration::from_micros(400));
            self.key_up(Key::ControlLeft);
        }

        // 4. Fire Ctrl+Shift keys
        if !ctrl_shift.is_empty() {
            self.key_down(Key::ControlLeft);
            self.key_down(Key::ShiftLeft);
            thread::sleep(Duration::from_micros(400));
            for &k in &ctrl_shift {
                self.key_down(k);
            }
            thread::sleep(Duration::from_micros(800));
            for &k in &ctrl_shift {
                self.key_up(k);
            }
            thread::sleep(Duration::from_micros(400));
            self.key_up(Key::ShiftLeft);
            self.key_up(Key::ControlLeft);
        }
    }

    /// Send velocity modifier key (Alt + Key)
    pub fn send_velocity(&self, vel_char: char) {
        let _lock = self.os_lock.lock();
        if let Some(rdev_key) = Self::char_to_rdev_key(vel_char) {
            self.key_down(Key::Alt);
            self.key_down(rdev_key);
            thread::sleep(Duration::from_micros(500));
            self.key_up(rdev_key);
            self.key_up(Key::Alt);
        }
    }

    /// Set sustain pedal (Spacebar)
    pub fn set_sustain(&self, is_down: bool) {
        if is_down {
            self.key_down(Key::Space);
        } else {
            self.key_up(Key::Space);
        }
    }

    /// Release all currently held keys (clean state)
    pub fn release_all(&self) {
        let _lock = self.os_lock.lock();
        let mut held = self.held_keys.lock();
        for key in held.drain() {
            self.send_key_event(key, 0);
        }
    }

    /// Check if a key is currently held down
    pub fn is_key_held(&self, key: Key) -> bool {
        self.held_keys.lock().contains(&key)
    }

    /// Auto-focus the target game window (e.g. "Roblox")
    pub fn auto_focus_window(title_hint: &str) {
        info!("Attempting to auto-focus window matching: {}", title_hint);

        #[cfg(target_os = "linux")]
        {
            // Try xdotool or wmctrl
            let _ = std::process::Command::new("xdotool")
                .args(["search", "--name", title_hint, "windowactivate"])
                .output();
            let _ = std::process::Command::new("wmctrl")
                .args(["-a", title_hint])
                .output();
        }

        #[cfg(target_os = "windows")]
        {
            // Windows focus via powershell or native call
            let script = format!(
                "$ws = New-Object -ComObject WScript.Shell; $ws.AppActivate('{}')",
                title_hint
            );
            let _ = std::process::Command::new("powershell")
                .args(["-Command", &script])
                .output();
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "tell application \"System Events\" to set frontmost of first process whose name contains \"{}\" to true",
                title_hint
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output();
        }
    }
}
