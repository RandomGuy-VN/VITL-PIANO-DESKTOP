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
}
