//! Build script: compiles the native C++ macro backend into the binary.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if !cfg!(target_os = "linux") {
        // Native backend is Linux-only; rdev handles other platforms.
        return;
    }

    println!("cargo:rerun-if-changed=native/macro_backend.cpp");
    println!("cargo:rerun-if-env-changed=CXX");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let src = PathBuf::from("native/macro_backend.cpp");
    let obj = out_dir.join("macro_backend.o");
    let lib = out_dir.join("libvitl_macro.a");

    let cxx = env::var("CXX").unwrap_or_else(|_| "g++".to_string());

    // Compile
    let status = Command::new(&cxx)
        .args([
            "-O2",
            "-std=c++17",
            "-fPIC",
            "-Wall",
            "-c",
        ])
        .arg(&src)
        .arg("-o")
        .arg(&obj)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {} (needed to build the native macro backend): {}", cxx, e));
    if !status.success() {
        panic!("C++ compilation of native/macro_backend.cpp failed");
    }

    // Archive into a static library
    let status = Command::new("ar")
        .args(["crs"])
        .arg(&lib)
        .arg(&obj)
        .status()
        .expect("failed to run ar (binutils)");
    if !status.success() {
        panic!("archiving libvitl_macro.a failed");
    }

    // Search path for the archive; the actual linkage is declared via
    // #[link(name = "vitl_macro")] so it propagates from the lib unit to
    // every binary that depends on it.
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rerun-if-changed=build.rs");

    stage_bundled_libs(&out_dir);
}

/// Stages bundled runtime libraries (shims for system version mismatches,
/// e.g. `lib/libjxl.so.0.12` expected by the installed libwebkit2gtk) next to
/// the built binaries, where they are found via RUNPATH `$ORIGIN/lib`.
fn stage_bundled_libs(out_dir: &PathBuf) {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let src_lib = manifest_dir.join("lib");
    if !src_lib.is_dir() {
        return;
    }

    // Cargo layout guarantee: <target>/<profile>/build/<pkg>/out
    //   ancestors(): 0=out, 1=<pkg>, 2=build, 3=<target>/<profile>
    let layout_ok = out_dir
        .ancestors()
        .nth(2)
        .map(|d| d.file_name().map(|n| n == "build").unwrap_or(false))
        .unwrap_or(false);
    if !layout_ok {
        return; // unexpected layout; skip staging silently
    }
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };

    let dest = profile_dir.join("lib");
    std::fs::create_dir_all(&dest).expect("failed to create bundle lib dir");

    println!("cargo:rerun-if-changed={}", src_lib.display());
    for entry in std::fs::read_dir(&src_lib).expect("failed to read lib/") {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let dst = dest.join(entry.file_name());
        let src_link = std::fs::read_link(entry.path());
        let dst_link = std::fs::read_link(&dst);

        // Refresh when missing or pointing somewhere else.
        if let (Ok(src_target), Ok(dst_target)) = (src_link.as_ref(), dst_link.as_ref()) {
            if src_target == dst_target {
                continue;
            }
        }
        if src_link.is_err() && !dst_link.is_ok() && dst.exists() {
            continue; // already a copied regular file
        }

        let _ = std::fs::remove_file(&dst);
        if let Ok(target) = src_link {
            #[cfg(unix)]
            {
                if std::os::unix::fs::symlink(target, &dst).is_ok() {
                    continue;
                }
            }
            // Non-unix or symlink failed: fall through and copy content.
            let _ = std::fs::copy(entry.path(), &dst);
        } else {
            let _ = std::fs::copy(entry.path(), &dst);
        }
    }
}
