//! Dependency-gated live FFmpeg / FFprobe integration tests.
//!
//! These tests are marked `#[ignore]` so `cargo test --workspace` never silently reports
//! them as passed when `ffmpeg` or `ffprobe` is missing. Run them explicitly on hosts with
//! FFmpeg installed:
//!
//!   cargo test -p refract-engine --test audio_ffmpeg_integration -- --ignored

use refract_core::cancel::CancelToken;
use refract_core::error::ErrorCode;
use refract_core::format::Format;
use refract_engine::audio;
use refract_engine::process::run_command_cancellable;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn require_audio_toolchain() {
    let deps = audio::detect_audio_dependencies();
    assert!(
        deps.is_ready(),
        "Live audio integration test requires ffmpeg and ffprobe on PATH (missing: {:?})",
        deps.missing()
    );
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_ffprobe_measures_all_audio_fixtures() {
    require_audio_toolchain();
    for name in ["sound.wav", "sound.mp3", "sound.m4a", "sound.ogg"] {
        let dur = audio::probe_duration(&fixture_path(name))
            .unwrap_or_else(|e| panic!("ffprobe failed for {}: {:?}", name, e));
        assert!(
            (2.5..=3.5).contains(&dur),
            "expected ~3s duration for {}, got {:.3}s",
            name,
            dur
        );
    }
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_convert_audio_wav_to_mp3_and_mp3_to_wav_with_bitrate() {
    require_audio_toolchain();
    let dir = tempfile::tempdir().unwrap();

    let out_mp3 = dir.path().join("converted.mp3");
    audio::convert_audio(&fixture_path("sound.wav"), &out_mp3, Format::Mp3, Some(256))
        .expect("WAV -> MP3");
    let mp3_bytes = std::fs::read(&out_mp3).unwrap();
    assert_eq!(refract_core::detect(&mp3_bytes), Some(Format::Mp3));
    let mp3_dur = audio::probe_duration(&out_mp3).unwrap();
    assert!((2.5..=3.5).contains(&mp3_dur));

    let out_wav = dir.path().join("converted.wav");
    audio::convert_audio(&fixture_path("sound.mp3"), &out_wav, Format::Wav, Some(192))
        .expect("MP3 -> WAV");
    let wav_bytes = std::fs::read(&out_wav).unwrap();
    assert_eq!(refract_core::detect(&wav_bytes), Some(Format::Wav));
    let wav_dur = audio::probe_duration(&out_wav).unwrap();
    assert!((2.5..=3.5).contains(&wav_dur));
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_trim_audio_explicit_end_eof_zero_and_out_of_bounds_validation() {
    require_audio_toolchain();
    let dir = tempfile::tempdir().unwrap();
    let wav_path = fixture_path("sound.wav");

    // 1. Explicit start & end: 0.2s -> 0.6s (~0.4s clip)
    let out_clip = dir.path().join("clip.wav");
    audio::trim_audio_cancellable(
        &wav_path,
        &out_clip,
        Some("0.2"),
        Some("0.6"),
        None,
        &CancelToken::none(),
    )
    .expect("trim 0.2..0.6");
    let clip_bytes = std::fs::read(&out_clip).unwrap();
    assert_eq!(refract_core::detect(&clip_bytes), Some(Format::Wav));
    let clip_dur = audio::probe_duration(&out_clip).unwrap();
    assert!(
        (0.3..=0.55).contains(&clip_dur),
        "expected ~0.4s clip, got {:.3}s",
        clip_dur
    );

    // 2. end = "0" means EOF: 1.0s -> EOF (~2.0s clip) with MP3 + bitrate
    let mp3_path = fixture_path("sound.mp3");
    let out_eof = dir.path().join("eof.mp3");
    audio::trim_audio_cancellable(
        &mp3_path,
        &out_eof,
        Some("1.0"),
        Some("0"),
        Some(192),
        &CancelToken::none(),
    )
    .expect("trim 1.0..EOF");
    let eof_bytes = std::fs::read(&out_eof).unwrap();
    assert_eq!(refract_core::detect(&eof_bytes), Some(Format::Mp3));
    let eof_dur = audio::probe_duration(&out_eof).unwrap();
    assert!(
        (1.7..=2.3).contains(&eof_dur),
        "expected ~2.0s EOF trim, got {:.3}s",
        eof_dur
    );

    // 3. Start past actual media duration (~3s) is rejected cleanly
    let out_bad = dir.path().join("bad.wav");
    let err = audio::trim_audio_cancellable(
        &wav_path,
        &out_bad,
        Some("10.0"),
        Some("0"),
        None,
        &CancelToken::none(),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidOption);
    assert!(!out_bad.exists());
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_ffmpeg_mid_process_cancellation_kills_process_and_cleans_temp_files() {
    require_audio_toolchain();
    let dir = tempfile::tempdir().unwrap();
    let partial_out = dir.path().join("canceled.mp3");
    let keep_existing = dir.path().join("keep.mp3");
    std::fs::write(&keep_existing, b"original-data").unwrap();

    let deps = audio::detect_audio_dependencies();
    let ffmpeg = deps.ffmpeg.unwrap();

    // Generate a 600-second synthetic sine stream with real-time throttling (-re)
    // so FFmpeg stays running long enough to exercise genuine mid-process cancellation.
    let token = CancelToken::new();
    let canceller = token.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        canceller.cancel();
    });

    let started = Instant::now();
    let err = run_command_cancellable(
        &ffmpeg,
        [
            "-y",
            "-re",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=600",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "192k",
            partial_out.to_str().unwrap(),
        ],
        &token,
        std::slice::from_ref(&partial_out),
        "ffmpeg",
    )
    .unwrap_err();

    assert_eq!(err.code, ErrorCode::Canceled);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "canceled ffmpeg took too long to stop: {:?}",
        started.elapsed()
    );
    assert!(
        !partial_out.exists(),
        "partial output file must be removed after cancellation"
    );
    assert_eq!(
        std::fs::read(&keep_existing).unwrap(),
        b"original-data",
        "existing files must remain untouched"
    );
}
