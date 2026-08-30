use serde::{Deserialize, Serialize};

/// Represents a single musical note event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteEvent {
    /// MIDI note number (0 - 127, where 60 = Middle C / C4)
    pub note: u8,
    /// Velocity (1 - 127)
    pub velocity: u8,
    /// Start time in milliseconds from song start
    pub start_ms: f64,
    /// Duration in milliseconds
    pub duration_ms: f64,
    /// Track index
    pub track: usize,
    /// Channel (0 - 15)
    pub channel: u8,
}

/// Represents a control change event (e.g., Sustain pedal CC64)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEvent {
    /// Time in milliseconds from song start
    pub time_ms: f64,
    /// Controller number (e.g. 64 = Sustain Pedal)
    pub controller: u8,
    /// Controller value (0 - 127)
    pub value: u8,
    /// Channel (0 - 15)
    pub channel: u8,
}

/// Represents a tempo change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoEvent {
    /// Time in milliseconds from song start
    pub time_ms: f64,
    /// Tempo in Beats Per Minute (BPM)
    pub bpm: f64,
    /// Microseconds per beat (quarter note)
    pub us_per_beat: u32,
}

/// Represents a single track within a song
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub channel: u8,
    pub notes: Vec<NoteEvent>,
    pub is_drum: bool,
}

/// Unified Song representation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub duration_ms: f64,
    pub bpm: f64,
    pub tracks: Vec<Track>,
    pub control_events: Vec<ControlEvent>,
    pub tempo_events: Vec<TempoEvent>,
    pub total_notes: usize,
    pub min_note: u8,
    pub max_note: u8,
    pub source_type: SongSourceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SongSourceType {
    #[default]
    MidiFile,
    VirtualPianoSheet,
    Generated,
}

impl Song {
    pub fn new(title: String) -> Self {
        Self {
            title,
            artist: "Unknown Artist".to_string(),
            duration_ms: 0.0,
            bpm: 120.0,
            tracks: Vec::new(),
            control_events: Vec::new(),
            tempo_events: Vec::new(),
            total_notes: 0,
            min_note: 127,
            max_note: 0,
            source_type: SongSourceType::MidiFile,
        }
    }

