//! Dependency-gated live FFmpeg / FFprobe integration tests.
//!
//! These tests are marked `#[ignore]` so `cargo test --workspace` never silently reports
//! them as passed when `ffmpeg` or `ffprobe` is missing. Run them explicitly on hosts with
//! FFmpeg installed. Encoding tests require the libmp3lame encoder:
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

fn require_audio_toolchain(mp3: bool) {
    let deps = audio::detect_audio_dependencies();
    assert!(
        deps.is_ready(),
        "Live audio integration test requires ffmpeg and ffprobe on PATH (missing: {:?})",
        deps.missing()
    );
    if mp3 {
        let output = std::process::Command::new(deps.ffmpeg.as_ref().unwrap())
            .args(["-hide_banner", "-encoders"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|s| s == "libmp3lame"),
            "Live encoding tests require FFmpeg built with the libmp3lame encoder"
        );
    }
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_ffprobe_measures_all_audio_fixtures() {
    require_audio_toolchain(false);
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
    require_audio_toolchain(true);
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

    // Pre-existing destination must never be overwritten on subsequent conversion
    let sentinel = dir.path().join("preexisting.mp3");
    std::fs::write(&sentinel, b"do-not-clobber-convert").unwrap();
    let collision = audio::convert_audio(
        &fixture_path("sound.wav"),
        &sentinel,
        Format::Mp3,
        Some(192),
    )
    .unwrap_err();
    assert_eq!(collision.details.as_deref(), Some("destination_exists"));
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"do-not-clobber-convert");

    // No temporary artifacts remain after success or collision
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".refract-audio-")
        })
        .collect();
    assert!(leftovers.is_empty(), "leftover temp files: {:?}", leftovers);
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_trim_audio_explicit_end_eof_zero_and_out_of_bounds_validation() {
    require_audio_toolchain(true);
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

    let out_mp3 = dir.path().join("clip.mp3");
    audio::trim_audio_cancellable(
        &wav_path,
        &out_mp3,
        Some("0.2"),
        Some("0.6"),
        None,
        &CancelToken::none(),
    )
    .expect("WAV to MP3 trim");
    assert_eq!(
        refract_core::detect(&std::fs::read(&out_mp3).unwrap()),
        Some(Format::Mp3)
    );
    assert!((0.3..0.6).contains(&audio::probe_duration(&out_mp3).unwrap()));

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

    // 4. Pre-existing destination is preserved on trim collision and leaves zero temp files
    let sentinel = dir.path().join("preexisting_trim.wav");
    std::fs::write(&sentinel, b"do-not-clobber-trim").unwrap();
    let trim_collision = audio::trim_audio_cancellable(
        &wav_path,
        &sentinel,
        Some("0.2"),
        Some("0.6"),
        None,
        &CancelToken::none(),
    )
    .unwrap_err();
    assert_eq!(
        trim_collision.details.as_deref(),
        Some("destination_exists")
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"do-not-clobber-trim");

    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".refract-audio-")
        })
        .collect();
    assert!(leftovers.is_empty(), "leftover temp files: {:?}", leftovers);
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_ffmpeg_mid_process_cancellation_kills_process_and_cleans_temp_files() {
    require_audio_toolchain(true);
    let dir = tempfile::tempdir().unwrap();
    let partial_out = dir.path().join("canceled.mp3");
    let keep_existing = dir.path().join("keep.mp3");
    let unrelated_temp = dir.path().join(".refract-audio-unrelated.mp3");
    std::fs::write(&keep_existing, b"original-data").unwrap();
    std::fs::write(&unrelated_temp, b"unrelated-temp-data").unwrap();

    let deps = audio::detect_audio_dependencies();
    let ffmpeg = deps.ffmpeg.unwrap();

    // Generate a 600-second synthetic sine stream with real-time throttling (-re)
    // so FFmpeg stays running long enough to exercise genuine mid-process cancellation.
    let token = CancelToken::new();
    let canceller = token.clone();
    let ready_path = partial_out.clone();
    let readiness = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !std::fs::metadata(&ready_path).is_ok_and(|m| m.len() > 0) {
            if Instant::now() >= deadline {
                canceller.cancel();
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        canceller.cancel();
        true
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
            "-flush_packets",
            "1",
            partial_out.to_str().unwrap(),
        ],
        &token,
        std::slice::from_ref(&partial_out),
        "ffmpeg",
    )
    .unwrap_err();

    assert!(
        readiness.join().unwrap(),
        "FFmpeg never produced partial output"
    );
    assert_eq!(err.code, ErrorCode::Canceled);
    assert!(
        started.elapsed() < Duration::from_secs(15),
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
    assert_eq!(
        std::fs::read(&unrelated_temp).unwrap(),
        b"unrelated-temp-data",
        "unrelated temp files must remain untouched"
    );
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_unique_audio_output_numbers_taken_names_without_overwriting() {
    require_audio_toolchain(true);
    let dir = tempfile::tempdir().unwrap();
    let taken = dir.path().join("sound (converted).mp3");
    std::fs::write(&taken, b"user file").unwrap();

    let out = audio::convert_audio_to(
        &fixture_path("sound.wav"),
        audio::AudioOutput::Unique {
            dir: dir.path(),
            name: "sound (converted).mp3",
        },
        Format::Mp3,
        Some(192),
        &CancelToken::none(),
        &audio::no_check,
    )
    .expect("convert into numbered name");
    assert_eq!(out, dir.path().join("sound (converted) (2).mp3"));
    assert_eq!(std::fs::read(&taken).unwrap(), b"user file");
    assert_eq!(
        refract_core::detect(&std::fs::read(&out).unwrap()),
        Some(Format::Mp3)
    );

    let trimmed_taken = dir.path().join("sound_trimmed.wav");
    std::fs::write(&trimmed_taken, b"user file").unwrap();
    let trimmed = audio::trim_audio_to(
        &fixture_path("sound.wav"),
        audio::AudioOutput::Unique {
            dir: dir.path(),
            name: "sound_trimmed.wav",
        },
        Some("0.5"),
        Some("1.5"),
        None,
        &CancelToken::none(),
        &audio::no_check,
    )
    .expect("trim into numbered name");
    assert_eq!(trimmed, dir.path().join("sound_trimmed (2).wav"));
    assert_eq!(std::fs::read(&trimmed_taken).unwrap(), b"user file");

    let leftovers = std::fs::read_dir(dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".refract-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
fn live_failed_audio_check_saves_nothing() {
    require_audio_toolchain(true);
    let dir = tempfile::tempdir().unwrap();
    let reject = |_: &std::path::Path| {
        Err(refract_core::error::RefractError::new(
            ErrorCode::VerificationFailed,
            "rejected by test",
        ))
    };
    let seen = std::cell::Cell::new(false);
    let inspect = |p: &std::path::Path| {
        // The check sees FFmpeg's finished temp file, before anything is saved.
        assert!(std::fs::metadata(p).unwrap().len() > 0);
        seen.set(true);
        reject(p)
    };

    let err = audio::convert_audio_to(
        &fixture_path("sound.wav"),
        audio::AudioOutput::Unique {
            dir: dir.path(),
            name: "out.mp3",
        },
        Format::Mp3,
        None,
        &CancelToken::none(),
        &inspect,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::VerificationFailed);
    assert!(seen.get());

    let err = audio::trim_audio_to(
        &fixture_path("sound.wav"),
        audio::AudioOutput::Unique {
            dir: dir.path(),
            name: "out.wav",
        },
        Some("0.5"),
        None,
        None,
        &CancelToken::none(),
        &reject,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::VerificationFailed);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
