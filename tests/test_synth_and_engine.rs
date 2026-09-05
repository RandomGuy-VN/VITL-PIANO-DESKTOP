use vitl_piano_desktop::core::config::{HumanizeConfig, KeyboardLayoutType};
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
    let song = SheetParser::parse_sheet(sheet, "Test Song".to_string(), Some(140.0))
        .expect("Sheet parse failed");

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
        NoteEvent {
            note: 100,
            velocity: 100,
            start_ms: 0.0,
            duration_ms: 200.0,
            track: 0,
            channel: 0,
        },
        NoteEvent {
            note: 102,
            velocity: 100,
            start_ms: 200.0,
            duration_ms: 200.0,
            track: 0,
            channel: 0,
        },
        NoteEvent {
            note: 104,
            velocity: 100,
            start_ms: 400.0,
            duration_ms: 200.0,
            track: 0,
            channel: 0,
        },
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
    assert!(
        sustain_energy > 0.05,
        "Sustain pedal failed to sustain note energy"
    );

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
        NoteEvent {
            note: 60,
            velocity: 100,
            start_ms: 1000.0,
            duration_ms: 500.0,
            track: 0,
            channel: 0,
        },
        NoteEvent {
            note: 64,
            velocity: 100,
            start_ms: 1000.0,
            duration_ms: 500.0,
            track: 0,
            channel: 0,
        },
        NoteEvent {
            note: 67,
            velocity: 100,
            start_ms: 1000.0,
            duration_ms: 500.0,
            track: 0,
            channel: 0,
        },
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
    let low_note = qwerty
        .get_piano_key(21, true)
        .expect("Low note 21 not found");
    assert!(low_note.is_ctrl);
    assert_eq!(low_note.key_char, '6');
}

#[test]
fn test_offline_wav_rendering() {
    let sheet = "[8u] w y u [8o]";
    let song =
        SheetParser::parse_sheet(sheet, "WAV Test".to_string(), Some(120.0)).expect("Parse failed");

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
    song.tempo_events
        .push(vitl_piano_desktop::core::song::TempoEvent {
            time_ms: 0.0,
            bpm: 120.0,
            us_per_beat: 500_000,
        });
    song.tempo_events
        .push(vitl_piano_desktop::core::song::TempoEvent {
            time_ms: 10_000.0, // After 10s, tempo changes to 180 BPM
            bpm: 180.0,
            us_per_beat: 333_333,
        });
    song.tempo_events
        .push(vitl_piano_desktop::core::song::TempoEvent {
            time_ms: 25_000.0, // After 25s, tempo drops to 90 BPM
            bpm: 90.0,
            us_per_beat: 666_667,
        });

    assert_eq!(song.get_bpm_at(0.0), 120.0);
    assert_eq!(song.get_bpm_at(5_000.0), 120.0);
    assert_eq!(song.get_bpm_at(10_000.0), 180.0);
    assert_eq!(song.get_bpm_at(18_000.0), 180.0);
    assert_eq!(song.get_bpm_at(30_000.0), 90.0);

    // Continuous accumulated beats test:
    // 0s to 10s at 120 BPM: (10 / 60) * 120 = 20 beats
    // 10s to 25s at 180 BPM: (15 / 60) * 180 = 45 beats (total 65 beats at 25s)
    // 25s to 35s at 90 BPM: (10 / 60) * 90 = 15 beats (total 80 beats at 35s)
    assert!((song.get_accumulated_beats(0.0) - 0.0).abs() < 1e-4);
    assert!((song.get_accumulated_beats(10_000.0) - 20.0).abs() < 1e-4);
    assert!((song.get_accumulated_beats(25_000.0) - 65.0).abs() < 1e-4);
    assert!((song.get_accumulated_beats(35_000.0) - 80.0).abs() < 1e-4);
}

#[test]
fn test_soundfont_optional_fallback() {
    let mut synth = PianoSynthEngine::new(44100.0);
    // Initially mode is PhysicalModeling
    assert_eq!(
        synth.mode,
        vitl_piano_desktop::core::config::SynthSoundMode::PhysicalModeling
    );

    // Attempting to load non-existent SoundFont should fail gracefully and fall back to built-in physical synth
    let res = synth.load_soundfont("/nonexistent/path/soundfont.sf2");
    assert!(res.is_err());
    assert_eq!(
        synth.mode,
        vitl_piano_desktop::core::config::SynthSoundMode::PhysicalModeling
    );

    // Synthesizer still plays properly with physical modeling
    synth.note_on(60, 100);
    let mut buf = vec![0.0f32; 512];
    synth.process_block(&mut buf);
    let energy: f32 = buf.iter().map(|s| s.abs()).sum();
    assert!(energy > 0.05);
}

