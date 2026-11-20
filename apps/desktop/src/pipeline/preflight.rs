use crate::pipeline::{DesktopToolOptions, JobRequest};
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use refract_core::limits::{
    check_limits, check_memory, limits_for_tool, memory_budget_bytes, LimitCheckInput,
    PlatformLimits,
};
use refract_core::tools::{Platform, ToolId};
use refract_engine::{audio, image, native, pdf};
use std::io::Read;
use std::path::Path;

const MIB: u64 = 1024 * 1024;
/// `refract_core::detect` inspects at most 1,024 bytes (for `%PDF-`).
const HEADER_SNIFF_BYTES: usize = 1024;

/// Basic filesystem metadata obtained without reading file contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub size_bytes: u64,
    pub is_file: bool,
}

/// Result of preflight validation for a single input file.
#[derive(Debug, Clone)]
pub struct ValidatedInput {
    pub format: Format,
    /// Already-read PDF bytes populated only after metadata, size, magic-byte, and byte-memory
    /// checks pass, avoiding redundant second disk reads.
    pub pdf_bytes: Option<Vec<u8>>,
    /// Page count measured during preflight (PDF inputs only), reused by output verification.
    pub page_count: Option<u32>,
    /// Which password opened this PDF; the engine must be given the same one.
    pub unlocked_with: PasswordSource,
}

/// Which password opened a PDF input during preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasswordSource {
    /// Not locked (unencrypted or owner-only); no password is passed to the engine.
    #[default]
    None,
    /// The password entered for this input (or the job's single password).
    Own,
    /// `guesses[n]`: another password entered for the batch, tried because this input had none.
    Guess(usize),
}

/// Passwords available to preflight for PDF inputs. Holds borrowed plaintext, so it has no `Debug`.
#[derive(Clone, Copy, Default)]
pub struct PdfPasswords<'a> {
    /// Password for every input (single-file jobs and legacy callers).
    pub all: Option<&'a str>,
    /// Password entered for a specific input, by input index (merge-pdf).
    pub per_input: &'a [Option<&'a str>],
    /// Other passwords entered for the batch, newest first. They are tried, in order, only on
    /// locked inputs that have none of their own, so files sharing a password unlock together.
    /// If none works, that input needs its own password (reported as `NeedsPassword`, not wrong).
    pub guesses: &'a [&'a str],
}

impl<'a> PdfPasswords<'a> {
    pub fn own(&self, index: usize) -> Option<&'a str> {
        self.per_input.get(index).copied().flatten().or(self.all)
    }

    /// The password string the engine should use for an input preflight opened with `source`.
    pub fn for_source(&self, index: usize, source: PasswordSource) -> Option<&'a str> {
        match source {
            PasswordSource::None => None,
            PasswordSource::Own => self.own(index),
            PasswordSource::Guess(n) => self.guesses.get(n).copied(),
        }
    }
}

fn locked_error(code: ErrorCode, index: usize, path: &Path) -> RefractError {
    let name = display_name(path);
    let message = match code {
        ErrorCode::WrongPassword => {
            format!(
                "That password did not unlock '{}'. Check it and try again.",
                name
            )
        }
        _ => format!("'{}' is protected. Enter its password to continue.", name),
    };
    RefractError::with_details(code, message, pdf::input_index_details(index))
}

/// Open PDF input `index` with the right password, returning its info and which password worked.
/// Errors name the file and carry its index (see [`pdf::input_index_of`]).
fn unlock_pdf_input<P: PreflightProbe>(
    probe: &P,
    bytes: &[u8],
    index: usize,
    path: &Path,
    passwords: &PdfPasswords<'_>,
) -> Result<(pdf::PdfInfo, PasswordSource), RefractError> {
    let password_error = |e: RefractError| match e.code {
        ErrorCode::NeedsPassword | ErrorCode::WrongPassword => locked_error(e.code, index, path),
        _ => e,
    };

    if let Some(own) = passwords.own(index) {
        let info = probe.probe_pdf(bytes, Some(own)).map_err(password_error)?;
        if info.requires_password {
            return Err(locked_error(ErrorCode::NeedsPassword, index, path));
        }
        return Ok((info, PasswordSource::Own));
    }

    let info = probe.probe_pdf(bytes, None).map_err(password_error)?;
    if !info.requires_password {
        return Ok((info, PasswordSource::None));
    }
    for (n, guess) in passwords.guesses.iter().enumerate() {
        match probe.probe_pdf(bytes, Some(guess)) {
            Ok(info) if !info.requires_password => return Ok((info, PasswordSource::Guess(n))),
            Ok(_) => {}
            Err(e) if e.code == ErrorCode::WrongPassword => {}
            Err(e) => return Err(password_error(e)),
        }
    }
    Err(locked_error(ErrorCode::NeedsPassword, index, path))
}

/// Result of preflight validation for a single job or batch execution unit.
#[derive(Debug, Clone)]
pub struct PreflightReport {
    pub inputs: Vec<ValidatedInput>,
}

/// Injectable probe abstraction so unit tests can deterministically exercise exact multi-GB,
/// gigapixel, and 20,000-page boundaries and assert zero full-file reads on rejected inputs.
pub trait PreflightProbe {
    fn stat(&self, path: &Path) -> Result<FileStat, RefractError>;
    fn read_header(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, RefractError>;
    fn probe_image_dimensions(&self, path: &Path) -> Result<(u32, u32), RefractError>;
    fn read_all(&self, path: &Path) -> Result<Vec<u8>, RefractError>;
    fn probe_pdf(&self, bytes: &[u8], password: Option<&str>)
        -> Result<pdf::PdfInfo, RefractError>;
    fn audio_dependencies(&self) -> audio::AudioDependencies;
}

/// Production Desktop probe backed by `std::fs`, `refract_engine::native`, `pdf`, and `audio`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativePreflightProbe;

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("input file")
        .to_string()
}

