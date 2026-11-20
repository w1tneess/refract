//! Final checks on every output before it is saved (Stage 2.3.4).
//!
//! Mirrors `packages/core/src/verify/verify.ts`: cheap byte checks first (non-empty, type by
//! magic bytes, JPEG completeness and no EXIF, size target), then a header probe for image size,
//! PDF page count or audio duration. Any mismatch fails the job with
//! `ErrorCode::VerificationFailed`, and the caller saves nothing.

use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::{Format, Kind};
use refract_engine::{image, pdf};
use std::io::Read;
use std::path::Path;

/// Allowed difference between expected and measured audio length, in seconds. Measured drift with
/// FFmpeg 9 on the fixtures is under 0.01 s; the margin covers older encoders' frame padding.
pub const DURATION_TOLERANCE_S: f64 = 0.25;

/// Measures an audio file's length in seconds (`ffprobe` in production, injected in tests).
pub type DurationProbe<'a> = &'a dyn Fn(&Path) -> Result<f64, RefractError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedFormat {
    Exact(Format),
    /// Any image type (embedded images extracted from a PDF keep their own format).
    AnyImage,
}

/// What one output must look like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Expect {
    pub format: ExpectedFormat,
    pub dimensions: Option<(u32, u32)>,
    pub max_bytes: Option<u64>,
    pub pages: Option<u32>,
    pub duration_s: Option<f64>,
    /// JPEGs copied byte for byte out of a PDF keep whatever EXIF they had; Refract did not
    /// re-encode them, so the no-EXIF rule (meant for our own encodes) does not apply.
    pub allow_exif: bool,
}

impl Expect {
    pub fn format(format: Format) -> Self {
        Self {
            format: ExpectedFormat::Exact(format),
            dimensions: None,
            max_bytes: None,
            pages: None,
            duration_s: None,
            allow_exif: false,
        }
    }

    pub fn extracted_image() -> Self {
        Self {
            format: ExpectedFormat::AnyImage,
            allow_exif: true,
            ..Self::format(Format::Png)
        }
    }

    pub fn dimensions(mut self, width: u32, height: u32) -> Self {
        self.dimensions = Some((width, height));
        self
    }

    pub fn max_bytes(mut self, max: u64) -> Self {
        self.max_bytes = Some(max);
        self
    }

    pub fn pages(mut self, pages: u32) -> Self {
        self.pages = Some(pages);
        self
    }

    pub fn duration(mut self, seconds: f64) -> Self {
        self.duration_s = Some(seconds);
        self
    }
}

fn failed(what: impl Into<String>) -> RefractError {
    let what = what.into();
    RefractError::with_details(
        ErrorCode::VerificationFailed,
        format!("The output didn't pass the final check ({}).", what),
        what,
    )
}

fn check_format(
    detected: Option<Format>,
    expected: ExpectedFormat,
) -> Result<Format, RefractError> {
    let format = detected.ok_or_else(|| failed("it is not a known file type"))?;
    match expected {
        ExpectedFormat::Exact(want) if format != want => Err(failed(format!(
            "it is {}, not {}",
            format.label(),
            want.label()
        ))),
        ExpectedFormat::AnyImage if format.kind() != Kind::Image => {
            Err(failed(format!("it is {}, not an image", format.label())))
        }
        _ => Ok(format),
    }
}

