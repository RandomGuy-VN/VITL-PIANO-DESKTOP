use crate::core::config::KeyboardLayoutType;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PianoKeyMap {
    pub key_char: char,
    pub is_shift: bool,
    pub is_ctrl: bool,
}

pub struct KeyMappingEngine {
    map_61: HashMap<u8, PianoKeyMap>,
    map_low: HashMap<u8, PianoKeyMap>,
    map_high: HashMap<u8, PianoKeyMap>,
    drums_map: HashMap<u8, char>,
    velocity_map: Vec<(u8, char)>,
}

impl KeyMappingEngine {
    pub fn new(layout: KeyboardLayoutType) -> Self {
        let mut engine = Self {
            map_61: HashMap::new(),
            map_low: HashMap::new(),
            map_high: HashMap::new(),
            drums_map: HashMap::new(),
            velocity_map: Vec::new(),
        };

        engine.init_default_maps();
        if layout != KeyboardLayoutType::QwertyUS {
            engine.apply_layout_remap(layout);
        }
        engine
    }

    fn init_default_maps(&mut self) {
        // Standard 61-key Virtual Piano Mapping (MIDI 36 to 96)
        let raw_61 = [
            (36, '1', false),
            (37, '1', true),
            (38, '2', false),
            (39, '2', true),
            (40, '3', false),
            (41, '4', false),
            (42, '4', true),
            (43, '5', false),
            (44, '5', true),
            (45, '6', false),
            (46, '6', true),
            (47, '7', false),
            (48, '8', false),
            (49, '8', true),
            (50, '9', false),
            (51, '9', true),
            (52, '0', false),
            (53, 'q', false),
            (54, 'q', true),
            (55, 'w', false),
            (56, 'w', true),
            (57, 'e', false),
            (58, 'e', true),
            (59, 'r', false),
            (60, 't', false),
            (61, 't', true),
            (62, 'y', false),
            (63, 'y', true),
            (64, 'u', false),
            (65, 'i', false),
            (66, 'i', true),
            (67, 'o', false),
            (68, 'o', true),
            (69, 'p', false),
            (70, 'p', true),
            (71, 'a', false),
            (72, 's', false),
            (73, 's', true),
            (74, 'd', false),
            (75, 'd', true),
            (76, 'f', false),
            (77, 'g', false),
            (78, 'g', true),
            (79, 'h', false),
            (80, 'h', true),
            (81, 'j', false),
            (82, 'j', true),
            (83, 'k', false),
            (84, 'l', false),
            (85, 'l', true),
            (86, 'z', false),
            (87, 'z', true),
            (88, 'x', false),
            (89, 'c', false),
            (90, 'c', true),
            (91, 'v', false),
            (92, 'v', true),
            (93, 'b', false),
            (94, 'b', true),
            (95, 'n', false),
            (96, 'm', false),
        ];

        for &(note, c, is_shift) in &raw_61 {
            self.map_61.insert(
                note,
                PianoKeyMap {
                    key_char: c,
                    is_shift,
                    is_ctrl: false,
                },
            );
        }

        // 88-Key Low Notes (MIDI 21 to 35: A0 to B1) -> Ctrl + Key
        let raw_low = [
            (21, '6', false), // A0 -> Ctrl + 6
            (22, '6', true),  // A#0 -> Ctrl + Shift + 6
            (23, '7', false), // B0 -> Ctrl + 7
            (24, '1', false), // C1 -> Ctrl + 1
            (25, '1', true),  // C#1 -> Ctrl + Shift + 1
            (26, '2', false), // D1 -> Ctrl + 2
            (27, '2', true),  // D#1 -> Ctrl + Shift + 2
            (28, '3', false), // E1 -> Ctrl + 3
            (29, '4', false), // F1 -> Ctrl + 4
            (30, '4', true),  // F#1 -> Ctrl + Shift + 4
            (31, '5', false), // G1 -> Ctrl + 5
            (32, '5', true),  // G#1 -> Ctrl + Shift + 5
            (33, '6', false), // A1 -> Ctrl + 6
            (34, '6', true),  // A#1 -> Ctrl + Shift + 6
            (35, '7', false), // B1 -> Ctrl + 7
        ];

        for &(note, c, is_shift) in &raw_low {
            self.map_low.insert(
                note,
                PianoKeyMap {
                    key_char: c,
                    is_shift,
                    is_ctrl: true,
                },
            );
        }

        // 88-Key High Notes (MIDI 97 to 108: C#7 to C8) -> Ctrl + Key
        let raw_high = [
            (97, '8', true),   // C#7 -> Ctrl + Shift + 8
            (98, '9', false),  // D7 -> Ctrl + 9
            (99, '9', true),   // D#7 -> Ctrl + Shift + 9
            (100, '0', false), // E7 -> Ctrl + 0
            (101, 'q', false), // F7 -> Ctrl + q
            (102, 'q', true),  // F#7 -> Ctrl + Shift + q
            (103, 'w', false), // G7 -> Ctrl + w
            (104, 'w', true),  // G#7 -> Ctrl + Shift + w
            (105, 'e', false), // A7 -> Ctrl + e
            (106, 'e', true),  // A#7 -> Ctrl + Shift + e
            (107, 'r', false), // B7 -> Ctrl + r
            (108, 't', false), // C8 -> Ctrl + t
        ];

        for &(note, c, is_shift) in &raw_high {
            self.map_high.insert(
                note,
                PianoKeyMap {
                    key_char: c,
                    is_shift,
                    is_ctrl: true,
                },
            );
        }

        // Velocity mapping
        self.velocity_map = vec![
            (0, '1'),
            (4, '2'),
            (8, '3'),
            (12, '4'),
            (16, '5'),
            (20, '6'),
            (24, '7'),
            (28, '8'),
            (32, '9'),
            (36, '0'),
            (40, 'q'),
            (44, 'w'),
            (48, 'e'),
            (52, 'r'),
            (56, 't'),
            (60, 'y'),
            (64, 'u'),
            (68, 'i'),
            (72, 'o'),
            (76, 'p'),
            (80, 'a'),
            (84, 's'),
            (88, 'd'),
            (92, 'f'),
            (96, 'g'),
            (100, 'h'),
            (104, 'j'),
            (108, 'k'),
            (112, 'l'),
            (116, 'z'),
            (120, 'x'),
            (124, 'c'),
        ];

        // Drums mapping (GM Drum Kit)
        self.drums_map.insert(35, 'b'); // Acoustic Bass Drum
        self.drums_map.insert(36, 'n'); // Bass Drum 1
        self.drums_map.insert(37, 's'); // Side Stick
        self.drums_map.insert(38, 'c'); // Acoustic Snare
        self.drums_map.insert(40, 'v'); // Electric Snare
        self.drums_map.insert(42, 'x'); // Closed Hi-Hat
        self.drums_map.insert(44, 'm'); // Pedal Hi-Hat
        self.drums_map.insert(46, 'z'); // Open Hi-Hat
        self.drums_map.insert(48, 'd'); // Low-Mid Tom
        self.drums_map.insert(50, 'f'); // High-Mid Tom
        self.drums_map.insert(49, 'r'); // Crash Cymbal 1
        self.drums_map.insert(55, 'e'); // Splash Cymbal
        self.drums_map.insert(51, 'u'); // Ride Cymbal 1
        self.drums_map.insert(53, 'i'); // Ride Bell
        self.drums_map.insert(39, 'a'); // Hand Clap
        self.drums_map.insert(52, 'o'); // Chinese Cymbal
    }