fn map_fs_error(path: &Path, err: &std::io::Error) -> RefractError {
    let name = display_name(path);
    match err.kind() {
        std::io::ErrorKind::NotFound => RefractError::new(
            ErrorCode::DamagedFile,
            format!("Input file could not be found: '{}'.", name),
        ),
        std::io::ErrorKind::PermissionDenied => RefractError::new(
            ErrorCode::DamagedFile,
            format!("Permission denied when accessing '{}'.", name),
        ),
        _ => RefractError::new(
            ErrorCode::DamagedFile,
            format!("Input file '{}' could not be accessed.", name),
        ),
    }
}

impl PreflightProbe for NativePreflightProbe {
    fn stat(&self, path: &Path) -> Result<FileStat, RefractError> {
        let meta = std::fs::metadata(path).map_err(|e| map_fs_error(path, &e))?;
        Ok(FileStat {
            size_bytes: meta.len(),
            is_file: meta.is_file(),
        })
    }

    fn read_header(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, RefractError> {
        let mut file = std::fs::File::open(path).map_err(|e| map_fs_error(path, &e))?;
        let mut buf = vec![0u8; max_bytes];
        let n = file.read(&mut buf).map_err(|e| map_fs_error(path, &e))?;
        buf.truncate(n);
        Ok(buf)
    }

    fn probe_image_dimensions(&self, path: &Path) -> Result<(u32, u32), RefractError> {
        native::probe_image(path).map_err(|e| match e.code {
            ErrorCode::EmptyFile | ErrorCode::UnsupportedFormat => e,
            _ => RefractError::new(
                ErrorCode::DamagedFile,
                format!(
                    "Could not read valid image dimensions from '{}'. The file may be damaged.",
                    display_name(path)
                ),
            ),
        })
    }

    fn read_all(&self, path: &Path) -> Result<Vec<u8>, RefractError> {
        std::fs::read(path).map_err(|e| map_fs_error(path, &e))
    }

    fn probe_pdf(
        &self,
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<pdf::PdfInfo, RefractError> {
        let result = match password {
            Some(pw) => pdf::probe_pdf_with_password(bytes, pw),
            None => pdf::probe_pdf(bytes),
        };
        result.map_err(|e| match e.code {
            ErrorCode::EmptyFile | ErrorCode::NeedsPassword | ErrorCode::WrongPassword => e,
            _ => RefractError::new(
                ErrorCode::DamagedFile,
                "The PDF file is damaged or has invalid metadata and could not be opened.",
            ),
        })
    }

    fn audio_dependencies(&self) -> audio::AudioDependencies {
        audio::detect_audio_dependencies()
    }
}

fn check_memory_budget(estimate_bytes: u64, budget_bytes: u64) -> Result<(), RefractError> {
    if budget_bytes == memory_budget_bytes(Platform::Desktop) {
        return check_memory(estimate_bytes, Platform::Desktop);
    }
    if estimate_bytes > budget_bytes {
        let ceil_mb = |b: u64| b.div_ceil(MIB);
        return Err(RefractError::new(
            ErrorCode::LimitExceeded,
            format!(
                "This job needs about {} MB of memory and the budget here is {} MB.",
                ceil_mb(estimate_bytes),
                ceil_mb(budget_bytes)
            ),
        ));
    }
    Ok(())
}

fn pdf_to_images_page_render_bytes(dpi: u32) -> u64 {
    // Matches `packages/core/src/tools/tools.ts`: A4 (8.27 x 11.69 in) at chosen DPI, RGBA x 3 buffers.
    let dpi_f = f64::from(dpi);
    (8.27 * 11.69 * dpi_f * dpi_f * 4.0 * 3.0).round() as u64
}

fn detect_and_validate_format(
    tool: ToolId,
    path: &Path,
    header: &[u8],
) -> Result<Format, RefractError> {
    if header.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            format!("'{}' is empty (0 bytes).", display_name(path)),
        ));
    }

    let Some(format) = refract_core::detect(header) else {
        return Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            format!(
                "'{}' is not a recognised file format for {}.",
                display_name(path),
                tool.label()
            ),
        ));
    };

    let accepted = tool.manifest().desktop_accepted_formats;
    if !accepted.contains(&format) {
        if format == Format::Heic {
            return Err(RefractError::new(
                ErrorCode::UnsupportedFormat,
                "HEIC/HEIF images are not supported by the native desktop build. Convert them first using Refract Web.",
            ));
        }
        return Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            format!(
                "'{}' is a {} file, which is not supported by {}.",
                display_name(path),
                format.label(),
                tool.label()
            ),
        ));
    }

    Ok(format)
}

/// Run Desktop input limits & resource preflight (canonical `refract-core` Desktop limits), with
/// the passwords available for PDF inputs (one for all, per input, and guesses for merge-pdf).
pub fn preflight_inputs_with_passwords(
    request: &JobRequest,
    paths: &[&Path],
    passwords: PdfPasswords<'_>,
) -> Result<PreflightReport, RefractError> {
    let limits = limits_for_tool(request.tool(), Platform::Desktop);
    let budget = memory_budget_bytes(Platform::Desktop);
    preflight_core(
        request,
        paths,
        passwords,
        &limits,
        budget,
        &NativePreflightProbe,
    )
}

