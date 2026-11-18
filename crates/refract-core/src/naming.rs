use crate::error::{ErrorCode, RefractError};
use crate::generated_manifest::ToolId;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const CONVERTED_TEMPLATE: &str = "{name} (converted)";

/// Extract the file stem matching `@refract/core`'s `outputName`/`stemOf` semantics:
/// - `IMG_0001.HEIC` -> `IMG_0001`
/// - `trip.v2.heic` -> `trip.v2`
/// - `photo` -> `photo`
/// - `.hidden` -> `.hidden` (leading dot is not treated as an extension)
pub fn stem_of(input_name: &str) -> &str {
    match input_name.rfind('.') {
        Some(dot) if dot > 0 => &input_name[..dot],
        _ => input_name,
    }
}

/// Build a canonical output file name from `template` and deduplicate case-insensitively
/// against `taken`, adding `" (2)"`, `" (3)"`, ... matching `packages/core/src/presets/naming.ts`.
pub fn output_name(
    template: &str,
    input_name: &str,
    ext: &str,
    taken: &mut HashSet<String>,
) -> String {
    let stem = stem_of(input_name);
    let base = template.replace("{name}", stem);
    let ext = ext.trim_start_matches('.');

    let format_candidate = |suffix: Option<usize>| -> String {
        let with_suffix = match suffix {
            Some(n) => format!("{} ({})", base, n),
            None => base.clone(),
        };
        if ext.is_empty() {
            with_suffix
        } else {
            format!("{}.{}", with_suffix, ext)
        }
    };

    let mut name = format_candidate(None);
    let mut n = 2usize;
    while taken.contains(&name.to_lowercase()) {
        name = format_candidate(Some(n));
        n += 1;
    }
    taken.insert(name.to_lowercase());
    name
}

/// Build the canonical output file name for a tool using its manifest `output_template`.
pub fn output_name_for_tool(
    tool: ToolId,
    input_name: &str,
    ext: &str,
    taken: &mut HashSet<String>,
) -> String {
    let template = tool.manifest().output_template;
    output_name(template, input_name, ext, taken)
}

/// Return a suffixed variant of `filename` (`"name (2).ext"`, `"name (3).ext"`, ...)
/// preserving leading-dot names and extensionless names.
pub fn suffixed_filename(filename: &str, suffix_number: usize) -> String {
    match filename.rfind('.') {
        Some(dot) if dot > 0 => {
            let stem = &filename[..dot];
            let ext = &filename[dot + 1..];
            format!("{} ({}).{}", stem, suffix_number, ext)
        }
        _ => format!("{} ({})", filename, suffix_number),
    }
}

/// Resolve a non-existing destination path inside `dir` for `desired_filename`,
/// using `" (2)"`, `" (3)"`, ... up to `max_attempts`.
///
/// Never returns an already-existing path; returns an error if suffix generation is exhausted.
pub fn resolve_unique_path(
    dir: &Path,
    desired_filename: &str,
    max_attempts: usize,
) -> Result<PathBuf, RefractError> {
    if max_attempts == 0 {
        return Err(RefractError::new(
            ErrorCode::EngineFailed,
            "Invalid unique path retry limit (0).",
        ));
    }

    let first = dir.join(desired_filename);
    if !first.exists() {
        return Ok(first);
    }

    for n in 2..=max_attempts + 1 {
        let candidate = dir.join(suffixed_filename(desired_filename, n));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(RefractError::new(
        ErrorCode::EngineFailed,
        format!(
            "Could not find an unused output filename for '{}' after {} attempts.",
            desired_filename, max_attempts
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_typescript_output_name_contract() {
        assert_eq!(
            output_name(
                CONVERTED_TEMPLATE,
                "IMG_0001.HEIC",
                "jpg",
                &mut HashSet::new()
            ),
            "IMG_0001 (converted).jpg"
        );

        // Case-insensitive clash handling: (2), (3)
        let mut taken = HashSet::new();
        assert_eq!(
            output_name(CONVERTED_TEMPLATE, "IMG.HEIC", "jpg", &mut taken),
            "IMG (converted).jpg"
        );
        assert_eq!(
            output_name(CONVERTED_TEMPLATE, "IMG.HEIC", "jpg", &mut taken),
            "IMG (converted) (2).jpg"
        );
        assert_eq!(
            output_name(CONVERTED_TEMPLATE, "img.heic", "jpg", &mut taken),
            "img (converted) (3).jpg"
        );
    }

    #[test]
    fn preserves_unicode_emoji_multi_dots_and_hidden_files() {
        assert_eq!(
            output_name(
                CONVERTED_TEMPLATE,
                "ছবি 😀.heic",
                "jpg",
                &mut HashSet::new()
            ),
            "ছবি 😀 (converted).jpg"
        );
        assert_eq!(
            output_name(
                CONVERTED_TEMPLATE,
                "trip.v2.heic",
                "jpg",
                &mut HashSet::new()
            ),
            "trip.v2 (converted).jpg"
        );
        assert_eq!(
            output_name(CONVERTED_TEMPLATE, "photo", "jpg", &mut HashSet::new()),
            "photo (converted).jpg"
        );
        assert_eq!(
            output_name("{name}_pages", ".hidden", "zip", &mut HashSet::new()),
            ".hidden_pages.zip"
        );
    }

    #[test]
    fn tool_manifest_templates_match_expected_names() {
        let mut taken = HashSet::new();
        assert_eq!(
            output_name_for_tool(ToolId::ConvertImage, "photo.png", "jpg", &mut taken),
            "photo (converted).jpg"
        );
        assert_eq!(
            output_name_for_tool(ToolId::CompressImage, "photo.png", "jpg", &mut taken),
            "photo_compressed.jpg"
        );
        assert_eq!(
            output_name_for_tool(ToolId::ResizeImage, "photo.png", "png", &mut taken),
            "photo_resized.png"
        );
        assert_eq!(
            output_name_for_tool(ToolId::MergePdf, "doc.pdf", "pdf", &mut taken),
            "doc_merged.pdf"
        );
        assert_eq!(
            output_name_for_tool(ToolId::ReorderPdf, "doc.pdf", "pdf", &mut taken),
            "doc_reordered.pdf"
        );
        assert_eq!(
            output_name_for_tool(ToolId::CompressPdf, "doc.pdf", "pdf", &mut taken),
            "doc_compressed.pdf"
        );
        assert_eq!(
            output_name_for_tool(ToolId::TrimAudio, "sound.wav", "wav", &mut taken),
            "sound_trimmed.wav"
        );
    }

    #[test]
    fn resolve_unique_path_suffixes_and_never_overwrites_when_exhausted() {
        let dir = tempfile::tempdir().unwrap();
        let base = "photo (converted).jpg";

        let p1 = resolve_unique_path(dir.path(), base, 2).unwrap();
        assert_eq!(p1.file_name().unwrap(), "photo (converted).jpg");
        std::fs::write(&p1, b"1").unwrap();

        let p2 = resolve_unique_path(dir.path(), base, 2).unwrap();
        assert_eq!(p2.file_name().unwrap(), "photo (converted) (2).jpg");
        std::fs::write(&p2, b"2").unwrap();

        let p3 = resolve_unique_path(dir.path(), base, 2).unwrap();
        assert_eq!(p3.file_name().unwrap(), "photo (converted) (3).jpg");
        std::fs::write(&p3, b"3").unwrap();

        // Exhausted: must error instead of returning an existing file path
        let err = resolve_unique_path(dir.path(), base, 2).unwrap_err();
        assert_eq!(err.code, ErrorCode::EngineFailed);
    }
}
