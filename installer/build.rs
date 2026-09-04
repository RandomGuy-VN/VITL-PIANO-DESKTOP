use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use flate2::write::GzEncoder;
use flate2::Compression;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("payload.bin.gz");

    let mut files_to_pack: Vec<(String, Vec<u8>)> = Vec::new();

    // 1. Locate the main executable
    let exe_candidates = [
        "../src-tauri/target/release/vitl-piano.exe",
        "../src-tauri/target/release/app.exe",
        "../src-tauri/target/debug/vitl-piano.exe",
    ];
    let mut exe_data = None;
    for cand in exe_candidates {
        if Path::new(cand).exists() {
            if let Ok(mut f) = File::open(cand) {
                let mut buf = Vec::new();
                if f.read_to_end(&mut buf).is_ok() {
                    exe_data = Some(buf);
                    break;
                }
            }
        }
    }

    if let Some(data) = exe_data {
        files_to_pack.push(("vitl-piano.exe".to_string(), data));
    } else {
        panic!("Could not find built vitl-piano.exe! Run cargo build in src-tauri first.");
    }

    // 2. WebView2Loader.dll
    let dll_candidates = [
        "../src-tauri/WebView2Loader.dll",
        "../src-tauri/target/release/WebView2Loader.dll",
    ];
    for cand in dll_candidates {
        if Path::new(cand).exists() {
            if let Ok(mut f) = File::open(cand) {
                let mut buf = Vec::new();
                if f.read_to_end(&mut buf).is_ok() {
                    files_to_pack.push(("WebView2Loader.dll".to_string(), buf));
                    break;
                }
            }
        }
    }

    // 3. Logo Icon
    let icon_path = "../src-tauri/icons/icon.ico";
    if Path::new(icon_path).exists() {
        if let Ok(mut f) = File::open(icon_path) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                files_to_pack.push(("app_icon.ico".to_string(), buf));
            }
        }
    }

    // 4. Logo PNG
    let logo_png = "../vitl-brand-logo.png";
    if Path::new(logo_png).exists() {
        if let Ok(mut f) = File::open(logo_png) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                files_to_pack.push(("vitl-brand-logo.png".to_string(), buf));
            }
        }
    }

    // 5. Samples
    if let Ok(entries) = std::fs::read_dir("../samples") {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
                    if let Ok(mut f) = File::open(&p) {
                        let mut buf = Vec::new();
                        if f.read_to_end(&mut buf).is_ok() {
                            files_to_pack.push((format!("samples/{}", fname), buf));
                        }
                    }
                }
            }
        }
    }

    // Pack into custom binary format and compress
    let file = File::create(&dest_path).unwrap();
    let mut encoder = GzEncoder::new(BufWriter::new(file), Compression::best());

    // Write file count (u32)
    encoder.write_all(&(files_to_pack.len() as u32).to_le_bytes()).unwrap();

    for (rel_path, data) in &files_to_pack {
        let path_bytes = rel_path.as_bytes();
        encoder.write_all(&(path_bytes.len() as u32).to_le_bytes()).unwrap();
        encoder.write_all(path_bytes).unwrap();
        encoder.write_all(&(data.len() as u64).to_le_bytes()).unwrap();
        encoder.write_all(data).unwrap();
    }

    encoder.finish().unwrap();

    println!("cargo:rerun-if-changed=../src-tauri/target/release/vitl-piano.exe");
    println!("cargo:rerun-if-changed=../samples");
    println!("cargo:rerun-if-changed=web");

    tauri_build::build();
}
