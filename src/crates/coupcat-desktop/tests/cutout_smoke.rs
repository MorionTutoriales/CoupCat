// SPDX-License-Identifier: AGPL-3.0-or-later
use concat_media::{DecodeOptions, Decoder, FrameSource};

#[test]
fn cutout_inference() {
    let Ok(dll) = std::env::var("COUPCAT_ORT_DLL") else { eprintln!("skip: no COUPCAT_ORT_DLL"); return; };
    let Ok(input) = std::env::var("COUPCAT_SMOKE_INPUT") else { eprintln!("skip"); return; };
    // Point ort's load-dynamic at the bundled ONNX Runtime.
    unsafe { std::env::set_var("ORT_DYLIB_PATH", &dll); }
    concat_media::init();

    let opts = DecodeOptions { max_frames: Some(1), ..Default::default() };
    let mut dec = Decoder::open(&input, &opts).expect("open");
    let frame = dec.next_frame().expect("decode").expect("a frame");

    let seg = concat_vision::Segmenter::selfie().expect("load compiled-in selfie model");
    seg.begin();
    let mask = seg.mask(&frame).expect("run inference");
    eprintln!("[cutout] input {}x{} -> mask {}x{} blank={}", frame.width(), frame.height(), mask.width(), mask.height(), mask.is_blank());
    assert!(mask.width() > 0 && mask.height() > 0);
}