/// Check one in-memory output (images and PDFs).
pub fn verify_bytes(bytes: &[u8], expect: &Expect) -> Result<(), RefractError> {
    if bytes.is_empty() {
        return Err(failed("it is empty"));
    }
    let format = check_format(refract_core::detect(bytes), expect.format)?;

    if format == Format::Jpeg {
        if !image::jpeg_has_complete_eoi(bytes) {
            return Err(failed("it ends early"));
        }
        if !expect.allow_exif && image::jpeg_has_exif(bytes) {
            return Err(failed("it still carries EXIF data"));
        }
    }
    if let Some(max) = expect.max_bytes {
        if bytes.len() as u64 > max {
            return Err(failed(format!(
                "it is {} KB, over the {} KB target",
                (bytes.len() as u64).div_ceil(1024),
                max / 1024
            )));
        }
    }

    match format.kind() {
        Kind::Image => {
            let (w, h) = image::probe_dimensions(bytes)
                .map_err(|_| failed("it could not be opened again"))?;
            if w == 0 || h == 0 {
                return Err(failed("it has no pixels"));
            }
            if let Some((ew, eh)) = expect.dimensions {
                if (w, h) != (ew, eh) {
                    return Err(failed(format!("it is {w}×{h}, expected {ew}×{eh}")));
                }
            }
        }
        Kind::Pdf => {
            let info = pdf::probe_pdf(bytes).map_err(|_| failed("it could not be opened again"))?;
            if info.requires_password {
                return Err(failed("it is still password-protected"));
            }
            if info.page_count == 0 {
                return Err(failed("it has no pages"));
            }
            if let Some(want) = expect.pages {
                if info.page_count != want {
                    return Err(failed(format!(
                        "it has {} pages, expected {}",
                        info.page_count, want
                    )));
                }
            }
        }
        Kind::Audio => {}
    }
    Ok(())
}

/// Check a set of outputs: the count first, then every file.
pub fn verify_all(outputs: &[&[u8]], expects: &[Expect]) -> Result<(), RefractError> {
    if outputs.len() != expects.len() {
        return Err(failed(format!(
            "{} files came out, expected {}",
            outputs.len(),
            expects.len()
        )));
    }
    for (i, (bytes, expect)) in outputs.iter().zip(expects).enumerate() {
        verify_bytes(bytes, expect).map_err(|err| {
            if outputs.len() == 1 {
                err
            } else {
                let what = format!(
                    "file {} of {}: {}",
                    i + 1,
                    outputs.len(),
                    err.details.unwrap_or_default()
                );
                failed(what)
            }
        })?;
    }
    Ok(())
}

