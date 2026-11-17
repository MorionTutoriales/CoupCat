# CoupCat

**A free, open-source video editor built in Rust** — CapCut-like editing, lots of
filters, no paywalls. CoupCat follows Artcraft's *"native, not wrapped"*
philosophy: a real desktop app, not a browser in a box. A Web App build (for
Android and other phones) is a planned target from the same codebase.

> **Status:** early scaffolding. The editing engine compiles and runs; the
> CoupCat user interface is not built yet. See the roadmap below.

## Where the code comes from

CoupCat does not start from scratch. It **forks the Rust engine** of
[`artcraft-android/capcraft-android`](https://github.com/artcraft-android/capcraft-android)
(itself built on the *Concat* engine by jub0t and contributors) — a complete,
platform-agnostic video engine split into `concat-*` crates. We keep that
engine and **replace the Android/Kotlin UI with a new desktop/web UI in
[Dioxus](https://dioxuslabs.com/)**.

[`jub0t/OpenCut`](https://github.com/jub0t/OpenCut) was evaluated too, but it is
a TypeScript rewrite-in-progress, so it serves only as design reference.

**License:** AGPL-3.0-or-later, inherited from the upstream engine. CoupCat
stays free and open source. See `LICENSE`, `NOTICE`, and `THIRD_PARTY_NOTICES.md`.

## The engine (`src/crates/`)

| Crate | Role |
|-------|------|
| `concat-core` | Time, frames and the timeline model shared by everything |
| `concat-project` | Project and multi-track timeline data |
| `concat-media` | Decode/encode via **FFmpeg** (the only crate that knows FFmpeg exists) |
| `concat-render` | Turning a timeline + timestamp into one frame (**wgpu**) |
| `concat-effects` | Filters and effects, drawn by shaders |
| `concat-text` | Titles and text |
| `concat-export` | Encoding the finished output |
| `concat-host` | Sessions, project folders, previews, playback *(AI — parked, see below)* |
| `concat-vision` | Cutout / background-removal models *(AI)* |
| `concat-speech` | On-device captions, whisper.cpp + sherpa-onnx *(AI — parked)* |

## Building on Windows

Build with the **MSVC toolchain**, from a Visual Studio Developer shell (it
needs cl.exe/link.exe and the MSVC headers for FFmpeg, ONNX Runtime and
whisper.cpp). The pinned toolchain (`rust-toolchain.toml`) resolves to the MSVC
host; `src/.cargo/config.toml` wires `FFMPEG_DIR` and `libclang`.

```powershell
$vc = "C:\Program Files\Microsoft Visual Studio\2022\Community"
Import-Module (Join-Path $vc "Common7\Tools\Microsoft.VisualStudio.DevShell.dll")
Enter-VsDevShell -VsInstallPath $vc -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64"
cd src
cargo run -p coupcat-desktop
```

Local dependencies under `third_party/` (gitignored, machine-downloaded): an
FFmpeg **shared dev build** (BtbN), **ONNX Runtime** (`onnxruntime-win-x64`, for
background removal — loaded dynamically), and a **Whisper model** (`ggml-base.bin`,
for captions). `build.rs` copies the DLLs and the model next to the exe
automatically, and `scripts\package.ps1` assembles a self-contained
`dist\CoupCat\` for distribution. `libclang` comes from Doxygen.

## Roadmap

- **M0 — Scaffolding ✅** Fork, Windows toolchain, FFmpeg wired, core engine compiles.
- **M1 — Desktop shell ✅** Dioxus window: import, timeline + preview playback.
- **M2 — Core editing ✅** Trim / split / move (mouse drag), multi-track, undo/redo,
  composed preview, export MP4.
- **M3 — Filters & effects ✅** 162 shader effects with adjustable parameters,
  text/titles, CapCut-style centered transitions.
- **M4 — On-device AI ✅** Background removal (person cutout) and automatic
  captions (whisper.cpp), both local. Settled on the MSVC toolchain.
- **M5 — Web App.** WASM target (WebCodecs / WebGPU) for Android browsers.
- **M6 — Polish & packaging.** In progress: auto-bundled runtime deps, release
  packaging script, window sizing. Still to do: installer, app icon/branding.
