use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use refract_core::tools::{ManifestOptionSpec, OptionDefault, ToolId};
use refract_engine::{audio, image, render};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a Desktop job writes its output file(s), captured immutably at enqueue time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputDestination {
    /// Save outputs alongside the input file's parent directory.
    NextToSource,
    /// Save outputs into a specific directory chosen by the user.
    Directory(PathBuf),
}

impl OutputDestination {
    /// Parse an output destination from current UI state without silently substituting CWD
    /// when custom folder mode is active with an empty path.
    pub fn from_ui(next_to_source: bool, folder: &str) -> Result<Self, RefractError> {
        if next_to_source {
            return Ok(Self::NextToSource);
        }
        let trimmed = folder.trim();
        if trimmed.is_empty() {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                "Choose an output folder or enable 'Save next to source file'.",
            ));
        }
        Ok(Self::Directory(PathBuf::from(trimmed)))
    }

    /// Validate that the destination is well-formed.
    pub fn validate(&self) -> Result<(), RefractError> {
        match self {
            Self::NextToSource => Ok(()),
            Self::Directory(path) => {
                if path.as_os_str().is_empty() {
                    Err(RefractError::new(
                        ErrorCode::InvalidOption,
                        "Output directory path cannot be empty.",
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Resolve the parent directory for a given input path.
    pub fn resolve_parent<'a>(&'a self, input_path: &'a Path) -> &'a Path {
        match self {
            Self::NextToSource => input_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
            Self::Directory(dir) => dir.as_path(),
        }
    }
}

/// Typed end-boundary semantics for `trim-audio`, preserving end-of-file (`0` / blank)
/// separately from an explicit positive timestamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrimEnd {
    /// Canonical `0` or blank input: trim through end-of-file.
    EndOfFile,
    /// Explicit end timestamp in seconds (`> 0.0`).
    AtSeconds(f64),
}

impl TrimEnd {
    /// Parse a timestamp string (`""`, `"0"`, `"90"`, `"1:30"`, `"01:30.5"`, `"01:00:00"`).
    pub fn parse(raw: &str) -> Result<Self, RefractError> {
        let secs = audio::parse_timestamp(raw)?;
        if secs == 0.0 {
            Ok(Self::EndOfFile)
        } else {
            Ok(Self::AtSeconds(secs))
        }
    }

    /// Format as an optional timestamp string for `refract_engine::audio::trim_audio*`.
    pub fn to_engine_arg(self) -> Option<String> {
        match self {
            Self::EndOfFile => None,
            Self::AtSeconds(secs) => Some(format!("{:.3}", secs)),
        }
    }
}

/// Strongly typed, validated per-tool options for all 11 Desktop tools.
#[derive(Debug, Clone, PartialEq)]
pub enum DesktopToolOptions {
    ConvertImage {
        target: Format,
        quality: u8,
    },
    CompressImage {
        target_kb: u32,
    },
    ResizeImage {
        unit: image::ResizeUnit,
        width: f64,
        height: f64,
        dpi: f64,
        target_format: Option<Format>,
        quality: u8,
    },
    MergePdf,
    SplitPdf {
        ranges: String,
    },
    ReorderPdf {
        order: String,
    },
    ImagesToPdf {
        dpi: f64,
        quality: u8,
    },
    PdfToImages {
        target: Format,
        dpi: u32,
    },
    CompressPdf,
    ConvertAudio {
        target: Format,
        bitrate_kbps: Option<u32>,
    },
    TrimAudio {
        start_secs: f64,
        end: TrimEnd,
        bitrate_kbps: u32,
    },
}

/// Transient snapshot of raw UI widget values, read only at enqueue time and immediately
/// parsed into a validated [`DesktopToolOptions`].
#[derive(Debug, Clone, Default)]
pub struct UiOptionInput<'a> {
    pub string_1: &'a str,
    pub string_2: &'a str,
    pub int_1: i32,
    pub int_2: i32,
    pub combo_1: &'a str,
    pub combo_2: &'a str,
    /// Optional explicit resize unit override (defaults to manifest `"unit"` when `None`).
    pub resize_unit: Option<&'a str>,
    /// Optional explicit resize DPI override (defaults to manifest `"dpi"` when `None`).
    pub resize_dpi: Option<f64>,
    /// Optional explicit resize output format override (defaults to manifest `"to"` when `None`).
    pub resize_target: Option<&'a str>,
    /// Optional explicit trim bitrate override (defaults to manifest `"bitrate"` when `None`).
    pub trim_bitrate: Option<&'a str>,
}

fn manifest_option(tool: ToolId, key: &str) -> Result<&'static ManifestOptionSpec, RefractError> {
    tool.manifest()
        .options
        .iter()
        .find(|opt| opt.key == key)
        .ok_or_else(|| {
            RefractError::new(
                ErrorCode::EngineFailed,
                format!(
                    "Canonical manifest is missing option '{}' for tool '{}'.",
                    key,
                    tool.id_str()
                ),
            )
        })
}

fn manifest_default_str(tool: ToolId, key: &str) -> Result<&'static str, RefractError> {
    match manifest_option(tool, key)?.default {
        OptionDefault::Text(s) => Ok(s),
        OptionDefault::Number(_) => Err(RefractError::new(
            ErrorCode::EngineFailed,
            format!(
                "Manifest option '{}' for '{}' expected a text default.",
                key,
                tool.id_str()
            ),
        )),
    }
}

fn manifest_default_num(tool: ToolId, key: &str) -> Result<f64, RefractError> {
    match manifest_option(tool, key)?.default {
        OptionDefault::Number(n) => Ok(n),
        OptionDefault::Text(_) => Err(RefractError::new(
            ErrorCode::EngineFailed,
            format!(
                "Manifest option '{}' for '{}' expected a numeric default.",
                key,
                tool.id_str()
            ),
        )),
    }
}

fn validate_manifest_number_bounds(
    tool: ToolId,
    key: &str,
    value: f64,
) -> Result<f64, RefractError> {
    if !value.is_finite() {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!("Invalid value for '{}' on '{}'.", key, tool.label()),
        ));
    }
    let spec = manifest_option(tool, key)?;
    if let Some(min) = spec.min {
        if value < min {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "{} ({}) is below the minimum of {}.",
                    spec.label, value, min
                ),
            ));
        }
    }
    if let Some(max) = spec.max {
        if value > max {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("{} ({}) exceeds the maximum of {}.", spec.label, value, max),
            ));
        }
    }
    Ok(value)
}

