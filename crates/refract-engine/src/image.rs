use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, Rgba, Rgb, ImageBuffer};
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

/// Detect if bytes are HEIC/HEIF format by magic bytes.
fn is_heic(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    // HEIC files are ISOBMFF-based. Check for 'ftyp' box with heic/heix/mif1/msf1 brands.
    // At offset 4: 'ftyp' (0x66 0x74 0x79 0x70)
    // At offset 8: major brand (4 bytes)
    if &bytes[4..8] != b"ftyp" {
        return false;
    }
    let major = &bytes[8..12];
    matches!(major, b"heic" | b"heix" | b"mif1" | b"msf1")
}

/// Decode image bytes into a `DynamicImage`.
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "The file is empty (0 bytes)"));
    }

    if is_heic(bytes) {
        return Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            "HEIC/HEIF images are not supported by the native build. Convert them first using the web app or an external tool.",
        ));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Cannot guess image format", e.to_string()))?;

    reader
        .decode()
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

/// Encode a `DynamicImage` to JPEG, flattening any alpha channel onto a white
/// background. This is needed for PDF XObjects which don't support transparency.
pub fn encode_jpeg_flattened(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, RefractError> {
    // Flatten onto white background
    let rgb = match img {
        DynamicImage::ImageRgba8(rgba) => {
            let (w, h) = rgba.dimensions();
            let mut out = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(w, h);
            for (x, y, pixel) in rgba.enumerate_pixels() {
                let Rgba([r, g, b, a]) = *pixel;
                if a == 255 {
                    out.put_pixel(x, y, Rgb([r, g, b]));
                } else if a == 0 {
                    out.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    let a_f = a as f32 / 255.0;
                    out.put_pixel(
                        x,
                        y,
                        Rgb([
                            (r as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                            (g as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                            (b as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                        ]),
                    );
                }
            }
            DynamicImage::ImageRgb8(out)
        }
        DynamicImage::ImageRgba16(rgba) => {
            let (w, h) = rgba.dimensions();
            let mut out = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(w, h);
            for (x, y, pixel) in rgba.enumerate_pixels() {
                let Rgba([r, g, b, a]) = *pixel;
                let r8 = (r >> 8) as u8;
                let g8 = (g >> 8) as u8;
                let b8 = (b >> 8) as u8;
                let a8 = (a >> 8) as u8;
                if a8 == 255 {
                    out.put_pixel(x, y, Rgb([r8, g8, b8]));
                } else if a8 == 0 {
                    out.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    let a_f = a8 as f32 / 255.0;
                    out.put_pixel(
                        x,
                        y,
                        Rgb([
                            (r8 as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                            (g8 as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                            (b8 as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                        ]),
                    );
                }
            }
            DynamicImage::ImageRgb8(out)
        }
        DynamicImage::ImageRgba32F(rgba) => {
            let (w, h) = rgba.dimensions();
            let mut out = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(w, h);
            for (x, y, pixel) in rgba.enumerate_pixels() {
                let Rgba([r, g, b, a]) = *pixel;
                let r8 = (r.clamp(0.0, 1.0) * 255.0).round() as u8;
                let g8 = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
                let b8 = (b.clamp(0.0, 1.0) * 255.0).round() as u8;
                let a8 = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
                if a8 == 255 {
                    out.put_pixel(x, y, Rgb([r8, g8, b8]));
                } else if a8 == 0 {
                    out.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    let a_f = a8 as f32 / 255.0;
                    out.put_pixel(
                        x,
                        y,
                        Rgb([
                            (r8 as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                            (g8 as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                            (b8 as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8,
                        ]),
                    );
                }
            }
            DynamicImage::ImageRgb8(out)
        }
        DynamicImage::ImageLumaA8(luma) => {
            let (w, h) = luma.dimensions();
            let mut out = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(w, h);
            for (x, y, pixel) in luma.enumerate_pixels() {
                let l = pixel[0];
                let a = pixel[1];
                if a == 255 {
                    out.put_pixel(x, y, Rgb([l, l, l]));
                } else if a == 0 {
                    out.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    let a_f = a as f32 / 255.0;
                    let v = (l as f32 * a_f + 255.0 * (1.0 - a_f)).round() as u8;
                    out.put_pixel(x, y, Rgb([v, v, v]));
                }
            }
            DynamicImage::ImageRgb8(out)
        }
        other => other.to_rgb8().into(),
    };

    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    encoder
        .encode_image(&rgb)
        .map_err(|e| RefractError::with_details(ErrorCode::EngineFailed, "JPEG encode failed", e.to_string()))?;
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

/// Resize an image preserving the original format and quality where possible.
/// If target format is given, output is encoded to that format. Otherwise
/// the source format is used when possible (JPEG/PNG/WebP), defaulting to JPEG.
pub fn resize_image_keep_format(
    bytes: &[u8],
    width: u32,
    height: u32,
    target_format: Option<Format>,
    quality: Option<u8>,
) -> Result<Vec<u8>, RefractError> {
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

    let fmt = target_format.unwrap_or_else(|| {
        // Try to infer from magic bytes
        if bytes.len() >= 4 {
            match &bytes[0..4] {
                [0xFF, 0xD8, 0xFF, _] => Format::Jpeg,
                [0x89, 0x50, 0x4E, 0x47] => Format::Png,
                [0x52, 0x49, 0x46, 0x46] => Format::Webp,
                _ => Format::Jpeg,
            }
        } else {
            Format::Jpeg
        }
    });

    match fmt {
        Format::Jpeg => encode_jpeg(&resized, quality.unwrap_or(85)),
        Format::Png => encode_png(&resized),
        Format::Webp => encode_webp(&resized),
        _ => encode_jpeg(&resized, quality.unwrap_or(85)),
    }
}

/// Resize an image by cm with custom DPI (default 300).
pub fn resize_image_cm(bytes: &[u8], width_cm: f64, height_cm: f64, dpi: f64) -> Result<Vec<u8>, RefractError> {
    let width_px = if width_cm > 0.0 { (width_cm * dpi / 2.54).round() as u32 } else { 0 };
    let height_px = if height_cm > 0.0 { (height_cm * dpi / 2.54).round() as u32 } else { 0 };
    resize_image(bytes, width_px, height_px)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    #[test]
    fn encode_jpeg_flattened_works_on_opaque() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(10, 10));
        let out = encode_jpeg_flattened(&img, 80).unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn is_heic_detects_heic_brand() {
        // Minimal ftyp box with 'heic' major brand
        let mut data = vec![0u8; 12];
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"heic");
        assert!(is_heic(&data));
    }

    #[test]
    fn is_heic_rejects_other() {
        let mut data = vec![0u8; 12];
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"avif");
        assert!(!is_heic(&data));
    }
}