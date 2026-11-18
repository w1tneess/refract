use crate::process::run_command_cancellable;
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Allowed audio bitrate bounds (kbps) for lossy codecs.
pub const MIN_BITRATE_KBPS: u32 = 64;
pub const MAX_BITRATE_KBPS: u32 = 320;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Resolved system audio tool binaries on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDependencies {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

impl AudioDependencies {
    /// True when both `ffmpeg` and `ffprobe` are available on `PATH`.
    pub fn is_ready(&self) -> bool {
        self.ffmpeg.is_some() && self.ffprobe.is_some()
    }

    /// Names of missing binaries, empty when ready.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.ffmpeg.is_none() {
            out.push("ffmpeg");
        }
        if self.ffprobe.is_none() {
            out.push("ffprobe");
        }
        out
    }
}

/// Probe for both `ffmpeg` and `ffprobe` explicitly on `PATH`.
pub fn detect_audio_dependencies() -> AudioDependencies {
    AudioDependencies {
        ffmpeg: which::which("ffmpeg").ok(),
        ffprobe: which::which("ffprobe").ok(),
    }
}

/// Check if `ffmpeg` binary is present on `PATH`.
pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// Check if `ffprobe` binary is present on `PATH`.
pub fn is_ffprobe_available() -> bool {
    which::which("ffprobe").is_ok()
}

/// Check if both `ffmpeg` and `ffprobe` are present on `PATH`.
pub fn is_audio_toolchain_available() -> bool {
    detect_audio_dependencies().is_ready()
}

/// Validate a lossy audio bitrate in kbps without silently clamping.
pub fn validate_bitrate_kbps(bitrate_kbps: u32) -> Result<u32, RefractError> {
    if !(MIN_BITRATE_KBPS..=MAX_BITRATE_KBPS).contains(&bitrate_kbps) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Invalid audio bitrate {} kbps. Must be between {} and {} kbps.",
                bitrate_kbps, MIN_BITRATE_KBPS, MAX_BITRATE_KBPS
            ),
        ));
    }
    Ok(bitrate_kbps)
}

/// Parse a canonical bitrate string (`"192k"` or `"192"`) into validated kbps.
pub fn parse_bitrate_str(raw: &str) -> Result<u32, RefractError> {
    let trimmed = raw.trim().trim_end_matches(['k', 'K']);
    let parsed: u32 = trimmed.parse().map_err(|_| {
        RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Invalid audio bitrate '{}'. Expected e.g. '192k'.",
                raw.trim()
            ),
        )
    })?;
    validate_bitrate_kbps(parsed)
}

/// Parse time string (`"90"`, `"1:30"`, `"01:30.5"`, `"01:00:00"`, or `""`) into non-negative finite seconds.
pub fn parse_timestamp(ts: &str) -> Result<f64, RefractError> {
    let ts = ts.trim();
    if ts.is_empty() {
        return Ok(0.0);
    }

    let validate = |v: f64, label: &str| -> Result<f64, RefractError> {
        if !v.is_finite() || v < 0.0 {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Invalid {} in timestamp: '{}'", label, ts),
            ));
        }
        Ok(v)
    };

    if let Ok(sec) = ts.parse::<f64>() {
        return validate(sec, "seconds");
    }

    let parts: Vec<&str> = ts.split(':').collect();
    match parts.len() {
        2 => {
            let m: f64 = parts[0].parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid minutes in timestamp: '{}'", ts),
                )
            })?;
            let s: f64 = parts[1].parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid seconds in timestamp: '{}'", ts),
                )
            })?;
            Ok(validate(m, "minutes")? * 60.0 + validate(s, "seconds")?)
        }
        3 => {
            let h: f64 = parts[0].parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid hours in timestamp: '{}'", ts),
                )
            })?;
            let m: f64 = parts[1].parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid minutes in timestamp: '{}'", ts),
                )
            })?;
            let s: f64 = parts[2].parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid seconds in timestamp: '{}'", ts),
                )
            })?;
            Ok(validate(h, "hours")? * 3600.0
                + validate(m, "minutes")? * 60.0
                + validate(s, "seconds")?)
        }
        _ => Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!("Invalid timestamp format: '{}'", ts),
        )),
    }
}

