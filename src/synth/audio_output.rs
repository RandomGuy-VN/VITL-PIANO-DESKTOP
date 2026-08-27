use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use super::engine::PianoSynthEngine;
use crate::core::song::Song;

pub struct AudioOutputManager {
    engine: Arc<Mutex<PianoSynthEngine>>,
    _stream: Option<Stream>,
    sample_rate: u32,
}

impl AudioOutputManager {
    /// Initialize audio device stream
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("No default audio output device found")?;

        let default_config = device.default_output_config()?;
        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();
        let sample_format = default_config.sample_format();

        let engine = Arc::new(Mutex::new(PianoSynthEngine::new(sample_rate as f32)));
        let engine_clone = Arc::clone(&engine);

        let stream_config: StreamConfig = default_config.into();

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut synth = engine_clone.lock();
                    synth.process_block(data);
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut synth = engine_clone.lock();
                    for frame in data.chunks_mut(channels as usize) {
                        let (l, r) = synth.next_sample();
                        if frame.len() >= 2 {
                            frame[0] = (l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            frame[1] = (r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        } else if !frame.is_empty() {
                            frame[0] = (((l + r) * 0.5).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        }
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let mut synth = engine_clone.lock();
                    for frame in data.chunks_mut(channels as usize) {
                        let (l, r) = synth.next_sample();
                        let mono = ((l + r) * 0.5).clamp(-1.0, 1.0);
                        let sample_u16 = ((mono + 1.0) * 0.5 * u16::MAX as f32) as u16;
                        for s in frame {
                            *s = sample_u16;
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => bail!("Unsupported audio sample format"),
        };

        stream.play().context("Failed to start audio stream playback")?;

        Ok(Self {
            engine,
            _stream: Some(stream),
            sample_rate,
        })
    }

    pub fn engine(&self) -> Arc<Mutex<PianoSynthEngine>> {
        Arc::clone(&self.engine)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Render a Song directly to a 16-bit 44.1kHz Stereo WAV file
    pub fn render_song_to_wav<P: AsRef<Path>>(song: &Song, path: P) -> Result<()> {
        let sample_rate = 44100u32;
        let mut synth = PianoSynthEngine::new(sample_rate as f32);
        synth.volume = 0.9;
        synth.set_reverb_params(0.35, 0.75);

        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = hound::WavWriter::create(path, spec)?;

        let all_notes = song.all_notes_flattened();
        let total_duration_s = (song.duration_ms / 1000.0) + 3.0; // 3 seconds reverb tail
        let total_samples = (total_duration_s * sample_rate as f64) as usize;

        // Events list: (sample_index, is_note_on, note, velocity)
        let mut sample_events: Vec<(usize, bool, u8, u8)> = Vec::new();

        for note in &all_notes {
            let on_sample = ((note.start_ms / 1000.0) * sample_rate as f64) as usize;
            let off_sample = (((note.start_ms + note.duration_ms) / 1000.0) * sample_rate as f64) as usize;

            sample_events.push((on_sample, true, note.note, note.velocity));
            sample_events.push((off_sample, false, note.note, 0));
        }

        for ctrl in &song.control_events {
            if ctrl.controller == 64 {
                // Sustain pedal
                let sample_idx = ((ctrl.time_ms / 1000.0) * sample_rate as f64) as usize;
                let is_down = ctrl.value > 63;
                sample_events.push((sample_idx, is_down, 255, ctrl.value)); // Special code 255 for pedal
            }
        }

        sample_events.sort_by_key(|&(idx, _, _, _)| idx);

        let mut event_idx = 0;
        for current_sample in 0..total_samples {
            while event_idx < sample_events.len() && sample_events[event_idx].0 == current_sample {
                let (_, is_on, note, vel) = sample_events[event_idx];
                if note == 255 {
                    synth.set_sustain(is_on);
                } else if is_on {
                    synth.note_on(note, vel);
                } else {
                    synth.note_off(note);
                }
                event_idx += 1;
            }

            let (l, r) = synth.next_sample();
            let sample_l_i16 = (l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            let sample_r_i16 = (r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;

            writer.write_sample(sample_l_i16)?;
            writer.write_sample(sample_r_i16)?;
        }

        writer.finalize()?;
        Ok(())
    }
}
