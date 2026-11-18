use crate::process::run_command_cancellable;
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use which::which;

/// One rasterized PDF page: `(1-based page number, encoded image bytes, primary extension)`.
pub type RenderedPage = (u32, Vec<u8>, &'static str);

/// Target output format for PDF rasterization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderImageFormat {
    Png,
    Jpeg,
}

impl RenderImageFormat {
    pub fn parse(raw: &str) -> Result<Self, RefractError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            other => Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Unsupported PDF render image format '{}'. Use PNG or JPEG.",
                    other
                ),
            )),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// Which external PDF rasterizer binary is available on the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererKind {
    Mutool,
    Pdftoppm,
    Pdftocairo,
    Gs,
}

/// A detected rasterizer with its kind and path.
#[derive(Debug, Clone)]
pub struct Renderer {
    pub kind: RendererKind,
    pub path: PathBuf,
}

/// Pure, testable command specification for rendering one PDF page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCommandSpec {
    pub args: Vec<OsString>,
    /// Output filename stem (without extension) inside `output_dir`.
    pub output_stem: String,
    /// Format produced directly by the external binary on disk.
    pub binary_output_format: RenderImageFormat,
    /// Whether the caller must transcode `binary_output_format` (PNG) to JPEG after rendering.
    /// Needed for `mutool draw`, whose native raster outputs do not include JPEG across versions.
    pub transcode_to_jpeg: bool,
}

/// Probe for available PDF rasterizer binaries on the system.
/// Returns the first one found in priority order: mutool > pdftoppm > pdftocairo > gs.
pub fn detect_renderer() -> Option<Renderer> {
    const CANDIDATES: &[(&str, RendererKind)] = &[
        ("mutool", RendererKind::Mutool),
        ("pdftoppm", RendererKind::Pdftoppm),
        ("pdftocairo", RendererKind::Pdftocairo),
        ("gs", RendererKind::Gs),
        ("gswin64c", RendererKind::Gs),
        ("gswin32c", RendererKind::Gs),
    ];

    for (name, kind) in CANDIDATES {
        if let Ok(path) = which(name) {
            return Some(Renderer { kind: *kind, path });
        }
    }
    None
}

/// Check if any PDF rasterizer is available.
pub fn has_renderer() -> bool {
    detect_renderer().is_some()
}

