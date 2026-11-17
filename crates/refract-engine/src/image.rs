use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use std::io::Cursor;

/// Probe image dimensions without fully decoding pixels where possible.
pub fn probe_dimensions(bytes: &[u8]) -> Result<(u32, u32), RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "The file is empty (0 bytes)"));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Cannot read image format", e.to_string()))?;

    reader.into_dimensions()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to read image dimensions", e.to_string()))
}

/// Decode image bytes into a `DynamicImage`.
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "The file is empty (0 bytes)"));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Cannot guess image format", e.to_string()))?;

    reader.decode()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to decode image data", e.to_string()))
}

/// Encode a `DynamicImage` to JPEG with specified quality (1–100).
pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, RefractError> {
    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    encoder.encode_image(img)
        .map_err(|e| RefractError::with_details(ErrorCode::EngineFailed, "JPEG encode failed", e.to_string()))?;
    Ok(buf)
}

/// Encode a `DynamicImage` to PNG.
pub fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, RefractError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| RefractError::with_details(ErrorCode::EngineFailed, "PNG encode failed", e.to_string()))?;
    Ok(buf)
}

/// Encode a `DynamicImage` to WebP.
pub fn encode_webp(img: &DynamicImage) -> Result<Vec<u8>, RefractError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::WebP)
        .map_err(|e| RefractError::with_details(ErrorCode::EngineFailed, "WebP encode failed", e.to_string()))?;
    Ok(buf)
}

/// Convert an image to a target format.
pub fn convert_image(bytes: &[u8], target: Format, quality: Option<u8>) -> Result<Vec<u8>, RefractError> {
    let img = decode_image(bytes)?;
    match target {
        Format::Jpeg => encode_jpeg(&img, quality.unwrap_or(85)),
        Format::Png => encode_png(&img),
        Format::Webp => encode_webp(&img),
        _ => Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            format!("Unsupported target image format: {:?}", target),
        )),
    }
}

/// Compress an image down to target file size in KB using binary quality search.
pub fn compress_image(bytes: &[u8], target_kb: u32) -> Result<Vec<u8>, RefractError> {
    let img = decode_image(bytes)?;
    let target_bytes = (target_kb as usize) * 1024;

    let mut low = 10u8;
    let mut high = 95u8;
    let mut best = None;

    while low <= high {
        let mid = (low + high) / 2;
        let candidate = encode_jpeg(&img, mid)?;
        let size = candidate.len();

        if size <= target_bytes {
            best = Some(candidate);
            low = mid + 1; // Try better quality
        } else {
            if mid == 10 {
                best = Some(candidate);
                break;
            }
            high = mid - 1; // Need smaller size
        }
    }

    best.ok_or_else(|| RefractError::new(ErrorCode::EngineFailed, "Failed to compress image"))
}

/// Resize an image using Lanczos3 resampling.
/// If one dimension is 0, aspect ratio is preserved.
pub fn resize_image(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RefractError> {
    let img = decode_image(bytes)?;
    let (orig_w, orig_h) = img.dimensions();

    if orig_w == 0 || orig_h == 0 {
        return Err(RefractError::new(ErrorCode::DamagedFile, "Image has 0 dimensions"));
    }

    let (target_w, target_h) = if width == 0 && height > 0 {
        let w = ((orig_w as f64 * height as f64) / orig_h as f64).round() as u32;
        (w.max(1), height)
    } else if height == 0 && width > 0 {
        let h = ((orig_h as f64 * width as f64) / orig_w as f64).round() as u32;
        (width, h.max(1))
    } else if width > 0 && height > 0 {
        (width, height)
    } else {
        (orig_w, orig_h)
    };

    let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3);
    encode_jpeg(&resized, 85)
}

/// Resize an image by cm with custom DPI (default 300).
pub fn resize_image_cm(bytes: &[u8], width_cm: f64, height_cm: f64, dpi: f64) -> Result<Vec<u8>, RefractError> {
    let width_px = if width_cm > 0.0 { (width_cm * dpi / 2.54).round() as u32 } else { 0 };
    let height_px = if height_cm > 0.0 { (height_cm * dpi / 2.54).round() as u32 } else { 0 };
    resize_image(bytes, width_px, height_px)
}