#[test]
fn test_musescore_importer_parsing() {
    use vitl_piano_desktop::core::musescore::MusescoreImporter;

    // Test URL parsing
    assert_eq!(
        MusescoreImporter::parse_score_id("https://musescore.com/user/3268481/scores/5475653"),
        Some(5475653)
    );
    assert_eq!(
        MusescoreImporter::parse_score_id("https://musescore.com/score/5475653"),
        Some(5475653)
    );
    assert_eq!(MusescoreImporter::parse_score_id("5475653"), Some(5475653));
    assert_eq!(MusescoreImporter::parse_score_id("invalid-url-no-id"), None);

    // Test LibreScore auth token calculation
    let auth = MusescoreImporter::compute_auth_token(5475653, "9654,4e");
    assert_eq!(auth.len(), 4);
}

#[test]
fn test_midi_note_lengths_handling() {
    let mut song = Song::new("Note Lengths Test".to_string());
    song.tracks.push(vitl_piano_desktop::core::song::Track {
        name: "Piano".to_string(),
        channel: 0,
        notes: vec![
            vitl_piano_desktop::core::song::NoteEvent {
                note: 60, // C4
                velocity: 100,
                start_ms: 0.0,
                duration_ms: 500.0, // Staccato / short note
                track: 0,
                channel: 0,
            },
            vitl_piano_desktop::core::song::NoteEvent {
                note: 64, // E4
                velocity: 100,
                start_ms: 0.0,
                duration_ms: 2000.0, // Sustained whole note
                track: 0,
                channel: 0,
            },
        ],
        is_drum: false,
    });
    song.finalize();

    assert_eq!(song.duration_ms, 2000.0);
    assert_eq!(song.tracks[0].notes[0].duration_ms, 500.0);
    assert_eq!(song.tracks[0].notes[1].duration_ms, 2000.0);

    // Test synth note damper release on duration completion
    let mut synth = PianoSynthEngine::new(44100.0);
    synth.note_on(60, 100);
    synth.note_on(64, 100);

    let mut buf = vec![0.0f32; 512];
    synth.process_block(&mut buf);
    let energy_before: f32 = buf.iter().map(|s| s.abs()).sum();
    assert!(energy_before > 0.05);

    // Release note 60 (duration finished) while note 64 keeps ringing
    synth.note_off(60);
    let mut buf2 = vec![0.0f32; 512];
    synth.process_block(&mut buf2);
    let energy_after: f32 = buf2.iter().map(|s| s.abs()).sum();
    assert!(energy_after > 0.01);
}

#[test]
fn test_velocity_configuration_scaling() {
    let mut config = vitl_piano_desktop::core::config::AppConfig::default();
    assert!(!config.velocity);
    assert_eq!(config.velocity_multiplier, 1.0);
    assert_eq!(config.fixed_velocity, 100);

    config.velocity = true;

    // Test dynamic scaling
    let raw_velocity = 80u8;
    config.velocity_multiplier = 1.25;
    let scaled = (((raw_velocity as f64) * config.velocity_multiplier).round() as i16)
        .clamp(config.min_velocity as i16, config.max_velocity as i16) as u8;
    assert_eq!(scaled, 100);

    // Test fixed velocity when velocity dynamics is turned off
    config.velocity = false;
    config.fixed_velocity = 95;
    let final_vel = if !config.velocity {
        config.fixed_velocity
    } else {
        raw_velocity
    };
    assert_eq!(final_vel, 95);
}

#[test]
fn test_sheet_inline_dynamic_bpm() {
    let sheet_with_bpm_tags = "[8u] w y u [bpm:160] [8o] w y o (bpm=140) [9i] e y i [180bpm] [8u] w {tempo 90} [8o]";
    let song = SheetParser::parse_sheet(
        sheet_with_bpm_tags,
        "Inline BPM Sheet".to_string(),
        Some(120.0),
    )
    .expect("Sheet parse failed");

    assert!(song.tempo_events.len() >= 5);
    assert_eq!(song.tempo_events[0].bpm, 120.0);
    assert_eq!(song.tempo_events[1].bpm, 160.0);
    assert_eq!(song.tempo_events[2].bpm, 140.0);
    assert_eq!(song.tempo_events[3].bpm, 180.0);
    assert_eq!(song.tempo_events[4].bpm, 90.0);
    assert_eq!(song.get_bpm_at(0.0), 120.0);

    // Test sheet export and re-import with dynamic BPM tags preserved
    let exported_sheet = song.to_sheet_text();
    assert!(exported_sheet.contains("[bpm:"));
    let reloaded = SheetParser::parse_sheet(&exported_sheet, "Reloaded".to_string(), None)
        .expect("Reload sheet failed");
    assert!(reloaded.tempo_events.len() >= 4);
}