/// Validated trim bounds resolved against actual audio duration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimRange {
    pub start_secs: f64,
    /// `None` means trim through the end of the file (`end == 0` or omitted).
    pub end_secs: Option<f64>,
    /// Expected output duration in seconds.
    pub expected_duration_secs: f64,
}

/// Resolve and validate `start` / `end` trim options against actual media `duration_secs`.
///
/// Canonical rules (matching Web `@refract/core`):
/// - Blank or `0` `end` means end-of-file (`end_secs: None`).
/// - `start >= duration_secs` is rejected with `ErrorCode::InvalidOption`.
/// - Non-zero `end <= start` is rejected with `ErrorCode::InvalidOption`.
/// - `end > duration_secs` clamps cleanly to `duration_secs`.
pub fn resolve_trim_range(
    start: Option<&str>,
    end: Option<&str>,
    duration_secs: f64,
) -> Result<TrimRange, RefractError> {
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "Audio file has no measurable duration.",
        ));
    }

    let start_secs = match start {
        Some(s) => parse_timestamp(s)?,
        None => 0.0,
    };
    let raw_end = match end {
        Some(e) => parse_timestamp(e)?,
        None => 0.0,
    };

    if start_secs >= duration_secs {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Start time ({:.2}s) is at or after the end of the file ({:.2}s).",
                start_secs, duration_secs
            ),
        ));
    }

    if raw_end == 0.0 {
        return Ok(TrimRange {
            start_secs,
            end_secs: None,
            expected_duration_secs: duration_secs - start_secs,
        });
    }

    if raw_end <= start_secs {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "End time ({:.3}s) must be greater than start time ({:.3}s).",
                raw_end, start_secs
            ),
        ));
    }

    let stop = raw_end.min(duration_secs);
    Ok(TrimRange {
        start_secs,
        end_secs: Some(stop),
        expected_duration_secs: stop - start_secs,
    })
}

/// Build FFmpeg codec and bitrate arguments for an audio target format.
/// Bitrate is omitted for lossless WAV and validated (`64..=320` kbps) for lossy codecs.
pub fn build_audio_codec_args(
    target: Format,
    bitrate_kbps: Option<u32>,
) -> Result<Vec<OsString>, RefractError> {
    let mut args: Vec<OsString> = Vec::new();
    match target {
        Format::Mp3 => {
            args.push(OsString::from("-c:a"));
            args.push(OsString::from("libmp3lame"));
            if let Some(br) = bitrate_kbps {
                let valid = validate_bitrate_kbps(br)?;
                args.push(OsString::from("-b:a"));
                args.push(OsString::from(format!("{}k", valid)));
            }
        }
        Format::Wav => {
            args.push(OsString::from("-c:a"));
            args.push(OsString::from("pcm_s16le"));
        }
        Format::M4a => {
            args.push(OsString::from("-c:a"));
            args.push(OsString::from("aac"));
            if let Some(br) = bitrate_kbps {
                let valid = validate_bitrate_kbps(br)?;
                args.push(OsString::from("-b:a"));
                args.push(OsString::from(format!("{}k", valid)));
            }
        }
        Format::Ogg => {
            args.push(OsString::from("-c:a"));
            args.push(OsString::from("libvorbis"));
            if let Some(br) = bitrate_kbps {
                let valid = validate_bitrate_kbps(br)?;
                args.push(OsString::from("-b:a"));
                args.push(OsString::from(format!("{}k", valid)));
            }
        }
        _ => {
            return Err(RefractError::new(
                ErrorCode::UnsupportedFormat,
                format!("Unsupported target audio format: {:?}", target),
            ));
        }
    }
    Ok(args)
}

