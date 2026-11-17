use refract_core::error::{ErrorCode, RefractError};
use std::path::Path;
use std::process::Command;

/// Check if a native binary (e.g. ffmpeg, qpdf) is available on the system PATH.
pub fn is_binary_available(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Execute a native tool with arguments.
pub fn run_tool(cmd: &str, args: &[&str]) -> Result<Vec<u8>, RefractError> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| RefractError::with_details(
            ErrorCode::EngineFailed,
            format!("Failed to execute {}", cmd),
            e.to_string(),
        ))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RefractError::with_details(
            ErrorCode::EngineFailed,
            format!("{} failed with exit code {:?}", cmd, output.status.code()),
            stderr.to_string(),
        ));
    }

    Ok(output.stdout)
}

/// Inspect an image file using the native `image` crate.
pub fn probe_image(path: &Path) -> Result<(u32, u32), RefractError> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Cannot open image file", e.to_string()))?
        .with_guessed_format()
        .map_err(|e| RefractError::with_details(ErrorCode::UnsupportedFormat, "Unknown image format", e.to_string()))?;

    let dims = reader.into_dimensions()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Cannot read image dimensions", e.to_string()))?;

    Ok(dims)
}