/// Strictly parse a format label (`"jpg"`, `"jpeg"`, `"png"`, `"webp"`, `"mp3"`, ...),
/// returning `ErrorCode::InvalidOption` on unrecognized strings (never falling back silently).
pub fn parse_format_strict(raw: &str) -> Result<Format, RefractError> {
    Format::from_label(raw).ok_or_else(|| {
        RefractError::new(
            ErrorCode::InvalidOption,
            format!("Unrecognized format '{}'.", raw.trim()),
        )
    })
}

fn validate_desktop_output_format(tool: ToolId, target: Format) -> Result<Format, RefractError> {
    let allowed = tool.manifest().desktop_output_formats;
    if !allowed.contains(&target) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Output format {} is not supported for {} on Desktop.",
                target.label(),
                tool.label()
            ),
        ));
    }
    Ok(target)
}

fn parse_u32_from_i32(raw: i32, field_label: &str) -> Result<u32, RefractError> {
    u32::try_from(raw).map_err(|_| {
        RefractError::new(
            ErrorCode::InvalidOption,
            format!("{} cannot be negative ({}).", field_label, raw),
        )
    })
}

fn parse_u8_from_i32(raw: i32, field_label: &str) -> Result<u8, RefractError> {
    u8::try_from(raw).map_err(|_| {
        RefractError::new(
            ErrorCode::InvalidOption,
            format!("Invalid {} ({}).", field_label, raw),
        )
    })
}

/// Syntactically validate a `split-pdf` range expression without knowing the PDF's total page
/// count yet. Empty `""` is valid and means "split every page" per the canonical manifest.
fn validate_split_ranges_syntax(raw: &str) -> Result<(), RefractError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    for part in trimmed.split(',') {
        let token = part.trim();
        if token.is_empty() {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Invalid empty entry in page ranges '{}'.", trimmed),
            ));
        }
        if let Some((start_s, end_s)) = token.split_once('-') {
            let start: u32 = start_s.trim().parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid start page in range '{}'.", token),
                )
            })?;
            if start == 0 {
                return Err(RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Page numbers start at 1 (got '{}').", token),
                ));
            }
            let end_trimmed = end_s.trim();
            if !end_trimmed.is_empty() && !end_trimmed.eq_ignore_ascii_case("end") {
                let end: u32 = end_trimmed.parse().map_err(|_| {
                    RefractError::new(
                        ErrorCode::InvalidOption,
                        format!("Invalid end page in range '{}'.", token),
                    )
                })?;
                if end == 0 || end < start {
                    return Err(RefractError::new(
                        ErrorCode::InvalidOption,
                        format!("Invalid page range '{}': end must be >= start.", token),
                    ));
                }
            }
        } else {
            let page: u32 = token.parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid page number '{}' in page ranges.", token),
                )
            })?;
            if page == 0 {
                return Err(RefractError::new(
                    ErrorCode::InvalidOption,
                    "Page numbers start at 1 (got 0).",
                ));
            }
        }
    }
    Ok(())
}

/// Syntactically validate a `reorder-pdf` page order expression before execution.
fn validate_reorder_syntax(raw: &str) -> Result<(), RefractError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Page order cannot be empty (e.g. 3, 1, 2).",
        ));
    }
    let mut seen = HashSet::new();
    for part in trimmed.split(',') {
        let token = part.trim();
        let page: u32 = token.parse().map_err(|_| {
            RefractError::new(
                ErrorCode::InvalidOption,
                format!("Invalid page number '{}' in page order.", token),
            )
        })?;
        if page == 0 {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                "Page numbers start at 1 (got 0).",
            ));
        }
        if !seen.insert(page) {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Duplicate page {} in page order.", page),
            ));
        }
    }
    Ok(())
}

