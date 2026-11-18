use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader};
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::io::Cursor;

/// Valid JPEG/image quality range aligned with `specs/tool-manifest.json`.
pub const MIN_IMAGE_QUALITY: u8 = 10;
pub const MAX_IMAGE_QUALITY: u8 = 100;
pub const DEFAULT_IMAGE_QUALITY: u8 = 90;

/// Valid print DPI range for centimetre resizing aligned with `specs/tool-manifest.json`.
pub const MIN_RESIZE_DPI: f64 = 72.0;
pub const MAX_RESIZE_DPI: f64 = 1200.0;
pub const DEFAULT_RESIZE_DPI: f64 = 300.0;

/// Upper bound on any single resized pixel dimension to prevent pathological allocations.
pub const MAX_RESIZE_DIMENSION_PX: u32 = 32_768;

/// Measurement unit for `resize-image` dimensions (`"px"` or `"cm"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeUnit {
    Px,
    Cm,
}

impl ResizeUnit {
    /// Parse a canonical manifest unit string (`"px"` or `"cm"`).
    pub fn parse(raw: &str) -> Result<Self, RefractError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "px" | "pixels" => Ok(Self::Px),
            "cm" | "centimetres" | "centimeters" => Ok(Self::Cm),
            other => Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Invalid resize unit '{}'. Expected 'px' or 'cm'.", other),
            )),
        }
    }
}

/// Canonical options for `resize-image`, aligned with `specs/tool-manifest.json`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResizeOptions {
    pub unit: ResizeUnit,
    /// Target width in `unit` (`0.0` preserves aspect ratio from `height`).
    pub width: f64,
    /// Target height in `unit` (`0.0` preserves aspect ratio from `width`).
    pub height: f64,
    /// Print resolution in DPI (used when `unit == ResizeUnit::Cm`; valid range `72..=1200`).
    pub dpi: f64,
    /// Target output format (`None` means `"same"` -> preserve JPEG/PNG/WebP, fallback JPEG).
    pub target_format: Option<Format>,
    /// JPEG quality (`10..=100`, default `90`). Ignored for PNG and lossless WebP.
    pub quality: u8,
}

impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            unit: ResizeUnit::Px,
            width: 1200.0,
            height: 0.0,
            dpi: DEFAULT_RESIZE_DPI,
            target_format: None,
            quality: DEFAULT_IMAGE_QUALITY,
        }
    }
}

pub(crate) fn swaps_axes(o: Orientation) -> bool {
    matches!(
        o,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// Validate image quality (`10..=100`) without silently clamping.
pub fn validate_image_quality(quality: u8) -> Result<u8, RefractError> {
    if !(MIN_IMAGE_QUALITY..=MAX_IMAGE_QUALITY).contains(&quality) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Invalid image quality {}. Must be between {} and {}.",
                quality, MIN_IMAGE_QUALITY, MAX_IMAGE_QUALITY
            ),
        ));
    }
    Ok(quality)
}

/// Validate print resolution DPI (`72..=1200`) for centimetre resizing without silently clamping.
pub fn validate_resize_dpi(dpi: f64) -> Result<f64, RefractError> {
    if !dpi.is_finite() || !(MIN_RESIZE_DPI..=MAX_RESIZE_DPI).contains(&dpi) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Invalid resize DPI '{}'. Must be between {} and {}.",
                dpi, MIN_RESIZE_DPI as u32, MAX_RESIZE_DPI as u32
            ),
        ));
    }
    Ok(dpi)
}

/// Convert centimetres to rounded pixels at the given DPI, matching `@refract/core`'s `cmToPx`.
pub fn cm_to_px(cm: f64, dpi: f64) -> Result<u32, RefractError> {
    let valid_dpi = validate_resize_dpi(dpi)?;
    if !cm.is_finite() || cm < 0.0 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!("Invalid centimetre dimension '{}'. Must be >= 0.", cm),
        ));
    }
    if cm == 0.0 {
        return Ok(0);
    }
    let raw = ((cm / 2.54) * valid_dpi).round();
    if !raw.is_finite() || raw < 1.0 || raw > MAX_RESIZE_DIMENSION_PX as f64 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Resized dimension ({} cm at {} DPI = {:.0} px) must be between 1 and {} px.",
                cm, valid_dpi, raw, MAX_RESIZE_DIMENSION_PX
            ),
        ));
    }
    Ok(raw as u32)
}

