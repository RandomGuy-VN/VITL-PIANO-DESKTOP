use anyhow::Result;

use super::song::{NoteEvent, Song, SongSourceType, Track};

const VIRTUAL_PIANO_NOTE_CHARS: [(u8, char); 61] = [
    (36, '1'),
    (37, '!'),
    (38, '2'),
    (39, '@'),
    (40, '3'),
    (41, '4'),
    (42, '$'),
    (43, '5'),
    (44, '%'),
    (45, '6'),
    (46, '^'),
    (47, '7'),
    (48, '8'),
    (49, '*'),
    (50, '9'),
    (51, '('),
    (52, '0'),
    (53, 'q'),
    (54, 'Q'),
    (55, 'w'),
    (56, 'W'),
    (57, 'e'),
    (58, 'E'),
    (59, 'r'),
    (60, 't'),
    (61, 'T'),
    (62, 'y'),
    (63, 'Y'),
    (64, 'u'),
    (65, 'i'),
    (66, 'I'),
    (67, 'o'),
    (68, 'O'),
    (69, 'p'),
    (70, 'P'),
    (71, 'a'),
    (72, 's'),
    (73, 'S'),
    (74, 'd'),
    (75, 'D'),
    (76, 'f'),
    (77, 'g'),
    (78, 'G'),
    (79, 'h'),
    (80, 'H'),
    (81, 'j'),
    (82, 'J'),
    (83, 'k'),
    (84, 'l'),
    (85, 'L'),
    (86, 'z'),
    (87, 'Z'),
    (88, 'x'),
    (89, 'c'),
    (90, 'C'),
    (91, 'v'),
    (92, 'V'),
    (93, 'b'),
    (94, 'B'),
    (95, 'n'),
    (96, 'm'),
];

fn metadata_value<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    let prefix = text.get(..label.len())?;
    if !prefix.eq_ignore_ascii_case(label) {
        return None;
    }

    let remainder = text.get(label.len()..)?.trim_start();
    let separator = remainder.chars().next()?;
    if separator != ':' && separator != '=' {
        return None;
    }
    Some(remainder[separator.len_utf8()..].trim())
}

fn parse_bpm_value(value: &str) -> Option<f64> {
    let number: String = value
        .chars()
        .skip_while(|character| !character.is_ascii_digit() && *character != '.')
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    let bpm = number.parse::<f64>().ok()?;
    (bpm > 10.0 && bpm < 500.0).then_some(bpm)
}

fn bpm_to_us_per_beat(bpm: f64) -> u32 {
    (60_000_000.0 / bpm).round().max(1.0) as u32
}

pub struct SheetParser;

impl SheetParser {
    /// Maps a standard 61-key Virtual Piano character to MIDI 36-96.
    pub fn char_to_midi_note(c: char) -> Option<u8> {
        VIRTUAL_PIANO_NOTE_CHARS
            .iter()
            .find_map(|&(note, character)| (character == c).then_some(note))
    }

    /// Maps MIDI 36-96 to its standard 61-key Virtual Piano character.
    ///
    /// The sheet grammar currently has no reversible Ctrl-modifier syntax for
    /// MIDI 21-35 or 97-108, so those notes deliberately return `None`.
    pub fn midi_note_to_char(note: u8) -> Option<char> {
        VIRTUAL_PIANO_NOTE_CHARS
            .iter()
            .find_map(|&(mapped_note, character)| (mapped_note == note).then_some(character))
    }