impl DesktopToolOptions {
    /// Return the canonical [`ToolId`] corresponding to this options variant.
    pub fn tool_id(&self) -> ToolId {
        match self {
            Self::ConvertImage { .. } => ToolId::ConvertImage,
            Self::CompressImage { .. } => ToolId::CompressImage,
            Self::ResizeImage { .. } => ToolId::ResizeImage,
            Self::MergePdf => ToolId::MergePdf,
            Self::SplitPdf { .. } => ToolId::SplitPdf,
            Self::ReorderPdf { .. } => ToolId::ReorderPdf,
            Self::ImagesToPdf { .. } => ToolId::ImagesToPdf,
            Self::PdfToImages { .. } => ToolId::PdfToImages,
            Self::CompressPdf => ToolId::CompressPdf,
            Self::ConvertAudio { .. } => ToolId::ConvertAudio,
            Self::TrimAudio { .. } => ToolId::TrimAudio,
        }
    }

    /// Derive default typed options directly from the canonical generated tool manifest.
    pub fn default_for(tool: ToolId) -> Result<Self, RefractError> {
        match tool {
            ToolId::ConvertImage => {
                let target = parse_format_strict(manifest_default_str(tool, "to")?)?;
                let quality = manifest_default_num(tool, "quality")?.round() as u8;
                Ok(Self::ConvertImage { target, quality })
            }
            ToolId::CompressImage => {
                let target_kb = manifest_default_num(tool, "targetKB")?.round() as u32;
                Ok(Self::CompressImage { target_kb })
            }
            ToolId::ResizeImage => {
                let unit = image::ResizeUnit::parse(manifest_default_str(tool, "unit")?)?;
                let width = manifest_default_num(tool, "width")?;
                let height = manifest_default_num(tool, "height")?;
                let dpi = manifest_default_num(tool, "dpi")?;
                let target_format =
                    image::parse_resize_target_format(manifest_default_str(tool, "to")?)?;
                Ok(Self::ResizeImage {
                    unit,
                    width,
                    height,
                    dpi,
                    target_format,
                    // `resize-image` has no quality option in `specs/tool-manifest.json`;
                    // Web and Desktop both encode resized JPEGs at 90% (`DEFAULT_IMAGE_QUALITY`).
                    quality: image::DEFAULT_IMAGE_QUALITY,
                })
            }
            ToolId::MergePdf => Ok(Self::MergePdf),
            ToolId::SplitPdf => {
                let ranges = manifest_default_str(tool, "ranges")?.to_string();
                Ok(Self::SplitPdf { ranges })
            }
            ToolId::ReorderPdf => {
                let order = manifest_default_str(tool, "order")?.to_string();
                Ok(Self::ReorderPdf { order })
            }
            ToolId::ImagesToPdf => {
                let dpi = manifest_default_num(tool, "dpi")?;
                let quality = manifest_default_num(tool, "quality")?.round() as u8;
                Ok(Self::ImagesToPdf { dpi, quality })
            }
            ToolId::PdfToImages => {
                let target = parse_format_strict(manifest_default_str(tool, "to")?)?;
                let dpi = manifest_default_num(tool, "dpi")?.round() as u32;
                Ok(Self::PdfToImages { target, dpi })
            }
            ToolId::CompressPdf => Ok(Self::CompressPdf),
            ToolId::ConvertAudio => {
                let target = parse_format_strict(manifest_default_str(tool, "to")?)?;
                let bitrate_kbps = if target == Format::Wav {
                    None
                } else {
                    Some(audio::parse_bitrate_str(manifest_default_str(
                        tool, "bitrate",
                    )?)?)
                };
                Ok(Self::ConvertAudio {
                    target,
                    bitrate_kbps,
                })
            }
            ToolId::TrimAudio => {
                let start_secs = manifest_default_num(tool, "start")?;
                let end_num = manifest_default_num(tool, "end")?;
                let end = if end_num == 0.0 {
                    TrimEnd::EndOfFile
                } else {
                    TrimEnd::AtSeconds(end_num)
                };
                let bitrate_kbps =
                    audio::parse_bitrate_str(manifest_default_str(tool, "bitrate")?)?;
                Ok(Self::TrimAudio {
                    start_secs,
                    end,
                    bitrate_kbps,
                })
            }
        }
    }

