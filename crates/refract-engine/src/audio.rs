use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::path::Path;
use std::process::Command;

/// Check if ffmpeg binary is present on PATH.
pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// Check if ffprobe binary is present on PATH.
pub fn is_ffprobe_available() -> bool {
    which::which("ffprobe").is_ok()
}

/// Parse time string ("90", "1:30", "01:30.5") into total non-negative finite seconds.
pub fn parse_timestamp(ts: &str) -> Result<f64, RefractError> {
    let ts = ts.trim();
    if ts.is_empty() {
        return Ok(0.0);
    }

    let validate = |v: f64, label: &str| -> Result<f64, RefractError> {
        if !v.is_finite() || v < 0.0 {
            return Err(RefractError::new(
                ErrorCode::DamagedFile,
                format!("Invalid {} in timestamp: {}", label, ts),
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
            let m: f64 = parts[0]
                .parse()
                .map_err(|_| RefractError::new(ErrorCode::DamagedFile, "Invalid minutes in timestamp"))?;
            let s: f64 = parts[1]
                .parse()
                .map_err(|_| RefractError::new(ErrorCode::DamagedFile, "Invalid seconds in timestamp"))?;
            Ok(validate(m, "minutes")? * 60.0 + validate(s, "seconds")?)
        }
        3 => {
            let h: f64 = parts[0]
                .parse()
                .map_err(|_| RefractError::new(ErrorCode::DamagedFile, "Invalid hours in timestamp"))?;
            let m: f64 = parts[1]
                .parse()
                .map_err(|_| RefractError::new(ErrorCode::DamagedFile, "Invalid minutes in timestamp"))?;
            let s: f64 = parts[2]
                .parse()
                .map_err(|_| RefractError::new(ErrorCode::DamagedFile, "Invalid seconds in timestamp"))?;
            Ok(validate(h, "hours")? * 3600.0 + validate(m, "minutes")? * 60.0 + validate(s, "seconds")?)
        }
        _ => Err(RefractError::new(ErrorCode::DamagedFile, format!("Invalid timestamp format: {}", ts))),
    }
}

/// Probe audio duration in seconds using ffprobe.
pub fn probe_duration(input: &Path) -> Result<f64, RefractError> {
    if !is_ffprobe_available() {
        return Err(RefractError::new(
            ErrorCode::EngineFailed,
            "ffprobe is not found on PATH. Please ensure FFmpeg is installed.",
        ));
    }

    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(input)
        .output()
        .map_err(|e| RefractError::with_details(ErrorCode::EngineFailed, "Failed to run ffprobe", e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RefractError::with_details(
            ErrorCode::DamagedFile,
            "ffprobe failed to probe audio file",
            stderr.to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<f64>().map_err(|_| {
        RefractError::with_details(
            ErrorCode::DamagedFile,
            "Could not parse audio duration from ffprobe",
            stdout.to_string(),
        )
    })
}

/// Convert an audio file using native FFmpeg.
pub fn convert_audio(
    input: &Path,
    output: &Path,
    target: Format,
    bitrate_kbps: Option<u32>,
) -> Result<(), RefractError> {
    if !is_ffmpeg_available() {
        return Err(RefractError::new(
            ErrorCode::EngineFailed,
            "ffmpeg is not found on PATH. Please ensure FFmpeg is installed.",
        ));
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-i").arg(input);

    match target {
        Format::Mp3 => {
            cmd.args(["-c:a", "libmp3lame"]);
            if let Some(br) = bitrate_kbps {
                cmd.args(["-b:a", &format!("{}k", br)]);
            }
        }
        Format::Wav => {
            cmd.args(["-c:a", "pcm_s16le"]);
        }
        Format::M4a => {
            cmd.args(["-c:a", "aac"]);
            if let Some(br) = bitrate_kbps {
                cmd.args(["-b:a", &format!("{}k", br)]);
            }
        }
        Format::Ogg => {
            cmd.args(["-c:a", "libvorbis"]);
            if let Some(br) = bitrate_kbps {
                cmd.args(["-b:a", &format!("{}k", br)]);
            }
        }
        _ => {
            return Err(RefractError::new(
                ErrorCode::UnsupportedFormat,
                format!("Unsupported target audio format: {:?}", target),
            ));
        }
    }

    cmd.arg(output);

    let res = cmd.output().map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to execute ffmpeg", e.to_string())
    })?;

    if !res.status.success() {
        let stderr = String::from_utf8_lossy(&res.stderr);
        return Err(RefractError::with_details(
            ErrorCode::EngineFailed,
            format!("ffmpeg exited with code {:?}", res.status.code()),
            stderr.to_string(),
        ));
    }

    Ok(())
}

/// Trim an audio file using native FFmpeg.
pub fn trim_audio(
    input: &Path,
    output: &Path,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(), RefractError> {
    if !is_ffmpeg_available() {
        return Err(RefractError::new(
            ErrorCode::EngineFailed,
            "ffmpeg is not found on PATH. Please ensure FFmpeg is installed.",
        ));
    }

    let start_secs = match start.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(parse_timestamp(s)?),
        None => None,
    };
    let end_secs = match end.map(str::trim).filter(|s| !s.is_empty()) {
        Some(e) => Some(parse_timestamp(e)?),
        None => None,
    };

    if let Some(e) = end_secs {
        let s = start_secs.unwrap_or(0.0);
        if e <= s {
            return Err(RefractError::new(
                ErrorCode::DamagedFile,
                format!("End time ({:.3}s) must be greater than start time ({:.3}s)", e, s),
            ));
        }
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");

    if let Some(secs) = start_secs {
        cmd.arg("-ss").arg(format!("{:.3}", secs));
    }

    if let Some(secs) = end_secs {
        cmd.arg("-to").arg(format!("{:.3}", secs));
    }

    cmd.arg("-i").arg(input);
    cmd.arg(output);

    let res = cmd.output().map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to execute ffmpeg for trim", e.to_string())
    })?;

    if !res.status.success() {
        let stderr = String::from_utf8_lossy(&res.stderr);
        return Err(RefractError::with_details(
            ErrorCode::EngineFailed,
            format!("ffmpeg trim exited with code {:?}", res.status.code()),
            stderr.to_string(),
        ));
    }

    Ok(())
}

/// In-memory audio conversion helper via temporary files.
pub fn convert_audio_bytes(
    bytes: &[u8],
    src_ext: &str,
    target: Format,
    bitrate_kbps: Option<u32>,
) -> Result<Vec<u8>, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "Audio file is empty (0 bytes)"));
    }

    let temp_dir = tempfile::tempdir().map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to create temp dir", e.to_string())
    })?;

    let in_file = temp_dir.path().join(format!("input.{}", src_ext));
    let out_file = temp_dir.path().join(format!("output.{}", target.primary_extension()));

    std::fs::write(&in_file, bytes).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to write temp audio", e.to_string())
    })?;

    convert_audio(&in_file, &out_file, target, bitrate_kbps)?;

    std::fs::read(&out_file).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to read converted audio", e.to_string())
    })
}
