use super::song::Song;

pub struct TranspositionOptimizer;

#[derive(Debug, Clone, PartialEq)]
pub struct TransposeAnalysis {
    pub best_offset: i8,
    pub out_of_range_count: usize,
    pub total_notes: usize,
    pub playable_percentage: f64,
}

impl TranspositionOptimizer {
    /// Calculate how many notes fit within a given MIDI range (e.g. 36..=96 for 61 keys, 21..=108 for 88 keys)
    pub fn analyze(song: &Song, allow_88_keys: bool) -> TransposeAnalysis {
        let (min_valid, max_valid) = if allow_88_keys {
            (21u8, 108u8)
        } else {
            (36u8, 96u8)
        };

        let all_notes = song.all_notes_flattened();
        let total_notes = all_notes.len();
        if total_notes == 0 {
            return TransposeAnalysis {
                best_offset: 0,
                out_of_range_count: 0,
                total_notes: 0,
                playable_percentage: 100.0,
            };
        }

        let mut best_offset: i8 = 0;
        let mut min_out_of_range = usize::MAX;

        // Try offsets from -24 to +24 semitones (±2 octaves)
        for offset in -24i8..=24i8 {
            let mut out_count = 0;
            for note in &all_notes {
                let transposed = (note.note as i16) + (offset as i16);
                if transposed < (min_valid as i16) || transposed > (max_valid as i16) {
                    out_count += 1;
                }
            }

            // Prefer offset = 0 if tie, or smaller absolute offset
            if out_count < min_out_of_range
                || (out_count == min_out_of_range && offset.abs() < best_offset.abs())
            {
                min_out_of_range = out_count;
                best_offset = offset;
            }
        }

        let playable = total_notes.saturating_sub(min_out_of_range);
        let percentage = (playable as f64 / total_notes as f64) * 100.0;

        TransposeAnalysis {
            best_offset,
            out_of_range_count: min_out_of_range,
            total_notes,
            playable_percentage: percentage,
        }
    }
}
