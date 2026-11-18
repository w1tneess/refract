use refract_core::error::{ErrorCode, RefractError};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;
use which::which;

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
            return Some(Renderer {
                kind: *kind,
                path,
            });
        }
    }
    None
}

/// Check if any PDF rasterizer is available.
pub fn has_renderer() -> bool {
    detect_renderer().is_some()
}

/// Render a PDF page to an image file using the detected rasterizer.
///
/// Returns the path to the rendered image file.
fn render_page(
    renderer: &Renderer,
    pdf_path: &Path,
    page_number: u32,
    dpi: u32,
    output_dir: &Path,
    format: &str, // "png" or "jpg"
) -> Result<PathBuf, RefractError> {
    let output_name = format!("page-{:04}.{}", page_number, format);
    let output_path = output_dir.join(&output_name);

    let mut cmd = Command::new(&renderer.path);
    cmd.stderr(Stdio::piped());

    match renderer.kind {
        RendererKind::Mutool => {
            // mutool draw -o output.png -r DPI input.pdf page
            cmd.arg("draw")
                .arg("-o")
                .arg(output_path.as_os_str())
                .arg("-r")
                .arg(dpi.to_string())
                .arg(pdf_path.as_os_str())
                .arg(page_number.to_string());
        }
        RendererKind::Pdftoppm => {
            // pdftoppm -png -r DPI -f N -l N input.pdf output_prefix
            // Note: pdftoppm writes output_prefix-N.png
            let prefix = output_dir.join(format!("page-{:04}", page_number));
            cmd.arg("-png")
                .arg("-r")
                .arg(dpi.to_string())
                .arg("-f")
                .arg(page_number.to_string())
                .arg("-l")
                .arg(page_number.to_string())
                .arg(pdf_path.as_os_str())
                .arg(prefix.as_os_str());
        }
        RendererKind::Pdftocairo => {
            // pdftocairo -png -r DPI -f N -l N input.pdf output_prefix
            let prefix = output_dir.join(format!("page-{:04}", page_number));
            cmd.arg("-png")
                .arg("-r")
                .arg(dpi.to_string())
                .arg("-f")
                .arg(page_number.to_string())
                .arg("-l")
                .arg(page_number.to_string())
                .arg(pdf_path.as_os_str())
                .arg(prefix.as_os_str());
        }
        RendererKind::Gs => {
            // gs -sDEVICE=png16m -dPDFFitPage -rDPI -dFirstPage=N -dLastPage=N -o output.png input.pdf
            let device = if format == "jpg" { "jpeg" } else { "png16m" };
            cmd.arg("-sDEVICE=").arg(device)
                .arg("-dPDFFitPage")
                .arg("-r").arg(dpi.to_string())
                .arg("-dFirstPage=").arg(page_number.to_string())
                .arg("-dLastPage=").arg(page_number.to_string())
                .arg("-o").arg(output_path.as_os_str())
                .arg("-q")
                .arg(pdf_path.as_os_str());
        }
    }

    let status = cmd.status().map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            format!("Failed to start PDF rasterizer ({})", renderer_name(renderer.kind)),
            e.to_string(),
        )
    })?;

    if !status.success() {
        let output = cmd.output().unwrap_or_else(|_| std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
        return Err(RefractError::with_details(
            ErrorCode::EngineFailed,
            format!("PDF rasterizer ({}) failed", renderer_name(renderer.kind)),
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    // For pdftoppm/pdftocairo, the output file has -page_number appended
    let output_path_clone = output_path.clone();
    let actual_path = match renderer.kind {
        RendererKind::Pdftoppm | RendererKind::Pdftocairo => {
            output_dir.join(format!("page-{:04}-{}.{}", page_number, page_number, format))
        }
        _ => output_path,
    };

    if actual_path.exists() {
        Ok(actual_path)
    } else {
        // Try the expected name
        if output_path_clone.exists() {
            Ok(output_path_clone)
        } else {
            Err(RefractError::new(
                ErrorCode::EngineFailed,
                format!("Rasterizer produced no output file for page {}", page_number),
            ))
        }
    }
}

/// Render all pages of a PDF to image files.
///
/// Returns a vector of (page_number, image_bytes, extension).
pub fn render_pdf_to_images(
    pdf_bytes: &[u8],
    dpi: u32,
    format: &str, // "png" or "jpg"
) -> Result<Vec<(u32, Vec<u8>, &'static str)>, RefractError> {
    let renderer = detect_renderer().ok_or_else(|| {
        RefractError::new(
            ErrorCode::EngineFailed,
            "No PDF rasterizer found. Install mutool, pdftoppm, pdftocairo, or Ghostscript (gs).",
        )
    })?;

    if pdf_bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "PDF file is empty (0 bytes)"));
    }

    let dpi = dpi.clamp(72, 600);
    let format = if format == "jpg" { "jpg" } else { "png" };

    let temp_dir = tempdir().map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to create temp directory", e.to_string())
    })?;
    let pdf_path = temp_dir.path().join("input.pdf");
    std::fs::write(&pdf_path, pdf_bytes).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to write PDF to temp file", e.to_string())
    })?;

    // First, probe page count using the renderer or lopdf
    let page_count = crate::pdf::probe_pdf(pdf_bytes)?.page_count;
    if page_count == 0 {
        return Err(RefractError::new(ErrorCode::DamagedFile, "PDF has no pages"));
    }

    let mut results = Vec::new();

    for page in 1..=page_count {
        let img_path = render_page(&renderer, &pdf_path, page, dpi, temp_dir.path(), format)?;
        let img_bytes = std::fs::read(&img_path).map_err(|e| {
            RefractError::with_details(ErrorCode::EngineFailed, "Failed to read rendered image", e.to_string())
        })?;
        results.push((page, img_bytes, format));
    }

    Ok(results)
}

/// Human-readable name for a renderer kind.
fn renderer_name(kind: RendererKind) -> &'static str {
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

    #[test]
    fn renderer_name_returns_correct_strings() {
        assert_eq!(renderer_name(RendererKind::Mutool), "mutool");
        assert_eq!(renderer_name(RendererKind::Pdftoppm), "pdftoppm");
        assert_eq!(renderer_name(RendererKind::Pdftocairo), "pdftocairo");
        assert_eq!(renderer_name(RendererKind::Gs), "Ghostscript (gs)");
    }
}