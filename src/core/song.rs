use serde::{Deserialize, Serialize};

const MIDI_TICKS_PER_BEAT: u16 = 480;
const DEFAULT_BPM: f64 = 120.0;
const MAX_MIDI_TEMPO_US_PER_BEAT: u32 = 0x00FF_FFFF;

#[derive(Debug, Clone, Copy)]
struct MidiTempoPoint {
    time_ms: f64,
    tick: f64,
    us_per_beat: u32,
}

fn valid_bpm_or(bpm: f64, fallback: f64) -> f64 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        fallback
    }
}

fn bpm_to_us_per_beat(bpm: f64) -> u32 {
    let bpm = valid_bpm_or(bpm, DEFAULT_BPM);
    (60_000_000.0 / bpm)
        .round()
        .clamp(1.0, MAX_MIDI_TEMPO_US_PER_BEAT as f64) as u32
}

fn build_midi_tempo_map(song: &Song) -> Vec<MidiTempoPoint> {
    let base_bpm = valid_bpm_or(song.bpm, DEFAULT_BPM);
    let base_us_per_beat = bpm_to_us_per_beat(base_bpm);
    let mut source_points: Vec<(f64, u32)> = song
        .tempo_events
        .iter()
        .filter_map(|event| {
            if !event.time_ms.is_finite() {
                return None;
            }

            let us_per_beat = if event.bpm.is_finite() && event.bpm > 0.0 {
                bpm_to_us_per_beat(event.bpm)
            } else {
                event.us_per_beat.clamp(1, MAX_MIDI_TEMPO_US_PER_BEAT)
            };
            Some((event.time_ms.max(0.0), us_per_beat))
        })
        .collect();

    source_points.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut deduplicated: Vec<(f64, u32)> = Vec::with_capacity(source_points.len() + 1);
    for point in source_points {
        if let Some(last) = deduplicated.last_mut() {
            if last.0 == point.0 {
                *last = point;
                continue;
            }
        }
        deduplicated.push(point);
    }

    if deduplicated
        .first()
        .map(|point| point.0 > 0.0)
        .unwrap_or(true)
    {
        deduplicated.insert(0, (0.0, base_us_per_beat));
    }

    let mut tempo_map = Vec::with_capacity(deduplicated.len());
    let mut previous_time_ms = 0.0;
    let mut previous_tick = 0.0;
    let mut previous_us_per_beat = deduplicated[0].1;

    for (index, (time_ms, us_per_beat)) in deduplicated.into_iter().enumerate() {
        if index > 0 {
            let delta_ms = time_ms - previous_time_ms;
            previous_tick +=
                delta_ms * MIDI_TICKS_PER_BEAT as f64 * 1000.0 / previous_us_per_beat as f64;
        }

        tempo_map.push(MidiTempoPoint {
            time_ms,
            tick: previous_tick,
            us_per_beat,
        });
        previous_time_ms = time_ms;
        previous_us_per_beat = us_per_beat;
    }

    tempo_map
}

