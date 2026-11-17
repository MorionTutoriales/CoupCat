// SPDX-License-Identifier: AGPL-3.0-or-later
use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[test]
fn transcribe() {
    let (Ok(model), Ok(wav)) = (
        std::env::var("COUPCAT_WHISPER_MODEL"),
        std::env::var("COUPCAT_SPEECH"),
    ) else {
        eprintln!("skip: set COUPCAT_WHISPER_MODEL + COUPCAT_SPEECH");
        return;
    };
    concat_media::init();

    let opts = AudioOptions {
        start: None,
        duration: None,
        filters: vec![],
        rate: 16000,
        channels: 1,
        format: SampleFormat::F32,
        stream: None,
    };
    let mut dec = AudioDecoder::open(&wav, &opts).expect("open audio");
    let pcm = dec.collect_f32().expect("decode pcm");
    eprintln!("[asr] {} samples ({:.1}s @16k)", pcm.len(), pcm.len() as f32 / 16000.0);

    let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
        .expect("load model");
    let mut state = ctx.create_state().expect("state");
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("es"));
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    state.full(params, &pcm).expect("transcribe");

    let n = state.full_n_segments();
    for i in 0..n {
        if let Some(seg) = state.get_segment(i) {
            let text = seg.to_str_lossy().map(|c| c.into_owned()).unwrap_or_default();
            eprintln!(
                "[asr] [{:.2}-{:.2}s] {}",
                seg.start_timestamp() as f32 / 100.0,
                seg.end_timestamp() as f32 / 100.0,
                text.trim()
            );
        }
    }
    assert!(n > 0, "no segments");
}