/// Build deterministic `ffprobe` arguments for probing media duration in seconds.
pub fn build_ffprobe_duration_args(input: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-v"),
        OsString::from("error"),
        OsString::from("-show_entries"),
        OsString::from("format=duration"),
        OsString::from("-of"),
        OsString::from("default=noprint_wrappers=1:nokey=1"),
        input.as_os_str().to_os_string(),
    ]
}

/// Build deterministic `ffmpeg` arguments for audio format conversion.
pub fn build_convert_audio_args(
    input: &Path,
    output: &Path,
    target: Format,
    bitrate_kbps: Option<u32>,
) -> Result<Vec<OsString>, RefractError> {
    let mut args = vec![
        OsString::from("-y"),
        OsString::from("-i"),
        input.as_os_str().to_os_string(),
    ];
    args.extend(build_audio_codec_args(target, bitrate_kbps)?);
    args.push(output.as_os_str().to_os_string());
    Ok(args)
}

/// Build deterministic `ffmpeg` arguments for audio trimming.
///
/// Uses `-ss <start>` before `-i` and `-t <duration>` after `-i` when `end_secs` is `Some`,
/// ensuring exact trimmed duration across all containers/FFmpeg versions. When `end_secs` is
/// `None` (`end=0` -> EOF), omits `-t` so FFmpeg runs cleanly through end-of-file.
pub fn build_trim_audio_args(
    input: &Path,
    output: &Path,
    range: TrimRange,
    target_format: Option<Format>,
    bitrate_kbps: Option<u32>,
) -> Result<Vec<OsString>, RefractError> {
    let mut args = vec![OsString::from("-y")];
    if range.start_secs > 0.0 {
        args.push(OsString::from("-ss"));
        args.push(OsString::from(format!("{:.3}", range.start_secs)));
    }
    args.push(OsString::from("-i"));
    args.push(input.as_os_str().to_os_string());
    if let Some(end_secs) = range.end_secs {
        let clip_duration = (end_secs - range.start_secs).max(0.001);
        args.push(OsString::from("-t"));
        args.push(OsString::from(format!("{:.3}", clip_duration)));
    }
    if let Some(fmt) = target_format {
        args.extend(build_audio_codec_args(fmt, bitrate_kbps)?);
    }
    args.push(output.as_os_str().to_os_string());
    Ok(args)
}

