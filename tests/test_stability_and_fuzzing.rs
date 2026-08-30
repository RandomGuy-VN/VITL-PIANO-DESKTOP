use std::sync::Arc;
use tokio::sync::broadcast;
use vitl_piano_desktop::core::config::AppConfig;
use vitl_piano_desktop::core::midi::MidiParser;
use vitl_piano_desktop::core::sheet::SheetParser;
use vitl_piano_desktop::core::song::{NoteEvent, Song, Track};
use vitl_piano_desktop::core::transcriber::AudioTranscriber;
use vitl_piano_desktop::player::engine::PlayerEngine;
use vitl_piano_desktop::synth::dsp::{StereoDelay, ThreeBandEqualizer};
use vitl_piano_desktop::synth::engine::PianoSynthEngine;

#[tokio::test]
async fn test_fuzz_empty_and_zero_note_song() {
    let mut empty_song = Song::new("Empty Song".to_string());
    empty_song.bpm = 120.0;
    assert_eq!(empty_song.total_notes, 0);
    assert_eq!(empty_song.duration_ms, 0.0);

    // Quantize with 0.0 grid_ms (should not divide by zero or panic)
    empty_song.quantize(0.0);
    empty_song.quantize(125.0);
    empty_song.quantize(-50.0);

    // Transpose
    empty_song.transpose(12);
    empty_song.transpose(-24);

    // MIDI Export of empty song
    let midi_bytes = empty_song.to_midi_bytes().expect("Empty song should serialize to valid MIDI");
    assert!(!midi_bytes.is_empty());

    // Re-parse empty MIDI
    let reloaded = MidiParser::parse_bytes(&midi_bytes, "Reloaded".to_string()).expect("Should parse empty MIDI");
    assert_eq!(reloaded.total_notes, 0);

    // Sheet generation
    let sheet = empty_song.to_sheet_text();
    assert!(sheet.is_empty() || sheet.starts_with("//"));

    // Load into PlayerEngine
    let config = Arc::new(parking_lot::Mutex::new(AppConfig::default()));
    let synth = Arc::new(parking_lot::Mutex::new(PianoSynthEngine::new(44100.0)));
    let (status_tx, _status_rx) = broadcast::channel(32);
    let player = PlayerEngine::new(synth, config, status_tx);
    player.load_song(empty_song);
    player.play();
    player.seek(500.0);
    player.stop();
}

#[test]
fn test_fuzz_extreme_bpm_and_tempos() {
    // 0 BPM
    let mut song_zero_bpm = Song::new("Zero BPM".to_string());
    song_zero_bpm.bpm = 0.0;
    assert!(song_zero_bpm.bpm >= 0.0);

    // 10,000 BPM
    let mut fast_song = Song::new("Ultra Fast".to_string());
    fast_song.bpm = 10000.0;
    let track = Track {
        name: "Piano".to_string(),
        channel: 0,
        notes: vec![NoteEvent {
            note: 60,
            start_ms: 0.0,
            duration_ms: 10.0,
            velocity: 100,
            track: 0,
            channel: 0,
        }],
        is_drum: false,
    };
    fast_song.tracks.push(track);
    fast_song.finalize();

    let bytes = fast_song.to_midi_bytes().expect("Ultra fast song should serialize");
    let parsed = MidiParser::parse_bytes(&bytes, "Fast".to_string()).expect("Parse fast MIDI");
    assert_eq!(parsed.total_notes, 1);
}

#[test]
fn test_fuzz_extreme_transpositions_and_clamping() {
    let mut song = Song::new("Transposition Fuzz".to_string());
    let track = Track {
        name: "Notes".to_string(),
        channel: 0,
        notes: vec![
            NoteEvent {
                note: 21, // Lowest piano note (A0)
                start_ms: 0.0,
                duration_ms: 500.0,
                velocity: 80,
                track: 0,
                channel: 0,
            },
            NoteEvent {
                note: 108, // Highest piano note (C8)
                start_ms: 500.0,
                duration_ms: 500.0,
                velocity: 80,
                track: 0,
                channel: 0,
            },
        ],
        is_drum: false,
    };
    song.tracks.push(track);
    song.finalize();

    // Extreme transpose down (-100)
    song.transpose(-100);
    for t in &song.tracks {
        for n in &t.notes {
            assert!(n.note >= 21 && n.note <= 108, "Note {} was out of piano bounds after transpose down", n.note);
        }
    }

    // Extreme transpose up (+200)
    song.transpose(120);
    for t in &song.tracks {
        for n in &t.notes {
            assert!(n.note >= 21 && n.note <= 108, "Note {} was out of piano bounds after transpose up", n.note);
        }
    }
}

