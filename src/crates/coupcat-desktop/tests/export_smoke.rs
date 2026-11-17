// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Headless smoke test for the export pipeline: build a one-clip project with
//! the engine, flatten it, and render a real MP4 (wgpu compositor + H.264
//! encode). Skipped unless `COUPCAT_SMOKE_INPUT` points at a video file.

use std::sync::atomic::AtomicBool;

use concat_project::commands::NewMedia;
use concat_project::model::MediaKind;
use concat_project::{Command, Editor};

#[test]
fn export_smoke() {
    let Ok(input) = std::env::var("COUPCAT_SMOKE_INPUT") else {
        eprintln!("skip: set COUPCAT_SMOKE_INPUT to a video file to run this");
        return;
    };
    let output =
        std::env::var("COUPCAT_SMOKE_OUTPUT").unwrap_or_else(|_| "coupcat_smoke_out.mp4".to_string());

    concat_media::init();
    let info = concat_media::probe(&input).expect("probe input");
    let v = info.video.clone().expect("input has a video stream");
    let fps = v.frame_rate.fps();

    let item = NewMedia {
        path: input.clone(),
        name: "smoke".to_string(),
        duration: info.duration.map(|r| r.as_f64()),
        kind: MediaKind::Video,
        width: Some(v.width),
        height: Some(v.height),
        frame_rate: Some(fps.as_f64()),
        frame_rate_fraction: Some(format!("{}/{}", fps.numerator(), fps.denominator())),
        video_codec: Some(v.codec.clone()),
        audio_codec: info.audio.as_ref().map(|a| a.codec.clone()),
        has_audio: info.audio.is_some(),
        audio_tracks: Vec::new(),
        origin: None,
        color_space: Default::default(),
    };

    let mut editor = Editor::new();
    let media_id = editor
        .apply(Command::AddMedia { item })
        .unwrap()
        .created_id
        .unwrap();
    editor
        .apply(Command::AddClipAtFirstFree {
            media_id,
            start: 0.0,
        })
        .unwrap();

    let clips = concat_export::flatten::flatten_timeline(editor.project(), None);
    assert!(!clips.is_empty(), "flatten produced no clips");

    let vs = editor.project().active().video;
    let request = concat_export::ExportRequest {
        output: output.clone(),
        width: vs.width,
        height: vs.height,
        rate_num: vs.rate_num,
        rate_den: vs.rate_den,
        crf: 23,
        preset: "veryfast".to_string(),
        codec: Default::default(),
        ten_bit: false,
        rate_mode: Default::default(),
        bitrate_kbps: 0,
        color_range: Default::default(),
        color_space: vs.color_space,
        hdr: false,
        effect_time_offset: 0.0,
        canvas: vs.canvas,
        clips,
    };

    let cancel = AtomicBool::new(false);
    let mut progress = |frame: i64, total: i64, stage: &'static str| {
        eprintln!("[export] {frame}/{total} {stage}");
    };
    let reporter = concat_export::Reporter {
        progress: &mut progress,
        cancel: &cancel,
    };

    let written = concat_export::render(&request, reporter).expect("render should succeed");
    eprintln!("[export] wrote {written}");
    let meta = std::fs::metadata(&written).expect("output file exists");
    assert!(meta.len() > 0, "output is empty");
    eprintln!("[export] output size = {} bytes", meta.len());
}

/// Smoke test for the composed-monitor path: compose one timeline frame at an
/// instant with `preview_frame` and check it returns a full RGBA buffer.
#[test]
fn preview_smoke() {
    let Ok(input) = std::env::var("COUPCAT_SMOKE_INPUT") else {
        eprintln!("skip: set COUPCAT_SMOKE_INPUT to a video file to run this");
        return;
    };
    concat_media::init();
    let info = concat_media::probe(&input).expect("probe input");
    let v = info.video.clone().expect("input has a video stream");
    let fps = v.frame_rate.fps();

    let item = NewMedia {
        path: input,
        name: "smoke".to_string(),
        duration: info.duration.map(|r| r.as_f64()),
        kind: MediaKind::Video,
        width: Some(v.width),
        height: Some(v.height),
        frame_rate: Some(fps.as_f64()),
        frame_rate_fraction: Some(format!("{}/{}", fps.numerator(), fps.denominator())),
        video_codec: Some(v.codec.clone()),
        audio_codec: info.audio.as_ref().map(|a| a.codec.clone()),
        has_audio: info.audio.is_some(),
        audio_tracks: Vec::new(),
        origin: None,
        color_space: Default::default(),
    };
    let mut editor = Editor::new();
    let media_id = editor
        .apply(Command::AddMedia { item })
        .unwrap()
        .created_id
        .unwrap();
    editor
        .apply(Command::AddClipAtFirstFree {
            media_id,
            start: 0.0,
        })
        .unwrap();

    let clips = concat_export::flatten::flatten_timeline(editor.project(), None);
    let vs = editor.project().active().video;
    let (w, h) = (640u32, 360u32);
    let request = concat_export::PreviewFrameRequest {
        time: 0.5,
        width: w,
        height: h,
        rate_num: vs.rate_num,
        rate_den: vs.rate_den,
        clips,
        color_space: vs.color_space,
        canvas: vs.canvas,
    };
    let pool = concat_media::ReaderPool::new(64 << 20, 4);
    let rgba = concat_export::preview_frame(&pool, &request).expect("preview_frame should succeed");
    assert_eq!(rgba.len(), (w * h * 4) as usize, "RGBA buffer size");
    eprintln!("[preview] composed {w}x{h}, {} bytes", rgba.len());
}