/// Parse the canonical `resize-image` `to` option (`"same"`, `"jpeg"`, `"png"`, `"webp"`).
pub fn parse_resize_target_format(raw: &str) -> Result<Option<Format>, RefractError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "same" => Ok(None),
        "jpg" | "jpeg" => Ok(Some(Format::Jpeg)),
        "png" => Ok(Some(Format::Png)),
        "webp" => Ok(Some(Format::Webp)),
        "heic" | "heif" => Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            "Encoding to HEIC/HEIF is not supported. Choose JPG, PNG, WebP, or Same format.",
        )),
        other => Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Invalid resize output format '{}'. Expected 'same', 'jpeg', 'png', or 'webp'.",
                other
            ),
        )),
    }
}

fn validate_output_image_format(target: Format) -> Result<Format, RefractError> {
    match target {
        Format::Jpeg | Format::Png | Format::Webp => Ok(target),
        _ => Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            format!("Unsupported target image format: {:?}", target),
        )),
    }
}

fn convert_dimension_to_px(value: f64, unit: ResizeUnit, dpi: f64) -> Result<u32, RefractError> {
    if !value.is_finite() || value < 0.0 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!("Invalid resize dimension '{}'. Must be >= 0.", value),
        ));
    }
    if value == 0.0 {
        return Ok(0);
    }
    match unit {
        ResizeUnit::Cm => cm_to_px(value, dpi),
        ResizeUnit::Px => {
            let rounded = value.round();
            if !rounded.is_finite() || rounded < 1.0 || rounded > MAX_RESIZE_DIMENSION_PX as f64 {
                return Err(RefractError::new(
                    ErrorCode::InvalidOption,
                    format!(
                        "Resize dimension ({:.0} px) must be between 1 and {} px.",
                        rounded, MAX_RESIZE_DIMENSION_PX
                    ),
                ));
            }
            Ok(rounded as u32)
        }
    }
}

/// Resolve requested `ResizeOptions` into concrete pixel dimensions `(target_w, target_h)`
/// against source upright dimensions `(orig_w, orig_h)`.
pub fn resolve_resize_dimensions(
    orig_w: u32,
    orig_h: u32,
    options: &ResizeOptions,
) -> Result<(u32, u32), RefractError> {
    if orig_w == 0 || orig_h == 0 {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "Image has 0 dimensions",
        ));
    }

    if options.unit == ResizeUnit::Cm {
        validate_resize_dpi(options.dpi)?;
    }

    let req_w = convert_dimension_to_px(options.width, options.unit, options.dpi)?;
    let req_h = convert_dimension_to_px(options.height, options.unit, options.dpi)?;

    if req_w == 0 && req_h == 0 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Width or height must be greater than 0.",
        ));
    }

    let (target_w, target_h) = if req_w == 0 {
        let computed = ((orig_w as f64 * req_h as f64) / orig_h as f64)
            .round()
            .max(1.0);
        if computed > MAX_RESIZE_DIMENSION_PX as f64 {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Computed width ({:.0} px) exceeds maximum allowed dimension ({} px).",
                    computed, MAX_RESIZE_DIMENSION_PX
                ),
            ));
        }
        (computed as u32, req_h)
    } else if req_h == 0 {
        let computed = ((orig_h as f64 * req_w as f64) / orig_w as f64)
            .round()
            .max(1.0);
        if computed > MAX_RESIZE_DIMENSION_PX as f64 {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Computed height ({:.0} px) exceeds maximum allowed dimension ({} px).",
                    computed, MAX_RESIZE_DIMENSION_PX
                ),
            ));
        }
        (req_w, computed as u32)
    } else {
        (req_w, req_h)
    };

    Ok((target_w, target_h))
}