    fn apply_layout_remap(&mut self, layout: KeyboardLayoutType) {
        let remap_char = |c: char| -> char {
            match layout {
                KeyboardLayoutType::AzertyFR => match c {
                    'a' => 'q',
                    'q' => 'a',
                    'z' => 'w',
                    'w' => 'z',
                    'm' => ',',
                    _ => c,
                },
                KeyboardLayoutType::QwertzDE => match c {
                    'y' => 'z',
                    'z' => 'y',
                    _ => c,
                },
                KeyboardLayoutType::Dvorak => match c {
                    'q' => '\'',
                    'w' => ',',
                    'e' => '.',
                    'r' => 'p',
                    't' => 'y',
                    'y' => 'f',
                    'u' => 'g',
                    'i' => 'c',
                    'o' => 'r',
                    'p' => 'l',
                    'a' => 'a',
                    's' => 'o',
                    'd' => 'e',
                    'f' => 'u',
                    'g' => 'i',
                    'h' => 'd',
                    'j' => 'h',
                    'k' => 't',
                    'l' => 'n',
                    'z' => ';',
                    'x' => 'q',
                    'c' => 'j',
                    'v' => 'k',
                    'b' => 'x',
                    'n' => 'b',
                    'm' => 'm',
                    _ => c,
                },
                _ => c,
            }
        };

        for item in self.map_61.values_mut() {
            item.key_char = remap_char(item.key_char);
        }
        for item in self.map_low.values_mut() {
            item.key_char = remap_char(item.key_char);
        }
        for item in self.map_high.values_mut() {
            item.key_char = remap_char(item.key_char);
        }
    }

    /// Find key mapping for note
    pub fn get_piano_key(&self, note: u8, allow_88: bool) -> Option<PianoKeyMap> {
        if let Some(map) = self.map_61.get(&note) {
            return Some(map.clone());
        }

        if allow_88 {
            if let Some(map) = self.map_low.get(&note) {
                return Some(map.clone());
            }
            if let Some(map) = self.map_high.get(&note) {
                return Some(map.clone());
            }
        }

        // Octave folding for standard 61-key piano (Roblox / Virtual Piano)
        let mut folded = note;
        while folded < 36 {
            folded += 12;
        }
        while folded > 96 {
            folded -= 12;
        }
        if let Some(map) = self.map_61.get(&folded) {
            return Some(map.clone());
        }

        None
    }

    /// Find drum macro key
    pub fn get_drum_key(&self, note: u8) -> Option<char> {
        self.drums_map.get(&note).copied()
    }

    /// Find velocity key character
    pub fn get_velocity_key(&self, velocity: u8) -> char {
        let mut best_char = '1';
        for &(threshold, c) in &self.velocity_map {
            if velocity >= threshold {
                best_char = c;
            } else {
                break;
            }
        }
        best_char
    }
}