/// Build the verified CLI argument list for rendering a single PDF page with `kind`.
///
/// Verified CLI contracts:
/// - `mutool draw`: `-F png -r <dpi> -o <file.png> <input.pdf> <page>` (transcodes PNG -> JPEG in Rust when JPEG is requested because `mutool draw` does not universally support direct JPEG output).
/// - `pdftoppm`: `[-png | -jpeg] -r <dpi> -f <page> -l <page> -singlefile <input.pdf> <prefix>`
/// - `pdftocairo`: `[-png | -jpeg] -r <dpi> -f <page> -l <page> -singlefile <input.pdf> <prefix>`
/// - `gs`: `-q -dSAFER -dNOPAUSE -dBATCH -sDEVICE=<png16m|jpeg> -r<dpi> -dFirstPage=<page> -dLastPage=<page> -o <file> <input.pdf>`
pub fn build_render_command_spec(
    kind: RendererKind,
    pdf_path: &Path,
    page_number: u32,
    dpi: u32,
    output_dir: &Path,
    requested_format: RenderImageFormat,
) -> RenderCommandSpec {
    let output_stem = format!("page-{:04}", page_number);
    let prefix = output_dir.join(&output_stem);

    match kind {
        RendererKind::Mutool => {
            let png_path = output_dir.join(format!("{}.png", output_stem));
            RenderCommandSpec {
                args: vec![
                    OsString::from("draw"),
                    OsString::from("-F"),
                    OsString::from("png"),
                    OsString::from("-r"),
                    OsString::from(dpi.to_string()),
                    OsString::from("-o"),
                    png_path.into_os_string(),
                    pdf_path.as_os_str().to_os_string(),
                    OsString::from(page_number.to_string()),
                ],
                output_stem,
                binary_output_format: RenderImageFormat::Png,
                transcode_to_jpeg: requested_format == RenderImageFormat::Jpeg,
            }
        }
        RendererKind::Pdftoppm | RendererKind::Pdftocairo => {
            let fmt_flag = match requested_format {
                RenderImageFormat::Png => "-png",
                RenderImageFormat::Jpeg => "-jpeg",
            };
            RenderCommandSpec {
                args: vec![
                    OsString::from(fmt_flag),
                    OsString::from("-r"),
                    OsString::from(dpi.to_string()),
                    OsString::from("-f"),
                    OsString::from(page_number.to_string()),
                    OsString::from("-l"),
                    OsString::from(page_number.to_string()),
                    OsString::from("-singlefile"),
                    pdf_path.as_os_str().to_os_string(),
                    prefix.into_os_string(),
                ],
                output_stem,
                binary_output_format: requested_format,
                transcode_to_jpeg: false,
            }
        }
        RendererKind::Gs => {
            let device = match requested_format {
                RenderImageFormat::Png => "png16m",
                RenderImageFormat::Jpeg => "jpeg",
            };
            let out_path =
                output_dir.join(format!("{}.{}", output_stem, requested_format.extension()));
            RenderCommandSpec {
                args: vec![
                    OsString::from("-q"),
                    OsString::from("-dSAFER"),
                    OsString::from("-dNOPAUSE"),
                    OsString::from("-dBATCH"),
                    OsString::from(format!("-sDEVICE={}", device)),
                    OsString::from(format!("-r{}", dpi)),
                    OsString::from(format!("-dFirstPage={}", page_number)),
                    OsString::from(format!("-dLastPage={}", page_number)),
                    OsString::from("-o"),
                    out_path.into_os_string(),
                    pdf_path.as_os_str().to_os_string(),
                ],
                output_stem,
                binary_output_format: requested_format,
                transcode_to_jpeg: false,
            }
        }
    }
}

/// Locate the rendered page file inside `output_dir` for both single-page (`<stem>.<ext>`)
/// and multi-page numbered outputs (`<stem>-1.<ext>`, `<stem>-01.<ext>`, `<stem>-001.<ext>`),
/// handling `.jpg` and `.jpeg` variants for JPEG outputs.
pub fn discover_rendered_page_file(
    output_dir: &Path,
    output_stem: &str,
    page_number: u32,
    format: RenderImageFormat,
) -> Option<PathBuf> {
    let valid_exts: &[&str] = match format {
        RenderImageFormat::Png => &["png"],
        RenderImageFormat::Jpeg => &["jpg", "jpeg"],
    };

    // 1. Exact single-file match (`<stem>.png`, `<stem>.jpg`, `<stem>.jpeg`).
    for ext in valid_exts {
        let direct = output_dir.join(format!("{}.{}", output_stem, ext));
        if direct.is_file() {
            return Some(direct);
        }
    }

    // 2. Numbered suffix fallback (`<stem>-<zero-padded-or-plain-page>.<ext>`),
    //    used if a Poppler build writes numbered files despite `-singlefile`.
    let prefix_dash = format!("{}-", output_stem);
    let mut matches: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !valid_exts.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Some(num_part) = stem.strip_prefix(&prefix_dash) {
                if !num_part.is_empty()
                    && num_part.chars().all(|c| c.is_ascii_digit())
                    && num_part.parse::<u32>().ok() == Some(page_number)
                {
                    matches.push(path);
                }
            }
        }
    }

    matches.sort();
    matches.into_iter().next()
}