    /// Parse raw UI widget state for `tool` into a validated [`DesktopToolOptions`].
    /// Returns `Err(RefractError)` on invalid values instead of silently clamping or substituting.
    pub fn from_ui(tool: ToolId, raw: &UiOptionInput<'_>) -> Result<Self, RefractError> {
        let opts = match tool {
            ToolId::ConvertImage => {
                let target = parse_format_strict(raw.combo_1)?;
                let quality = if target == Format::Jpeg {
                    parse_u8_from_i32(raw.int_1, "Quality")?
                } else {
                    manifest_default_num(tool, "quality")?.round() as u8
                };
                Self::ConvertImage { target, quality }
            }
            ToolId::CompressImage => {
                let target_kb = parse_u32_from_i32(raw.int_1, "Target size (KB)")?;
                Self::CompressImage { target_kb }
            }
            ToolId::ResizeImage => {
                let unit = match raw.resize_unit {
                    Some(u) => image::ResizeUnit::parse(u)?,
                    None => image::ResizeUnit::parse(manifest_default_str(tool, "unit")?)?,
                };
                let width = f64::from(parse_u32_from_i32(raw.int_1, "Width")?);
                let height = f64::from(parse_u32_from_i32(raw.int_2, "Height")?);
                let dpi = match raw.resize_dpi {
                    Some(d) => d,
                    None => manifest_default_num(tool, "dpi")?,
                };
                let target_format = match raw.resize_target {
                    Some(t) => image::parse_resize_target_format(t)?,
                    None => image::parse_resize_target_format(manifest_default_str(tool, "to")?)?,
                };
                Self::ResizeImage {
                    unit,
                    width,
                    height,
                    dpi,
                    target_format,
                    quality: image::DEFAULT_IMAGE_QUALITY,
                }
            }
            ToolId::MergePdf => Self::MergePdf,
            ToolId::SplitPdf => Self::SplitPdf {
                ranges: raw.string_1.trim().to_string(),
            },
            ToolId::ReorderPdf => Self::ReorderPdf {
                order: raw.string_1.trim().to_string(),
            },
            ToolId::ImagesToPdf => {
                let dpi = f64::from(parse_u32_from_i32(raw.int_2, "Resolution (DPI)")?);
                let quality = parse_u8_from_i32(raw.int_1, "JPEG quality")?;
                Self::ImagesToPdf { dpi, quality }
            }
            ToolId::PdfToImages => {
                let target = parse_format_strict(raw.combo_1)?;
                let dpi = parse_u32_from_i32(raw.int_1, "Resolution (DPI)")?;
                Self::PdfToImages { target, dpi }
            }
            ToolId::CompressPdf => Self::CompressPdf,
            ToolId::ConvertAudio => {
                let target = parse_format_strict(raw.combo_1)?;
                let bitrate_kbps = if target == Format::Wav {
                    None
                } else {
                    Some(audio::parse_bitrate_str(raw.combo_2)?)
                };
                Self::ConvertAudio {
                    target,
                    bitrate_kbps,
                }
            }
            ToolId::TrimAudio => {
                let start_secs = audio::parse_timestamp(raw.string_1)?;
                let end = TrimEnd::parse(raw.string_2)?;
                let bitrate_str = match raw.trim_bitrate {
                    Some(s) => s,
                    None => manifest_default_str(tool, "bitrate")?,
                };
                let bitrate_kbps = audio::parse_bitrate_str(bitrate_str)?;
                Self::TrimAudio {
                    start_secs,
                    end,
                    bitrate_kbps,
                }
            }
        };

        opts.validate()?;
        Ok(opts)
    }

    /// Validate all fields strictly without clamping or substituting defaults.
    pub fn validate(&self) -> Result<(), RefractError> {
        match self {
            Self::ConvertImage { target, quality } => {
                validate_desktop_output_format(ToolId::ConvertImage, *target)?;
                validate_manifest_number_bounds(
                    ToolId::ConvertImage,
                    "quality",
                    f64::from(*quality),
                )?;
                image::validate_image_quality(*quality)?;
                Ok(())
            }
            Self::CompressImage { target_kb } => {
                validate_manifest_number_bounds(
                    ToolId::CompressImage,
                    "targetKB",
                    f64::from(*target_kb),
                )?;
                Ok(())
            }
            Self::ResizeImage {
                unit,
                width,
                height,
                dpi,
                target_format,
                quality,
            } => {
                validate_manifest_number_bounds(ToolId::ResizeImage, "width", *width)?;
                validate_manifest_number_bounds(ToolId::ResizeImage, "height", *height)?;
                if *width == 0.0 && *height == 0.0 {
                    return Err(RefractError::new(
                        ErrorCode::InvalidOption,
                        "Width or height must be greater than 0.",
                    ));
                }
                if *unit == image::ResizeUnit::Cm {
                    validate_manifest_number_bounds(ToolId::ResizeImage, "dpi", *dpi)?;
                    image::validate_resize_dpi(*dpi)?;
                    if *width > 0.0 {
                        image::cm_to_px(*width, *dpi)?;
                    }
                    if *height > 0.0 {
                        image::cm_to_px(*height, *dpi)?;
                    }
                } else if !dpi.is_finite() {
                    return Err(RefractError::new(
                        ErrorCode::InvalidOption,
                        "Resize DPI must be a finite number.",
                    ));
                }
                if let Some(fmt) = target_format {
                    validate_desktop_output_format(ToolId::ResizeImage, *fmt)?;
                }
                image::validate_image_quality(*quality)?;
                Ok(())
            }
            Self::MergePdf | Self::CompressPdf => Ok(()),
            Self::SplitPdf { ranges } => validate_split_ranges_syntax(ranges),
            Self::ReorderPdf { order } => validate_reorder_syntax(order),
            Self::ImagesToPdf { dpi, quality } => {
                validate_manifest_number_bounds(ToolId::ImagesToPdf, "dpi", *dpi)?;
                validate_manifest_number_bounds(
                    ToolId::ImagesToPdf,
                    "quality",
                    f64::from(*quality),
                )?;
                image::validate_image_quality(*quality)?;
                Ok(())
            }
            Self::PdfToImages { target, dpi } => {
                validate_desktop_output_format(ToolId::PdfToImages, *target)?;
                // Native Desktop rendering supports 36..=600 DPI (`render::validate_render_dpi`
                // and `app.slint` SpinBox up to 600), whereas the shared manifest entry lists 300
                // as the Web-safe ceiling.
                render::validate_render_dpi(*dpi)?;
                Ok(())
            }
            Self::ConvertAudio {
                target,
                bitrate_kbps,
            } => {
                validate_desktop_output_format(ToolId::ConvertAudio, *target)?;
                match (*target, *bitrate_kbps) {
                    (Format::Wav, None) => Ok(()),
                    (Format::Wav, Some(br)) => {
                        audio::validate_bitrate_kbps(br)?;
                        Ok(())
                    }
                    (_, Some(br)) => {
                        audio::validate_bitrate_kbps(br)?;
                        Ok(())
                    }
                    (_, None) => Err(RefractError::new(
                        ErrorCode::InvalidOption,
                        format!(
                            "Bitrate is required when converting audio to {}.",
                            target.label()
                        ),
                    )),
                }
            }
            Self::TrimAudio {
                start_secs,
                end,
                bitrate_kbps,
            } => {
                validate_manifest_number_bounds(ToolId::TrimAudio, "start", *start_secs)?;
                if let TrimEnd::AtSeconds(end_secs) = *end {
                    validate_manifest_number_bounds(ToolId::TrimAudio, "end", end_secs)?;
                    if end_secs <= *start_secs {
                        return Err(RefractError::new(
                            ErrorCode::InvalidOption,
                            format!(
                                "End time ({:.3}s) must be greater than start time ({:.3}s).",
                                end_secs, start_secs
                            ),
                        ));
                    }
                }
                audio::validate_bitrate_kbps(*bitrate_kbps)?;
                Ok(())
            }
        }
    }