    /// Parse Virtual Piano sheet text into a playable `Song`
    pub fn parse_sheet(sheet_text: &str, title: String, default_bpm: Option<f64>) -> Result<Song> {
        let mut bpm = default_bpm.unwrap_or(120.0).max(1.0);
        let mut parsed_title = title;
        let mut cleaned_text = String::new();

        // Parse metadata from both comment headers and bare headers before
        // discarding comment content from the playable notation.
        for line in sheet_text.lines() {
            let trimmed = line.trim();
            let (metadata_text, is_comment) = if let Some(comment) = trimmed.strip_prefix("//") {
                (comment.trim(), true)
            } else if let Some(comment) = trimmed.strip_prefix('#') {
                (comment.trim(), true)
            } else {
                (trimmed, false)
            };

            let mut is_metadata = false;
            if let Some(value) = metadata_value(metadata_text, "title") {
                if !value.is_empty() {
                    parsed_title = value.to_string();
                }
                is_metadata = true;
            }
            if let Some(value) = metadata_value(metadata_text, "bpm")
                .or_else(|| metadata_value(metadata_text, "tempo"))
            {
                if let Some(parsed_bpm) = parse_bpm_value(value) {
                    bpm = parsed_bpm;
                }
                is_metadata = true;
            }

            if is_comment || is_metadata {
                continue;
            }

            if trimmed.starts_with('!')
                && trimmed.len() > 1
                && trimmed[1..]
                    .chars()
                    .next()
                    .map(|character| character.is_ascii_digit())
                    .unwrap_or(false)
            {
                let number: String = trimmed[1..]
                    .chars()
                    .take_while(|character| character.is_ascii_digit() || *character == '.')
                    .collect();
                if let Some(parsed_bpm) = parse_bpm_value(&number) {
                    bpm = parsed_bpm;
                }
            } else {
                cleaned_text.push_str(trimmed);
                cleaned_text.push(' ');
            }
        }

        // Base timing calculation (quarter note duration)
        let beat_duration_ms = 60_000.0 / bpm;
        let mut step_duration_ms = beat_duration_ms / 2.0; // Eighth note default step

        let mut song = Song::new(parsed_title);
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
                '[' | '{' | '(' => {
                    // Check if this bracket contains a dynamic BPM / tempo tag (e.g. [bpm:140], (bpm=120), [160bpm], {tempo 90})
                    let close_char = match c {
                        '[' => ']',
                        '{' => '}',
                        '(' => ')',
                        _ => ']',
                    };
                    let mut tag_content = String::new();
                    let mut lookahead = i + 1;
                    while lookahead < chars.len() && chars[lookahead] != close_char {
                        tag_content.push(chars[lookahead]);
                        lookahead += 1;
                    }
                    let tag_lower = tag_content.to_lowercase();
                    let is_tempo_tag = tag_lower.contains("bpm")
                        || tag_lower.contains("tempo")
                        || tag_lower.starts_with("t:")
                        || tag_lower.starts_with("t=")
                        || tag_lower.starts_with("t ")
                        || tag_lower.starts_with("speed:")
                        || tag_lower.starts_with("speed=");

                    if is_tempo_tag {
                        let num_str: String = tag_content
                            .chars()
                            .filter(|ch| ch.is_ascii_digit() || *ch == '.')
                            .collect();
                        if let Ok(parsed_bpm) = num_str.parse::<f64>() {
                            if parsed_bpm >= 10.0 && parsed_bpm <= 500.0 {
                                bpm = parsed_bpm;
                                step_duration_ms = (60_000.0 / bpm) / 2.0;
                                if current_ms <= 0.0 {
                                    song.bpm = bpm;
                                    if let Some(first_te) = song.tempo_events.first_mut() {
                                        first_te.bpm = bpm;
                                        first_te.us_per_beat = (60_000_000.0 / bpm) as u32;
                                    } else {
                                        song.tempo_events.push(crate::core::song::TempoEvent {
                                            time_ms: 0.0,
                                            bpm,
                                            us_per_beat: (60_000_000.0 / bpm) as u32,
                                        });
                                    }
                                } else {
                                    song.tempo_events.push(crate::core::song::TempoEvent {
                                        time_ms: current_ms,
                                        bpm,
                                        us_per_beat: (60_000_000.0 / bpm) as u32,
                                    });
                                }
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
