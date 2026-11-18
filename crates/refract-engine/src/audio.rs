use crate::process::run_command_cancellable;
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tempfile::{Builder as TempFileBuilder, TempPath};

/// Allowed audio bitrate bounds (kbps) for lossy codecs.
pub const MIN_BITRATE_KBPS: u32 = 64;
pub const MAX_BITRATE_KBPS: u32 = 320;

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

/// Exclusively allocate a uniquely named temporary file in `output`'s parent directory
/// using OS-level `O_CREAT | O_EXCL` / `CREATE_NEW` semantics via `tempfile::Builder`,
/// preserving `.ext` so FFmpeg selects the intended muxer.
///
/// Converting immediately to [`TempPath`] closes the file handle on Windows before spawning
/// FFmpeg while retaining exclusive RAII ownership so dropping always removes only this file.
fn create_sibling_temp_audio_file(output: &Path, ext: &str) -> Result<TempPath, RefractError> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let clean_ext = ext.trim_start_matches('.');
    let suffix = if clean_ext.is_empty() {
        String::new()
    } else {
        format!(".{}", clean_ext)
    };

    let named = TempFileBuilder::new()
        .prefix(".refract-audio-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::EngineFailed,
                "Failed to allocate temporary audio output file",
                e.to_string(),
            )
        })?;

    Ok(named.into_temp_path())
}

/// Atomically persist an owned [`TempPath`] to `output` with strict no-clobber semantics.
///
/// - On Windows, `TempPath::persist_noclobber` uses `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`,
///   failing atomically with `AlreadyExists` if `output` exists.
/// - On Unix, `TempPath::persist_noclobber` uses `link` + `unlink`, failing atomically with `AlreadyExists`.
/// - On any error (including collision), the returned `PathPersistError`'s owned `TempPath` is
///   dropped, removing only this operation's temporary file while leaving `output` untouched.
pub fn persist_audio_temp_noclobber(temp: TempPath, output: &Path) -> Result<(), RefractError> {
    match temp.persist_noclobber(output) {
        Ok(()) => Ok(()),
        Err(err) => {
            let io_err = err.error;
            // Explicitly drop `err.path` so our owned temporary file is cleaned up immediately.
            drop(err.path);
            if io_err.kind() == std::io::ErrorKind::AlreadyExists {
                Err(RefractError::with_details(
                    ErrorCode::EngineFailed,
                    format!(
                        "Output file already exists and will not be overwritten: {}",
                        output.display()
                    ),
                    "destination_exists",
                ))
            } else {
                Err(RefractError::with_details(
                    ErrorCode::EngineFailed,
                    "Failed to finalize audio output file",
                    io_err.to_string(),
                ))
            }
        }
    }
}

