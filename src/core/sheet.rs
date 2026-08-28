use anyhow::Result;
use super::song::{NoteEvent, Song, SongSourceType, Track};

pub struct SheetParser;

impl SheetParser {
    /// Maps a Virtual Piano character to a MIDI note number (36 to 96)
    pub fn char_to_midi_note(c: char) -> Option<u8> {
        match c {
            // Low octave (MIDI 36 - 47)
            '1' => Some(36), // C2
            '!' => Some(37), // C#2
            '2' => Some(38), // D2
            '@' => Some(39), // D#2
            '3' => Some(40), // E2
            '4' => Some(41), // F2
            '$' => Some(42), // F#2
            '5' => Some(43), // G2
            '%' => Some(44), // G#2
            '6' => Some(45), // A2
            '^' => Some(46), // A#2
            '7' => Some(47), // B2

            // Mid-low octave (MIDI 48 - 59)
            '8' => Some(48), // C3
            '*' => Some(49), // C#3
            '9' => Some(50), // D3
            '(' => Some(51), // D#3
            '0' => Some(52), // E3
            'q' => Some(53), // F3
            'Q' => Some(54), // F#3
            'w' => Some(55), // G3
            'W' => Some(56), // G#3
            'e' => Some(57), // A3
            'E' => Some(58), // A#3
            'r' => Some(59), // B3

            // Middle octave (MIDI 60 - 71) - C4 = 60 ('t')
            't' => Some(60), // C4 (Middle C)
            'T' => Some(61), // C#4
            'y' => Some(62), // D4
            'Y' => Some(63), // D#4
            'u' => Some(64), // E4
            'i' => Some(65), // F4
            'I' => Some(66), // F#4
            'o' => Some(67), // G4
            'O' => Some(68), // G#4
            'p' => Some(69), // A4
            'P' => Some(70), // A#4
            'a' => Some(71), // B4

            // High octave (MIDI 72 - 83)
            's' => Some(72), // C5
            'S' => Some(73), // C#5
            'd' => Some(74), // D5
            'D' => Some(75), // D#5
            'f' => Some(76), // E5
            'g' => Some(77), // F5
            'G' => Some(78), // F#5
            'h' => Some(79), // G5
            'H' => Some(80), // G#5
            'j' => Some(81), // A5
            'J' => Some(82), // A#5
            'k' => Some(83), // B5

            // Very high octave (MIDI 84 - 96)
            'l' => Some(84), // C6
            'L' => Some(85), // C#6
            'z' => Some(86), // D6
            'Z' => Some(87), // D#6
            'x' => Some(88), // E6
            'c' => Some(89), // F6
            'C' => Some(90), // F#6
            'v' => Some(91), // G6
            'V' => Some(92), // G#6
            'b' => Some(93), // A6
            'B' => Some(94), // A#6
            'n' => Some(95), // B6
            'm' => Some(96), // C7

            _ => None,
        }
    }

    /// Maps a MIDI note (21 to 108) to its Virtual Piano representation
    pub fn midi_note_to_char(note: u8) -> Option<char> {
        match note {
            36 => Some('1'), 37 => Some('!'), 38 => Some('2'), 39 => Some('@'),
            40 => Some('3'), 41 => Some('4'), 42 => Some('$'), 43 => Some('5'),
            44 => Some('%'), 45 => Some('6'), 46 => Some('^'), 47 => Some('7'),
            48 => Some('8'), 49 => Some('*'), 50 => Some('9'), 51 => Some('('),
            52 => Some('0'), 53 => Some('q'), 54 => Some('Q'), 55 => Some('w'),
            56 => Some('W'), 57 => Some('e'), 58 => Some('E'), 59 => Some('r'),
            60 => Some('t'), 61 => Some('T'), 62 => Some('y'), 63 => Some('Y'),
            64 => Some('u'), 65 => Some('i'), 66 => Some('I'), 67 => Some('o'),
            68 => Some('O'), 69 => Some('p'), 70 => Some('P'), 71 => Some('a'),
            72 => Some('s'), 73 => Some('S'), 74 => Some('d'), 75 => Some('D'),
            76 => Some('f'), 77 => Some('g'), 78 => Some('G'), 79 => Some('h'),
            80 => Some('H'), 81 => Some('j'), 82 => Some('J'), 83 => Some('k'),
            84 => Some('l'), 85 => Some('L'), 86 => Some('z'), 87 => Some('Z'),
            88 => Some('x'), 89 => Some('c'), 90 => Some('C'), 91 => Some('v'),
            92 => Some('V'), 93 => Some('b'), 94 => Some('B'), 95 => Some('n'),
            96 => Some('m'),
            _ => None,
        }
    }

    /// Parse Virtual Piano sheet text into a playable `Song`
    pub fn parse_sheet(sheet_text: &str, title: String, default_bpm: Option<f64>) -> Result<Song> {
        let mut bpm = default_bpm.unwrap_or(120.0);
        let mut cleaned_text = String::new();

        // Check for BPM annotations in comments or headers like [bpm: 140] or !140
        for line in sheet_text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }
            let lower = trimmed.to_lowercase();
            if lower.starts_with("bpm:") || lower.starts_with("tempo:") {
                let rest = if lower.starts_with("bpm:") { &trimmed[4..] } else { &trimmed[6..] };
                let num_str: String = rest.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
                if let Ok(parsed_bpm) = num_str.parse::<f64>() {
                    if parsed_bpm > 10.0 && parsed_bpm < 500.0 {
                        bpm = parsed_bpm;
                    }
                }
            } else if trimmed.starts_with('!') && trimmed.len() > 1 && trimmed[1..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                let num_str: String = trimmed[1..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                if let Ok(parsed_bpm) = num_str.parse::<f64>() {
                    if parsed_bpm > 10.0 && parsed_bpm < 500.0 {
                        bpm = parsed_bpm;
                    }
                }
            } else {
                cleaned_text.push_str(trimmed);
                cleaned_text.push(' ');
            }
        }