#[test]
fn test_song_set_bpm_scaling() {
    let sheet = "[8u] w y u [8o] w y o";
    let mut song = SheetParser::parse_sheet(sheet, "Scale BPM".to_string(), Some(120.0)).unwrap();
    let initial_dur = song.duration_ms;
    let initial_note_start = song.tracks[0].notes[3].start_ms;

    // Double the BPM from 120 to 240
    song.set_bpm(240.0);
    assert_eq!(song.bpm, 240.0);
    assert!((song.duration_ms - initial_dur / 2.0).abs() < 1.0);
    assert!((song.tracks[0].notes[3].start_ms - initial_note_start / 2.0).abs() < 1.0);
    assert_eq!(song.tempo_events[0].bpm, 240.0);
}

#[test]
fn test_note_lengths_config_clamping_and_hold() {
    let config = vitl_piano_desktop::core::config::AppConfig::default();
    assert!(config.note_lengths);
    assert_eq!(config.min_note_length_ms, 30.0);
    assert_eq!(config.max_note_length_ms, 5000.0);

    // Test duration clamping
    let short_dur: f64 = 5.0; // 5ms is too short for OS window event loop
    let clamped_short = short_dur.clamp(config.min_note_length_ms, config.max_note_length_ms);
    assert_eq!(clamped_short, 30.0);

    let long_dur: f64 = 12_000.0; // 12s exceeds maximum hold
    let clamped_long = long_dur.clamp(config.min_note_length_ms, config.max_note_length_ms);
    assert_eq!(clamped_long, 5000.0);

    let normal_dur: f64 = 1500.0; // 1.5s quarter note
    let clamped_normal = normal_dur.clamp(config.min_note_length_ms, config.max_note_length_ms);
    assert_eq!(clamped_normal, 1500.0);
}

#[test]
fn test_soundfont_preset_and_discovery() {
    use vitl_piano_desktop::synth::discover_system_soundfonts;

    // Discovery should run safely without panicking on any platform
    let soundfonts = discover_system_soundfonts();
    println!("Discovered {} system/local soundfonts", soundfonts.len());
    assert!(
        !soundfonts.is_empty(),
        "Expected at least 1 soundfont discovered in ./soundfonts"
    );

    let mut synth = PianoSynthEngine::new(44100.0);
    let presets = synth.get_soundfont_presets();
    assert!(presets.is_empty()); // None loaded yet

    // Preset selection when no soundfont loaded returns error gracefully
    let res = synth.set_soundfont_preset(0, 5);
    assert!(res.is_err());

    // Test loading TimGM6mb.sf2
    let sf_path = "soundfonts/TimGM6mb.sf2";
    if std::path::Path::new(sf_path).exists() {
        let load_res = synth.load_soundfont(sf_path);
        assert!(load_res.is_ok());
        let presets = synth.get_soundfont_presets();
        assert!(!presets.is_empty());
        println!("TimGM6mb has {} presets", presets.len());

        // Select preset 0 (Grand Piano) and synthesize note
        let set_res = synth.set_soundfont_preset(0, 0);
        assert!(set_res.is_ok());
        synth.note_on(60, 100);
        let mut buf = vec![0.0f32; 512];
        synth.process_block(&mut buf);
        let energy: f32 = buf.iter().map(|s| s.abs()).sum();
        assert!(energy > 0.01);
    }

    // Config defaults
    let config = vitl_piano_desktop::core::config::AppConfig::default();
    assert_eq!(config.synth.soundfont_bank, 0);
    assert_eq!(config.synth.soundfont_patch, 0);
}

