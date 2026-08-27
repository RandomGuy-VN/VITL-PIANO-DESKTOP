pub mod hotkeys;
pub mod mapping;
pub mod simulator;

pub use hotkeys::{HotkeyAction, HotkeyManager};
pub use mapping::{KeyMappingEngine, PianoKeyMap};
pub use simulator::InputSimulator;