#[test]
fn test_fuzz_dsp_equalizer_and_delay_extreme_values() {
    let sample_rate = 44100.0;
    let mut eq = ThreeBandEqualizer::new(sample_rate);

    // Extreme gains: -100 dB, +100 dB (clamped internally)
    eq.set_gains(-100.0, 100.0, -50.0);
    for _ in 0..1000 {
        let (l, r) = eq.process(0.5, -0.5);
        assert!(l.is_finite() && !l.is_nan(), "EQ output left is NaN or inf");
        assert!(r.is_finite() && !r.is_nan(), "EQ output right is NaN or inf");
    }

    // Reset EQ
    eq.reset();

    // Delay with extreme parameters
    let mut delay = StereoDelay::new(sample_rate);
    delay.enabled = true;
    delay.delay_time_ms = 0.0;
    delay.feedback = 0.99;
    delay.wet_mix = 0.5;
    let (dl, dr) = delay.process(1.0, 1.0);
    assert!(dl.is_finite() && dr.is_finite());

    delay.delay_time_ms = 10000.0; // 10 sec delay time
    delay.feedback = 1.5;
    delay.wet_mix = 1.0;
    for _ in 0..2000 {
        let (dl, dr) = delay.process(0.2, 0.2);
        assert!(dl.is_finite() && !dl.is_nan());
        assert!(dr.is_finite() && !dr.is_nan());
    }
}

#[test]
fn test_fuzz_malformed_virtual_piano_sheets() {
    let test_cases = vec![
        "",                                      // empty
        "   \n\n  \t  ",                         // whitespace only
        "[[[abc]]",                              // unbalanced brackets
        "[1234567890!@#$%^&*()_+]",              // symbols
        "[a] [b] [c] | [d] [e] [f]",             // bar lines
        "{bpm: 0} [t u o]",                      // 0 bpm inline
        "{bpm: 999999} [t u o]",                 // huge bpm inline
        "{bpm: -50} [t u o]",                    // negative bpm inline
        "Non-bracketed text with unicode: 🎵 🎹", // unicode
        "[tuo] [tuo] [tuo] [tuo]",               // valid chords
    ];

    for case in test_cases {
        let res = SheetParser::parse_sheet(case, "Fuzz Sheet".to_string(), None);
        assert!(res.is_ok(), "SheetParser should not panic on case: {}", case);
        let song = res.unwrap();
        assert!(song.bpm > 0.0);
        let _ = song.to_midi_bytes();
    }
}

#[test]
fn test_fuzz_corrupted_midi_bytes() {
    let corrupted_samples: Vec<&[u8]> = vec![
        b"",
        b"MThd",
        b"MThd\x00\x00\x00\x06\x00\x01\x00\x01\x01\xe0",
        b"\x00\x00\x00\x00\xff\xff\xff\xff",
        &[0x4d, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    ];

    for bytes in corrupted_samples {
        let res = MidiParser::parse_bytes(bytes, "Corrupted".to_string());
        // Should return Err or valid Song without panic
        match res {
            Ok(s) => assert!(s.duration_ms >= 0.0),
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}

#[test]
fn test_fuzz_audio_transcriber_file_pipeline() {
    let tmp_dir = std::env::temp_dir();
    let wav_path = tmp_dir.join("test_fuzz_audio.wav");
    let out_midi = tmp_dir.join("test_fuzz_audio.mid");

    // Write a standard 44.1kHz 1-second 440Hz sine wave WAV
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Ok(mut writer) = hound::WavWriter::create(&wav_path, spec) {
        for t in 0..44100 {
            let sample = ((2.0 * std::f32::consts::PI * 440.0 * (t as f32 / 44100.0)).sin() * 15000.0) as i16;
            let _ = writer.write_sample(sample);
        }
        let _ = writer.finalize();

        let res = AudioTranscriber::transcribe_file(&wav_path, &out_midi);
        assert!(res.is_ok(), "AudioTranscriber transcribe_file should succeed on valid WAV");
        let song = res.unwrap();
        assert!(song.duration_ms >= 0.0);
    }
}

#[test]
fn test_fuzz_synthesizer_concurrent_note_triggers() {
    let mut synth = PianoSynthEngine::new(44100.0);
    
    // Rapidly trigger 88 notes simultaneously
    for note in 21..=108 {
        synth.note_on(note, 100);
    }
    
    let mut buf = vec![0.0f32; 1024];
    synth.process_block(&mut buf);

    for s in buf.iter() {
        assert!(s.is_finite() && !s.is_nan(), "Synthesizer output contained NaN/Inf under high polyphony");
    }

    // Turn all off
    for note in 21..=108 {
        synth.note_off(note);
    }
    synth.all_notes_off();
}