#[test]
fn test_midi_export_and_reparse() {
    use vitl_piano_desktop::core::midi::MidiParser;
    use vitl_piano_desktop::core::song::{NoteEvent, Song, Track};

    let mut song = Song::new("Test Export Song".to_string());
    song.bpm = 140.0;
    let mut track = Track {
        name: "Piano Melody".to_string(),
        channel: 0,
        notes: Vec::new(),
        is_drum: false,
    };

    // Add scale C4, D4, E4, F4, G4, A4, B4, C5
    let pitches = [60, 62, 64, 65, 67, 69, 71, 72];
    for (i, &p) in pitches.iter().enumerate() {
        track.notes.push(NoteEvent {
            note: p,
            velocity: 100,
            start_ms: (i as f64) * 250.0,
            duration_ms: 200.0,
            track: 0,
            channel: 0,
        });
    }
    song.tracks.push(track);
    song.finalize();

    // Export to MIDI bytes
    let midi_bytes = song
        .to_midi_bytes()
        .expect("Should export MIDI bytes successfully");
    assert!(!midi_bytes.is_empty());
    assert_eq!(&midi_bytes[0..4], b"MThd", "Should have valid MIDI header");

    // Parse exported bytes back
    let reloaded = MidiParser::parse_bytes(&midi_bytes, "reloaded.mid".to_string())
        .expect("Should parse generated MIDI back");
    assert_eq!(reloaded.total_notes, 8);
    assert!((reloaded.bpm - 140.0).abs() < 1.0);
}

#[test]
fn test_sheet_generation_and_chords() {
    use vitl_piano_desktop::core::song::{NoteEvent, Song, Track};

    let mut song = Song::new("Fur Elise Snippet".to_string());
    song.bpm = 120.0;
    let mut track = Track {
        name: "Main".to_string(),
        channel: 0,
        notes: Vec::new(),
        is_drum: false,
    };

    // Note 76 = 'f' in VP, Note 75 = 'D' in VP, Chord [60, 64, 67] = [tuo]
    track.notes.push(NoteEvent {
        note: 76,
        velocity: 80,
        start_ms: 0.0,
        duration_ms: 100.0,
        track: 0,
        channel: 0,
    });
    track.notes.push(NoteEvent {
        note: 75,
        velocity: 80,
        start_ms: 150.0,
        duration_ms: 100.0,
        track: 0,
        channel: 0,
    });
    // Chord
    track.notes.push(NoteEvent {
        note: 60,
        velocity: 80,
        start_ms: 300.0,
        duration_ms: 100.0,
        track: 0,
        channel: 0,
    });
    track.notes.push(NoteEvent {
        note: 64,
        velocity: 80,
        start_ms: 300.0,
        duration_ms: 100.0,
        track: 0,
        channel: 0,
    });
    track.notes.push(NoteEvent {
        note: 67,
        velocity: 80,
        start_ms: 300.0,
        duration_ms: 100.0,
        track: 0,
        channel: 0,
    });

    song.tracks.push(track);
    song.finalize();

    let sheet = song.to_sheet_text();
    assert!(sheet.contains("Fur Elise Snippet"));
    assert!(
        sheet.contains("[") && sheet.contains("]"),
        "Should format simultaneous notes as chord"
    );
}

#[test]
fn test_dsp_equalizer_and_delay() {
    use vitl_piano_desktop::synth::dsp::{StereoDelay, ThreeBandEqualizer};

    let mut eq = ThreeBandEqualizer::new(44100.0);
    eq.set_gains(6.0, -3.0, 4.0);
    let (l, r) = eq.process(0.5, 0.5);
    assert!(l.is_finite());
    assert!(r.is_finite());

    let mut delay = StereoDelay::new(44100.0);
    delay.enabled = true;
    delay.delay_time_ms = 100.0;
    delay.feedback = 0.5;
    delay.wet_mix = 0.5;

    let mut out_l = 0.0;
    let mut out_r = 0.0;
    for i in 0..1000 {
        let input = if i == 0 { 1.0 } else { 0.0 };
        let (dl, dr) = delay.process(input, input);
        out_l += dl.abs();
        out_r += dr.abs();
    }
    assert!(
        out_l > 0.1 && out_r > 0.1,
        "Delay should produce feedback tails on both channels"
    );
}

#[test]
fn test_theme_and_visualizer_configs() {
    use vitl_piano_desktop::core::config::AppConfig;

    let mut config = AppConfig::default();
    config.theme.active_theme = "cyberpunk-2077".to_string();
    config.theme.custom_css = "body { filter: contrast(110%); }".to_string();
    config.visualizer.palette = "sakura".to_string();
    config.effects.eq_low = 3.5;
    config.effects.delay_enabled = true;

    let serialized = serde_json::to_string(&config).expect("Serialize config");
    let deserialized: AppConfig = serde_json::from_str(&serialized).expect("Deserialize config");

    assert_eq!(deserialized.theme.active_theme, "cyberpunk-2077");
    assert_eq!(deserialized.visualizer.palette, "sakura");
    assert_eq!(deserialized.effects.eq_low, 3.5);
    assert!(deserialized.effects.delay_enabled);
}