/// Detect if bytes are HEIC/HEIF format by magic bytes via `refract_core::detect`.
fn is_heic(bytes: &[u8]) -> bool {
    refract_core::detect(bytes) == Some(Format::Heic)
}

/// Verify that a JPEG byte stream reaches an authentic End-Of-Image (`0xFF 0xD9`) marker
/// rather than ending mid-header or mid-scan (since `zune-jpeg` permits truncated scans by default).
fn jpeg_has_complete_eoi(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return false;
    }
    let mut i = 2usize;
    while i < bytes.len() {
        if bytes[i] != 0xFF {
            return false;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            return false;
        }
        let marker = bytes[i];
        i += 1;

        if marker == 0xD9 {
            return true;
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            continue;
        }
        if i + 2 > bytes.len() {
            return false;
        }
        let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if seg_len < 2 || i + seg_len > bytes.len() {
            return false;
        }
        i += seg_len;

        if marker == 0xDA {
            // Scan entropy-coded data, skipping byte-stuffed 0xFF 0x00 and RST0..RST7 markers.
            let mut found_next_marker = false;
            while i < bytes.len() {
                if bytes[i] == 0xFF {
                    let mut next = i + 1;
                    while next < bytes.len() && bytes[next] == 0xFF {
                        next += 1;
                    }
                    if next >= bytes.len() {
                        return false;
                    }
                    let m = bytes[next];
                    if m == 0x00 || (0xD0..=0xD7).contains(&m) {
                        i = next + 1;
                        continue;
                    }
                    i = next - 1;
                    found_next_marker = true;
                    break;
                }
                i += 1;
            }
            if !found_next_marker {
                return false;
            }
        }
    }
    false
}

/// Probe displayed image dimensions (respecting EXIF orientation) without fully decoding pixels.
pub fn probe_dimensions(bytes: &[u8]) -> Result<(u32, u32), RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "The file is empty (0 bytes)",
        ));
    }

    if is_heic(bytes) {
        return Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            "HEIC/HEIF images are not supported by the native build. Convert them first using the web app or an external tool.",
        ));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Cannot read image format",
                e.to_string(),
            )
        })?;

    let mut decoder = reader.into_decoder().map_err(|e| {
        RefractError::with_details(
            ErrorCode::DamagedFile,
            "Failed to open image decoder",
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

/// Decode image bytes into an upright `DynamicImage` (baking EXIF orientation into pixels)
/// with cooperative cancellation checkpoints.
pub fn decode_image_cancellable(
    bytes: &[u8],
    cancel: &CancelToken,
) -> Result<DynamicImage, RefractError> {
    cancel.check()?;
    if bytes.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "The file is empty (0 bytes)",
        ));
    }

    if is_heic(bytes) {
        return Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            "HEIC/HEIF images are not supported by the native build. Convert them first using the web app or an external tool.",
        ));
    }

    if refract_core::detect(bytes) == Some(Format::Jpeg) && !jpeg_has_complete_eoi(bytes) {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "The JPEG file is truncated (missing End-Of-Image marker).",
        ));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Cannot guess image format",
                e.to_string(),
            )
        })?;

    let mut decoder = reader.into_decoder().map_err(|e| {
        RefractError::with_details(
            ErrorCode::DamagedFile,
            "Failed to open image decoder",
            e.to_string(),
        )
    })?;

    cancel.check()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| {
        RefractError::with_details(
            ErrorCode::DamagedFile,
            "Failed to decode image data",
            e.to_string(),
        )
    })?;

    cancel.check()?;
    if orientation != Orientation::NoTransforms {
        img.apply_orientation(orientation);
    }

    Ok(img)
}

/// Decode image bytes into an upright `DynamicImage` (baking EXIF orientation into pixels).
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, RefractError> {
    decode_image_cancellable(bytes, &CancelToken::none())
}