/// Detect audio format from file header bytes, falling back to extension only if header read fails.
fn detect_audio_format_from_file(path: &Path) -> Option<Format> {
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut header = [0u8; 64];
        if let Ok(n) = file.read(&mut header) {
            if let Some(fmt) = refract_core::detect(&header[..n]) {
                if fmt.kind() == refract_core::format::Kind::Audio {
                    return Some(fmt);
                }
            }
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(Format::from_extension)
        .filter(|fmt| fmt.kind() == refract_core::format::Kind::Audio)
}

/// Build a unique sibling temp path beside `output`, keeping the target file extension
/// so FFmpeg selects the matching muxer/container while never corrupting `output` on error/cancel.
fn sibling_temp_path(output: &Path, ext: &str) -> PathBuf {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let ext = ext.trim_start_matches('.');
    if ext.is_empty() {
        parent.join(format!(".refract-audio-tmp-{}-{}", pid, seq))
    } else {
        parent.join(format!(".refract-audio-tmp-{}-{}.{}", pid, seq, ext))
    }
}

/// Probe audio duration in seconds using an explicit `ffprobe` binary path and cancellation token.
pub fn probe_duration_with_binary(
    ffprobe_bin: &Path,
    input: &Path,
    cancel: &CancelToken,
) -> Result<f64, RefractError> {
    cancel.check()?;
    let args = build_ffprobe_duration_args(input);
    let output = run_command_cancellable(ffprobe_bin, args, cancel, &[], "ffprobe")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration = stdout.trim().parse::<f64>().map_err(|_| {
        RefractError::with_details(
            ErrorCode::DamagedFile,
            "Could not parse audio duration from ffprobe",
            stdout.trim().to_string(),
        )
    })?;

    if !duration.is_finite() || duration <= 0.0 {
        return Err(RefractError::with_details(
            ErrorCode::DamagedFile,
            "Audio file has an invalid duration",
            stdout.trim().to_string(),
        ));
    }

    Ok(duration)
}

/// Probe audio duration in seconds using system `ffprobe` with cancellation.
pub fn probe_duration_cancellable(input: &Path, cancel: &CancelToken) -> Result<f64, RefractError> {
    cancel.check()?;
    let ffprobe_bin = which::which("ffprobe").map_err(|_| {
        RefractError::new(
            ErrorCode::MissingDependency,
            "ffprobe was not found on PATH. Install FFmpeg (including ffprobe) to inspect audio.",
        )
    })?;
    probe_duration_with_binary(&ffprobe_bin, input, cancel)
}

/// Probe audio duration in seconds using `ffprobe`.
pub fn probe_duration(input: &Path) -> Result<f64, RefractError> {
    probe_duration_cancellable(input, &CancelToken::none())
}

/// Convert an audio file using an explicit `ffmpeg` binary path and cancellation token.
/// Writes to a sibling temp file first and renames to `output` only on success so failures
/// and cancellations never leave partial output or overwrite an existing file.
pub fn convert_audio_with_binary(
    ffmpeg_bin: &Path,
    input: &Path,
    output: &Path,
    target: Format,
    bitrate_kbps: Option<u32>,
    cancel: &CancelToken,
) -> Result<(), RefractError> {
    cancel.check()?;

    let tmp_path = sibling_temp_path(output, target.primary_extension());
    let args = build_convert_audio_args(input, &tmp_path, target, bitrate_kbps)?;

    run_command_cancellable(
        ffmpeg_bin,
        args,
        cancel,
        std::slice::from_ref(&tmp_path),
        "ffmpeg",
    )?;

    if cancel.is_canceled() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(RefractError::canceled());
    }

    std::fs::rename(&tmp_path, output).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to finalize converted audio file",
            e.to_string(),
        )
    })
}

/// Convert an audio file using system `ffmpeg` with cooperative cancellation.
pub fn convert_audio_cancellable(
    input: &Path,
    output: &Path,
    target: Format,
    bitrate_kbps: Option<u32>,
    cancel: &CancelToken,
) -> Result<(), RefractError> {
    cancel.check()?;
    let ffmpeg_bin = which::which("ffmpeg").map_err(|_| {
        RefractError::new(
            ErrorCode::MissingDependency,
            "ffmpeg was not found on PATH. Install FFmpeg to convert audio.",
        )
    })?;
    convert_audio_with_binary(&ffmpeg_bin, input, output, target, bitrate_kbps, cancel)
}

/// Convert an audio file using native FFmpeg.
pub fn convert_audio(
    input: &Path,
    output: &Path,
    target: Format,
    bitrate_kbps: Option<u32>,
) -> Result<(), RefractError> {
    convert_audio_cancellable(input, output, target, bitrate_kbps, &CancelToken::none())
}

/// Trim an audio file using explicit `ffmpeg` and `ffprobe` paths, validating `start`/`end`
/// against actual media duration (`end=0` means EOF) and honoring `bitrate_kbps`.
#[allow(clippy::too_many_arguments)]
pub fn trim_audio_with_binaries(
    ffmpeg_bin: &Path,
    ffprobe_bin: &Path,
    input: &Path,
    output: &Path,
    start: Option<&str>,
    end: Option<&str>,
    bitrate_kbps: Option<u32>,
    cancel: &CancelToken,
) -> Result<(), RefractError> {
    cancel.check()?;

    let duration_secs = probe_duration_with_binary(ffprobe_bin, input, cancel)?;
    let range = resolve_trim_range(start, end, duration_secs)?;

    let target_format = detect_audio_format_from_file(input);
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .or_else(|| target_format.map(|f| f.primary_extension()))
        .unwrap_or("mp3");
    let tmp_path = sibling_temp_path(output, ext);

    let args = build_trim_audio_args(input, &tmp_path, range, target_format, bitrate_kbps)?;

    run_command_cancellable(
        ffmpeg_bin,
        args,
        cancel,
        std::slice::from_ref(&tmp_path),
        "ffmpeg",
    )?;

    if cancel.is_canceled() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(RefractError::canceled());
    }

    std::fs::rename(&tmp_path, output).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to finalize trimmed audio file",
            e.to_string(),
        )
    })
}