    /// Convert `ResizeImage` options directly into `refract_engine::image::ResizeOptions`.
    pub fn as_engine_resize_options(&self) -> Option<image::ResizeOptions> {
        match *self {
            Self::ResizeImage {
                unit,
                width,
                height,
                dpi,
                target_format,
                quality,
            } => Some(image::ResizeOptions {
                unit,
                width,
                height,
                dpi,
                target_format,
                quality,
            }),
            _ => None,
        }
    }
}

/// Immutable, validated snapshot of a Desktop job's configuration captured at enqueue time.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRequest {
    tool: ToolId,
    options: DesktopToolOptions,
    destination: OutputDestination,
}

impl JobRequest {
    /// Construct a validated, immutable [`JobRequest`].
    pub fn new(
        tool: ToolId,
        options: DesktopToolOptions,
        destination: OutputDestination,
    ) -> Result<Self, RefractError> {
        if options.tool_id() != tool {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Options variant for '{}' does not match tool '{}'.",
                    options.tool_id().id_str(),
                    tool.id_str()
                ),
            ));
        }
        options.validate()?;
        destination.validate()?;
        Ok(Self {
            tool,
            options,
            destination,
        })
    }

    /// Capture and validate a [`JobRequest`] from current UI state at enqueue time.
    pub fn from_ui(
        tool_id: &str,
        raw_options: &UiOptionInput<'_>,
        output_next_to_source: bool,
        output_folder: &str,
    ) -> Result<Self, RefractError> {
        let tool = ToolId::from_id_str(tool_id).ok_or_else(|| {
            RefractError::new(
                ErrorCode::UnsupportedFormat,
                format!("Unknown tool: {}", tool_id),
            )
        })?;
        let options = DesktopToolOptions::from_ui(tool, raw_options)?;
        let destination = OutputDestination::from_ui(output_next_to_source, output_folder)?;
        Self::new(tool, options, destination)
    }

    pub fn tool(&self) -> ToolId {
        self.tool
    }

    pub fn options(&self) -> &DesktopToolOptions {
        &self.options
    }

    pub fn destination(&self) -> &OutputDestination {
        &self.destination
    }

    /// True if two requests can safely execute together in a single batch (`merge-pdf` or
    /// `images-to-pdf`) without discarding or mutating either job's options or destination.
    pub fn is_batch_compatible_with(&self, other: &Self) -> bool {
        self.tool == other.tool
            && self.tool.spec().batch
            && self.options == other.options
            && self.destination == other.destination
    }
}

/// An execution unit planned from queued jobs: either a single-file job or a batch of
/// request-compatible jobs (`merge-pdf`, `images-to-pdf`) in deterministic queue order.
#[derive(Debug, Clone, PartialEq)]
pub enum PlannedUnit<T> {
    Single(T),
    Batch {
        tool: ToolId,
        request: JobRequest,
        items: Vec<T>,
    },
}

/// Plan execution units from waiting jobs while preserving deterministic queue order and never
/// combining batch jobs whose [`JobRequest`] options or destinations are incompatible.
pub fn plan_execution_units<T, F>(jobs: Vec<T>, request_of: F) -> Vec<PlannedUnit<T>>
where
    F: Fn(&T) -> &JobRequest,
{
    let mut units: Vec<PlannedUnit<T>> = Vec::new();

    for job in jobs {
        let req = request_of(&job).clone();
        if !req.tool().spec().batch {
            units.push(PlannedUnit::Single(job));
            continue;
        }

        if let Some(PlannedUnit::Batch { items, .. }) = units.iter_mut().find(|unit| {
            matches!(
                unit,
                PlannedUnit::Batch { request, .. } if request.is_batch_compatible_with(&req)
            )
        }) {
            items.push(job);
        } else {
            units.push(PlannedUnit::Batch {
                tool: req.tool(),
                request: req,
                items: vec![job],
            });
        }
    }

    units
}