/// Render one PDF page to encoded image bytes using the detected rasterizer.
fn render_page(
    renderer: &Renderer,
    pdf_path: &Path,
    page_number: u32,
    dpi: u32,
    output_dir: &Path,
    requested_format: RenderImageFormat,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;

    let spec = build_render_command_spec(
        renderer.kind,
        pdf_path,
        page_number,
        dpi,
        output_dir,
        requested_format,
    );

    let expected_direct = output_dir.join(format!(
        "{}.{}",
        spec.output_stem,
        spec.binary_output_format.extension()
    ));

    run_command_cancellable(
        &renderer.path,
        spec.args,
        cancel,
        std::slice::from_ref(&expected_direct),
        renderer_name(renderer.kind),
    )?;

    cancel.check()?;

    let rendered_path = discover_rendered_page_file(
        output_dir,
        &spec.output_stem,
        page_number,
        spec.binary_output_format,
    )
    .ok_or_else(|| {
        RefractError::new(
            ErrorCode::EngineFailed,
            format!(
                "Rasterizer produced no output file for page {}",
                page_number
            ),
        )
    })?;

    let raw_bytes = std::fs::read(&rendered_path).map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to read rendered image",
            e.to_string(),
        )
    })?;

    // Clean up temporary page file immediately after reading into memory.
    let _ = std::fs::remove_file(&rendered_path);

    if spec.transcode_to_jpeg {
        cancel.check()?;
        crate::image::convert_image(&raw_bytes, Format::Jpeg, Some(90))
    } else {
        Ok(raw_bytes)
    }
}

/// Validated, decrypted render request ready to invoke an external rasterizer.
#[derive(Debug, Clone)]
pub struct PreparedPdfRender {
    pub pdf_bytes: Vec<u8>,
    pub page_count: u32,
    pub dpi: u32,
    pub format: RenderImageFormat,
}

/// Validate render options and unlock encrypted PDFs (auto-unlocking owner-only PDFs with `""`),
/// preserving the caller's exact requested `format` and `dpi`.
pub fn prepare_pdf_for_render(
    pdf_bytes: &[u8],
    password: Option<&str>,
    dpi: u32,
    format: &str,
) -> Result<PreparedPdfRender, RefractError> {
    if pdf_bytes.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "PDF file is empty (0 bytes)",
        ));
    }

    let requested_format = RenderImageFormat::parse(format)?;
    if !(36..=600).contains(&dpi) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Invalid PDF render DPI {}. Must be between 36 and 600.",
                dpi
            ),
        ));
    }

    let info = crate::pdf::probe_pdf(pdf_bytes)?;
    let (prepared_bytes, page_count) = if info.is_encrypted {
        let unlocked = crate::pdf::unlock_pdf_bytes(pdf_bytes, password)?;
        let unlocked_info = crate::pdf::probe_pdf(&unlocked)?;
        (unlocked, unlocked_info.page_count)
    } else {
        (pdf_bytes.to_vec(), info.page_count)
    };

    if page_count == 0 {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "PDF has no pages",
        ));
    }

    Ok(PreparedPdfRender {
        pdf_bytes: prepared_bytes,
        page_count,
        dpi,
        format: requested_format,
    })
}

/// Render all pages of a PDF byte buffer to image bytes with optional password and cancellation.
/// Preserves exact requested `dpi` and `format` across both normal and password-unlocked paths.
pub fn render_pdf_to_images_with_password_cancellable(
    pdf_bytes: &[u8],
    password: Option<&str>,
    dpi: u32,
    format: &str,
    cancel: &CancelToken,
) -> Result<Vec<RenderedPage>, RefractError> {
    cancel.check()?;

    let prepared = prepare_pdf_for_render(pdf_bytes, password, dpi, format)?;

    let renderer = detect_renderer().ok_or_else(|| {
        RefractError::new(
            ErrorCode::MissingDependency,
            "No PDF rasterizer found. Install mutool, pdftoppm, pdftocairo, or Ghostscript (gs).",
        )
    })?;

    let temp_dir = tempdir().map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to create temp directory",
            e.to_string(),
        )
    })?;
    let pdf_path = temp_dir.path().join("input.pdf");
    std::fs::write(&pdf_path, &prepared.pdf_bytes).map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to write PDF to temp file",
            e.to_string(),
        )
    })?;

    render_pdf_path_pages(
        &renderer,
        &pdf_path,
        prepared.page_count,
        prepared.dpi,
        prepared.format,
        temp_dir.path(),
        cancel,
    )
}

