use rand::Rng;
use crate::core::config::HumanizeConfig;
use crate::core::song::NoteEvent;

pub struct HumanizerEngine {
    pub config: HumanizeConfig,
    active_fingers: usize,
}

impl HumanizerEngine {
    pub fn new(config: HumanizeConfig) -> Self {
        Self {
            config,
            active_fingers: 0,
        }
    }

    /// Humanize a chord or group of simultaneous notes
    pub fn process_chord_notes(&mut self, notes: &[NoteEvent]) -> Vec<NoteEvent> {
        let mut rng = rand::thread_rng();
        let mut humanized: Vec<NoteEvent> = Vec::with_capacity(notes.len());

        let base_delay = self.config.chord_delay_ms;
        let jitter_range = self.config.jitter_ms;
        let mistake_prob = (self.config.mistake_rate / 100.0).clamp(0.0, 0.5);

        for (idx, note) in notes.iter().enumerate() {
            // Finger limit check
            if self.config.finger_limit > 0 && idx >= self.config.finger_limit {
                continue;
            }

            let mut note_num = note.note;

            // Random human mistake simulation (slip to adjacent key)
            if mistake_prob > 0.0 && rng.gen_bool(mistake_prob) {
                let delta = if rng.gen_bool(0.5) { 1 } else { -1 };
                let slipped = (note_num as i16 + delta).clamp(21, 108) as u8;
                note_num = slipped;
            }

            // Strumming flam delay
            let chord_offset = (idx as f64) * base_delay * rng.gen_range(0.8..1.2);

            // Jitter offset
            let jitter = if jitter_range > 0.0 {
                rng.gen_range(-jitter_range..jitter_range)
            } else {
                0.0
            };

            let start_ms = (note.start_ms + chord_offset + jitter).max(0.0);

            // Velocity humanization
            let vel_var = self.config.velocity_variation;
            let vel_offset = if vel_var > 0.0 {
                rng.gen_range(-vel_var..vel_var) as i16
            } else {
                0
            };
            let velocity = ((note.velocity as i16) + vel_offset).clamp(1, 127) as u8;

            humanized.push(NoteEvent {
                note: note_num,
                velocity,
                start_ms,
                duration_ms: note.duration_ms,
                track: note.track,
                channel: note.channel,
            });
        }

        humanized
    }

    pub fn reset(&mut self) {
        self.active_fingers = 0;
    }
}