/// Resolve a batch strictly from its recorded member IDs in their exact recorded order.
///
/// Fails explicitly if `recorded_member_ids` is empty, does not contain `target_id`,
/// contains duplicate IDs, references a missing/deleted member, or contains a member with an
/// incompatible [`JobRequest`] or ineligible state.
pub fn resolve_recorded_batch_indices<T, FId, FReq, FEligible>(
    all_jobs: &[T],
    target_id: i32,
    recorded_member_ids: &[i32],
    id_of: FId,
    request_of: FReq,
    is_eligible: FEligible,
) -> Result<Vec<usize>, RefractError>
where
    FId: Fn(&T) -> i32,
    FReq: Fn(&T) -> &JobRequest,
    FEligible: Fn(&T) -> bool,
{
    let target_idx = all_jobs
        .iter()
        .position(|j| id_of(j) == target_id)
        .ok_or_else(|| {
            RefractError::new(
                ErrorCode::InvalidOption,
                format!("Job {} was not found in the queue.", target_id),
            )
        })?;
    let target_req = request_of(&all_jobs[target_idx]);

    if !target_req.tool().spec().batch {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!("Tool '{}' is not a batch tool.", target_req.tool().id_str()),
        ));
    }

    if recorded_member_ids.is_empty() {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Batch cannot run because its original member list was not recorded.",
        ));
    }

    if !recorded_member_ids.contains(&target_id) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!("Job {} does not belong to its recorded batch.", target_id),
        ));
    }

    let mut seen = HashSet::with_capacity(recorded_member_ids.len());
    let mut indices = Vec::with_capacity(recorded_member_ids.len());

    // Iterate in the exact order recorded when the batch originally ran, never current UI order.
    for &member_id in recorded_member_ids {
        if !seen.insert(member_id) {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Recorded batch contains duplicate job {}.", member_id),
            ));
        }
        let idx = all_jobs
            .iter()
            .position(|j| id_of(j) == member_id)
            .ok_or_else(|| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!(
                        "Cannot run batch: original member job {} is missing or was removed.",
                        member_id
                    ),
                )
            })?;
        if !request_of(&all_jobs[idx]).is_batch_compatible_with(target_req) {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Cannot run batch: member job {} has an incompatible request.",
                    member_id
                ),
            ));
        }
        if idx != target_idx && !is_eligible(&all_jobs[idx]) {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Cannot run batch: member job {} is not in a retryable state.",
                    member_id
                ),
            ));
        }
        indices.push(idx);
    }

    Ok(indices)
}

