// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Copy the runtime dependencies next to the built exe, so `cargo run` and the
//! packaged build both find them without hand-copying: FFmpeg (DLLs and program),
//! ONNX Runtime (loaded dynamically for background removal), and the Whisper
//! caption model. Sources live under `third_party/` (machine-local, gitignored).
//! Idempotent: a file already present and up to date is left alone.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    // Embed the app icon into the exe (Explorer / taskbar).
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        let _ = res.compile();
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // crates/coupcat-desktop -> crates -> src -> <repo root>
    let root = manifest.join("..").join("..").join("..");
    let third = root.join("third_party");

    // The profile dir (target/<profile>) holds the exe: OUT_DIR is
    // target/<profile>/build/<pkg>-<hash>/out.
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out.ancestors().nth(3) else {
        return;
    };

    let ffmpeg_bin = third
        .join("ffmpeg")
        .join("ffmpeg-master-latest-win64-gpl-shared")
        .join("bin");
    if ffmpeg_bin.is_dir() {
        if let Ok(entries) = fs::read_dir(&ffmpeg_bin) {
            for entry in entries.flatten() {
                let p = entry.path();
                // ffmpeg.exe records the screen (see recorder.rs).
                let program = p.file_name().is_some_and(|n| n == "ffmpeg.exe");
                if program || p.extension().and_then(|e| e.to_str()) == Some("dll") {
                    copy_if_newer(&p, &profile_dir.join(p.file_name().unwrap()));
                }
            }
        }
    }

    copy_if_newer(
        &third
            .join("onnxruntime")
            .join("onnxruntime-win-x64-1.32.0")
            .join("lib")
            .join("onnxruntime.dll"),
        &profile_dir.join("onnxruntime.dll"),
    );

    copy_if_newer(
        &third.join("whisper").join("ggml-base.bin"),
        &profile_dir.join("ggml-base.bin"),
    );
}

/// Copy `src` to `dst` unless `dst` already exists with the same size.
fn copy_if_newer(src: &Path, dst: &Path) {
    if !src.exists() {
        return;
    }
    if let (Ok(a), Ok(b)) = (fs::metadata(src), fs::metadata(dst)) {
        if a.len() == b.len() {
            return;
        }
    }
    let _ = fs::copy(src, dst);
}
