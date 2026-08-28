use vitl_piano_desktop::core::config::{AppConfig, HumanizeConfig, KeyboardLayoutType};
use vitl_piano_desktop::core::midi::MidiParser;
use vitl_piano_desktop::core::sheet::SheetParser;
use vitl_piano_desktop::core::song::{NoteEvent, Song};
use vitl_piano_desktop::core::transposition::TranspositionOptimizer;
use vitl_piano_desktop::input::mapping::KeyMappingEngine;
use vitl_piano_desktop::player::humanizer::HumanizerEngine;
use vitl_piano_desktop::synth::audio_output::AudioOutputManager;
use vitl_piano_desktop::synth::engine::PianoSynthEngine;

#[test]
fn test_sheet_parser_and_converter() {
    let sheet = "[8u] w y u [8o] w y o | [9i] e y i";
    let song = SheetParser::parse_sheet(sheet, "Test Song".to_string(), Some(140.0)).expect("Sheet parse failed");

    assert_eq!(song.bpm, 140.0);
    assert_eq!(song.tracks.len(), 1);
    assert!(song.total_notes >= 8);

    // Test reverse conversion to sheet
    let generated_sheet = SheetParser::song_to_sheet(&song);
    assert!(!generated_sheet.is_empty());
    assert!(generated_sheet.contains("[8u]") || generated_sheet.contains('8'));
}

#[test]
fn test_transposition_optimizer() {
    let mut song = Song::new("High Song".to_string());
    // Add notes that are very high (e.g. MIDI 100 - 106)
    let notes = vec![
        NoteEvent { note: 100, velocity: 100, start_ms: 0.0, duration_ms: 200.0, track: 0, channel: 0 },
        NoteEvent { note: 102, velocity: 100, start_ms: 200.0, duration_ms: 200.0, track: 0, channel: 0 },
        NoteEvent { note: 104, velocity: 100, start_ms: 400.0, duration_ms: 200.0, track: 0, channel: 0 },
    ];
    song.tracks.push(vitl_piano_desktop::core::song::Track {
        name: "High Track".to_string(),
        channel: 0,
        notes,
        is_drum: false,
    });
    song.finalize();

    // 61-key limit is 36..=96. These notes are out of 61-key range.
    let analysis_61 = TranspositionOptimizer::analyze(&song, false);
    assert!(analysis_61.best_offset < 0); // Should shift down
    assert_eq!(analysis_61.playable_percentage, 100.0);
}

#[test]
fn test_synth_engine_sample_generation() {
    let mut synth = PianoSynthEngine::new(44100.0);
    synth.volume = 0.8;
    synth.set_reverb_params(0.3, 0.7);

    // Trigger Middle C (60) and G4 (67)
    synth.note_on(60, 100);
    synth.note_on(67, 100);

    let mut buffer = vec![0.0f32; 1024];
    synth.process_block(&mut buffer);

    // Ensure audio samples are generated and non-zero
    let energy: f32 = buffer.iter().map(|s| s.abs()).sum();
    assert!(energy > 0.1, "Synthesizer did not generate audio output");

    // Test sustain pedal
    synth.set_sustain(true);
    synth.note_off(60);
    synth.note_off(67);

    let mut sustain_buffer = vec![0.0f32; 1024];
    synth.process_block(&mut sustain_buffer);
    let sustain_energy: f32 = sustain_buffer.iter().map(|s| s.abs()).sum();
    assert!(sustain_energy > 0.05, "Sustain pedal failed to sustain note energy");

    // Release pedal
    synth.set_sustain(false);
    let mut release_buffer = vec![0.0f32; 44100]; // 1 second
    synth.process_block(&mut release_buffer);
}

#[test]
fn test_humanizer_chord_flam() {
    let mut humanizer = HumanizerEngine::new(HumanizeConfig {
        chord_delay_ms: 15.0,
        jitter_ms: 2.0,
        mistake_rate: 0.0,
        velocity_variation: 5.0,
        finger_limit: 10,
    });

    let chord = vec![
        NoteEvent { note: 60, velocity: 100, start_ms: 1000.0, duration_ms: 500.0, track: 0, channel: 0 },
        NoteEvent { note: 64, velocity: 100, start_ms: 1000.0, duration_ms: 500.0, track: 0, channel: 0 },
        NoteEvent { note: 67, velocity: 100, start_ms: 1000.0, duration_ms: 500.0, track: 0, channel: 0 },
    ];

    let result = humanizer.process_chord_notes(&chord);
    assert_eq!(result.len(), 3);
    // 2nd note should start after 1st note due to chord flam spread
    assert!(result[1].start_ms > result[0].start_ms);
    assert!(result[2].start_ms > result[1].start_ms);
}

