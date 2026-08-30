use anyhow::{bail, Context, Result};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::collections::HashMap;
use std::path::Path;

use super::song::{ControlEvent, NoteEvent, Song, SongSourceType, TempoEvent, Track};

pub struct MidiParser;

#[derive(Debug, Clone, Copy)]
enum MidiTimeDivision {
    Metrical { ticks_per_beat: f64 },
    Timecode { ticks_per_second: f64 },
}

impl MidiParser {
    /// Parse a standard MIDI file from disk
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Song> {
        let path = path.as_ref();
        let data =
            std::fs::read(path).with_context(|| format!("Failed to read file: {:?}", path))?;
        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        Self::parse_bytes(&data, filename)
    }

    /// Parse MIDI data from raw byte slice
    pub fn parse_bytes(bytes: &[u8], title_hint: String) -> Result<Song> {
        let smf = Smf::parse(bytes).map_err(|e| anyhow::anyhow!("MIDI parse error: {:?}", e))?;

        let time_division = match smf.header.timing {
            Timing::Metrical(ticks_per_beat) => {
                let ticks_per_beat = ticks_per_beat.as_int() as f64;
                if ticks_per_beat <= 0.0 {
                    bail!("Invalid MIDI metrical timing specification");
                }
                MidiTimeDivision::Metrical { ticks_per_beat }
            }
            Timing::Timecode(fps, subframes_per_frame) => {
                let ticks_per_second = fps.as_f32() as f64 * subframes_per_frame as f64;
                if ticks_per_second <= 0.0 {
                    bail!("Invalid MIDI SMPTE timing specification");
                }
                MidiTimeDivision::Timecode { ticks_per_second }
            }
        };

        // First pass: extract all tempo changes to build an accurate tick-to-ms timeline
        let mut raw_tempo_events: Vec<(u64, u32)> = Vec::new();

        for track in &smf.tracks {
            let mut abs_tick = 0u64;
            for event in track {
                abs_tick += event.delta.as_int() as u64;
                if let TrackEventKind::Meta(MetaMessage::Tempo(tempo)) = event.kind {
                    raw_tempo_events.push((abs_tick, tempo.as_int()));
                }
            }
        }

        // Sort tempo events by tick and retain the last event at duplicate ticks.
        raw_tempo_events.sort_by_key(|&(tick, _)| tick);
        let mut deduplicated_tempos: Vec<(u64, u32)> =
            Vec::with_capacity(raw_tempo_events.len() + 1);
        for (tick, us_per_beat) in raw_tempo_events {
            let us_per_beat = us_per_beat.max(1);
            if let Some(last) = deduplicated_tempos.last_mut() {
                if last.0 == tick {
                    *last = (tick, us_per_beat);
                    continue;
                }
            }
            deduplicated_tempos.push((tick, us_per_beat));
        }
        let mut raw_tempo_events = deduplicated_tempos;

        // MIDI defaults to 120 BPM until the first explicit tempo event. A future
        // event must never be applied retroactively to tick zero.
        if raw_tempo_events
            .first()
            .map(|event| event.0 != 0)
            .unwrap_or(true)
        {
            raw_tempo_events.insert(0, (0, 500_000));
        }

        // Metrical files use a piecewise tempo map. SMPTE files use fixed
        // ticks-per-second and tempo messages are metadata only.
        let mut tempo_map: Vec<(u64, f64, u32)> = Vec::new();
        if let MidiTimeDivision::Metrical { ticks_per_beat } = time_division {
            let mut current_ms = 0.0;
            let mut previous_tick = 0_u64;
            let mut current_us_per_beat = 500_000_u32;

            for &(tick, us_per_beat) in &raw_tempo_events {
                let delta_ticks = tick - previous_tick;
                let ms_per_tick = current_us_per_beat as f64 / 1000.0 / ticks_per_beat;
                current_ms += delta_ticks as f64 * ms_per_tick;
                tempo_map.push((tick, current_ms, us_per_beat));
                previous_tick = tick;
                current_us_per_beat = us_per_beat;
            }
        }

        let tick_to_ms = |target_tick: u64| -> f64 {
            match time_division {
                MidiTimeDivision::Metrical { ticks_per_beat } => {
                    let mut active = tempo_map[0];
                    for &point in tempo_map.iter().skip(1) {
                        if target_tick >= point.0 {
                            active = point;
                        } else {
                            break;
                        }
                    }

                    let delta_ticks = target_tick - active.0;
                    let ms_per_tick = active.2 as f64 / 1000.0 / ticks_per_beat;
                    active.1 + delta_ticks as f64 * ms_per_tick
                }
                MidiTimeDivision::Timecode { ticks_per_second } => {
                    target_tick as f64 * 1000.0 / ticks_per_second
                }
            }
        };

        let mut song = Song::new(title_hint.clone());
        song.source_type = SongSourceType::MidiFile;

        // Store tempo events in song model
        for &(tick, us_pb) in &raw_tempo_events {
            let time_ms = tick_to_ms(tick);
            let bpm = 60_000_000.0 / ((us_pb as f64).max(1.0));
            song.tempo_events.push(TempoEvent {
                time_ms,
                bpm,
                us_per_beat: us_pb.max(1),
            });
        }

        // Parse each track
        for (track_idx, smf_track) in smf.tracks.iter().enumerate() {
            let mut track_name = format!("Track {}", track_idx + 1);
            let mut abs_tick = 0u64;
            let mut notes: Vec<NoteEvent> = Vec::new();
            // Active note tracking: (note, channel) -> (start_tick, start_ms, velocity)
            let mut active_notes: HashMap<(u8, u8), Vec<(u64, f64, u8)>> = HashMap::new();
            let mut is_drum = false;

            for event in smf_track {
                abs_tick += event.delta.as_int() as u64;
                let current_time_ms = tick_to_ms(abs_tick);

                match event.kind {
                    TrackEventKind::Meta(MetaMessage::TrackName(name_bytes)) => {
                        if let Ok(name) = std::str::from_utf8(name_bytes) {
                            let trimmed = name.trim().to_string();
                            if !trimmed.is_empty() {
                                track_name = trimmed.clone();
                                if track_idx == 0 && song.title == title_hint {
                                    song.title = trimmed;
                                }
                            }
                        }
                    }
                    TrackEventKind::Midi { channel, message } => {
                        let ch = channel.as_int();
                        if ch == 9 {
                            is_drum = true;
                        }

                        match message {
                            MidiMessage::NoteOn { key, vel } => {
                                let note_num = key.as_int();
                                let velocity = vel.as_int();

                                if velocity > 0 {
                                    active_notes.entry((note_num, ch)).or_default().push((
                                        abs_tick,
                                        current_time_ms,
                                        velocity,
                                    ));
                                } else {
                                    // Velocity 0 is Note Off
                                    if let Some(list) = active_notes.get_mut(&(note_num, ch)) {
                                        if !list.is_empty() {
                                            let (_start_tick, start_ms, velocity) = list.remove(0);
                                            let duration_ms =
                                                (current_time_ms - start_ms).max(10.0);
                                            notes.push(NoteEvent {
                                                note: note_num,
                                                velocity,
                                                start_ms,
                                                duration_ms,
                                                track: track_idx,
                                                channel: ch,
                                            });
                                        }
                                    }
                                }
                            }
                            MidiMessage::NoteOff { key, .. } => {
                                let note_num = key.as_int();
                                if let Some(list) = active_notes.get_mut(&(note_num, ch)) {
                                    if !list.is_empty() {
                                        let (_start_tick, start_ms, velocity) = list.remove(0);
                                        let duration_ms = (current_time_ms - start_ms).max(10.0);
                                        notes.push(NoteEvent {
                                            note: note_num,
                                            velocity,
                                            start_ms,
                                            duration_ms,
                                            track: track_idx,
                                            channel: ch,
                                        });
                                    }
                                }
                            }
                            MidiMessage::Controller { controller, value } => {
                                song.control_events.push(ControlEvent {
                                    time_ms: current_time_ms,
                                    controller: controller.as_int(),
                                    value: value.as_int(),
                                    channel: ch,
                                });
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            // Close any remaining dangling notes
            let end_ms = tick_to_ms(abs_tick);
            for ((note_num, ch), list) in active_notes {
                for (_start_tick, start_ms, velocity) in list {
                    let duration_ms = (end_ms - start_ms).max(100.0);
                    notes.push(NoteEvent {
                        note: note_num,
                        velocity,
                        start_ms,
                        duration_ms,
                        track: track_idx,
                        channel: ch,
                    });
                }
            }

            if !notes.is_empty() {
                song.tracks.push(Track {
                    name: track_name,
                    channel: 0,
                    notes,
                    is_drum,
                });
            }
        }

        song.finalize();
        Ok(song)
    }

    /// Export a Song using the canonical serializer implemented by `Song`.
    pub fn export_to_midi(song: &Song) -> Result<Vec<u8>> {
        song.to_midi_bytes().map_err(anyhow::Error::msg)
    }
}