/// Flatten transparency onto a white background so any alpha-bearing `DynamicImage`
/// (`Rgba8`, `Rgba16`, `Rgba32F`, `LumaA8`, `LumaA16`) encodes cleanly to JPEG.
fn flatten_to_rgb8(img: &DynamicImage) -> image::RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        if a == 255 {
            rgb.put_pixel(x, y, image::Rgb([r, g, b]));
        } else if a == 0 {
            rgb.put_pixel(x, y, image::Rgb([255, 255, 255]));
        } else {
            let alpha = a as f32 / 255.0;
            let inv = 1.0 - alpha;
            let rr = (r as f32 * alpha + 255.0 * inv).round() as u8;
            let gg = (g as f32 * alpha + 255.0 * inv).round() as u8;
            let bb = (b as f32 * alpha + 255.0 * inv).round() as u8;
            rgb.put_pixel(x, y, image::Rgb([rr, gg, bb]));
        }
    }
    rgb
}

fn encode_rgb8_jpeg(rgb: &image::RgbImage, quality: u8) -> Result<Vec<u8>, RefractError> {
    let mut buf = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    encoder.encode_image(rgb).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "JPEG encode failed", e.to_string())
    })?;
    Ok(buf)
}

/// Encode a `DynamicImage` to JPEG with specified quality (1–100), stripping EXIF and flattening alpha onto white.
pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, RefractError> {
    let rgb = flatten_to_rgb8(img);
    encode_rgb8_jpeg(&rgb, quality)
}

/// Encode a `DynamicImage` to PNG.
pub fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, RefractError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| {
            RefractError::with_details(ErrorCode::EngineFailed, "PNG encode failed", e.to_string())
        })?;
    Ok(buf)
}

/// Encode a `DynamicImage` to JPEG, flattening any alpha channel onto a white
/// background. Consolidated onto [`encode_jpeg`] (preserved for API compatibility).
pub fn encode_jpeg_flattened(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, RefractError> {
    encode_jpeg(img, quality)
}

/// Encode a `DynamicImage` to lossless WebP via the pure-Rust `image` crate encoder.
pub fn encode_webp(img: &DynamicImage) -> Result<Vec<u8>, RefractError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::WebP)
        .map_err(|e| {
            RefractError::with_details(ErrorCode::EngineFailed, "WebP encode failed", e.to_string())
        })?;
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

/// Convert an image to a target format (`JPEG`, `PNG`, or `WebP`) with cooperative cancellation.
pub fn convert_image_cancellable(
    bytes: &[u8],
    target: Format,
    quality: Option<u8>,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    validate_output_image_format(target)?;
    let valid_quality = match quality {
        Some(q) => validate_image_quality(q)?,
        None => DEFAULT_IMAGE_QUALITY,
    };

    let img = decode_image_cancellable(bytes, cancel)?;
    cancel.check()?;
    encode_as(&img, target, valid_quality)
}

/// Convert an image to a target format.
pub fn convert_image(
    bytes: &[u8],
    target: Format,
    quality: Option<u8>,
) -> Result<Vec<u8>, RefractError> {
    convert_image_cancellable(bytes, target, quality, &CancelToken::none())
}

/// Compress an image down to a target JPEG file size in KB using binary quality search with
/// scale-down fallback and cooperative cancellation checkpoints between every resize and encode step.
///
/// Note: Desktop native compression outputs JPEG only (no external C/C++ `libwebp` dependency).
pub fn compress_image_cancellable(
    bytes: &[u8],
    target_kb: u32,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    compress_image_cancellable_with_hook(bytes, target_kb, cancel, || {})
}