#[test]
fn test_keyboard_layout_mapping() {
    let qwerty = KeyMappingEngine::new(KeyboardLayoutType::QwertyUS);
    let azerty = KeyMappingEngine::new(KeyboardLayoutType::AzertyFR);

    // Note 53 is 'q' on QWERTY
    let map_q = qwerty.get_piano_key(53, false).expect("Note 53 not found");
    assert_eq!(map_q.key_char, 'q');

    // On AZERTY, 'q' is in 'a' position
    let map_az = azerty.get_piano_key(53, false).expect("Note 53 not found");
    assert_eq!(map_az.key_char, 'a');

    // 88-Key low note (MIDI 21 = A0 -> Ctrl + 6)
    let low_note = qwerty.get_piano_key(21, true).expect("Low note 21 not found");
    assert!(low_note.is_ctrl);
    assert_eq!(low_note.key_char, '6');
}

#[test]
fn test_offline_wav_rendering() {
    let sheet = "[8u] w y u [8o]";
    let song = SheetParser::parse_sheet(sheet, "WAV Test".to_string(), Some(120.0)).expect("Parse failed");

    let temp_wav = std::env::temp_dir().join("vitl_test_render.wav");
    let res = AudioOutputManager::render_song_to_wav(&song, &temp_wav);
    assert!(res.is_ok(), "Offline WAV rendering failed: {:?}", res.err());
    assert!(temp_wav.exists(), "WAV file was not created");

    let file_size = std::fs::metadata(&temp_wav).unwrap().len();
    assert!(file_size > 1000, "WAV file is too small");

    let _ = std::fs::remove_file(&temp_wav);
}

#[test]
fn test_dynamic_bpm_handling() {
    let mut song = Song::new("Dynamic BPM Test".to_string());
    song.bpm = 120.0;
    song.tempo_events.push(vitl_piano_desktop::core::song::TempoEvent {
        time_ms: 0.0,
        bpm: 120.0,
        us_per_beat: 500_000,
    });
    song.tempo_events.push(vitl_piano_desktop::core::song::TempoEvent {
        time_ms: 10_000.0, // After 10s, tempo changes to 180 BPM
        bpm: 180.0,
        us_per_beat: 333_333,
    });
    song.tempo_events.push(vitl_piano_desktop::core::song::TempoEvent {
        time_ms: 25_000.0, // After 25s, tempo drops to 90 BPM
        bpm: 90.0,
        us_per_beat: 666_667,
    });

    assert_eq!(song.get_bpm_at(0.0), 120.0);
    assert_eq!(song.get_bpm_at(5_000.0), 120.0);
    assert_eq!(song.get_bpm_at(10_000.0), 180.0);
    assert_eq!(song.get_bpm_at(18_000.0), 180.0);
    assert_eq!(song.get_bpm_at(30_000.0), 90.0);
}

#[test]
fn test_soundfont_optional_fallback() {
    let mut synth = PianoSynthEngine::new(44100.0);
    // Initially mode is PhysicalModeling
    assert_eq!(synth.mode, vitl_piano_desktop::core::config::SynthSoundMode::PhysicalModeling);

    // Attempting to load non-existent SoundFont should fail gracefully and fall back to built-in physical synth
    let res = synth.load_soundfont("/nonexistent/path/soundfont.sf2");
    assert!(res.is_err());
    assert_eq!(synth.mode, vitl_piano_desktop::core::config::SynthSoundMode::PhysicalModeling);

    // Synthesizer still plays properly with physical modeling
    synth.note_on(60, 100);
    let mut buf = vec![0.0f32; 512];
    synth.process_block(&mut buf);
    let energy: f32 = buf.iter().map(|s| s.abs()).sum();
    assert!(energy > 0.05);
}