/// Render all pages of a PDF byte buffer to image bytes with cooperative cancellation.
pub fn render_pdf_to_images_cancellable(
    pdf_bytes: &[u8],
    dpi: u32,
    format: &str,
    cancel: &CancelToken,
) -> Result<Vec<RenderedPage>, RefractError> {
    render_pdf_to_images_with_password_cancellable(pdf_bytes, None, dpi, format, cancel)
}

/// Render all pages of an existing on-disk PDF file directly without copying the PDF bytes.
pub fn render_pdf_path_to_images_cancellable(
    pdf_path: &Path,
    page_count: u32,
    dpi: u32,
    format: &str,
    cancel: &CancelToken,
) -> Result<Vec<RenderedPage>, RefractError> {
    cancel.check()?;
    let requested_format = RenderImageFormat::parse(format)?;
    if page_count == 0 {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "PDF has no pages",
        ));
    }
    let renderer = detect_renderer().ok_or_else(|| {
        RefractError::new(
            ErrorCode::MissingDependency,
            "No PDF rasterizer found. Install mutool, pdftoppm, pdftocairo, or Ghostscript (gs).",
        )
    })?;
    let dpi = dpi.clamp(36, 600);
    let temp_dir = tempdir().map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to create temp directory",
            e.to_string(),
        )
    })?;
    render_pdf_path_pages(
        &renderer,
        pdf_path,
        page_count,
        dpi,
        requested_format,
        temp_dir.path(),
        cancel,
    )
}

fn render_pdf_path_pages(
    renderer: &Renderer,
    pdf_path: &Path,
    page_count: u32,
    dpi: u32,
    requested_format: RenderImageFormat,
    work_dir: &Path,
    cancel: &CancelToken,
) -> Result<Vec<RenderedPage>, RefractError> {
    let mut results = Vec::with_capacity(page_count as usize);
    let out_ext = requested_format.extension();

    for page in 1..=page_count {
        cancel.check()?;
        let img_bytes = render_page(
            renderer,
            pdf_path,
            page,
            dpi,
            work_dir,
            requested_format,
            cancel,
        )?;
        results.push((page, img_bytes, out_ext));
    }

    Ok(results)
}

/// Render all pages of a PDF to image files (convenience non-cancellable wrapper).
pub fn render_pdf_to_images(
    pdf_bytes: &[u8],
    dpi: u32,
    format: &str,
) -> Result<Vec<RenderedPage>, RefractError> {
    render_pdf_to_images_cancellable(pdf_bytes, dpi, format, &CancelToken::none())
}