        // Base timing calculation (quarter note duration)
        let beat_duration_ms = 60_000.0 / bpm;
        let mut step_duration_ms = beat_duration_ms / 2.0; // Eighth note default step

        let mut song = Song::new(title);
        song.bpm = bpm;
        song.tempo_events.push(crate::core::song::TempoEvent {
            time_ms: 0.0,
            bpm,
            us_per_beat: (60_000_000.0 / bpm) as u32,
        });
        song.source_type = SongSourceType::VirtualPianoSheet;

        let mut notes: Vec<NoteEvent> = Vec::new();
        let mut current_ms = 0.0;
        let chars: Vec<char> = cleaned_text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            match c {
                '[' | '{' => {
                    // Check if this bracket contains a dynamic BPM / tempo tag (e.g. [bpm:140] or [tempo=120])
                    let close_char = if c == '[' { ']' } else { '}' };
                    let mut tag_content = String::new();
                    let mut lookahead = i + 1;
                    while lookahead < chars.len() && chars[lookahead] != close_char {
                        tag_content.push(chars[lookahead]);
                        lookahead += 1;
                    }
                    let tag_lower = tag_content.to_lowercase();
                    if tag_lower.starts_with("bpm") || tag_lower.starts_with("tempo") {
                        let num_str: String = tag_content.chars().filter(|ch| ch.is_ascii_digit() || *ch == '.').collect();
                        if let Ok(parsed_bpm) = num_str.parse::<f64>() {
                            if parsed_bpm > 10.0 && parsed_bpm < 500.0 {
                                bpm = parsed_bpm;
                                step_duration_ms = (60_000.0 / bpm) / 2.0;
                                song.tempo_events.push(crate::core::song::TempoEvent {
                                    time_ms: current_ms,
                                    bpm,
                                    us_per_beat: (60_000_000.0 / bpm) as u32,
                                });
                            }
                        }
                        i = lookahead;
                    } else {
                        // Chord start
                        let mut chord_notes = Vec::new();
                        i += 1;
                        while i < chars.len() && chars[i] != close_char {
                            if let Some(midi_note) = Self::char_to_midi_note(chars[i]) {
                                chord_notes.push(midi_note);
                            }
                            i += 1;
                        }

                        for &note_num in &chord_notes {
                            notes.push(NoteEvent {
                                note: note_num,
                                velocity: 95,
                                start_ms: current_ms,
                                duration_ms: step_duration_ms * 1.5,
                                track: 0,
                                channel: 0,
                            });
                        }

                        current_ms += step_duration_ms;
                    }
                }
                ' ' => {
                    // Space = small pause / step
                    current_ms += step_duration_ms * 0.5;
                }
                '|' => {
                    // Measure separator = full beat pause
                    current_ms += step_duration_ms;
                }
                '-' | '_' => {
                    // Sustained extension
                    current_ms += step_duration_ms * 0.5;
                }
                '\n' | '\r' | '\t' => {
                    current_ms += step_duration_ms;
                }
                _ => {
                    if let Some(midi_note) = Self::char_to_midi_note(c) {
                        notes.push(NoteEvent {
                            note: midi_note,
                            velocity: 90,
                            start_ms: current_ms,
                            duration_ms: step_duration_ms * 1.2,
                            track: 0,
                            channel: 0,
                        });
                        current_ms += step_duration_ms;
                    }
                }
            }

            i += 1;
        }

        if !notes.is_empty() {
            song.tracks.push(Track {
                name: "Virtual Piano Sheet".to_string(),
                channel: 0,
                notes,
                is_drum: false,
            });
        }

        song.finalize();
        Ok(song)
    }

    /// Converts any Song into a formatted Virtual Piano sheet
    pub fn song_to_sheet(song: &Song) -> String {
        let all_notes = song.all_notes_flattened();
        if all_notes.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        output.push_str(&format!("# Title: {}\n", song.title));
        output.push_str(&format!("# BPM: {}\n\n", song.bpm.round()));

        // Group notes that start within a 25ms time window as chords
        let mut chord_groups: Vec<(f64, Vec<char>)> = Vec::new();
        for note in &all_notes {
            if let Some(vp_char) = Self::midi_note_to_char(note.note) {
                if let Some((anchor_time, chars)) = chord_groups.last_mut() {
                    if (note.start_ms - *anchor_time).abs() < 25.0 {
                        if !chars.contains(&vp_char) {
                            chars.push(vp_char);
                        }
                        continue;
                    }
                }
                chord_groups.push((note.start_ms, vec![vp_char]));
            }
        }

        let mut last_time = 0.0;
        let mut notes_in_line = 0;

        for (time_ms, chars) in chord_groups {
            let delta = time_ms - last_time;
            if delta > 800.0 {
                output.push_str(" | ");
            } else if delta > 400.0 {
                output.push(' ');
            }

            if chars.len() > 1 {
                output.push('[');
                for c in chars {
                    output.push(c);
                }
                output.push(']');
            } else if let Some(&c) = chars.first() {
                output.push(c);
            }

            output.push(' ');
            notes_in_line += 1;
            if notes_in_line >= 16 {
                output.push('\n');
                notes_in_line = 0;
            }

            last_time = time_ms;
        }

        output
    }
}