#[test]
fn test_song_quantization_and_transposition() {
    use vitl_piano_desktop::core::song::{NoteEvent, Song, Track};

    let mut song = Song::new("Quantize and Transpose Test".to_string());
    song.bpm = 120.0;
    let track = Track {
        name: "Piano".to_string(),
        channel: 0,
        notes: vec![
            NoteEvent {
                note: 60, // C4
                velocity: 90,
                start_ms: 118.0, // Unquantized close to 125.0
                duration_ms: 240.0,
                track: 0,
                channel: 0,
            },
            NoteEvent {
                note: 64, // E4
                velocity: 90,
                start_ms: 263.0, // Unquantized close to 250.0
                duration_ms: 240.0,
                track: 0,
                channel: 0,
            },
            NoteEvent {
                note: 67, // G4
                velocity: 90,
                start_ms: 495.0, // Unquantized close to 500.0
                duration_ms: 240.0,
                track: 0,
                channel: 0,
            },
        ],
        is_drum: false,
    };
    song.tracks.push(track);
    song.finalize();

    // Test Quantization (1/16 = 125ms)
    song.quantize(125.0);
    assert_eq!(song.tracks[0].notes[0].start_ms, 125.0);
    assert_eq!(song.tracks[0].notes[1].start_ms, 250.0);
    assert_eq!(song.tracks[0].notes[2].start_ms, 500.0);

    // Test Transposition (+2 semitones: C4 -> D4, E4 -> F#4, G4 -> A4)
    song.transpose(2);
    assert_eq!(song.tracks[0].notes[0].note, 62);
    assert_eq!(song.tracks[0].notes[1].note, 66);
    assert_eq!(song.tracks[0].notes[2].note, 69);
    assert_eq!(song.min_note, 62);
    assert_eq!(song.max_note, 69);

    // Test Transposition clamping at boundaries
    song.transpose(100);
    assert_eq!(song.tracks[0].notes[2].note, 108); // Clamped at C8
}

#[test]
fn test_synth_heavy_polyphony_and_all_notes_off() {
    let mut synth = PianoSynthEngine::new(44100.0);

    // Trigger all 88 keys simultaneously
    for p in 21..=108 {
        synth.note_on(p, 100);
    }

    let mut buf = vec![0.0f32; 1024];
    synth.process_block(&mut buf);
    let energy: f32 = buf.iter().map(|s| s.abs()).sum();
    assert!(
        energy > 1.0,
        "Massive polyphony should produce strong energy without crashing"
    );
    for sample in &buf {
        assert!(
            sample.is_finite(),
            "Audio buffer must contain finite samples"
        );
    }

    // Release all notes
    synth.all_notes_off();
    for _ in 0..10 {
        synth.process_block(&mut buf);
    }
    let energy_after: f32 = buf.iter().map(|s| s.abs()).sum();
    assert!(
        energy_after < energy * 0.1,
        "After all_notes_off and decay, energy should drop significantly"
    );
}

#[test]
fn test_client_actions_serialization_roundtrip() {
    use vitl_piano_desktop::server::app::ClientAction;

    let actions = vec![
        ClientAction::Play,
        ClientAction::Pause,
        ClientAction::Stop,
        ClientAction::Seek { time_ms: 15400.0 },
        ClientAction::SetSpeed { speed: 1.25 },
        ClientAction::SetBpm { bpm: 144.0 },
        ClientAction::SetTranspose { transpose: -3 },
        ClientAction::TriggerNote {
            note: 60,
            velocity: 100,
            is_on: true,
        },
        ClientAction::QuantizeSong { grid_ms: 125.0 },
        ClientAction::TransposeSong { semitones: 5 },
        ClientAction::SetEffects {
            eq_low: 2.0,
            eq_mid: -1.0,
            eq_high: 3.0,
            delay_enabled: true,
            delay_time_ms: 300.0,
            delay_feedback: 0.4,
            delay_mix: 0.3,
        },
    ];

    for act in actions {
        let json_str = serde_json::to_string(&act).expect("Serialize ClientAction");
        let parsed: ClientAction =
            serde_json::from_str(&json_str).expect("Deserialize ClientAction");
        let re_json = serde_json::to_string(&parsed).expect("Reserialize ClientAction");
        assert_eq!(json_str, re_json);
    }
}

