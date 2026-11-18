use crate::error::{ErrorCode, RefractError};
use crate::generated_manifest::{Platform, ToolId, DESKTOP_MEMORY_BUDGET_MB, WEB_MEMORY_BUDGET_MB};

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformLimits {
    pub max_mb: u64,
    pub max_files: usize,
    pub max_pages: Option<u32>,
    pub max_pixels: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LimitCheckInput {
    pub files: Option<usize>,
    pub bytes: Option<u64>,
    pub pixels: Option<u64>,
    pub pages: Option<u32>,
}

/// Return manifest-backed limits for a tool on the given platform.
pub fn limits_for_tool(tool: ToolId, platform: Platform) -> PlatformLimits {
    let spec = tool.limits();
    match platform {
        Platform::Web => PlatformLimits {
            max_mb: spec.web_max_mb,
            max_files: spec.web_max_files,
            max_pages: spec.web_max_pages,
            max_pixels: spec.web_max_pixels,
        },
        Platform::Desktop => PlatformLimits {
            max_mb: spec.desktop_max_mb,
            max_files: spec.desktop_max_files,
            max_pages: spec.desktop_max_pages,
            max_pixels: spec.desktop_max_pixels,
        },
    }
}

/// Return manifest-backed per-job memory budget in bytes for the given platform.
pub fn memory_budget_bytes(platform: Platform) -> u64 {
    match platform {
        Platform::Web => WEB_MEMORY_BUDGET_MB * MIB,
        Platform::Desktop => DESKTOP_MEMORY_BUDGET_MB * MIB,
    }
}

fn ceil_mb(bytes: u64) -> u64 {
    bytes.div_ceil(MIB)
}

/// Validate input counts, file size, pixel count, and page count against manifest limits.
pub fn check_limits(limits: &PlatformLimits, input: &LimitCheckInput) -> Result<(), RefractError> {
    if let Some(files) = input.files {
        if files > limits.max_files {
            return Err(RefractError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "This job has {} files and the limit is {}. Split them into smaller groups.",
                    files, limits.max_files
                ),
            ));
        }
    }

    if let Some(bytes) = input.bytes {
        let max_bytes = limits.max_mb.saturating_mul(MIB);
        if bytes > max_bytes {
            return Err(RefractError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "The file is {} MB and the limit here is {} MB. Try a smaller file.",
                    ceil_mb(bytes),
                    limits.max_mb
                ),
            ));
        }
    }

    if let (Some(max_pixels), Some(pixels)) = (limits.max_pixels, input.pixels) {
        if pixels > max_pixels {
            let mp = |n: u64| (n + 500_000) / 1_000_000;
            return Err(RefractError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "The image is {} megapixels and the limit here is {}. Try a smaller image.",
                    mp(pixels),
                    mp(max_pixels)
                ),
            ));
        }
    }

    if let (Some(max_pages), Some(pages)) = (limits.max_pages, input.pages) {
        if pages > max_pages {
            return Err(RefractError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "The document has {} pages and the limit here is {}. Split it into smaller parts first.",
                    pages, max_pages
                ),
            ));
        }
    }

    Ok(())
}

/// Validate estimated peak memory usage against the platform memory budget.
pub fn check_memory(estimate_bytes: u64, platform: Platform) -> Result<(), RefractError> {
    let budget = memory_budget_bytes(platform);
    if estimate_bytes > budget {
        return Err(RefractError::new(
            ErrorCode::LimitExceeded,
            format!(
                "This job needs about {} MB of memory and the budget here is {} MB.",
                ceil_mb(estimate_bytes),
                ceil_mb(budget)
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_file_count_size_pixels_and_pages_against_manifest() {
        let img = limits_for_tool(ToolId::ConvertImage, Platform::Desktop);
        let pdf = limits_for_tool(ToolId::MergePdf, Platform::Desktop);

        assert!(check_limits(
            &img,
            &LimitCheckInput {
                files: Some(img.max_files),
                bytes: Some(img.max_mb * MIB),
                pixels: img.max_pixels,
                pages: None,
            }
        )
        .is_ok());

        assert_eq!(
            check_limits(
                &img,
                &LimitCheckInput {
                    files: Some(img.max_files + 1),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::LimitExceeded
        );

        assert_eq!(
            check_limits(
                &img,
                &LimitCheckInput {
                    bytes: Some(img.max_mb * MIB + 1),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::LimitExceeded
        );

        assert_eq!(
            check_limits(
                &img,
                &LimitCheckInput {
                    pixels: Some(img.max_pixels.unwrap() + 1),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::LimitExceeded
        );

        assert_eq!(
            check_limits(
                &pdf,
                &LimitCheckInput {
                    pages: Some(pdf.max_pages.unwrap() + 1),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::LimitExceeded
        );
    }

    #[test]
    fn memory_budget_checks_web_and_desktop() {
        let desktop_budget = memory_budget_bytes(Platform::Desktop);
        assert!(check_memory(desktop_budget, Platform::Desktop).is_ok());
        assert_eq!(
            check_memory(desktop_budget + 1, Platform::Desktop)
                .unwrap_err()
                .code,
            ErrorCode::LimitExceeded
        );
    }
}
