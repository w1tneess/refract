use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageDecoder, ImageFormat, ImageReader, Rgb, Rgba};
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::io::Cursor;

fn swaps_axes(o: Orientation) -> bool {
    matches!(
        o,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// Probe displayed image dimensions (respecting EXIF orientation) without fully decoding pixels.
pub fn probe_dimensions(bytes: &[u8]) -> Result<(u32, u32), RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "The file is empty (0 bytes)"));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Cannot read image format", e.to_string()))?;

    let mut decoder = reader
        .into_decoder()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to open image decoder", e.to_string()))?;

    let (w, h) = decoder.dimensions();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    if swaps_axes(orientation) {
        Ok((h, w))
    } else {
        Ok((w, h))
    }
}

/// Detect if bytes are HEIC/HEIF format by magic bytes.
fn is_heic(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    if &bytes[4..8] != b"ftyp" {
        return false;
    }
    let major = &bytes[8..12];
    matches!(major, b"heic" | b"heix" | b"mif1" | b"msf1")
}

/// Decode image bytes into an upright `DynamicImage` (baking EXIF orientation into pixels).
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

    let mut decoder = reader
        .into_decoder()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to open image decoder", e.to_string()))?;

    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to decode image data", e.to_string()))?;

    if orientation != Orientation::NoTransforms {
        img.apply_orientation(orientation);
    }

    Ok(img)
}

/// Flatten transparency onto a white background so RGBA images encode cleanly to JPEG.
fn flatten_to_rgb8(img: &DynamicImage) -> image::RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = px[3] as f32 / 255.0;
        let inv = 1.0 - a;
        let r = (px[0] as f32 * a + 255.0 * inv).round() as u8;
        let g = (px[1] as f32 * a + 255.0 * inv).round() as u8;
        let b = (px[2] as f32 * a + 255.0 * inv).round() as u8;
        rgb.put_pixel(x, y, image::Rgb([r, g, b]));
    }
    rgb
}

/// Encode a `DynamicImage` to JPEG with specified quality (1–100), stripping EXIF and flattening alpha onto white.
pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, RefractError> {
    let rgb = flatten_to_rgb8(img);
    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    encoder
        .encode_image(&rgb)
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

/// Encode a `DynamicImage` into a target `Format`.
fn encode_as(img: &DynamicImage, target: Format, quality: u8) -> Result<Vec<u8>, RefractError> {
    match target {
        Format::Jpeg => encode_jpeg(img, quality),
        Format::Png => encode_png(img),
        Format::Webp => encode_webp(img),
        _ => Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            format!("Unsupported target image format: {:?}", target),
        )),
    }
}

/// Convert an image to a target format.
pub fn convert_image(bytes: &[u8], target: Format, quality: Option<u8>) -> Result<Vec<u8>, RefractError> {
    let img = decode_image(bytes)?;
    encode_as(&img, target, quality.unwrap_or(85))
}

/// Compress an image down to target file size in KB using binary quality search with scale-down fallback.
pub fn compress_image(bytes: &[u8], target_kb: u32) -> Result<Vec<u8>, RefractError> {
    if target_kb == 0 {
        return Err(RefractError::new(ErrorCode::LimitExceeded, "Target size must be at least 1 KB"));
    }

    let img = decode_image(bytes)?;
    let (orig_w, orig_h) = img.dimensions();
    let target_bytes = (target_kb as usize) * 1024;

    let mut scale = 1.0f64;
    for _ in 0..=5 {
        let current = if scale < 1.0 {
            let w = ((orig_w as f64) * scale).round().max(1.0) as u32;
            let h = ((orig_h as f64) * scale).round().max(1.0) as u32;
            img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        } else {
            img.clone()
        };

        let mut low = 10u8;
        let mut high = 95u8;
        let mut best: Option<Vec<u8>> = None;

        for _ in 0..8 {
            if low > high {
                break;
            }
            let mid = (low + high) / 2;
            let candidate = encode_jpeg(&current, mid)?;
            if candidate.len() <= target_bytes {
                best = Some(candidate);
                low = mid + 1;
            } else {
                if mid == 10 {
                    break;
                }
                high = mid - 1;
            }
        }

        if let Some(out) = best {
            return Ok(out);
        }
        scale *= 0.9;
    }

    Err(RefractError::new(
        ErrorCode::LimitExceeded,
        format!("Could not compress image under {} KB even after scaling down", target_kb),
    ))
}

/// Resize an image using Lanczos3 resampling, preserving the source format (or encoding to `target` if provided).
/// If one dimension is 0, aspect ratio is preserved.
pub fn resize_image_with_format(
    bytes: &[u8],
    width: u32,
    height: u32,
    target: Option<Format>,
) -> Result<(Vec<u8>, Format), RefractError> {
    let img = decode_image(bytes)?;
    let (orig_w, orig_h) = img.dimensions();

    if orig_w == 0 || orig_h == 0 {
        return Err(RefractError::new(ErrorCode::DamagedFile, "Image has 0 dimensions"));
    }
    if width == 0 && height == 0 {
        return Err(RefractError::new(ErrorCode::DamagedFile, "Width or height must be greater than 0"));
    }

    let (target_w, target_h) = if width == 0 {
        let w = ((orig_w as f64 * height as f64) / orig_h as f64).round() as u32;
        (w.max(1), height)
    } else if height == 0 {
        let h = ((orig_h as f64 * width as f64) / orig_w as f64).round() as u32;
        (width, h.max(1))
    } else {
        (width, height)
    };

    let out_fmt = match target.or_else(|| refract_core::detect(bytes)) {
        Some(Format::Png) => Format::Png,
        Some(Format::Webp) => Format::Webp,
        _ => Format::Jpeg,
    };

    let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3);
    let out_bytes = encode_as(&resized, out_fmt, 90)?;
    Ok((out_bytes, out_fmt))
}

/// Resize an image using Lanczos3 resampling, preserving its detected format (PNG/WebP/JPEG).
/// If one dimension is 0, aspect ratio is preserved.
pub fn resize_image(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RefractError> {
    resize_image_with_format(bytes, width, height, None).map(|(b, _)| b)
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