/// Human-readable name for a renderer kind.
pub fn renderer_name(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Mutool => "mutool",
        RendererKind::Pdftoppm => "pdftoppm",
        RendererKind::Pdftocairo => "pdftocairo",
        RendererKind::Gs => "Ghostscript (gs)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_strings(spec: &RenderCommandSpec) -> Vec<String> {
        spec.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn renderer_name_returns_correct_strings() {
        assert_eq!(renderer_name(RendererKind::Mutool), "mutool");
        assert_eq!(renderer_name(RendererKind::Pdftoppm), "pdftoppm");
        assert_eq!(renderer_name(RendererKind::Pdftocairo), "pdftocairo");
        assert_eq!(renderer_name(RendererKind::Gs), "Ghostscript (gs)");
    }

    #[test]
    fn mutool_args_render_png_and_flag_jpeg_transcode() {
        let dir = Path::new("/tmp/render");
        let pdf = Path::new("/tmp/doc.pdf");

        let png = build_render_command_spec(
            RendererKind::Mutool,
            pdf,
            3,
            150,
            dir,
            RenderImageFormat::Png,
        );
        let expected_png = dir.join("page-0003.png").to_string_lossy().into_owned();
        assert_eq!(
            args_strings(&png),
            vec![
                "draw",
                "-F",
                "png",
                "-r",
                "150",
                "-o",
                &expected_png,
                "/tmp/doc.pdf",
                "3"
            ]
        );
        assert_eq!(png.binary_output_format, RenderImageFormat::Png);
        assert!(!png.transcode_to_jpeg);

        // JPEG requests render PNG via mutool and transcode in Rust (mutool draw has no universal JPEG target).
        let jpg = build_render_command_spec(
            RendererKind::Mutool,
            pdf,
            3,
            200,
            dir,
            RenderImageFormat::Jpeg,
        );
        assert_eq!(
            args_strings(&jpg),
            vec![
                "draw",
                "-F",
                "png",
                "-r",
                "200",
                "-o",
                &expected_png,
                "/tmp/doc.pdf",
                "3"
            ]
        );
        assert_eq!(jpg.binary_output_format, RenderImageFormat::Png);
        assert!(jpg.transcode_to_jpeg);
    }

    #[test]
    fn pdftoppm_and_pdftocairo_use_proper_png_and_jpeg_flags_with_singlefile() {
        let dir = Path::new("/tmp/render");
        let pdf = Path::new("/tmp/doc.pdf");
        let expected_prefix = dir.join("page-0002").to_string_lossy().into_owned();

        for kind in [RendererKind::Pdftoppm, RendererKind::Pdftocairo] {
            let png = build_render_command_spec(kind, pdf, 2, 150, dir, RenderImageFormat::Png);
            assert_eq!(
                args_strings(&png),
                vec![
                    "-png",
                    "-r",
                    "150",
                    "-f",
                    "2",
                    "-l",
                    "2",
                    "-singlefile",
                    "/tmp/doc.pdf",
                    &expected_prefix
                ]
            );
            assert!(!png.transcode_to_jpeg);

            let jpg = build_render_command_spec(kind, pdf, 2, 300, dir, RenderImageFormat::Jpeg);
            assert_eq!(
                args_strings(&jpg),
                vec![
                    "-jpeg",
                    "-r",
                    "300",
                    "-f",
                    "2",
                    "-l",
                    "2",
                    "-singlefile",
                    "/tmp/doc.pdf",
                    &expected_prefix
                ]
            );
            assert!(!jpg.transcode_to_jpeg);
        }
    }

    #[test]
    fn ghostscript_combines_device_resolution_and_page_flags_as_single_args() {
        let dir = Path::new("/tmp/render");
        let pdf = Path::new("/tmp/doc.pdf");

        let png =
            build_render_command_spec(RendererKind::Gs, pdf, 1, 150, dir, RenderImageFormat::Png);
        let expected_png = dir.join("page-0001.png").to_string_lossy().into_owned();
        assert_eq!(
            args_strings(&png),
            vec![
                "-q",
                "-dSAFER",
                "-dNOPAUSE",
                "-dBATCH",
                "-sDEVICE=png16m",
                "-r150",
                "-dFirstPage=1",
                "-dLastPage=1",
                "-o",
                &expected_png,
                "/tmp/doc.pdf"
            ]
        );

        let jpg =
            build_render_command_spec(RendererKind::Gs, pdf, 4, 240, dir, RenderImageFormat::Jpeg);
        let expected_jpg = dir.join("page-0004.jpg").to_string_lossy().into_owned();
        assert_eq!(
            args_strings(&jpg),
            vec![
                "-q",
                "-dSAFER",
                "-dNOPAUSE",
                "-dBATCH",
                "-sDEVICE=jpeg",
                "-r240",
                "-dFirstPage=4",
                "-dLastPage=4",
                "-o",
                &expected_jpg,
                "/tmp/doc.pdf"
            ]
        );
    }

    #[test]
    fn discovers_single_page_and_zero_padded_multi_page_outputs() {
        let dir = tempdir().unwrap();

        // Direct single-file output
        let direct = dir.path().join("page-0001.png");
        std::fs::write(&direct, b"png").unwrap();
        assert_eq!(
            discover_rendered_page_file(dir.path(), "page-0001", 1, RenderImageFormat::Png),
            Some(direct)
        );

        // Multi-page zero-padded output (e.g. Poppler on a 12-page PDF: page-0002-02.jpg)
        let padded = dir.path().join("page-0002-02.jpg");
        std::fs::write(&padded, b"jpg").unwrap();
        assert_eq!(
            discover_rendered_page_file(dir.path(), "page-0002", 2, RenderImageFormat::Jpeg),
            Some(padded)
        );

        // Multi-page 3-digit padded output with .jpeg extension
        let padded3 = dir.path().join("page-0007-007.jpeg");
        std::fs::write(&padded3, b"jpeg").unwrap();
        assert_eq!(
            discover_rendered_page_file(dir.path(), "page-0007", 7, RenderImageFormat::Jpeg),
            Some(padded3)
        );

        // Wrong page number must not match
        assert_eq!(
            discover_rendered_page_file(dir.path(), "page-0002", 3, RenderImageFormat::Jpeg),
            None
        );
    }

    #[test]
    fn encrypted_pdf_returns_needs_password_before_zero_page_or_renderer_check() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let locked_bytes =
            std::fs::read(manifest_dir.join("../../fixtures/locked.pdf")).expect("read locked.pdf");

        let err = render_pdf_to_images(&locked_bytes, 150, "png").unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::NeedsPassword,
            "encrypted PDF must return NeedsPassword, got {:?}",
            err
        );
    }

    #[test]
    fn invalid_format_and_pre_canceled_token_fail_cleanly() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let plain_bytes =
            std::fs::read(manifest_dir.join("../../fixtures/plain.pdf")).expect("read plain.pdf");

        let fmt_err = render_pdf_to_images(&plain_bytes, 150, "bmp").unwrap_err();
        assert_eq!(fmt_err.code, ErrorCode::InvalidOption);

        let token = CancelToken::new();
        token.cancel();
        let cancel_err =
            render_pdf_to_images_cancellable(&plain_bytes, 150, "png", &token).unwrap_err();
        assert_eq!(cancel_err.code, ErrorCode::Canceled);

        // Invalid DPI must be rejected instead of silently clamped
        let dpi_err = prepare_pdf_for_render(&plain_bytes, None, 10, "png").unwrap_err();
        assert_eq!(dpi_err.code, ErrorCode::InvalidOption);
    }

    #[test]
    fn pdf_to_images_unlock_preserves_requested_format_and_dpi() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let locked_bytes =
            std::fs::read(manifest_dir.join("../../fixtures/locked.pdf")).expect("read locked.pdf");
        let owner_only = std::fs::read(manifest_dir.join("../../fixtures/owner-only.pdf"))
            .expect("read owner-only.pdf");

        // Owner-only PDF prepares automatically without a password, preserving JPEG & 220 DPI
        let owner_prep =
            prepare_pdf_for_render(&owner_only, None, 220, "jpg").expect("prepare owner-only");
        assert_eq!(owner_prep.page_count, 3);
        assert_eq!(owner_prep.dpi, 220);
        assert_eq!(owner_prep.format, RenderImageFormat::Jpeg);
        assert!(
            !crate::pdf::probe_pdf(&owner_prep.pdf_bytes)
                .unwrap()
                .is_encrypted
        );

        // Locked PDF without password -> NeedsPassword
        assert_eq!(
            prepare_pdf_for_render(&locked_bytes, None, 240, "jpeg")
                .unwrap_err()
                .code,
            ErrorCode::NeedsPassword
        );

        // Locked PDF with wrong password -> WrongPassword
        assert_eq!(
            prepare_pdf_for_render(&locked_bytes, Some("wrong-pw"), 240, "jpeg")
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );

        // Locked PDF with right password unlocks cleanly and preserves exact requested JPEG & 240 DPI
        let unlocked_prep = prepare_pdf_for_render(&locked_bytes, Some("user-pw-123"), 240, "jpeg")
            .expect("prepare locked with right password");
        assert_eq!(unlocked_prep.page_count, 3);
        assert_eq!(unlocked_prep.dpi, 240);
        assert_eq!(unlocked_prep.format, RenderImageFormat::Jpeg);
        assert!(
            !crate::pdf::probe_pdf(&unlocked_prep.pdf_bytes)
                .unwrap()
                .is_encrypted
        );
    }
}