/// [`preflight_inputs_with_passwords`] with one optional password for every input (tests).
#[cfg(test)]
pub fn preflight_inputs(
    request: &JobRequest,
    paths: &[&Path],
    password: Option<&str>,
) -> Result<PreflightReport, RefractError> {
    preflight_inputs_with_passwords(
        request,
        paths,
        PdfPasswords {
            all: password,
            ..Default::default()
        },
    )
}

/// Run preflight with explicit limits, memory budget, and injectable [`PreflightProbe`] (tests).
#[cfg(test)]
pub fn preflight_inputs_with<P: PreflightProbe>(
    request: &JobRequest,
    paths: &[&Path],
    password: Option<&str>,
    limits: &PlatformLimits,
    memory_budget: u64,
    probe: &P,
) -> Result<PreflightReport, RefractError> {
    let passwords = PdfPasswords {
        all: password,
        ..Default::default()
    };
    preflight_core(request, paths, passwords, limits, memory_budget, probe)
}

fn preflight_core<P: PreflightProbe>(
    request: &JobRequest,
    paths: &[&Path],
    passwords: PdfPasswords<'_>,
    limits: &PlatformLimits,
    memory_budget: u64,
    probe: &P,
) -> Result<PreflightReport, RefractError> {
    let tool = request.tool();
    let spec = tool.spec();

    // 1. Validate file count bounds before touching the filesystem.
    if paths.len() < spec.min_files {
        let msg = if tool == ToolId::MergePdf {
            "Merge PDF needs at least 2 compatible PDF files with the same output destination."
                .to_string()
        } else {
            format!(
                "{} needs at least {} input file(s).",
                tool.label(),
                spec.min_files
            )
        };
        return Err(RefractError::new(ErrorCode::InvalidOption, msg));
    }

    check_limits(
        limits,
        &LimitCheckInput {
            files: Some(paths.len()),
            ..Default::default()
        },
    )?;

    // 2. Metadata-first pass: existence, regular file, non-empty, byte limit, and 1 KB magic-byte
    //    format detection across ALL inputs before any full-file read or decode.
    let mut detected_formats = Vec::with_capacity(paths.len());
    let mut total_bytes = 0u64;

    for &path in paths {
        let stat = probe.stat(path)?;
        if !stat.is_file {
            return Err(RefractError::new(
                ErrorCode::UnsupportedFormat,
                format!("'{}' is a folder, not a file.", display_name(path)),
            ));
        }
        if stat.size_bytes == 0 {
            return Err(RefractError::new(
                ErrorCode::EmptyFile,
                format!("'{}' is empty (0 bytes).", display_name(path)),
            ));
        }

        check_limits(
            limits,
            &LimitCheckInput {
                bytes: Some(stat.size_bytes),
                ..Default::default()
            },
        )?;

        total_bytes = total_bytes.saturating_add(stat.size_bytes);
        let header = probe.read_header(path, HEADER_SNIFF_BYTES)?;
        let format = detect_and_validate_format(tool, path, &header)?;

        detected_formats.push(format);
    }

    // 3. Domain-specific lightweight metadata probes & memory budget checks.
    match request.options() {
        DesktopToolOptions::ConvertImage { .. }
        | DesktopToolOptions::CompressImage { .. }
        | DesktopToolOptions::ResizeImage { .. }
        | DesktopToolOptions::ImagesToPdf { .. } => {
            let mut validated = Vec::with_capacity(paths.len());
            let mut max_pixels = 0u64;
            let mut max_target_pixels = 0u64;

            for (&path, &format) in paths.iter().zip(detected_formats.iter()) {
                let (w, h) = probe.probe_image_dimensions(path)?;
                if w == 0 || h == 0 {
                    return Err(RefractError::new(
                        ErrorCode::DamagedFile,
                        format!(
                            "'{}' has invalid image dimensions ({}x{}).",
                            display_name(path),
                            w,
                            h
                        ),
                    ));
                }
                let pixels = u64::from(w).saturating_mul(u64::from(h));
                check_limits(
                    limits,
                    &LimitCheckInput {
                        pixels: Some(pixels),
                        ..Default::default()
                    },
                )?;
                max_pixels = max_pixels.max(pixels);

                if let Some(engine_resize) = request.options().as_engine_resize_options() {
                    let (tw, th) = image::resolve_resize_dimensions(w, h, &engine_resize)?;
                    let target_pixels = u64::from(tw).saturating_mul(u64::from(th));
                    check_limits(
                        limits,
                        &LimitCheckInput {
                            pixels: Some(target_pixels),
                            ..Default::default()
                        },
                    )?;
                    max_target_pixels = max_target_pixels.max(target_pixels);
                }

                validated.push(ValidatedInput {
                    format,
                    pdf_bytes: None,
                    page_count: None,
                    unlocked_with: PasswordSource::None,
                });
            }

            let estimated_memory_bytes = match request.options() {
                DesktopToolOptions::ConvertImage { .. } => max_pixels.saturating_mul(12),
                DesktopToolOptions::CompressImage { .. } => max_pixels.saturating_mul(16),
                DesktopToolOptions::ResizeImage { .. } => {
                    max_pixels.max(max_target_pixels).saturating_mul(16)
                }
                DesktopToolOptions::ImagesToPdf { .. } => max_pixels
                    .saturating_mul(12)
                    .saturating_add(total_bytes.saturating_mul(2)),
                _ => 0,
            };
            check_memory_budget(estimated_memory_bytes, memory_budget)?;

            Ok(PreflightReport { inputs: validated })
        }

        DesktopToolOptions::MergePdf
        | DesktopToolOptions::SplitPdf { .. }
        | DesktopToolOptions::ReorderPdf { .. }
        | DesktopToolOptions::PdfToImages { .. }
        | DesktopToolOptions::CompressPdf => {
            // Enforce byte-based memory budget BEFORE reading full PDF files into RAM.
            let extra_render_bytes = match request.options() {
                DesktopToolOptions::PdfToImages { dpi, .. } => {
                    pdf_to_images_page_render_bytes(*dpi)
                }
                _ => 0,
            };
            let estimated_memory_bytes = total_bytes
                .saturating_mul(5)
                .saturating_add(extra_render_bytes);
            check_memory_budget(estimated_memory_bytes, memory_budget)?;

            let mut validated = Vec::with_capacity(paths.len());
            let mut total_pages = 0u32;

            for (index, (&path, &format)) in paths.iter().zip(detected_formats.iter()).enumerate() {
                let bytes = probe.read_all(path)?;
                let actual_len = bytes.len() as u64;
                if actual_len == 0 {
                    return Err(RefractError::new(
                        ErrorCode::EmptyFile,
                        format!("'{}' is empty (0 bytes).", display_name(path)),
                    ));
                }
                check_limits(
                    limits,
                    &LimitCheckInput {
                        bytes: Some(actual_len),
                        ..Default::default()
                    },
                )?;

                let (info, unlocked_with) =
                    unlock_pdf_input(probe, &bytes, index, path, &passwords)?;
                if info.page_count == 0 {
                    return Err(RefractError::new(
                        ErrorCode::DamagedFile,
                        format!(
                            "'{}' has no pages and cannot be processed.",
                            display_name(path)
                        ),
                    ));
                }

                check_limits(
                    limits,
                    &LimitCheckInput {
                        pages: Some(info.page_count),
                        ..Default::default()
                    },
                )?;
                total_pages = total_pages.saturating_add(info.page_count);
                check_limits(
                    limits,
                    &LimitCheckInput {
                        pages: Some(total_pages),
                        ..Default::default()
                    },
                )?;

                validated.push(ValidatedInput {
                    format,
                    pdf_bytes: Some(bytes),
                    page_count: Some(info.page_count),
                    unlocked_with,
                });
            }

            Ok(PreflightReport { inputs: validated })
        }

        DesktopToolOptions::ConvertAudio { .. } | DesktopToolOptions::TrimAudio { .. } => {
            let deps = probe.audio_dependencies();
            if deps.ffmpeg.is_none() {
                return Err(RefractError::new(
                    ErrorCode::MissingDependency,
                    "FFmpeg was not found on PATH. Install FFmpeg to process audio.",
                ));
            }
            if matches!(request.options(), DesktopToolOptions::TrimAudio { .. })
                && deps.ffprobe.is_none()
            {
                return Err(RefractError::new(
                    ErrorCode::MissingDependency,
                    "ffprobe was not found on PATH. Install FFmpeg (including ffprobe) to trim audio.",
                ));
            }

            let estimated_memory_bytes = total_bytes.saturating_mul(4);
            check_memory_budget(estimated_memory_bytes, memory_budget)?;

            let validated = detected_formats
                .into_iter()
                .map(|format| ValidatedInput {
                    format,
                    pdf_bytes: None,
                    page_count: None,
                    unlocked_with: PasswordSource::None,
                })
                .collect();

            Ok(PreflightReport { inputs: validated })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::options::OutputDestination;
    use crate::pipeline::TrimEnd;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    struct MockEntry {
        stat: Result<FileStat, RefractError>,
        header: Vec<u8>,
        dims: Option<Result<(u32, u32), RefractError>>,
        pdf_info: Option<Result<pdf::PdfInfo, RefractError>>,
        body: Vec<u8>,
    }

    struct MockProbe {
        entries: HashMap<PathBuf, MockEntry>,
        audio_deps: audio::AudioDependencies,
        read_all_calls: RefCell<usize>,
        header_calls: RefCell<usize>,
        image_dim_calls: RefCell<usize>,
    }

    impl MockProbe {
        fn new() -> Self {
            Self {
                entries: HashMap::new(),
                audio_deps: audio::AudioDependencies {
                    ffmpeg: Some(PathBuf::from("ffmpeg")),
                    ffprobe: Some(PathBuf::from("ffprobe")),
                },
                read_all_calls: RefCell::new(0),
                header_calls: RefCell::new(0),
                image_dim_calls: RefCell::new(0),
            }
        }

        fn insert(&mut self, path: impl Into<PathBuf>, entry: MockEntry) {
            self.entries.insert(path.into(), entry);
        }
    }

    impl PreflightProbe for MockProbe {
        fn stat(&self, path: &Path) -> Result<FileStat, RefractError> {
            match self.entries.get(path) {
                Some(entry) => entry.stat.clone(),
                None => Err(RefractError::new(
                    ErrorCode::DamagedFile,
                    format!("Input file could not be found: '{}'.", display_name(path)),
                )),
            }
        }

        fn read_header(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, RefractError> {
            *self.header_calls.borrow_mut() += 1;
            let entry = self
                .entries
                .get(path)
                .ok_or_else(|| RefractError::new(ErrorCode::DamagedFile, "missing"))?;
            Ok(entry.header.iter().copied().take(max_bytes).collect())
        }

        fn probe_image_dimensions(&self, path: &Path) -> Result<(u32, u32), RefractError> {
            *self.image_dim_calls.borrow_mut() += 1;
            let entry = self
                .entries
                .get(path)
                .ok_or_else(|| RefractError::new(ErrorCode::DamagedFile, "missing"))?;
            entry
                .dims
                .clone()
                .unwrap_or_else(|| Err(RefractError::new(ErrorCode::DamagedFile, "bad dims")))
        }

        fn read_all(&self, path: &Path) -> Result<Vec<u8>, RefractError> {
            *self.read_all_calls.borrow_mut() += 1;
            let entry = self
                .entries
                .get(path)
                .ok_or_else(|| RefractError::new(ErrorCode::DamagedFile, "missing"))?;
            Ok(entry.body.clone())
        }

        fn probe_pdf(
            &self,
            _bytes: &[u8],
            _password: Option<&str>,
        ) -> Result<pdf::PdfInfo, RefractError> {
            // Find the single entry matching `_bytes` (mock encodes index in first byte if needed,
            // or falls back to scanning entries by body).
            for entry in self.entries.values() {
                if entry.body == _bytes {
                    if let Some(res) = &entry.pdf_info {
                        return res.clone();
                    }
                }
            }
            Err(RefractError::new(
                ErrorCode::DamagedFile,
                "no mock pdf info",
            ))
        }

        fn audio_dependencies(&self) -> audio::AudioDependencies {
            self.audio_deps.clone()
        }
    }

    fn png_header() -> Vec<u8> {
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0]
    }

    fn pdf_header() -> Vec<u8> {
        b"%PDF-1.7\n".to_vec()
    }

    fn wav_header() -> Vec<u8> {
        b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec()
    }

    #[test]
    fn oversized_files_are_rejected_before_any_full_file_read() {
        let img_limits = limits_for_tool(ToolId::ConvertImage, Platform::Desktop);
        let pdf_limits = limits_for_tool(ToolId::CompressPdf, Platform::Desktop);
        let audio_limits = limits_for_tool(ToolId::ConvertAudio, Platform::Desktop);
        let budget = memory_budget_bytes(Platform::Desktop);

        // 1. Oversized image (2048 MB + 1 byte) rejected before header or full read
        let mut probe = MockProbe::new();
        let big_img = PathBuf::from("huge.png");
        probe.insert(
            big_img.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: img_limits.max_mb * MIB + 1,
                    is_file: true,
                }),
                header: png_header(),
                dims: Some(Ok((400, 300))),
                pdf_info: None,
                body: vec![],
            },
        );
        let req = JobRequest::new(
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Jpeg,
                quality: 90,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let err = preflight_inputs_with(
            &req,
            &[big_img.as_path()],
            None,
            &img_limits,
            budget,
            &probe,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert_eq!(*probe.read_all_calls.borrow(), 0);
        assert_eq!(*probe.header_calls.borrow(), 0);
        assert_eq!(*probe.image_dim_calls.borrow(), 0);

        // 2. Oversized PDF (4096 MB + 1 byte) rejected before read_all
        let mut pdf_probe = MockProbe::new();
        let big_pdf = PathBuf::from("huge.pdf");
        pdf_probe.insert(
            big_pdf.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: pdf_limits.max_mb * MIB + 1,
                    is_file: true,
                }),
                header: pdf_header(),
                dims: None,
                pdf_info: Some(Ok(pdf::PdfInfo {
                    page_count: 10,
                    is_encrypted: false,
                    requires_password: false,
                })),
                body: b"%PDF-1.7".to_vec(),
            },
        );
        let pdf_req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        let pdf_err = preflight_inputs_with(
            &pdf_req,
            &[big_pdf.as_path()],
            None,
            &pdf_limits,
            budget,
            &pdf_probe,
        )
        .unwrap_err();
        assert_eq!(pdf_err.code, ErrorCode::LimitExceeded);
        assert_eq!(*pdf_probe.read_all_calls.borrow(), 0);

        // 3. PDF within file limit (2000 MB) whose 5x memory estimate (10000 MB) exceeds 8192 MB
        //    Desktop memory budget is rejected BEFORE reading full PDF bytes!
        let mut mem_probe = MockProbe::new();
        let mem_pdf = PathBuf::from("mem_heavy.pdf");
        mem_probe.insert(
            mem_pdf.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 2000 * MIB,
                    is_file: true,
                }),
                header: pdf_header(),
                dims: None,
                pdf_info: Some(Ok(pdf::PdfInfo {
                    page_count: 10,
                    is_encrypted: false,
                    requires_password: false,
                })),
                body: b"%PDF-1.7".to_vec(),
            },
        );
        let mem_err = preflight_inputs_with(
            &pdf_req,
            &[mem_pdf.as_path()],
            None,
            &pdf_limits,
            budget,
            &mem_probe,
        )
        .unwrap_err();
        assert_eq!(mem_err.code, ErrorCode::LimitExceeded);
        assert!(mem_err.message.contains("memory"));
        assert_eq!(
            *mem_probe.read_all_calls.borrow(),
            0,
            "PDF exceeding memory budget must be rejected before full read"
        );

        // 4. Oversized audio file (4096 MB + 1 byte) rejected cleanly
        let mut audio_probe = MockProbe::new();
        let big_audio = PathBuf::from("huge.wav");
        audio_probe.insert(
            big_audio.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: audio_limits.max_mb * MIB + 1,
                    is_file: true,
                }),
                header: wav_header(),
                dims: None,
                pdf_info: None,
                body: vec![],
            },
        );
        let audio_req = JobRequest::new(
            ToolId::ConvertAudio,
            DesktopToolOptions::ConvertAudio {
                target: Format::Mp3,
                bitrate_kbps: Some(192),
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let audio_err = preflight_inputs_with(
            &audio_req,
            &[big_audio.as_path()],
            None,
            &audio_limits,
            budget,
            &audio_probe,
        )
        .unwrap_err();
        assert_eq!(audio_err.code, ErrorCode::LimitExceeded);
        assert_eq!(*audio_probe.read_all_calls.borrow(), 0);
    }

    #[test]
    fn exact_boundaries_pass_and_plus_one_fails_for_bytes_pixels_pages_and_memory() {
        let img_limits = limits_for_tool(ToolId::ConvertImage, Platform::Desktop);
        let pdf_limits = limits_for_tool(ToolId::CompressPdf, Platform::Desktop);
        let budget = memory_budget_bytes(Platform::Desktop);

        // 1. Exact Desktop image byte limit (2048 MB) passes; +1 byte fails
        let mut probe = MockProbe::new();
        let p_ok = PathBuf::from("exact_bytes.png");
        probe.insert(
            p_ok.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: img_limits.max_mb * MIB,
                    is_file: true,
                }),
                header: png_header(),
                dims: Some(Ok((1000, 1000))),
                pdf_info: None,
                body: vec![],
            },
        );
        let conv_req = JobRequest::new(
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Jpeg,
                quality: 90,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        assert!(preflight_inputs_with(
            &conv_req,
            &[p_ok.as_path()],
            None,
            &img_limits,
            budget,
            &probe
        )
        .is_ok());

        // 2. Pixel limit: exact 100,000 pixels passes under custom limit; 100,001 fails.
        //    Also test Desktop 1,000,000,000 pixel limit directly via MockProbe!
        let tight_img_limits = PlatformLimits {
            max_pixels: Some(100_000),
            ..img_limits
        };
        let p_exact_px = PathBuf::from("exact_px.png");
        let p_over_px = PathBuf::from("over_px.png");
        probe.insert(
            p_exact_px.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 1024,
                    is_file: true,
                }),
                header: png_header(),
                dims: Some(Ok((400, 250))), // 100,000 px
                pdf_info: None,
                body: vec![],
            },
        );
        probe.insert(
            p_over_px.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 1024,
                    is_file: true,
                }),
                header: png_header(),
                dims: Some(Ok((401, 250))), // 100,250 px
                pdf_info: None,
                body: vec![],
            },
        );
        assert!(preflight_inputs_with(
            &conv_req,
            &[p_exact_px.as_path()],
            None,
            &tight_img_limits,
            budget,
            &probe
        )
        .is_ok());
        let px_err = preflight_inputs_with(
            &conv_req,
            &[p_over_px.as_path()],
            None,
            &tight_img_limits,
            budget,
            &probe,
        )
        .unwrap_err();
        assert_eq!(px_err.code, ErrorCode::LimitExceeded);
        assert!(px_err.message.contains("megapixels"));

        // Desktop 1000 MP pixel limit: 40_000 x 25_001 = 1,000,040,000 > 1,000,000,000 -> LimitExceeded
        let p_over_desktop_px = PathBuf::from("over_desktop_px.png");
        probe.insert(
            p_over_desktop_px.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 1024,
                    is_file: true,
                }),
                header: png_header(),
                dims: Some(Ok((40_000, 25_001))),
                pdf_info: None,
                body: vec![],
            },
        );
        assert_eq!(
            preflight_inputs_with(
                &conv_req,
                &[p_over_desktop_px.as_path()],
                None,
                &img_limits,
                budget,
                &probe
            )
            .unwrap_err()
            .code,
            ErrorCode::LimitExceeded
        );

        // 3. PDF page limit: exact 20,000 pages passes; 20,001 pages fails
        let mut pdf_probe = MockProbe::new();
        let p_exact_pages = PathBuf::from("exact_pages.pdf");
        let p_over_pages = PathBuf::from("over_pages.pdf");
        pdf_probe.insert(
            p_exact_pages.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 4096,
                    is_file: true,
                }),
                header: pdf_header(),
                dims: None,
                pdf_info: Some(Ok(pdf::PdfInfo {
                    page_count: pdf_limits.max_pages.unwrap(),
                    is_encrypted: false,
                    requires_password: false,
                })),
                body: b"%PDF-exact".to_vec(),
            },
        );
        pdf_probe.insert(
            p_over_pages.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 4096,
                    is_file: true,
                }),
                header: pdf_header(),
                dims: None,
                pdf_info: Some(Ok(pdf::PdfInfo {
                    page_count: pdf_limits.max_pages.unwrap() + 1,
                    is_encrypted: false,
                    requires_password: false,
                })),
                body: b"%PDF-over".to_vec(),
            },
        );
        let pdf_req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        assert!(preflight_inputs_with(
            &pdf_req,
            &[p_exact_pages.as_path()],
            None,
            &pdf_limits,
            budget,
            &pdf_probe
        )
        .is_ok());
        let page_err = preflight_inputs_with(
            &pdf_req,
            &[p_over_pages.as_path()],
            None,
            &pdf_limits,
            budget,
            &pdf_probe,
        )
        .unwrap_err();
        assert_eq!(page_err.code, ErrorCode::LimitExceeded);
        assert!(page_err.message.contains("pages"));
    }

    #[test]
    fn real_fixtures_validate_unsupported_missing_inaccessible_directory_and_encrypted_pdfs() {
        let temp_dir = tempfile::tempdir().unwrap();

        // 1. Missing file
        let missing = temp_dir.path().join("does_not_exist.png");
        let conv_req = JobRequest::new(
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Jpeg,
                quality: 90,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let miss_err = preflight_inputs(&conv_req, &[missing.as_path()], None).unwrap_err();
        assert_eq!(miss_err.code, ErrorCode::DamagedFile);
        assert!(miss_err.message.contains("could not be found"));

        // 2. Directory passed instead of file
        let dir_err = preflight_inputs(&conv_req, &[temp_dir.path()], None).unwrap_err();
        assert_eq!(dir_err.code, ErrorCode::UnsupportedFormat);
        assert!(dir_err.message.contains("folder"));

        // 3. Empty file
        let empty = fixture_path("empty.heic");
        let empty_err = preflight_inputs(&conv_req, &[empty.as_path()], None).unwrap_err();
        assert_eq!(empty_err.code, ErrorCode::EmptyFile);

        // 4. Unsupported HEIC on Desktop (including disguised really-heic.jpg)
        for name in ["plain.heic", "really-heic.jpg"] {
            let p = fixture_path(name);
            let err = preflight_inputs(&conv_req, &[p.as_path()], None).unwrap_err();
            assert_eq!(err.code, ErrorCode::UnsupportedFormat);
            assert!(err.message.contains("HEIC"));
        }

        // 5. Wrong domain (PDF passed to convert-image; PNG passed to compress-pdf)
        let pdf_path = fixture_path("plain.pdf");
        let wrong_dom = preflight_inputs(&conv_req, &[pdf_path.as_path()], None).unwrap_err();
        assert_eq!(wrong_dom.code, ErrorCode::UnsupportedFormat);

        let compress_pdf_req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        let png_path = fixture_path("plain.png");
        let wrong_pdf =
            preflight_inputs(&compress_pdf_req, &[png_path.as_path()], None).unwrap_err();
        assert_eq!(wrong_pdf.code, ErrorCode::UnsupportedFormat);

        // 6. Permission denied (via MockProbe for cross-platform determinism on Windows/Unix)
        let mut perm_probe = MockProbe::new();
        let secret_file = PathBuf::from("locked_perms.png");
        perm_probe.insert(
            secret_file.clone(),
            MockEntry {
                stat: Err(RefractError::new(
                    ErrorCode::DamagedFile,
                    "Permission denied when accessing 'locked_perms.png'.",
                )),
                header: vec![],
                dims: None,
                pdf_info: None,
                body: vec![],
            },
        );
        let perm_err = preflight_inputs_with(
            &conv_req,
            &[secret_file.as_path()],
            None,
            &limits_for_tool(ToolId::ConvertImage, Platform::Desktop),
            memory_budget_bytes(Platform::Desktop),
            &perm_probe,
        )
        .unwrap_err();
        assert_eq!(perm_err.code, ErrorCode::DamagedFile);
        assert!(perm_err.message.contains("Permission denied"));

        // 7. Password-protected PDF: preflight without password preserves NeedsPassword;
        //    wrong password returns WrongPassword; right password unlocks and validates page limits!
        let locked = fixture_path("locked.pdf");
        assert_eq!(
            preflight_inputs(&compress_pdf_req, &[locked.as_path()], None)
                .unwrap_err()
                .code,
            ErrorCode::NeedsPassword
        );
        assert_eq!(
            preflight_inputs(&compress_pdf_req, &[locked.as_path()], Some("wrong-pw"))
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        let unlocked_report =
            preflight_inputs(&compress_pdf_req, &[locked.as_path()], Some("user-pw-123")).unwrap();
        assert_eq!(unlocked_report.inputs[0].format, Format::Pdf);
        assert!(unlocked_report.inputs[0].pdf_bytes.is_some());

        // After unlock, page limit is enforced! (locked.pdf has 3 pages; limit = 2 -> LimitExceeded)
        let tight_pdf_limits = PlatformLimits {
            max_pages: Some(2),
            ..limits_for_tool(ToolId::CompressPdf, Platform::Desktop)
        };
        assert_eq!(
            preflight_inputs_with(
                &compress_pdf_req,
                &[locked.as_path()],
                Some("user-pw-123"),
                &tight_pdf_limits,
                memory_budget_bytes(Platform::Desktop),
                &NativePreflightProbe,
            )
            .unwrap_err()
            .code,
            ErrorCode::LimitExceeded
        );

        // Owner-only encrypted PDF opens without password prompt and validates pages
        let owner_only = fixture_path("owner-only.pdf");
        let owner_report =
            preflight_inputs(&compress_pdf_req, &[owner_only.as_path()], None).unwrap();
        assert_eq!(owner_report.inputs[0].format, Format::Pdf);
        assert!(owner_report.inputs[0].pdf_bytes.is_some());
    }

    #[test]
    fn batch_preflight_validates_every_input_and_aggregate_limits() {
        let plain_pdf = fixture_path("plain.pdf"); // 3 pages
        let five_pages = fixture_path("5pages.pdf"); // 5 pages
        let plain_png = fixture_path("plain.png"); // 400x300
        let heic = fixture_path("plain.heic");

        // 1. merge-pdf: individual PDFs fit (<= 5 pages), but aggregate (3 + 5 = 8 > 6) fails!
        let merge_req = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        let tight_merge_limits = PlatformLimits {
            max_pages: Some(6),
            ..limits_for_tool(ToolId::MergePdf, Platform::Desktop)
        };
        let agg_err = preflight_inputs_with(
            &merge_req,
            &[plain_pdf.as_path(), five_pages.as_path()],
            None,
            &tight_merge_limits,
            memory_budget_bytes(Platform::Desktop),
            &NativePreflightProbe,
        )
        .unwrap_err();
        assert_eq!(agg_err.code, ErrorCode::LimitExceeded);
        assert!(agg_err.message.contains("8 pages"));

        // 2. merge-pdf where second input is unsupported PNG fails cleanly without partial merge
        let bad_second = preflight_inputs(
            &merge_req,
            &[plain_pdf.as_path(), plain_png.as_path()],
            None,
        )
        .unwrap_err();
        assert_eq!(bad_second.code, ErrorCode::UnsupportedFormat);

        // 3. images-to-pdf where one input is unsupported HEIC fails the entire batch cleanly
        let imgs_req = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 85,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let bad_img_batch =
            preflight_inputs(&imgs_req, &[plain_png.as_path(), heic.as_path()], None).unwrap_err();
        assert_eq!(bad_img_batch.code, ErrorCode::UnsupportedFormat);
    }

    #[test]
    fn audio_preflight_checks_format_size_and_missing_dependencies() {
        let wav = fixture_path("sound.wav");
        let trim_req = JobRequest::new(
            ToolId::TrimAudio,
            DesktopToolOptions::TrimAudio {
                start_secs: 0.0,
                end: TrimEnd::EndOfFile,
                bitrate_kbps: 192,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let conv_audio_req = JobRequest::new(
            ToolId::ConvertAudio,
            DesktopToolOptions::ConvertAudio {
                target: Format::Mp3,
                bitrate_kbps: Some(192),
            },
            OutputDestination::NextToSource,
        )
        .unwrap();

        let limits = limits_for_tool(ToolId::TrimAudio, Platform::Desktop);
        let budget = memory_budget_bytes(Platform::Desktop);

        // Missing ffprobe fails trim-audio with MissingDependency while convert-audio still passes
        let mut probe_no_ffprobe = MockProbe::new();
        probe_no_ffprobe.audio_deps = audio::AudioDependencies {
            ffmpeg: Some(PathBuf::from("ffmpeg")),
            ffprobe: None,
        };
        probe_no_ffprobe.insert(
            wav.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 1024,
                    is_file: true,
                }),
                header: wav_header(),
                dims: None,
                pdf_info: None,
                body: vec![],
            },
        );
        assert_eq!(
            preflight_inputs_with(
                &trim_req,
                &[wav.as_path()],
                None,
                &limits,
                budget,
                &probe_no_ffprobe
            )
            .unwrap_err()
            .code,
            ErrorCode::MissingDependency
        );
        assert!(preflight_inputs_with(
            &conv_audio_req,
            &[wav.as_path()],
            None,
            &limits,
            budget,
            &probe_no_ffprobe
        )
        .is_ok());

        // Missing ffmpeg fails both convert-audio and trim-audio with MissingDependency
        probe_no_ffprobe.audio_deps.ffmpeg = None;
        assert_eq!(
            preflight_inputs_with(
                &conv_audio_req,
                &[wav.as_path()],
                None,
                &limits,
                budget,
                &probe_no_ffprobe
            )
            .unwrap_err()
            .code,
            ErrorCode::MissingDependency
        );

        // Non-audio file (plain.png) rejected as UnsupportedFormat before checking ffmpeg
        let png = fixture_path("plain.png");
        let err = preflight_inputs(&conv_audio_req, &[png.as_path()], None).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedFormat);
    }

    #[test]
    fn all_eleven_tools_pass_preflight_with_valid_inputs() {
        let png = fixture_path("plain.png");
        let jpg = fixture_path("rotated.jpg");
        let pdf1 = fixture_path("plain.pdf");
        let pdf2 = fixture_path("5pages.pdf");
        let wav = fixture_path("sound.wav");

        let single_cases = vec![
            (
                JobRequest::new(
                    ToolId::ConvertImage,
                    DesktopToolOptions::default_for(ToolId::ConvertImage).unwrap(),
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![png.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::CompressImage,
                    DesktopToolOptions::default_for(ToolId::CompressImage).unwrap(),
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![jpg.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::ResizeImage,
                    DesktopToolOptions::default_for(ToolId::ResizeImage).unwrap(),
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![png.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::MergePdf,
                    DesktopToolOptions::MergePdf,
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![pdf1.as_path(), pdf2.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::SplitPdf,
                    DesktopToolOptions::default_for(ToolId::SplitPdf).unwrap(),
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![pdf1.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::ReorderPdf,
                    DesktopToolOptions::ReorderPdf {
                        order: "3, 1, 2".into(),
                    },
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![pdf1.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::ImagesToPdf,
                    DesktopToolOptions::default_for(ToolId::ImagesToPdf).unwrap(),
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![png.as_path(), jpg.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::PdfToImages,
                    DesktopToolOptions::default_for(ToolId::PdfToImages).unwrap(),
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![pdf1.as_path()],
            ),
            (
                JobRequest::new(
                    ToolId::CompressPdf,
                    DesktopToolOptions::CompressPdf,
                    OutputDestination::NextToSource,
                )
                .unwrap(),
                vec![pdf1.as_path()],
            ),
        ];

        for (req, paths) in single_cases {
            let report = preflight_inputs(&req, &paths, None)
                .unwrap_or_else(|e| panic!("preflight failed for {:?}: {}", req.tool(), e));
            assert_eq!(report.inputs.len(), paths.len());
        }

        // Audio tools tested with MockProbe (deterministic regardless of host FFmpeg)
        let mut audio_probe = MockProbe::new();
        audio_probe.insert(
            wav.clone(),
            MockEntry {
                stat: Ok(FileStat {
                    size_bytes: 264_678,
                    is_file: true,
                }),
                header: wav_header(),
                dims: None,
                pdf_info: None,
                body: vec![],
            },
        );
        for tool in [ToolId::ConvertAudio, ToolId::TrimAudio] {
            let req = JobRequest::new(
                tool,
                DesktopToolOptions::default_for(tool).unwrap(),
                OutputDestination::NextToSource,
            )
            .unwrap();
            let report = preflight_inputs_with(
                &req,
                &[wav.as_path()],
                None,
                &limits_for_tool(tool, Platform::Desktop),
                memory_budget_bytes(Platform::Desktop),
                &audio_probe,
            )
            .unwrap();
            assert_eq!(report.inputs.len(), 1);
            assert_eq!(report.inputs[0].format, Format::Wav);
        }
    }
}