fn milliseconds_to_tick(time_ms: f64, tempo_map: &[MidiTempoPoint]) -> u64 {
    let time_ms = if time_ms.is_finite() {
        time_ms.max(0.0)
    } else {
        0.0
    };
    let mut active = &tempo_map[0];
    for point in tempo_map.iter().skip(1) {
        if point.time_ms <= time_ms {
            active = point;
        } else {
            break;
        }
    }

    let tick = active.tick
        + (time_ms - active.time_ms) * MIDI_TICKS_PER_BEAT as f64 * 1000.0
            / active.us_per_beat as f64;
    tick.round().max(0.0) as u64
}

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
            track
                .notes
                .sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
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

        self.control_events
            .sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
        self.tempo_events
            .sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));

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
        let note_count = self.tracks.iter().map(|track| track.notes.len()).sum();
        let mut all = Vec::with_capacity(note_count);
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
        if !new_bpm.is_finite() || new_bpm <= 0.0 {
            return;
        }

        let current_bpm = valid_bpm_or(self.bpm, DEFAULT_BPM);
        let ratio = new_bpm / current_bpm;
        self.bpm = new_bpm;
        for tempo_event in &mut self.tempo_events {
            let current_event_bpm = if tempo_event.bpm.is_finite() && tempo_event.bpm > 0.0 {
                tempo_event.bpm
            } else {
                60_000_000.0 / tempo_event.us_per_beat.max(1) as f64
            };
            tempo_event.bpm = current_event_bpm * ratio;
            tempo_event.us_per_beat = bpm_to_us_per_beat(tempo_event.bpm);
        }
    }

    /// Transpose all notes in the song by semitones offset (-24 to +24)
    pub fn transpose(&mut self, semitones: i8) {
        if semitones == 0 {
            return;
        }
        for track in &mut self.tracks {
            for note in &mut track.notes {
                if note.channel == 9 {
                    continue;
                }
                let shifted = (note.note as i16) + (semitones as i16);
                note.note = shifted.clamp(21, 108) as u8;
            }
        }
        self.finalize();
    }

    /// Quantize all note start times to the nearest grid interval in milliseconds
    pub fn quantize(&mut self, grid_ms: f64) {
        if grid_ms <= 1.0 {
            return;
        }
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
        let mut track_map: std::collections::BTreeMap<usize, Vec<NoteEvent>> =
            std::collections::BTreeMap::new();
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
                    let ch = t_notes
                        .first()
                        .map(|n| n.channel)
                        .unwrap_or((t_idx % 16) as u8);
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
        use midly::num::{u15, u24, u28, u4, u7};
        use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

        let tempo_map = build_midi_tempo_map(self);
        let header = Header::new(
            Format::Parallel,
            Timing::Metrical(u15::from(MIDI_TICKS_PER_BEAT)),
        );
        let mut smf = Smf::new(header);

        // MIDI Format 1 conventionally keeps tempo and global controller events in a
        // conductor track. ControlEvent has channel but no source-track identity, so
        // this preserves all available information without assigning a false track.
        let mut conductor_events = Vec::new();
        conductor_events.push((
            0_u64,
            0_u8,
            TrackEventKind::Meta(MetaMessage::TrackName(self.title.as_bytes())),
        ));
        for point in &tempo_map {
            conductor_events.push((
                point.tick.round().max(0.0) as u64,
                1,
                TrackEventKind::Meta(MetaMessage::Tempo(u24::from(point.us_per_beat))),
            ));
        }
        for control in &self.control_events {
            conductor_events.push((
                milliseconds_to_tick(control.time_ms, &tempo_map),
                2,
                TrackEventKind::Midi {
                    channel: u4::from(control.channel.min(15)),
                    message: MidiMessage::Controller {
                        controller: u7::from(control.controller.min(127)),
                        value: u7::from(control.value.min(127)),
                    },
                },
            ));
        }
        conductor_events.sort_by_key(|(tick, priority, _)| (*tick, *priority));

        let mut conductor_track = Vec::with_capacity(conductor_events.len() + 1);
        let mut last_tick = 0_u64;
        for (absolute_tick, _, kind) in conductor_events {
            let delta = absolute_tick.saturating_sub(last_tick).min(0x0FFF_FFFF) as u32;
            conductor_track.push(midly::TrackEvent {
                delta: u28::from(delta),
                kind,
            });
            last_tick = absolute_tick;
        }
        conductor_track.push(midly::TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(conductor_track);

        for track in &self.tracks {
            let track_name = if track.name.is_empty() {
                b"Track".as_slice()
            } else {
                track.name.as_bytes()
            };
            let mut absolute_events = vec![(
                0_u64,
                0_u8,
                TrackEventKind::Meta(MetaMessage::TrackName(track_name)),
            )];

            for note in &track.notes {
                let start_tick = milliseconds_to_tick(note.start_ms, &tempo_map);
                let end_tick =
                    milliseconds_to_tick(note.start_ms + note.duration_ms.max(10.0), &tempo_map);
                let channel = u4::from(note.channel.min(15));
                let key = u7::from(note.note.min(127));
                let velocity = u7::from(note.velocity.clamp(1, 127));

                // Note-offs sort before note-ons at the same tick so repeated pitches
                // are released before being retriggered.
                absolute_events.push((
                    end_tick,
                    1,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::NoteOff {
                            key,
                            vel: u7::from(0),
                        },
                    },
                ));
                absolute_events.push((
                    start_tick,
                    2,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::NoteOn { key, vel: velocity },
                    },
                ));
            }

            absolute_events.sort_by_key(|(tick, priority, _)| (*tick, *priority));

            let mut midi_track = Vec::with_capacity(absolute_events.len() + 1);
            let mut last_tick = 0_u64;
            for (absolute_tick, _, kind) in absolute_events {
                let delta = absolute_tick.saturating_sub(last_tick).min(0x0FFF_FFFF) as u32;
                midi_track.push(midly::TrackEvent {
                    delta: u28::from(delta),
                    kind,
                });
                last_tick = absolute_tick;
            }
            midi_track.push(midly::TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            });
            smf.tracks.push(midi_track);
        }

        let mut output = Vec::new();
        smf.write(&mut output)
            .map_err(|error| format!("Failed to encode MIDI bytes: {:?}", error))?;
        Ok(output)
    }

    /// Convert Song to Virtual Piano sheet notation text with chord brackets
    pub fn to_sheet_text(&self) -> String {
        let all_notes = self.all_notes_flattened();
        if all_notes.is_empty() {
            return String::new();
        }

        // Group notes that start within 25ms of each other into chords.
        // The sheet grammar has no Ctrl-modifier token for the 88-key extensions,
        // so only the standard Virtual Piano range (MIDI 36-96) is representable.
        let mut chords: Vec<Vec<char>> = Vec::new();
        let mut current_chord: Vec<char> = Vec::new();
        let mut chord_time = -9999.0;

        let mut omitted_notes = 0_usize;
        for n in &all_notes {
            if let Some(ch) = crate::core::sheet::SheetParser::midi_note_to_char(n.note) {
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
            } else {
                omitted_notes += 1;
            }
        }
        if !current_chord.is_empty() {
            chords.push(current_chord);
        }

        let mut sheet_buf = String::new();
        sheet_buf.push_str(&format!("// Title: {}\n// BPM: {}\n", self.title, self.bpm));
        if omitted_notes > 0 {
            sheet_buf.push_str(&format!(
                "// Warning: {} note(s) outside MIDI 36-96 were omitted; the sheet format has no Ctrl-modifier syntax.\n",
                omitted_notes
            ));
        }
        sheet_buf.push('\n');

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