/// Check an audio file written by FFmpeg (before it is moved into place).
///
/// `probe_duration` is `None` when `ffprobe` is unavailable (convert-audio does not require it);
/// the duration check is then skipped and the type checks still apply.
pub fn verify_audio_file(
    path: &Path,
    expect: &Expect,
    probe_duration: Option<DurationProbe<'_>>,
) -> Result<(), RefractError> {
    let mut header = Vec::with_capacity(64);
    std::fs::File::open(path)
        .and_then(|f| f.take(64).read_to_end(&mut header))
        .map_err(|_| failed("it could not be opened again"))?;
    if header.is_empty() {
        return Err(failed("it is empty"));
    }
    check_format(refract_core::detect(&header), expect.format)?;

    if let (Some(want), Some(probe)) = (expect.duration_s, probe_duration) {
        let got = probe(path).map_err(|e| {
            if e.code.is_cancellation() {
                e
            } else {
                failed("its length could not be measured")
            }
        })?;
        if (got - want).abs() > DURATION_TOLERANCE_S {
            return Err(failed(format!(
                "it is {:.2} s long, expected {:.2} s",
                got, want
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use refract_core::format::Format;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures")
                .join(name),
        )
        .unwrap()
    }

    fn detail(err: RefractError) -> String {
        assert_eq!(err.code, ErrorCode::VerificationFailed);
        err.details.unwrap()
    }

    fn jpeg_output() -> Vec<u8> {
        image::convert_image(&fixture("plain.png"), Format::Jpeg, Some(85)).unwrap()
    }

    #[test]
    fn valid_outputs_pass() {
        verify_bytes(
            &jpeg_output(),
            &Expect::format(Format::Jpeg).dimensions(400, 300),
        )
        .unwrap();
        verify_bytes(&fixture("plain.pdf"), &Expect::format(Format::Pdf).pages(3)).unwrap();
        verify_bytes(&fixture("rotated.jpg"), &Expect::extracted_image()).unwrap();
    }

    #[test]
    fn empty_output_fails() {
        let err = verify_bytes(&[], &Expect::format(Format::Png)).unwrap_err();
        assert_eq!(detail(err), "it is empty");
    }

    #[test]
    fn wrong_format_fails() {
        let err = verify_bytes(&fixture("plain.png"), &Expect::format(Format::Webp)).unwrap_err();
        assert!(detail(err).contains("not WebP"));
        let err = verify_bytes(&fixture("plain.pdf"), &Expect::extracted_image()).unwrap_err();
        assert!(detail(err).contains("not an image"));
        let err = verify_bytes(b"hello", &Expect::format(Format::Png)).unwrap_err();
        assert_eq!(detail(err), "it is not a known file type");
    }

    #[test]
    fn truncated_jpeg_fails() {
        let out = jpeg_output();
        let err = verify_bytes(&out[..out.len() - 2], &Expect::format(Format::Jpeg)).unwrap_err();
        assert_eq!(detail(err), "it ends early");
    }

    #[test]
    fn jpeg_with_exif_fails_unless_extracted() {
        let err = verify_bytes(&fixture("rotated.jpg"), &Expect::format(Format::Jpeg)).unwrap_err();
        assert_eq!(detail(err), "it still carries EXIF data");
    }

    #[test]
    fn over_target_size_fails() {
        let out = jpeg_output();
        let err = verify_bytes(&out, &Expect::format(Format::Jpeg).max_bytes(1024)).unwrap_err();
        assert!(detail(err).contains("over the 1 KB target"));
    }

    #[test]
    fn wrong_dimensions_fail() {
        let err = verify_bytes(
            &fixture("plain.png"),
            &Expect::format(Format::Png).dimensions(200, 150),
        )
        .unwrap_err();
        assert_eq!(detail(err), "it is 400×300, expected 200×150");
    }

    #[test]
    fn wrong_page_count_fails() {
        let err =
            verify_bytes(&fixture("plain.pdf"), &Expect::format(Format::Pdf).pages(2)).unwrap_err();
        assert_eq!(detail(err), "it has 3 pages, expected 2");
    }

    #[test]
    fn still_locked_pdf_fails() {
        let err = verify_bytes(&fixture("locked.pdf"), &Expect::format(Format::Pdf)).unwrap_err();
        assert_eq!(detail(err), "it is still password-protected");
    }

    #[test]
    fn output_count_and_position_are_reported() {
        let png = fixture("plain.png");
        let err = verify_all(
            &[&png],
            &[Expect::format(Format::Png), Expect::format(Format::Png)],
        )
        .unwrap_err();
        assert_eq!(detail(err), "1 files came out, expected 2");
        let err = verify_all(
            &[&png, b"bad"],
            &[Expect::format(Format::Png), Expect::format(Format::Png)],
        )
        .unwrap_err();
        assert_eq!(detail(err), "file 2 of 2: it is not a known file type");
    }

    #[test]
    fn a_canceled_length_probe_reports_cancel_not_a_failed_check() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("out.wav");
        std::fs::write(&wav, fixture("sound.wav")).unwrap();
        let canceled = |_: &Path| Err(RefractError::canceled());
        let err = verify_audio_file(
            &wav,
            &Expect::format(Format::Wav).duration(3.0),
            Some(&canceled),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Canceled);
    }

    #[test]
    fn audio_type_and_duration_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("out.wav");
        std::fs::write(&wav, fixture("sound.wav")).unwrap();
        let three_s = |_: &Path| Ok(3.0);

        verify_audio_file(
            &wav,
            &Expect::format(Format::Wav).duration(3.1),
            Some(&three_s),
        )
        .unwrap();
        // ffprobe missing: duration skipped, type still checked.
        verify_audio_file(&wav, &Expect::format(Format::Wav).duration(9.0), None).unwrap();

        let err = verify_audio_file(
            &wav,
            &Expect::format(Format::Wav).duration(2.5),
            Some(&three_s),
        )
        .unwrap_err();
        assert_eq!(detail(err), "it is 3.00 s long, expected 2.50 s");
        let err = verify_audio_file(&wav, &Expect::format(Format::Mp3), None).unwrap_err();
        assert!(detail(err).contains("not MP3"));

        let empty = dir.path().join("empty.mp3");
        std::fs::write(&empty, b"").unwrap();
        let err = verify_audio_file(&empty, &Expect::format(Format::Mp3), None).unwrap_err();
        assert_eq!(detail(err), "it is empty");
    }
}
