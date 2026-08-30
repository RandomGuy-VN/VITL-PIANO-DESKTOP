//! End-to-end validation of the macro backend: keystrokes injected by
//! `InputSimulator` must actually arrive on the kernel input subsystem.
//!
//! The test reads events back from the "VITL Piano Autoplayer" uinput device
//! registered under /dev/input, so it only runs meaningfully on Linux with
//! write access to /dev/uinput (input group).
#![cfg(target_os = "linux")]

use evdev::{EventSummary, KeyCode};
use vitl_piano_desktop::input::InputSimulator;

fn find_virtual_keyboard() -> Option<evdev::Device> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        for (_, device) in evdev::enumerate() {
            if device
                .name()
                .map(|n| n.contains("VITL Piano Autoplayer"))
                .unwrap_or(false)
            {
                return Some(device);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

#[test]
fn macro_backend_injects_keystrokes_via_uinput() {
    // Surface backend errors/warnings from the simulator.
    let _ = tracing_subscriber::fmt::try_init();

    let sim = InputSimulator::new();

    // First tap lazily creates the uinput virtual keyboard.
    sim.tap_piano_key('q', false, false, 0);

    // Allow uinput device to settle in the kernel input tree
    std::thread::sleep(std::time::Duration::from_millis(300));

    let mut reader =
        find_virtual_keyboard().expect("uinput virtual keyboard not found in /dev/input");
    let _ = reader.set_nonblocking(true);

    let sim_clone = sim;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        sim_clone.tap_piano_key('a', true, false, 20);
    });

    let mut saw_press = false;
    let mut saw_release = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline && !(saw_press && saw_release) {
        match reader.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let EventSummary::Key(_, code, value) = ev.destructure() {
                        if code == KeyCode::KEY_A {
                            if value == 1 {
                                saw_press = true;
                            }
                            if value == 0 {
                                saw_release = true;
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("failed to read from virtual keyboard: {}", e),
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(
        saw_press && saw_release,
        "keystroke 'a' (with Shift) was not delivered to the kernel input subsystem \
         (press={saw_press}, release={saw_release})"
    );
}
