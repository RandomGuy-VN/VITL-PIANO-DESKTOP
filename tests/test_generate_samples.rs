use vitl_piano_desktop::core::midi::MidiParser;
use vitl_piano_desktop::core::sheet::SheetParser;
use std::fs;
use std::path::Path;

#[test]
fn generate_sample_midis() {
    let samples = [
        ("samples/fur_elise.txt", "samples/fur_elise.mid", "Fur Elise", 130.0),
        ("samples/rush_e.txt", "samples/rush_e.mid", "Rush E", 160.0),
        ("samples/canon_in_d.txt", "samples/canon_in_d.mid", "Canon in D", 90.0),
    ];

    for &(txt_path, mid_path, title, bpm) in &samples {
        if let Ok(content) = fs::read_to_string(txt_path) {
            if let Ok(song) = SheetParser::parse_sheet(&content, title.to_string(), Some(bpm)) {
                if let Ok(midi_bytes) = MidiParser::export_to_midi(&song) {
                    let _ = fs::write(mid_path, midi_bytes);
                }
            }
        }
    }
}