/// Select the deterministic ordered indices of jobs to run when retrying a job.
///
/// For batch tools (`merge-pdf`, `images-to-pdf`):
/// - When `recorded_member_ids` is non-empty, resolves members in the **exact recorded order**
///   via [`resolve_recorded_batch_indices`], failing explicitly on missing, incompatible,
///   ineligible, or mismatched members instead of silently substituting UI order or a subset.
/// - When `recorded_member_ids` is empty (batch has never run yet), falls back to compatible
///   eligible peers in queue order.
pub fn select_batch_retry_indices<T, FId, FReq, FEligible>(
    all_jobs: &[T],
    target_id: i32,
    recorded_member_ids: &[i32],
    id_of: FId,
    request_of: FReq,
    is_eligible: FEligible,
) -> Result<Vec<usize>, RefractError>
where
    FId: Fn(&T) -> i32,
    FReq: Fn(&T) -> &JobRequest,
    FEligible: Fn(&T) -> bool,
{
    let target_idx = all_jobs
        .iter()
        .position(|j| id_of(j) == target_id)
        .ok_or_else(|| {
            RefractError::new(
                ErrorCode::InvalidOption,
                format!("Job {} was not found in the queue.", target_id),
            )
        })?;
    let target_req = request_of(&all_jobs[target_idx]);

    if !target_req.tool().spec().batch {
        return Ok(vec![target_idx]);
    }

    if !recorded_member_ids.is_empty() {
        return resolve_recorded_batch_indices(
            all_jobs,
            target_id,
            recorded_member_ids,
            id_of,
            request_of,
            is_eligible,
        );
    }

    Ok(all_jobs
        .iter()
        .enumerate()
        .filter(|(idx, job)| {
            (*idx == target_idx || is_eligible(job))
                && request_of(job).is_batch_compatible_with(target_req)
        })
        .map(|(idx, _)| idx)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eleven_tools_have_manifest_derived_defaults() {
        assert_eq!(ToolId::ALL.len(), 11);
        for &tool in ToolId::ALL {
            let defaults = DesktopToolOptions::default_for(tool)
                .unwrap_or_else(|e| panic!("default_for({:?}) failed: {}", tool, e));
            assert_eq!(defaults.tool_id(), tool);
            // Every tool except ReorderPdf (whose canonical manifest default is "") validates directly.
            if tool == ToolId::ReorderPdf {
                assert_eq!(
                    defaults,
                    DesktopToolOptions::ReorderPdf {
                        order: String::new()
                    }
                );
                assert_eq!(
                    defaults.validate().unwrap_err().code,
                    ErrorCode::InvalidOption
                );
            } else {
                defaults
                    .validate()
                    .unwrap_or_else(|e| panic!("default for {:?} should validate: {}", tool, e));
            }
        }

        // Spot-check exact alignment with specs/tool-manifest.json
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::ConvertImage).unwrap(),
            DesktopToolOptions::ConvertImage {
                target: Format::Jpeg,
                quality: 90,
            }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::CompressImage).unwrap(),
            DesktopToolOptions::CompressImage { target_kb: 200 }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::ResizeImage).unwrap(),
            DesktopToolOptions::ResizeImage {
                unit: image::ResizeUnit::Px,
                width: 1200.0,
                height: 0.0,
                dpi: 300.0,
                target_format: None,
                quality: 90,
            }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::SplitPdf).unwrap(),
            DesktopToolOptions::SplitPdf {
                ranges: String::new(),
            }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::ImagesToPdf).unwrap(),
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 85,
            }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::PdfToImages).unwrap(),
            DesktopToolOptions::PdfToImages {
                target: Format::Png,
                dpi: 150,
            }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::ConvertAudio).unwrap(),
            DesktopToolOptions::ConvertAudio {
                target: Format::Mp3,
                bitrate_kbps: Some(192),
            }
        );
        assert_eq!(
            DesktopToolOptions::default_for(ToolId::TrimAudio).unwrap(),
            DesktopToolOptions::TrimAudio {
                start_secs: 0.0,
                end: TrimEnd::EndOfFile,
                bitrate_kbps: 192,
            }
        );
    }

    #[test]
    fn invalid_options_and_mismatched_requests_are_rejected_strictly() {
        // OutputDestination: custom folder mode with empty path rejected
        assert_eq!(
            OutputDestination::from_ui(false, "   ").unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // Unknown format rejected (no silent JPEG fallback)
        assert_eq!(
            parse_format_strict("bmp").unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // ConvertImage: invalid quality and unsupported target
        assert_eq!(
            DesktopToolOptions::ConvertImage {
                target: Format::Jpeg,
                quality: 5,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::ConvertImage {
                target: Format::Heic,
                quality: 90,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // CompressImage: out of manifest [5, 20000] bounds
        assert_eq!(
            DesktopToolOptions::CompressImage { target_kb: 4 }
                .validate()
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::CompressImage { target_kb: 20_001 }
                .validate()
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );

        // ResizeImage: both 0, negative from UI, out-of-range cm DPI
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::ResizeImage,
                &UiOptionInput {
                    int_1: 0,
                    int_2: 0,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::ResizeImage,
                &UiOptionInput {
                    int_1: -50,
                    int_2: 100,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::ResizeImage,
                &UiOptionInput {
                    int_1: 10,
                    int_2: 0,
                    resize_unit: Some("cm"),
                    resize_dpi: Some(36.0),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // SplitPdf: invalid syntax rejected; empty string accepted (every page)
        assert!(DesktopToolOptions::SplitPdf {
            ranges: String::new(),
        }
        .validate()
        .is_ok());
        for bad_range in ["0", "3-1", "1-2, ", "abc"] {
            assert_eq!(
                DesktopToolOptions::SplitPdf {
                    ranges: bad_range.to_string(),
                }
                .validate()
                .unwrap_err()
                .code,
                ErrorCode::InvalidOption,
                "expected InvalidOption for split range '{}'",
                bad_range
            );
        }

        // ReorderPdf: empty, zero, duplicate rejected
        for bad_order in ["", "   ", "0, 1", "1, 1, 2", "1, a"] {
            assert_eq!(
                DesktopToolOptions::ReorderPdf {
                    order: bad_order.to_string(),
                }
                .validate()
                .unwrap_err()
                .code,
                ErrorCode::InvalidOption
            );
        }

        // ImagesToPdf: DPI & quality bounds
        assert_eq!(
            DesktopToolOptions::ImagesToPdf {
                dpi: 50.0,
                quality: 85,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 101,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // PdfToImages: invalid format or DPI
        assert_eq!(
            DesktopToolOptions::PdfToImages {
                target: Format::Webp,
                dpi: 150,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::PdfToImages {
                target: Format::Png,
                dpi: 20,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // ConvertAudio: invalid bitrate, missing bitrate for lossy, WAV omits bitrate
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::ConvertAudio,
                &UiOptionInput {
                    combo_1: "mp3",
                    combo_2: "999",
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::ConvertAudio {
                target: Format::Mp3,
                bitrate_kbps: None,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::ConvertAudio,
                &UiOptionInput {
                    combo_1: "wav",
                    combo_2: "192",
                    ..Default::default()
                }
            )
            .unwrap(),
            DesktopToolOptions::ConvertAudio {
                target: Format::Wav,
                bitrate_kbps: None,
            }
        );

        // TrimAudio: end <= start rejected; end="0" maps to EndOfFile
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::TrimAudio,
                &UiOptionInput {
                    string_1: "10",
                    string_2: "5",
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            DesktopToolOptions::from_ui(
                ToolId::TrimAudio,
                &UiOptionInput {
                    string_1: "1:30",
                    string_2: "0",
                    ..Default::default()
                }
            )
            .unwrap(),
            DesktopToolOptions::TrimAudio {
                start_secs: 90.0,
                end: TrimEnd::EndOfFile,
                bitrate_kbps: 192,
            }
        );

        // JobRequest rejects mismatched tool + options variant
        assert_eq!(
            JobRequest::new(
                ToolId::MergePdf,
                DesktopToolOptions::CompressPdf,
                OutputDestination::NextToSource,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
    }

    #[test]
    fn batch_planner_separates_incompatible_requests_and_preserves_deterministic_order() {
        #[derive(Clone, Debug, PartialEq)]
        struct StubJob {
            id: i32,
            req: JobRequest,
        }

        let img_150_src = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 85,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let img_300_src = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 300.0,
                quality: 85,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let img_150_dir = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 85,
            },
            OutputDestination::Directory(PathBuf::from("/out")),
        )
        .unwrap();
        let merge_src = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        let merge_dir = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(PathBuf::from("/merged")),
        )
        .unwrap();

        let jobs = vec![
            StubJob {
                id: 1,
                req: img_150_src.clone(),
            },
            StubJob {
                id: 2,
                req: img_300_src.clone(),
            },
            StubJob {
                id: 3,
                req: img_150_src.clone(),
            },
            StubJob {
                id: 4,
                req: img_150_dir.clone(),
            },
            StubJob {
                id: 5,
                req: merge_src.clone(),
            },
            StubJob {
                id: 6,
                req: merge_dir.clone(),
            },
            StubJob {
                id: 7,
                req: merge_src.clone(),
            },
        ];

        let planned = plan_execution_units(jobs, |j| &j.req);
        assert_eq!(planned.len(), 5);

        match &planned[0] {
            PlannedUnit::Batch {
                tool,
                request,
                items,
            } => {
                assert_eq!(*tool, ToolId::ImagesToPdf);
                assert_eq!(*request, img_150_src);
                assert_eq!(items.iter().map(|j| j.id).collect::<Vec<_>>(), vec![1, 3]);
            }
            other => panic!("expected batch, got {:?}", other),
        }
        match &planned[1] {
            PlannedUnit::Batch { request, items, .. } => {
                assert_eq!(*request, img_300_src);
                assert_eq!(items.iter().map(|j| j.id).collect::<Vec<_>>(), vec![2]);
            }
            other => panic!("expected batch, got {:?}", other),
        }
        match &planned[2] {
            PlannedUnit::Batch { request, items, .. } => {
                assert_eq!(*request, img_150_dir);
                assert_eq!(items.iter().map(|j| j.id).collect::<Vec<_>>(), vec![4]);
            }
            other => panic!("expected batch, got {:?}", other),
        }
        match &planned[3] {
            PlannedUnit::Batch {
                tool,
                request,
                items,
            } => {
                assert_eq!(*tool, ToolId::MergePdf);
                assert_eq!(*request, merge_src);
                assert_eq!(items.iter().map(|j| j.id).collect::<Vec<_>>(), vec![5, 7]);
            }
            other => panic!("expected batch, got {:?}", other),
        }
        match &planned[4] {
            PlannedUnit::Batch { request, items, .. } => {
                assert_eq!(*request, merge_dir);
                assert_eq!(items.iter().map(|j| j.id).collect::<Vec<_>>(), vec![6]);
            }
            other => panic!("expected batch, got {:?}", other),
        }
    }

    #[test]
    fn recorded_batch_resolution_preserves_original_order_and_rejects_invalid_states() {
        #[derive(Clone, Debug)]
        struct StubJob {
            id: i32,
            req: JobRequest,
            eligible: bool,
        }

        let merge_a = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(PathBuf::from("/out-a")),
        )
        .unwrap();
        let merge_b = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(PathBuf::from("/out-b")),
        )
        .unwrap();

        // Store has been reordered in UI to [3, 1, 2], while recorded order was [1, 2, 3].
        let reordered = vec![
            StubJob {
                id: 3,
                req: merge_a.clone(),
                eligible: true,
            },
            StubJob {
                id: 1,
                req: merge_a.clone(),
                eligible: true,
            },
            StubJob {
                id: 2,
                req: merge_a.clone(),
                eligible: true,
            },
        ];

        let indices = select_batch_retry_indices(
            &reordered,
            2,
            &[1, 2, 3],
            |j| j.id,
            |j| &j.req,
            |j| j.eligible,
        )
        .unwrap();
        let resolved_ids: Vec<i32> = indices.iter().map(|&i| reordered[i].id).collect();
        assert_eq!(resolved_ids, vec![1, 2, 3]);

        // Target not in recorded batch is rejected
        assert_eq!(
            select_batch_retry_indices(
                &reordered,
                3,
                &[1, 2],
                |j| j.id,
                |j| &j.req,
                |j| j.eligible,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // Missing member ID is rejected explicitly (never silently dropped)
        assert_eq!(
            select_batch_retry_indices(
                &reordered,
                1,
                &[1, 2, 99],
                |j| j.id,
                |j| &j.req,
                |j| j.eligible,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // Incompatible member request is rejected explicitly
        let mut with_incompat = reordered.clone();
        with_incompat[2].req = merge_b;
        assert_eq!(
            select_batch_retry_indices(
                &with_incompat,
                1,
                &[1, 2, 3],
                |j| j.id,
                |j| &j.req,
                |j| j.eligible,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );

        // Ineligible peer member is rejected explicitly
        let mut with_ineligible = reordered;
        with_ineligible[0].eligible = false; // job 3
        assert_eq!(
            select_batch_retry_indices(
                &with_ineligible,
                1,
                &[1, 2, 3],
                |j| j.id,
                |j| &j.req,
                |j| j.eligible,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidOption
        );
    }
}