/// Trim an audio file using system `ffmpeg` and `ffprobe` with bitrate and cancellation support.
pub fn trim_audio_cancellable(
    input: &Path,
    output: &Path,
    start: Option<&str>,
    end: Option<&str>,
    bitrate_kbps: Option<u32>,
    cancel: &CancelToken,
) -> Result<(), RefractError> {
    cancel.check()?;
    let deps = detect_audio_dependencies();
    let ffmpeg_bin = deps.ffmpeg.ok_or_else(|| {
        RefractError::new(
            ErrorCode::MissingDependency,
            "ffmpeg was not found on PATH. Install FFmpeg to trim audio.",
        )
    })?;
    let ffprobe_bin = deps.ffprobe.ok_or_else(|| {
        RefractError::new(
            ErrorCode::MissingDependency,
            "ffprobe was not found on PATH. Install FFmpeg (including ffprobe) to trim audio.",
        )
    })?;
    trim_audio_with_binaries(
        &ffmpeg_bin,
        &ffprobe_bin,
        input,
        output,
        start,
        end,
        bitrate_kbps,
        cancel,
    )
}

/// Trim an audio file using native FFmpeg (validating against duration; `end=0` or `None` means EOF).
pub fn trim_audio(
    input: &Path,
    output: &Path,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(), RefractError> {
    trim_audio_cancellable(input, output, start, end, None, &CancelToken::none())
}

/// In-memory audio conversion helper via temporary files with cancellation support.
pub fn convert_audio_bytes_cancellable(
    bytes: &[u8],
    src_ext: &str,
    target: Format,
    bitrate_kbps: Option<u32>,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    if bytes.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "Audio file is empty (0 bytes)",
        ));
    }

    let temp_dir = tempfile::tempdir().map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to create temp dir",
            e.to_string(),
        )
    })?;

    let in_file = temp_dir.path().join(format!("input.{}", src_ext));
    let out_file = temp_dir
        .path()
        .join(format!("output.{}", target.primary_extension()));

    std::fs::write(&in_file, bytes).map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to write temp audio",
            e.to_string(),
        )
    })?;

    convert_audio_cancellable(&in_file, &out_file, target, bitrate_kbps, cancel)?;

    std::fs::read(&out_file).map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to read converted audio",
            e.to_string(),
        )
    })
}

