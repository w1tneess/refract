use crate::image::swaps_axes;
use crate::process::run_command_cancellable;
use image::metadata::Orientation;
use image::ImageDecoder;
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::io::Read;
use std::path::Path;

/// Check if a native binary (e.g. ffmpeg, qpdf) is available on the system PATH.
pub fn is_binary_available(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Execute a native tool with arguments through the shared cancellable subprocess runner
/// (applying Windows `CREATE_NO_WINDOW`, deadlock-free stdout/stderr capture, and clear error codes).
pub fn run_tool(cmd: &str, args: &[&str]) -> Result<Vec<u8>, RefractError> {
    let output = run_command_cancellable(cmd, args, &CancelToken::none(), &[], cmd)?;
    Ok(output.stdout)
}

/// Inspect an image file's upright displayed dimensions (respecting EXIF orientation)
/// without fully decoding pixel data.
pub fn probe_image(path: &Path) -> Result<(u32, u32), RefractError> {
    // Check header bytes first so empty files and unsupported HEIC return explicit errors.
    let mut header = [0u8; refract_core::detect::HEADER_SNIFF_BYTES];
    let read_len = {
        let mut file = std::fs::File::open(path).map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Cannot open image file",
                e.to_string(),
            )
        })?;
        file.read(&mut header).map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Cannot read image file",
                e.to_string(),
            )
        })?
    };

    if read_len == 0 {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "The file is empty (0 bytes)",
        ));
    }

    if refract_core::detect(&header[..read_len]) == Some(Format::Heic) {
        return Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            "HEIC/HEIF images are not supported by the native build. Convert them first using the web app or an external tool.",
        ));
    }

    let reader = image::ImageReader::open(path)
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Cannot open image file",
                e.to_string(),
            )
        })?
        .with_guessed_format()
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::UnsupportedFormat,
                "Unknown image format",
                e.to_string(),
            )
        })?;

    let mut decoder = reader.into_decoder().map_err(|e| {
        RefractError::with_details(
            ErrorCode::DamagedFile,
            "Cannot read image dimensions",
            e.to_string(),
        )
    })?;

    let (w, h) = decoder.dimensions();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    if swaps_axes(orientation) {
        Ok((h, w))
    } else {
        Ok((w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    #[test]
    fn probe_image_respects_exif_orientation_and_rejects_empty_and_heic() {
        assert_eq!(probe_image(&fixture_path("plain.png")).unwrap(), (400, 300));
        assert_eq!(
            probe_image(&fixture_path("rotated.jpg")).unwrap(),
            (300, 400)
        );
        assert_eq!(
            probe_image(&fixture_path("empty.heic")).unwrap_err().code,
            ErrorCode::EmptyFile
        );
        assert_eq!(
            probe_image(&fixture_path("plain.heic")).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        assert_eq!(
            probe_image(&fixture_path("really-heic.jpg"))
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedFormat
        );
    }

    #[test]
    fn run_tool_reports_missing_binary_as_missing_dependency() {
        let err = run_tool("refract-missing-native-tool-99999", &[]).unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
    }
}

#[cfg(test)]
mod review_regressions {
    use super::*;
    #[test]
    fn compatible_heic_brand_after_byte_64_is_explicitly_unsupported() {
        let mut bytes = vec![0; 72];
        bytes[..4].copy_from_slice(&72u32.to_be_bytes());
        bytes[4..8].copy_from_slice(b"ftyp");
        bytes[8..12].copy_from_slice(b"zzzz");
        bytes[68..72].copy_from_slice(b"heic");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.heic");
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(
            probe_image(&path).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        std::fs::write(&path, []).unwrap();
        assert_eq!(probe_image(&path).unwrap_err().code, ErrorCode::EmptyFile);
    }
}
