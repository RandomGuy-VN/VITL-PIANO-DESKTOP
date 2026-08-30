use anyhow::{bail, Context, Result};
use midly::{Header, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::collections::HashMap;
use std::path::Path;

use super::song::{ControlEvent, NoteEvent, Song, SongSourceType, TempoEvent, Track};

pub struct MidiParser;

impl MidiParser {
    /// Parse a standard MIDI file from disk
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Song> {
        let path = path.as_ref();
        let data = std::fs::read(path).with_context(|| format!("Failed to read file: {:?}", path))?;
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

        let ticks_per_beat = match smf.header.timing {
            Timing::Metrical(tpb) => tpb.as_int() as f64,
            Timing::Timecode(fps, subframe) => {
                (fps.as_int() as f64) * (subframe as f64)
            }
        };

        if ticks_per_beat <= 0.0 {
            bail!("Invalid MIDI timing specification");
        }

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

        // Sort tempo events by tick
        raw_tempo_events.sort_by_key(|&(tick, _)| tick);
        raw_tempo_events.dedup_by_key(|&mut (tick, _)| tick);

        // Ensure there is always a baseline tempo at tick 0 (default 120 BPM = 500,000 us/beat)
        if raw_tempo_events.is_empty() || raw_tempo_events[0].0 != 0 {
            let initial_tempo = raw_tempo_events.first().map(|e| e.1).unwrap_or(500_000);
            raw_tempo_events.insert(0, (0, initial_tempo));
        }

        // Precompute tempo maps with millisecond timestamps
        let mut tempo_map: Vec<(u64, f64, u32)> = Vec::new(); // (tick, start_ms, us_per_beat)
        let mut current_ms = 0.0;
        let mut prev_tick = 0u64;
        let mut current_us_per_beat = 500_000u32;

        for &(tick, us_per_beat) in &raw_tempo_events {
            let delta_ticks = tick - prev_tick;
            let ms_per_tick = (current_us_per_beat as f64 / 1000.0) / ticks_per_beat;
            current_ms += delta_ticks as f64 * ms_per_tick;
            tempo_map.push((tick, current_ms, us_per_beat));
            prev_tick = tick;
            current_us_per_beat = us_per_beat;
        }

        // Helper function to convert any tick to absolute milliseconds
        let tick_to_ms = |target_tick: u64| -> f64 {
            let mut best_idx = 0;
            for (i, &(t, _, _)) in tempo_map.iter().enumerate() {
                if target_tick >= t {
                    best_idx = i;
                } else {
                    break;
                }
            }

            let (anchor_tick, anchor_ms, us_pb) = tempo_map[best_idx];
            let delta = target_tick - anchor_tick;
            let ms_per_tick = (us_pb as f64 / 1000.0) / ticks_per_beat;
            anchor_ms + (delta as f64 * ms_per_tick)
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
                                        active_notes
                                            .entry((note_num, ch))
                                            .or_default()
                                            .push((abs_tick, current_time_ms, velocity));
                                    } else {
                                        // Velocity 0 is Note Off
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

    /// Export a Song to standard MIDI byte stream (Type 0 or Type 1)
    pub fn export_to_midi(song: &Song) -> Result<Vec<u8>> {
        use midly::num::{u15, u24, u28, u4, u7};
        use midly::{TrackEvent, Format};

        let tpb = 480u16;
        let mut header = Header::new(Format::SingleTrack, Timing::Metrical(u15::new(tpb)));
        if song.tracks.len() > 1 {
            header.format = Format::Parallel;
        }

        let mut smf = Smf::new(header);
        let us_per_beat = (60_000_000.0 / song.bpm.max(1.0)).round() as u32;
        let ms_per_tick = (us_per_beat as f64 / 1000.0) / (tpb as f64);

        for (track_idx, track) in song.tracks.iter().enumerate() {
            let mut track_events: Vec<TrackEvent<'static>> = Vec::new();

            // Track name
            let name_bytes = track.name.as_bytes().to_vec();
            track_events.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Meta(MetaMessage::TrackName(Box::leak(name_bytes.into_boxed_slice()))),
            });

            // Set tempo if first track
            if track_idx == 0 {
                track_events.push(TrackEvent {
                    delta: u28::new(0),
                    kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us_per_beat))),
                });
            }

            // Collect all note on and off events with absolute ticks
            let mut raw_events: Vec<(u64, TrackEventKind<'static>)> = Vec::new();

            for note in &track.notes {
                let on_tick = (note.start_ms / ms_per_tick).round() as u64;
                let off_tick = ((note.start_ms + note.duration_ms) / ms_per_tick).round() as u64;
                let ch = u4::new(note.channel.min(15));
                let key = u7::new(note.note.min(127));
                let vel = u7::new(note.velocity.min(127).max(1));

                raw_events.push((
                    on_tick,
                    TrackEventKind::Midi {
                        channel: ch,
                        message: MidiMessage::NoteOn { key, vel },
                    },
                ));

                raw_events.push((
                    off_tick,
                    TrackEventKind::Midi {
                        channel: ch,
                        message: MidiMessage::NoteOff {
                            key,
                            vel: u7::new(0),
                        },
                    },
                ));
            }

            raw_events.sort_by_key(|&(tick, _)| tick);

            let mut last_tick = 0u64;
            for (tick, kind) in raw_events {
                let delta = (tick.saturating_sub(last_tick)).min(0x0FFFFFFF) as u32;
                track_events.push(TrackEvent {
                    delta: u28::new(delta),
                    kind,
                });
                last_tick = tick;
            }

            // End of Track
            track_events.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            });

            smf.tracks.push(track_events);
        }

        let mut output = Vec::new();
        smf.write(&mut output).map_err(|e| anyhow::anyhow!("Failed to encode MIDI bytes: {:?}", e))?;
        Ok(output)
    }
}