#[test]
fn test_black_midi_detection_and_metrics() {
    // 1. Normal song: 10 notes over 5 seconds
    let mut normal_song = Song::new("Normal Song".to_string());
    let mut normal_notes = Vec::new();
    for i in 0..10 {
        normal_notes.push(NoteEvent {
            note: 60 + (i % 12),
            velocity: 80,
            start_ms: (i as f64) * 500.0,
            duration_ms: 250.0,
            track: 0,
            channel: 0,
        });
    }
    normal_song.tracks.push(vitl_piano_desktop::core::song::Track {
        name: "Piano".to_string(),
        channel: 0,
        notes: normal_notes,
        is_drum: false,
    });
    normal_song.finalize();

    assert!(!normal_song.is_black_midi());
    assert!((normal_song.note_density() - 2.0).abs() < 0.5);

    // 2. Black MIDI by note count: 16,000 notes
    let mut dense_song = Song::new("Black MIDI 16k".to_string());
    let mut dense_notes = Vec::with_capacity(16_000);
    for i in 0..16_000 {
        dense_notes.push(NoteEvent {
            note: 21 + ((i % 88) as u8),
            velocity: 90,
            start_ms: (i as f64) * 10.0,
            duration_ms: 50.0,
            track: 0,
            channel: 0,
        });
    }
    dense_song.tracks.push(vitl_piano_desktop::core::song::Track {
        name: "Black MIDI Track".to_string(),
        channel: 0,
        notes: dense_notes,
        is_drum: false,
    });
    dense_song.finalize();

    assert_eq!(dense_song.total_notes, 16_000);
    assert!(dense_song.is_black_midi());

    // 3. Black MIDI by density rate: 300 notes in 1 second
    let mut fast_burst_song = Song::new("Burst Song".to_string());
    let mut burst_notes = Vec::new();
    for i in 0..300 {
        burst_notes.push(NoteEvent {
            note: 21 + ((i % 88) as u8),
            velocity: 100,
            start_ms: (i as f64) * 3.0,
            duration_ms: 20.0,
            track: 0,
            channel: 0,
        });
    }
    fast_burst_song.tracks.push(vitl_piano_desktop::core::song::Track {
        name: "Burst Track".to_string(),
        channel: 0,
        notes: burst_notes,
        is_drum: false,
    });
    fast_burst_song.finalize();

    assert!(fast_burst_song.is_black_midi());
    assert!(fast_burst_song.note_density() >= 200.0);
    assert!(fast_burst_song.peak_note_density() >= 250);
}

#[test]
fn test_synth_under_high_polyphony_burst() {
    let mut synth = PianoSynthEngine::new(44100.0);
    synth.volume = 0.8;

    // Fire 200 notes simultaneously to stress test voice stealing and allocation
    for i in 0..200 {
        let pitch = 21 + ((i % 88) as u8);
        let vel = (40 + (i % 80)) as u8;
        synth.note_on(pitch, vel);
    }

    // Render audio block: must not panic, must produce finite valid floats
    let mut buffer = vec![0.0f32; 1024];
    synth.process_block(&mut buffer);

    for &sample in &buffer {
        assert!(sample.is_finite(), "Audio sample is NaN or infinite!");
        assert!(sample >= -1.5 && sample <= 1.5, "Audio sample out of soft limiter range: {}", sample);
    }

    // Turn off notes
    for i in 0..200 {
        let pitch = 21 + ((i % 88) as u8);
        synth.note_off(pitch);
    }
}

#[test]
fn test_black_midi_config_defaults() {
    use vitl_piano_desktop::core::config::{AppConfig, BlackMidiConfig};

    let cfg = AppConfig::default();
    assert!(cfg.black_midi.enabled);
    assert!(cfg.black_midi.auto_detect);
    assert_eq!(cfg.black_midi.voice_limit, 96);
    assert_eq!(cfg.black_midi.max_macro_rate, 600);
    assert_eq!(cfg.black_midi.low_velocity_cull, 8);
    assert!(cfg.black_midi.visual_lod);

    // Test JSON roundtrip
    let json = serde_json::to_string(&cfg.black_midi).expect("serialize black midi config");
    let parsed: BlackMidiConfig = serde_json::from_str(&json).expect("deserialize black midi config");
    assert_eq!(parsed.voice_limit, 96);
}