fn reject_if_destination_exists(output: &Path) -> Result<(), RefractError> {
    if output.exists() {
        return Err(RefractError::with_details(
            ErrorCode::EngineFailed,
            format!(
                "Output file already exists and will not be overwritten: {}",
                output.display()
            ),
            "destination_exists",
        ));
    }
    Ok(())
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
///
/// Exclusively allocates a temporary sibling file, runs FFmpeg into it, and finalizes via
/// atomic no-clobber persistence so failures, cancellations, and destination collisions never
/// overwrite an existing user file or leave temporary artifacts behind.
pub fn convert_audio_with_binary(
    ffmpeg_bin: &Path,
    input: &Path,
    output: &Path,
    target: Format,
    bitrate_kbps: Option<u32>,
    cancel: &CancelToken,
) -> Result<(), RefractError> {
    cancel.check()?;
    reject_if_destination_exists(output)?;

    let temp_file = create_sibling_temp_audio_file(output, target.primary_extension())?;
    let args = build_convert_audio_args(input, temp_file.as_ref(), target, bitrate_kbps)?;

    // `temp_file` owns cleanup via `Drop` on error or cancellation, so cleanup never touches unrelated files.
    run_command_cancellable(ffmpeg_bin, args, cancel, &[], "ffmpeg")?;

    if cancel.is_canceled() {
        drop(temp_file);
        return Err(RefractError::canceled());
    }

    persist_audio_temp_noclobber(temp_file, output)
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
/// against actual media duration (`end=0` means EOF), honoring `bitrate_kbps`, and finalizing
/// with atomic no-clobber persistence.
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
    reject_if_destination_exists(output)?;

    let duration_secs = probe_duration_with_binary(ffprobe_bin, input, cancel)?;
    let range = resolve_trim_range(start, end, duration_secs)?;

    let target_format = detect_audio_format_from_file(input);
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .or_else(|| target_format.map(|f| f.primary_extension()))
        .unwrap_or("mp3");
    let temp_file = create_sibling_temp_audio_file(output, ext)?;

    let args = build_trim_audio_args(
        input,
        temp_file.as_ref(),
        range,
        target_format,
        bitrate_kbps,
    )?;

    // `temp_file` owns cleanup via `Drop` on error or cancellation.
    run_command_cancellable(ffmpeg_bin, args, cancel, &[], "ffmpeg")?;

    if cancel.is_canceled() {
        drop(temp_file);
        return Err(RefractError::canceled());
    }

    persist_audio_temp_noclobber(temp_file, output)
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

    fn temp_audio_files_in(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(".refract-audio-"))
                    .unwrap_or(false)
            })
            .collect()
    }

    #[test]
    fn missing_ffmpeg_and_ffprobe_return_missing_dependency_and_leave_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let dummy_in = dir.path().join("in.wav");
        let new_out = dir.path().join("new_out.mp3");
        std::fs::write(&dummy_in, b"RIFF0000WAVE").unwrap();

        let missing_bin = Path::new("refract-missing-ffmpeg-binary-99999");

        let probe_err =
            probe_duration_with_binary(missing_bin, &dummy_in, &CancelToken::none()).unwrap_err();
        assert_eq!(probe_err.code, ErrorCode::MissingDependency);

        let conv_err = convert_audio_with_binary(
            missing_bin,
            &dummy_in,
            &new_out,
            Format::Mp3,
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(conv_err.code, ErrorCode::MissingDependency);
        assert!(!new_out.exists());
        assert!(
            temp_audio_files_in(dir.path()).is_empty(),
            "no temporary audio files may remain after spawn failure"
        );

        let trim_err = trim_audio_with_binaries(
            missing_bin,
            missing_bin,
            &dummy_in,
            &new_out,
            Some("0"),
            Some("0"),
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(trim_err.code, ErrorCode::MissingDependency);
        assert!(!new_out.exists());
        assert!(temp_audio_files_in(dir.path()).is_empty());

        // AudioDependencies helper reports missing binaries explicitly
        let partial_deps = AudioDependencies {
            ffmpeg: Some(PathBuf::from("ffmpeg")),
            ffprobe: None,
        };
        assert!(!partial_deps.is_ready());
        assert_eq!(partial_deps.missing(), vec!["ffprobe"]);
    }

    #[test]
    fn destination_collision_is_rejected_and_preserves_original_file_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let dummy_in = dir.path().join("in.wav");
        let existing_out = dir.path().join("existing.mp3");
        std::fs::write(&dummy_in, b"RIFF0000WAVE").unwrap();
        std::fs::write(&existing_out, b"original-user-audio").unwrap();

        // 1. Direct atomic persist_noclobber collision (simulates TOCTOU race after FFmpeg succeeds)
        let temp = create_sibling_temp_audio_file(&existing_out, "mp3").unwrap();
        assert_eq!(temp.extension().and_then(|e| e.to_str()), Some("mp3"));
        std::fs::write(&temp, b"new-converted-audio").unwrap();

        let err = persist_audio_temp_noclobber(temp, &existing_out).unwrap_err();
        assert_eq!(err.code, ErrorCode::EngineFailed);
        assert_eq!(err.details.as_deref(), Some("destination_exists"));
        assert!(err.message.contains("already exists"));
        assert_eq!(
            std::fs::read(&existing_out).unwrap(),
            b"original-user-audio",
            "existing destination must never be overwritten"
        );
        assert!(
            temp_audio_files_in(dir.path()).is_empty(),
            "temporary file must be removed after no-clobber collision"
        );

        // 2. Upfront collision check on convert and trim preserves existing destination
        let dummy_bin = Path::new("refract-missing-ffmpeg-binary-99999");
        let conv_err = convert_audio_with_binary(
            dummy_bin,
            &dummy_in,
            &existing_out,
            Format::Mp3,
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(conv_err.details.as_deref(), Some("destination_exists"));
        assert_eq!(
            std::fs::read(&existing_out).unwrap(),
            b"original-user-audio"
        );

        let trim_err = trim_audio_with_binaries(
            dummy_bin,
            dummy_bin,
            &dummy_in,
            &existing_out,
            Some("0"),
            Some("0"),
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(trim_err.details.as_deref(), Some("destination_exists"));
        assert_eq!(
            std::fs::read(&existing_out).unwrap(),
            b"original-user-audio"
        );
        assert!(temp_audio_files_in(dir.path()).is_empty());
    }

    #[test]
    fn exclusive_temp_allocation_never_clobbers_or_deletes_unrelated_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let unrelated = dir.path().join(".refract-audio-unrelated.mp3");
        let dest = dir.path().join("target.mp3");
        std::fs::write(&unrelated, b"unrelated-sibling").unwrap();

        {
            let owned_temp = create_sibling_temp_audio_file(&dest, "mp3").unwrap();
            assert_ne!(owned_temp.as_ref() as &Path, unrelated.as_path());
            assert_eq!(owned_temp.extension().and_then(|e| e.to_str()), Some("mp3"));
            std::fs::write(&owned_temp, b"owned-work").unwrap();
            // Simulate failure/cancellation drop of `owned_temp`
        }

        // Unrelated sibling file must remain completely untouched while owned temp file is gone
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"unrelated-sibling");
        assert_eq!(temp_audio_files_in(dir.path()), vec![unrelated]);
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
        assert!(temp_audio_files_in(dir.path()).is_empty());
    }

    fn write_test_script(dir: &Path, name: &str, windows_body: &str, unix_body: &str) -> PathBuf {
        #[cfg(windows)]
        {
            let _ = unix_body;
            let path = dir.join(format!("{}.cmd", name));
            std::fs::write(&path, windows_body).unwrap();
            path
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = windows_body;
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{}\n", unix_body)).unwrap();
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
            path
        }
    }

    #[test]
    fn toctou_collision_on_convert_and_trim_preserves_destination_and_cleans_owned_temp() {
        let dir = tempfile::tempdir().unwrap();
        let dummy_in = dir.path().join("in.wav");
        let convert_out = dir.path().join("raced_convert.mp3");
        let trim_out = dir.path().join("raced_trim.mp3");
        let unrelated = dir.path().join(".refract-audio-unrelated.mp3");
        std::fs::write(&dummy_in, b"RIFF0000WAVE").unwrap();
        std::fs::write(&unrelated, b"keep-unrelated").unwrap();

        // Fake ffprobe returning 3.0s duration
        let fake_ffprobe = write_test_script(
            dir.path(),
            "fake_ffprobe",
            "@echo off\r\necho 3.0\r\nexit /b 0\r\n",
            "echo 3.0\nexit 0",
        );

        // Fake ffmpeg that writes to its last arg (temp output) AND races by creating convert_out
        let fake_ffmpeg_convert = write_test_script(
            dir.path(),
            "fake_ffmpeg_convert",
            &format!(
                "@echo off\r\n:loop\r\nif \"%~2\"==\"\" goto done\r\nshift\r\ngoto loop\r\n:done\r\necho new-temp>\"%~1\"\r\n<nul set /p =raced-original>\"{}\"\r\nexit /b 0\r\n",
                convert_out.display()
            ),
            &format!(
                "for last; do :; done\necho new-temp > \"$last\"\nprintf 'raced-original' > '{}'\nexit 0",
                convert_out.display()
            ),
        );

        let err = convert_audio_with_binary(
            &fake_ffmpeg_convert,
            &dummy_in,
            &convert_out,
            Format::Mp3,
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::EngineFailed);
        assert_eq!(err.details.as_deref(), Some("destination_exists"));
        assert_eq!(std::fs::read(&convert_out).unwrap(), b"raced-original");
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep-unrelated");
        assert_eq!(temp_audio_files_in(dir.path()), vec![unrelated.clone()]);

        // Same TOCTOU race for trim_audio_with_binaries
        let fake_ffmpeg_trim = write_test_script(
            dir.path(),
            "fake_ffmpeg_trim",
            &format!(
                "@echo off\r\n:loop\r\nif \"%~2\"==\"\" goto done\r\nshift\r\ngoto loop\r\n:done\r\necho new-trim-temp>\"%~1\"\r\n<nul set /p =raced-trim-original>\"{}\"\r\nexit /b 0\r\n",
                trim_out.display()
            ),
            &format!(
                "for last; do :; done\necho new-trim-temp > \"$last\"\nprintf 'raced-trim-original' > '{}'\nexit 0",
                trim_out.display()
            ),
        );

        let trim_err = trim_audio_with_binaries(
            &fake_ffmpeg_trim,
            &fake_ffprobe,
            &dummy_in,
            &trim_out,
            Some("0.2"),
            Some("1.0"),
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(trim_err.code, ErrorCode::EngineFailed);
        assert_eq!(trim_err.details.as_deref(), Some("destination_exists"));
        assert_eq!(std::fs::read(&trim_out).unwrap(), b"raced-trim-original");
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep-unrelated");
        assert_eq!(temp_audio_files_in(dir.path()), vec![unrelated]);
    }

    #[test]
    fn ffmpeg_failure_and_mid_run_cancel_clean_only_owned_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let dummy_in = dir.path().join("in.wav");
        let out_fail = dir.path().join("failed.mp3");
        let out_cancel = dir.path().join("canceled.mp3");
        let unrelated = dir.path().join(".refract-audio-unrelated.mp3");
        std::fs::write(&dummy_in, b"RIFF0000WAVE").unwrap();
        std::fs::write(&unrelated, b"keep-unrelated").unwrap();

        // 1. Failing process writes partial bytes into temp output then exits non-zero
        let failing_ffmpeg = write_test_script(
            dir.path(),
            "failing_ffmpeg",
            "@echo off\r\n:loop\r\nif \"%~2\"==\"\" goto done\r\nshift\r\ngoto loop\r\n:done\r\necho partial>\"%~1\"\r\necho codec error 1>&2\r\nexit /b 2\r\n",
            "for last; do :; done\necho partial > \"$last\"\necho codec error >&2\nexit 2",
        );

        let err = convert_audio_with_binary(
            &failing_ffmpeg,
            &dummy_in,
            &out_fail,
            Format::Mp3,
            Some(192),
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::EngineFailed);
        assert!(!out_fail.exists());
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep-unrelated");
        assert_eq!(temp_audio_files_in(dir.path()), vec![unrelated.clone()]);

        // 2. Mid-process cancellation cleans only the owned temp file and keeps unrelated files
        let slow_ffmpeg = write_test_script(
            dir.path(),
            "slow_ffmpeg",
            "@echo off\r\n:spin\r\ngoto spin\r\n",
            "exec sleep 30",
        );

        let token = CancelToken::new();
        let canceller = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            canceller.cancel();
        });

        let started = std::time::Instant::now();
        let cancel_err = convert_audio_with_binary(
            &slow_ffmpeg,
            &dummy_in,
            &out_cancel,
            Format::Mp3,
            Some(192),
            &token,
        )
        .unwrap_err();
        assert_eq!(cancel_err.code, ErrorCode::Canceled);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "canceled audio subprocess took too long: {:?}",
            started.elapsed()
        );
        assert!(!out_cancel.exists());
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep-unrelated");
        assert_eq!(temp_audio_files_in(dir.path()), vec![unrelated]);
    }
}