/// In-memory audio conversion helper via temporary files.
pub fn convert_audio_bytes(
    bytes: &[u8],
    src_ext: &str,
    target: Format,
    bitrate_kbps: Option<u32>,
) -> Result<Vec<u8>, RefractError> {
    convert_audio_bytes_cancellable(bytes, src_ext, target, bitrate_kbps, &CancelToken::none())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn parse_timestamp_and_bitrate_validation() {
        assert_eq!(parse_timestamp("").unwrap(), 0.0);
        assert_eq!(parse_timestamp("0").unwrap(), 0.0);
        assert_eq!(parse_timestamp("90").unwrap(), 90.0);
        assert_eq!(parse_timestamp("1:30").unwrap(), 90.0);
        assert_eq!(parse_timestamp("01:30.5").unwrap(), 90.5);
        assert_eq!(parse_timestamp("01:00:00").unwrap(), 3600.0);

        for bad in ["-5", "NaN", "inf", "1:2:3:4", "abc", "1:-2"] {
            assert_eq!(
                parse_timestamp(bad).unwrap_err().code,
                ErrorCode::InvalidOption,
                "expected InvalidOption for timestamp '{}'",
                bad
            );
        }

        assert_eq!(parse_bitrate_str("128k").unwrap(), 128);
        assert_eq!(parse_bitrate_str("192").unwrap(), 192);
        assert_eq!(parse_bitrate_str("320K").unwrap(), 320);
        assert_eq!(
            parse_bitrate_str("32k").unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_bitrate_str("512k").unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_bitrate_str("fast").unwrap_err().code,
            ErrorCode::InvalidOption
        );
    }

    #[test]
    fn resolve_trim_range_handles_end_zero_eof_and_validates_against_duration() {
        // end = 0 or None or "" means EOF
        for end_opt in [None, Some(""), Some("0"), Some("0.0"), Some("0:00")] {
            let r = resolve_trim_range(Some("1.0"), end_opt, 3.0).unwrap();
            assert_eq!(r.start_secs, 1.0);
            assert_eq!(r.end_secs, None);
            assert!((r.expected_duration_secs - 2.0).abs() < 1e-6);
        }

        // Explicit valid end within duration
        let r = resolve_trim_range(Some("0.2"), Some("0.6"), 3.0).unwrap();
        assert_eq!(r.start_secs, 0.2);
        assert_eq!(r.end_secs, Some(0.6));
        assert!((r.expected_duration_secs - 0.4).abs() < 1e-6);

        // End beyond duration clamps to file duration (matching Web)
        let clamped = resolve_trim_range(Some("1.5"), Some("10.0"), 3.0).unwrap();
        assert_eq!(clamped.end_secs, Some(3.0));
        assert!((clamped.expected_duration_secs - 1.5).abs() < 1e-6);

        // Start at or past duration is rejected
        assert_eq!(
            resolve_trim_range(Some("3.0"), Some("0"), 3.0)
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            resolve_trim_range(Some("5.0"), None, 3.0).unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // Non-zero end <= start is rejected
        assert_eq!(
            resolve_trim_range(Some("1.5"), Some("1.5"), 3.0)
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            resolve_trim_range(Some("2.0"), Some("1.0"), 3.0)
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );

        // Non-positive / NaN duration rejected as DamagedFile
        assert_eq!(
            resolve_trim_range(Some("0"), Some("1"), 0.0)
                .unwrap_err()
                .code,
            ErrorCode::DamagedFile
        );
    }

    #[test]
    fn convert_and_trim_argument_builders_match_codec_and_range_contracts() {
        let input = Path::new("/tmp/in.wav");
        let output = Path::new("/tmp/out.mp3");

        // Convert to MP3 with bitrate
        let mp3_args = build_convert_audio_args(input, output, Format::Mp3, Some(256)).unwrap();
        assert_eq!(
            as_strings(&mp3_args),
            vec![
                "-y",
                "-i",
                "/tmp/in.wav",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "256k",
                "/tmp/out.mp3"
            ]
        );

        // Convert to WAV omits -b:a even if bitrate was passed
        let wav_args =
            build_convert_audio_args(input, Path::new("/tmp/out.wav"), Format::Wav, Some(320))
                .unwrap();
        assert_eq!(
            as_strings(&wav_args),
            vec![
                "-y",
                "-i",
                "/tmp/in.wav",
                "-c:a",
                "pcm_s16le",
                "/tmp/out.wav"
            ]
        );

        // Convert to M4A and OGG
        let m4a_args =
            build_convert_audio_args(input, Path::new("/tmp/out.m4a"), Format::M4a, Some(192))
                .unwrap();
        assert!(as_strings(&m4a_args).contains(&"aac".to_string()));
        assert!(as_strings(&m4a_args).contains(&"192k".to_string()));

        let ogg_args =
            build_convert_audio_args(input, Path::new("/tmp/out.ogg"), Format::Ogg, Some(128))
                .unwrap();
        assert!(as_strings(&ogg_args).contains(&"libvorbis".to_string()));
        assert!(as_strings(&ogg_args).contains(&"128k".to_string()));

        // Non-audio target rejected
        assert_eq!(
            build_convert_audio_args(input, output, Format::Png, None)
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedFormat
        );

        // Trim with explicit end uses -ss before -i and -t clip_duration after -i
        let range = resolve_trim_range(Some("0.5"), Some("2.0"), 3.0).unwrap();
        let trim_args =
            build_trim_audio_args(input, output, range, Some(Format::Mp3), Some(192)).unwrap();
        assert_eq!(
            as_strings(&trim_args),
            vec![
                "-y",
                "-ss",
                "0.500",
                "-i",
                "/tmp/in.wav",
                "-t",
                "1.500",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "192k",
                "/tmp/out.mp3"
            ]
        );

        // Trim with end=0 (EOF) omits -t
        let eof_range = resolve_trim_range(Some("1.0"), Some("0"), 3.0).unwrap();
        let eof_args = build_trim_audio_args(
            input,
            Path::new("/tmp/out.wav"),
            eof_range,
            Some(Format::Wav),
            Some(192),
        )
        .unwrap();
        assert_eq!(
            as_strings(&eof_args),
            vec![
                "-y",
                "-ss",
                "1.000",
                "-i",
                "/tmp/in.wav",
                "-c:a",
                "pcm_s16le",
                "/tmp/out.wav"
            ]
        );
    }

    #[test]
    fn missing_ffmpeg_and_ffprobe_return_missing_dependency_and_preserve_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let dummy_in = dir.path().join("in.wav");
        let existing_out = dir.path().join("existing.mp3");
        std::fs::write(&dummy_in, b"RIFF0000WAVE").unwrap();
        std::fs::write(&existing_out, b"keep-existing-audio").unwrap();

        let missing_bin = Path::new("refract-missing-ffmpeg-binary-99999");

        let probe_err =
            probe_duration_with_binary(missing_bin, &dummy_in, &CancelToken::none()).unwrap_err();
        assert_eq!(probe_err.code, ErrorCode::MissingDependency);

        let conv_err = convert_audio_with_binary(
            missing_bin,
            &dummy_in,
            &existing_out,
            Format::Mp3,
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(conv_err.code, ErrorCode::MissingDependency);
        assert_eq!(
            std::fs::read(&existing_out).unwrap(),
            b"keep-existing-audio",
            "failed conversion must never overwrite existing file"
        );

        let trim_err = trim_audio_with_binaries(
            missing_bin,
            missing_bin,
            &dummy_in,
            &existing_out,
            Some("0"),
            Some("0"),
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(trim_err.code, ErrorCode::MissingDependency);
        assert_eq!(
            std::fs::read(&existing_out).unwrap(),
            b"keep-existing-audio"
        );

        // AudioDependencies helper reports missing binaries explicitly
        let partial_deps = AudioDependencies {
            ffmpeg: Some(PathBuf::from("ffmpeg")),
            ffprobe: None,
        };
        assert!(!partial_deps.is_ready());
        assert_eq!(partial_deps.missing(), vec!["ffprobe"]);
    }

    #[test]
    fn pre_canceled_token_aborts_before_invoking_audio_binaries() {
        let dir = tempfile::tempdir().unwrap();
        let dummy_in = dir.path().join("in.wav");
        let out = dir.path().join("out.mp3");
        let token = CancelToken::new();
        token.cancel();

        assert_eq!(
            probe_duration_cancellable(&dummy_in, &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert_eq!(
            convert_audio_cancellable(&dummy_in, &out, Format::Mp3, Some(192), &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert_eq!(
            trim_audio_cancellable(&dummy_in, &out, Some("0"), Some("0"), Some(192), &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert!(!out.exists());
    }
}