fn compress_image_cancellable_with_hook<F: FnMut()>(
    bytes: &[u8],
    target_kb: u32,
    cancel: &CancelToken,
    mut on_iteration: F,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    if target_kb == 0 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Target size must be at least 1 KB.",
        ));
    }

    let img = decode_image_cancellable(bytes, cancel)?;
    let (orig_w, orig_h) = img.dimensions();
    let target_bytes = (target_kb as usize).saturating_mul(1024);

    let mut scale = 1.0f64;
    for _ in 0..=5 {
        cancel.check()?;
        let rgb = if scale < 1.0 {
            let w = ((orig_w as f64) * scale).round().max(1.0) as u32;
            let h = ((orig_h as f64) * scale).round().max(1.0) as u32;
            let scaled = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
            cancel.check()?;
            flatten_to_rgb8(&scaled)
        } else {
            flatten_to_rgb8(&img)
        };

        let mut low = 10u8;
        let mut high = 95u8;
        let mut best: Option<Vec<u8>> = None;

        for _ in 0..8 {
            cancel.check()?;
            if low > high {
                break;
            }
            let mid = (low + high) / 2;
            let candidate = encode_rgb8_jpeg(&rgb, mid)?;
            on_iteration();
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

        cancel.check()?;
        if let Some(out) = best {
            return Ok(out);
        }
        scale *= 0.9;
    }

    Err(RefractError::new(
        ErrorCode::LimitExceeded,
        format!(
            "Could not compress image under {} KB even after scaling down",
            target_kb
        ),
    ))
}

/// Compress an image down to target file size in KB using binary quality search with scale-down fallback.
pub fn compress_image(bytes: &[u8], target_kb: u32) -> Result<Vec<u8>, RefractError> {
    compress_image_cancellable(bytes, target_kb, &CancelToken::none())
}

/// Resize an image using full manifest-aligned [`ResizeOptions`] (`px` or `cm` + DPI, aspect ratio,
/// output format selection or source format preservation, quality validation) with cooperative cancellation.
pub fn resize_image_with_options_cancellable(
    bytes: &[u8],
    options: &ResizeOptions,
    cancel: &CancelToken,
) -> Result<(Vec<u8>, Format), RefractError> {
    cancel.check()?;
    let valid_quality = validate_image_quality(options.quality)?;
    if let Some(fmt) = options.target_format {
        validate_output_image_format(fmt)?;
    }

    // Fail fast on obviously invalid numeric inputs before decoding large images.
    if options.unit == ResizeUnit::Cm {
        validate_resize_dpi(options.dpi)?;
    }
    let req_w = convert_dimension_to_px(options.width, options.unit, options.dpi)?;
    let req_h = convert_dimension_to_px(options.height, options.unit, options.dpi)?;
    if req_w == 0 && req_h == 0 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Width or height must be greater than 0.",
        ));
    }

    let img = decode_image_cancellable(bytes, cancel)?;
    let (orig_w, orig_h) = img.dimensions();
    let (target_w, target_h) = resolve_resize_dimensions(orig_w, orig_h, options)?;

    let out_fmt = match options.target_format {
        Some(fmt) => fmt,
        None => match refract_core::detect(bytes) {
            Some(Format::Png) => Format::Png,
            Some(Format::Webp) => Format::Webp,
            _ => Format::Jpeg,
        },
    };

    cancel.check()?;
    let out_bytes = if target_w == orig_w && target_h == orig_h {
        encode_as(&img, out_fmt, valid_quality)?
    } else {
        let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3);
        cancel.check()?;
        encode_as(&resized, out_fmt, valid_quality)?
    };

    cancel.check()?;
    Ok((out_bytes, out_fmt))
}

/// Resize an image using full manifest-aligned [`ResizeOptions`].
pub fn resize_image_with_options(
    bytes: &[u8],
    options: &ResizeOptions,
) -> Result<(Vec<u8>, Format), RefractError> {
    resize_image_with_options_cancellable(bytes, options, &CancelToken::none())
}

/// Resize an image using Lanczos3 resampling, preserving the source format (or encoding to `target` if provided).
/// If one dimension is 0, aspect ratio is preserved.
pub fn resize_image_with_format(
    bytes: &[u8],
    width: u32,
    height: u32,
    target: Option<Format>,
) -> Result<(Vec<u8>, Format), RefractError> {
    let options = ResizeOptions {
        unit: ResizeUnit::Px,
        width: width as f64,
        height: height as f64,
        dpi: DEFAULT_RESIZE_DPI,
        target_format: target,
        quality: DEFAULT_IMAGE_QUALITY,
    };
    resize_image_with_options_cancellable(bytes, &options, &CancelToken::none())
}