    /// Recalculates total duration, note count, min/max notes, and sorts all events
    pub fn finalize(&mut self) {
        let mut max_end_time = 0.0;
        let mut count = 0;
        let mut min_n = 127u8;
        let mut max_n = 0u8;

        for track in &mut self.tracks {
            track.notes.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
            for note in &track.notes {
                count += 1;
                if note.note < min_n {
                    min_n = note.note;
                }
                if note.note > max_n {
                    max_n = note.note;
                }
                let end = note.start_ms + note.duration_ms;
                if end > max_end_time {
                    max_end_time = end;
                }
            }
        }

        self.control_events.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
        self.tempo_events.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));

        if let Some(first_tempo) = self.tempo_events.first() {
            self.bpm = first_tempo.bpm;
        }

        self.duration_ms = max_end_time;
        self.total_notes = count;
        self.min_note = if count > 0 { min_n } else { 0 };
        self.max_note = if count > 0 { max_n } else { 0 };
    }

    /// Flatten all notes into a single chronological list
    pub fn all_notes_flattened(&self) -> Vec<NoteEvent> {
        let mut all = Vec::with_capacity(self.total_notes);
        for track in &self.tracks {
            all.extend_from_slice(&track.notes);
        }
        all.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
        all
    }

    /// Formats the song duration as MM:SS or HH:MM:SS
    pub fn formatted_duration(&self) -> String {
        let total_seconds = (self.duration_ms / 1000.0).round() as u64;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{:02}:{:02}", minutes, seconds)
        }
    }

    /// Returns the active BPM at a specific point in time (in milliseconds)
    pub fn get_bpm_at(&self, time_ms: f64) -> f64 {
        if self.tempo_events.is_empty() {
            return self.bpm.max(1.0);
        }
        let mut current_bpm = self.bpm;
        for te in &self.tempo_events {
            if te.time_ms <= time_ms {
                current_bpm = te.bpm;
            } else {
                break;
            }
        }
        current_bpm.max(1.0)
    }

    /// Sets or scales base BPM dynamically
    pub fn set_bpm(&mut self, new_bpm: f64) {
        let ratio = new_bpm / self.bpm.max(1.0);
        self.bpm = new_bpm;
        if ratio > 0.0 {
            for te in &mut self.tempo_events {
                te.bpm *= ratio;
            }
        }
    }

    /// Transpose all notes in the song by semitones offset (-24 to +24)
    pub fn transpose(&mut self, semitones: i8) {
        if semitones == 0 { return; }
        for track in &mut self.tracks {
            for note in &mut track.notes {
                let shifted = (note.note as i16) + (semitones as i16);
                note.note = shifted.clamp(21, 108) as u8;
            }
        }
        self.finalize();
    }

    /// Quantize all note start times to the nearest grid interval in milliseconds
    pub fn quantize(&mut self, grid_ms: f64) {
        if grid_ms <= 1.0 { return; }
        for track in &mut self.tracks {
            for note in &mut track.notes {
                note.start_ms = (note.start_ms / grid_ms).round() * grid_ms;
                note.duration_ms = ((note.duration_ms / grid_ms).round() * grid_ms).max(grid_ms);
            }
        }
        self.finalize();
    }

    /// Replace song notes with edited note events
    pub fn update_notes(&mut self, notes: Vec<NoteEvent>) {
        // Group notes by track
        let mut track_map: std::collections::BTreeMap<usize, Vec<NoteEvent>> = std::collections::BTreeMap::new();
        for n in notes {
            track_map.entry(n.track).or_default().push(n);
        }

        if track_map.is_empty() {
            self.tracks = vec![Track {
                name: "Main Track".to_string(),
                channel: 0,
                notes: Vec::new(),
                is_drum: false,
            }];
        } else {
            self.tracks = track_map
                .into_iter()
                .map(|(t_idx, t_notes)| {
                    let ch = t_notes.first().map(|n| n.channel).unwrap_or((t_idx % 16) as u8);
                    Track {
                        name: format!("Track {}", t_idx + 1),
                        channel: ch,
                        notes: t_notes,
                        is_drum: ch == 9,
                    }
                })
                .collect();
        }
        self.finalize();
    }

    /// Convert Song to standard MIDI (SMF Format 1) binary bytes
    pub fn to_midi_bytes(&self) -> Result<Vec<u8>, String> {
        let ticks_per_beat = 480u16;
        let smf_header = midly::Header::new(
            midly::Format::Parallel,
            midly::Timing::Metrical(midly::num::u15::from(ticks_per_beat)),
        );

        let mut smf = midly::Smf::new(smf_header);

        // Tempo track (Track 0)
        let mut tempo_track = Vec::new();
        let us_per_beat = (60_000_000.0 / self.bpm.max(1.0)).round() as u32;
        tempo_track.push(midly::TrackEvent {
            delta: 0.into(),
            kind: midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(midly::num::u24::from(us_per_beat))),
        });
        tempo_track.push(midly::TrackEvent {
            delta: 0.into(),
            kind: midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(self.title.as_bytes())),
        });
        tempo_track.push(midly::TrackEvent {
            delta: 0.into(),
            kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        });
        smf.tracks.push(tempo_track);

        // Helper to convert milliseconds to MIDI ticks
        let ms_to_ticks = |ms: f64, bpm: f64| -> u32 {
            let ticks_per_ms = (ticks_per_beat as f64 * bpm) / 60000.0;
            (ms * ticks_per_ms).round().max(0.0) as u32
        };

        // Note tracks
        for (idx, track) in self.tracks.iter().enumerate() {
            let mut events_with_abs_time: Vec<(u32, midly::TrackEventKind)> = Vec::new();

            // Track name
            let track_name_bytes: &[u8] = if track.name.is_empty() {
                b"Track"
            } else {
                track.name.as_bytes()
            };
            events_with_abs_time.push((0, midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(track_name_bytes))));

            for note in &track.notes {
                let start_tick = ms_to_ticks(note.start_ms, self.bpm);
                let end_tick = ms_to_ticks(note.start_ms + note.duration_ms.max(10.0), self.bpm);

                let channel = midly::num::u4::from(note.channel.min(15));
                let key = midly::num::u7::from(note.note.clamp(0, 127));
                let vel = midly::num::u7::from(note.velocity.clamp(1, 127));

                // Note On
                events_with_abs_time.push((
                    start_tick,
                    midly::TrackEventKind::Midi {
                        channel,
                        message: midly::MidiMessage::NoteOn { key, vel },
                    },
                ));

                // Note Off
                events_with_abs_time.push((
                    end_tick,
                    midly::TrackEventKind::Midi {
                        channel,
                        message: midly::MidiMessage::NoteOff {
                            key,
                            vel: midly::num::u7::from(0),
                        },
                    },
                ));
            }

            // Sort events by absolute tick
            events_with_abs_time.sort_by_key(|&(tick, _)| tick);

            // Convert to delta ticks
            let mut track_events = Vec::new();
            let mut last_tick = 0u32;
            for (abs_tick, kind) in events_with_abs_time {
                let delta = (abs_tick.saturating_sub(last_tick)).min(0x0FFFFFFF);
                last_tick = abs_tick;
                track_events.push(midly::TrackEvent {
                    delta: midly::num::u28::from(delta),
                    kind,
                });
            }

            // End of track marker
            track_events.push(midly::TrackEvent {
                delta: 0.into(),
                kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
            });

            smf.tracks.push(track_events);
        }

        let mut out = Vec::new();
        smf.write(&mut out).map_err(|e| format!("Failed to encode MIDI bytes: {:?}", e))?;
        Ok(out)
    }

    /// Convert Song to Virtual Piano sheet notation text with chord brackets
    pub fn to_sheet_text(&self) -> String {
        let all_notes = self.all_notes_flattened();
        if all_notes.is_empty() {
            return String::new();
        }

        // Virtual Piano character mapping for notes 21 to 108
        // Note 36 (C2) -> '1', 60 (C4) -> '8' or 'o', standard VP layout
        let note_to_vp = |note: u8| -> Option<char> {
            let map = [
                (36, '1'), (37, '!'), (38, '2'), (39, '@'), (40, '3'), (41, '4'), (42, '$'), (43, '5'),
                (44, '%'), (45, '6'), (46, '^'), (47, '7'), (48, '8'), (49, '*'), (50, '9'), (51, '('),
                (52, '0'), (53, 'q'), (54, 'Q'), (55, 'w'), (56, 'W'), (57, 'e'), (58, 'E'), (59, 'r'),
                (60, 't'), (61, 'T'), (62, 'y'), (63, 'Y'), (64, 'u'), (65, 'i'), (66, 'I'), (67, 'o'),
                (68, 'O'), (69, 'p'), (70, 'P'), (71, 'a'), (72, 's'), (73, 'S'), (74, 'd'), (75, 'D'),
                (76, 'f'), (77, 'g'), (78, 'G'), (79, 'h'), (80, 'H'), (81, 'j'), (82, 'J'), (83, 'k'),
                (84, 'l'), (85, 'L'), (86, 'z'), (87, 'Z'), (88, 'x'), (89, 'C'), (90, 'v'), (91, 'V'),
                (92, 'b'), (93, 'B'), (94, 'n'), (95, 'm'), (96, 'M'),
            ];
            map.iter().find(|&&(n, _)| n == note).map(|&(_, c)| c)
        };

        // Group notes that start within 25ms of each other into chords
        let mut chords: Vec<Vec<char>> = Vec::new();
        let mut current_chord: Vec<char> = Vec::new();
        let mut chord_time = -9999.0;

        for n in &all_notes {
            if let Some(ch) = note_to_vp(n.note) {
                if (n.start_ms - chord_time).abs() <= 25.0 {
                    if !current_chord.contains(&ch) {
                        current_chord.push(ch);
                    }
                } else {
                    if !current_chord.is_empty() {
                        chords.push(std::mem::take(&mut current_chord));
                    }
                    current_chord.push(ch);
                    chord_time = n.start_ms;
                }
            }
        }
        if !current_chord.is_empty() {
            chords.push(current_chord);
        }

        let mut sheet_buf = String::new();
        sheet_buf.push_str(&format!("// Title: {}\n// BPM: {}\n\n", self.title, self.bpm));

        let mut col = 0;
        for chord in chords {
            let chord_str = if chord.len() == 1 {
                chord[0].to_string()
            } else {
                let inner: String = chord.into_iter().collect();
                format!("[{}]", inner)
            };

            sheet_buf.push_str(&chord_str);
            sheet_buf.push(' ');
            col += chord_str.len() + 1;

            if col >= 72 {
                sheet_buf.push('\n');
                col = 0;
            }
        }

        sheet_buf
    }
}