/// Resize an image using Lanczos3 resampling, preserving its detected format (PNG/WebP/JPEG).
/// If one dimension is 0, aspect ratio is preserved.
pub fn resize_image(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RefractError> {
    resize_image_with_format(bytes, width, height, None).map(|(b, _)| b)
}

/// Resize an image preserving the original format and optional JPEG quality.
/// Consolidated onto [`resize_image_with_options_cancellable`] using `refract_core::detect`.
pub fn resize_image_keep_format(
    bytes: &[u8],
    width: u32,
    height: u32,
    target_format: Option<Format>,
    quality: Option<u8>,
) -> Result<Vec<u8>, RefractError> {
    let options = ResizeOptions {
        unit: ResizeUnit::Px,
        width: width as f64,
        height: height as f64,
        dpi: DEFAULT_RESIZE_DPI,
        target_format,
        quality: quality.unwrap_or(DEFAULT_IMAGE_QUALITY),
    };
    resize_image_with_options_cancellable(bytes, &options, &CancelToken::none()).map(|(b, _)| b)
}

/// Resize an image by centimetres with custom DPI (valid range `72..=1200`), preserving source format.
pub fn resize_image_cm(
    bytes: &[u8],
    width_cm: f64,
    height_cm: f64,
    dpi: f64,
) -> Result<Vec<u8>, RefractError> {
    let options = ResizeOptions {
        unit: ResizeUnit::Cm,
        width: width_cm,
        height: height_cm,
        dpi,
        target_format: None,
        quality: DEFAULT_IMAGE_QUALITY,
    };
    resize_image_with_options_cancellable(bytes, &options, &CancelToken::none()).map(|(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GrayAlphaImage, LumaA, RgbImage, Rgba, RgbaImage};
    use std::path::PathBuf;

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
    }

    #[test]
    fn encode_jpeg_flattened_handles_opaque_rgba8_rgba16_and_lumaa8() {
        let opaque = DynamicImage::ImageRgb8(RgbImage::new(10, 10));
        let out = encode_jpeg_flattened(&opaque, 80).unwrap();
        assert_eq!(refract_core::detect(&out), Some(Format::Jpeg));

        // Fully transparent RGBA8 pixel must flatten to white (near 255, 255, 255)
        let rgba8 = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0])));
        let jpg_rgba8 = encode_jpeg_flattened(&rgba8, 95).unwrap();
        let decoded = decode_image(&jpg_rgba8).unwrap().to_rgb8();
        let p = decoded.get_pixel(8, 8);
        assert!(p[0] > 245 && p[1] > 245 && p[2] > 245, "got {:?}", p);

        // Semi-transparent LumaA8 pixel must flatten cleanly without error
        let lumaa8 = DynamicImage::ImageLumaA8(GrayAlphaImage::from_pixel(16, 16, LumaA([0, 128])));
        let jpg_lumaa8 = encode_jpeg_flattened(&lumaa8, 90).unwrap();
        let decoded_luma = decode_image(&jpg_lumaa8).unwrap().to_rgb8();
        let pl = decoded_luma.get_pixel(8, 8);
        assert!(
            (110..=145).contains(&pl[0]),
            "expected ~128 gray over white, got {:?}",
            pl
        );

        // Rgba16 must also flatten cleanly
        let rgba16 =
            DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(8, 8, Rgba([0u16, 0, 0, 0])));
        let jpg_rgba16 = encode_jpeg_flattened(&rgba16, 95).unwrap();
        let decoded_16 = decode_image(&jpg_rgba16).unwrap().to_rgb8();
        let p16 = decoded_16.get_pixel(4, 4);
        assert!(p16[0] > 245 && p16[1] > 245 && p16[2] > 245);
    }

    #[test]
    fn is_heic_detects_heic_brands_and_native_decode_rejects_heic_cleanly() {
        let mut data = vec![0u8; 12];
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"heic");
        assert!(is_heic(&data));

        let err = decode_image(&data).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedFormat);

        let probe_err = probe_dimensions(&data).unwrap_err();
        assert_eq!(probe_err.code, ErrorCode::UnsupportedFormat);

        // Non-HEIC ftyp box is not treated as HEIC
        data[8..12].copy_from_slice(b"avif");
        assert!(!is_heic(&data));
    }

    #[test]
    fn resize_options_support_px_cm_dpi_aspect_ratio_and_output_format() {
        let plain_png = fixture_bytes("plain.png"); // 400x300

        // 1. cm + DPI with height=0 preserves 4:3 aspect ratio and source PNG format:
        //    2.54 cm @ 300 DPI = 300 px wide -> 300x225
        let cm_opts = ResizeOptions {
            unit: ResizeUnit::Cm,
            width: 2.54,
            height: 0.0,
            dpi: 300.0,
            target_format: None,
            quality: 90,
        };
        let (cm_out, cm_fmt) = resize_image_with_options(&plain_png, &cm_opts).unwrap();
        assert_eq!(cm_fmt, Format::Png);
        assert_eq!(refract_core::detect(&cm_out), Some(Format::Png));
        assert_eq!(probe_dimensions(&cm_out).unwrap(), (300, 225));

        // 2. cm + custom DPI + explicit target format (WebP):
        //    5.08 cm @ 72 DPI = 144 px wide, explicit height 2.54 cm @ 72 DPI = 72 px tall
        let webp_opts = ResizeOptions {
            unit: ResizeUnit::Cm,
            width: 5.08,
            height: 2.54,
            dpi: 72.0,
            target_format: parse_resize_target_format("webp").unwrap(),
            quality: 90,
        };
        let (webp_out, webp_fmt) = resize_image_with_options(&plain_png, &webp_opts).unwrap();
        assert_eq!(webp_fmt, Format::Webp);
        assert_eq!(refract_core::detect(&webp_out), Some(Format::Webp));
        assert_eq!(probe_dimensions(&webp_out).unwrap(), (144, 72));

        // 3. px + explicit JPEG target format with height-only (width=0)
        let jpg_opts = ResizeOptions {
            unit: ResizeUnit::Px,
            width: 0.0,
            height: 150.0,
            dpi: 300.0,
            target_format: parse_resize_target_format("jpeg").unwrap(),
            quality: 85,
        };
        let (jpg_out, jpg_fmt) = resize_image_with_options(&plain_png, &jpg_opts).unwrap();
        assert_eq!(jpg_fmt, Format::Jpeg);
        assert_eq!(refract_core::detect(&jpg_out), Some(Format::Jpeg));
        assert_eq!(probe_dimensions(&jpg_out).unwrap(), (200, 150));

        // 4. Legacy helpers resize_image_cm and resize_image_keep_format stay consistent
        let cm_legacy = resize_image_cm(&plain_png, 2.54, 0.0, 150.0).unwrap();
        assert_eq!(refract_core::detect(&cm_legacy), Some(Format::Png));
        assert_eq!(probe_dimensions(&cm_legacy).unwrap(), (150, 113));

        let keep_legacy =
            resize_image_keep_format(&plain_png, 100, 0, Some(Format::Jpeg), Some(80)).unwrap();
        assert_eq!(refract_core::detect(&keep_legacy), Some(Format::Jpeg));
        assert_eq!(probe_dimensions(&keep_legacy).unwrap(), (100, 75));
    }

    #[test]
    fn invalid_options_are_rejected_without_silent_clamping() {
        let plain_png = fixture_bytes("plain.png");

        // ResizeUnit parsing
        assert_eq!(ResizeUnit::parse("px").unwrap(), ResizeUnit::Px);
        assert_eq!(ResizeUnit::parse("CM").unwrap(), ResizeUnit::Cm);
        assert_eq!(
            ResizeUnit::parse("inches").unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // parse_resize_target_format
        assert_eq!(parse_resize_target_format("same").unwrap(), None);
        assert_eq!(
            parse_resize_target_format("heic").unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        assert_eq!(
            parse_resize_target_format("tiff").unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // Both width and height 0
        let both_zero = ResizeOptions {
            width: 0.0,
            height: 0.0,
            ..Default::default()
        };
        assert_eq!(
            resize_image_with_options(&plain_png, &both_zero)
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );

        // Negative or NaN dimensions
        for bad_w in [-10.0, f64::NAN, f64::INFINITY] {
            let opts = ResizeOptions {
                width: bad_w,
                height: 0.0,
                ..Default::default()
            };
            assert_eq!(
                resize_image_with_options(&plain_png, &opts)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidOption
            );
        }

        // Out-of-range DPI for cm mode
        for bad_dpi in [36.0, 71.9, 1200.1, f64::NAN] {
            let opts = ResizeOptions {
                unit: ResizeUnit::Cm,
                width: 5.0,
                height: 0.0,
                dpi: bad_dpi,
                ..Default::default()
            };
            assert_eq!(
                resize_image_with_options(&plain_png, &opts)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidOption
            );
        }

        // Out-of-range quality on convert and resize
        for bad_q in [0u8, 9, 101] {
            assert_eq!(
                convert_image(&plain_png, Format::Jpeg, Some(bad_q))
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidOption
            );
            let opts = ResizeOptions {
                quality: bad_q,
                ..Default::default()
            };
            assert_eq!(
                resize_image_with_options(&plain_png, &opts)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidOption
            );
        }

        // Unsupported target format on convert and resize
        assert_eq!(
            convert_image(&plain_png, Format::Pdf, Some(90))
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedFormat
        );
        assert_eq!(
            resize_image_with_format(&plain_png, 100, 0, Some(Format::Heic))
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedFormat
        );

        // Compress target_kb == 0 rejected as InvalidOption
        assert_eq!(
            compress_image(&plain_png, 0).unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // Empty and truncated images rejected cleanly
        assert_eq!(decode_image(&[]).unwrap_err().code, ErrorCode::EmptyFile);
        let truncated = fixture_bytes("truncated.jpg");
        assert_eq!(
            decode_image(&truncated).unwrap_err().code,
            ErrorCode::DamagedFile
        );
    }

    #[test]
    fn cancellation_aborts_decode_convert_compress_and_resize() {
        let plain_png = fixture_bytes("plain.png");
        let token = CancelToken::new();
        token.cancel();

        assert_eq!(
            decode_image_cancellable(&plain_png, &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert_eq!(
            convert_image_cancellable(&plain_png, Format::Jpeg, Some(90), &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert_eq!(
            compress_image_cancellable(&plain_png, 10, &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert_eq!(
            resize_image_with_options_cancellable(&plain_png, &ResizeOptions::default(), &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
    }

    #[test]
    fn compress_image_cancellable_stops_mid_loop_when_canceled() {
        // Generate a noisy high-entropy 320x240 image so compression runs multiple binary-search steps.
        let mut img = RgbImage::new(320, 240);
        let mut state: u32 = 0x1234_5678;
        for px in img.pixels_mut() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let r = (state >> 24) as u8;
            let g = (state >> 16) as u8;
            let b = (state >> 8) as u8;
            *px = image::Rgb([r, g, b]);
        }
        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let token = CancelToken::new();
        let mut iterations = 0usize;
        let err = compress_image_cancellable_with_hook(&png_bytes, 1, &token, || {
            iterations += 1;
            if iterations == 2 {
                token.cancel();
            }
        })
        .unwrap_err();

        assert_eq!(err.code, ErrorCode::Canceled);
        assert_eq!(
            iterations, 2,
            "compression loop must abort immediately after cancellation checkpoint"
        );
    }
}
