pub mod audio;
pub mod image;
pub mod native;
pub mod pdf;
pub mod render;

pub use audio::*;
pub use image::*;
pub use native::*;
pub use pdf::*;
pub use render::*;

#[cfg(test)]
mod tests {
    use super::*;
    use refract_core::format::Format;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join("../../fixtures").join(name)
    }

    #[test]
    fn test_image_probe_dimensions() {
        let plain_png = std::fs::read(fixture_path("plain.png")).expect("Read plain.png");
        let (w, h) = image::probe_dimensions(&plain_png).expect("Probe dimensions");
        assert_eq!((w, h), (400, 300));
    }

    #[test]
    fn test_image_convert_png_to_jpeg() {
        let plain_png = std::fs::read(fixture_path("plain.png")).expect("Read plain.png");
        let jpg_bytes = image::convert_image(&plain_png, Format::Jpeg, Some(80)).expect("Convert to JPEG");
        assert_eq!(&jpg_bytes[0..3], &[0xFF, 0xD8, 0xFF]);
        let (w, h) = image::probe_dimensions(&jpg_bytes).expect("Probe converted jpg");
        assert_eq!((w, h), (400, 300));
    }

    #[test]
    fn test_image_compress_target_kb() {
        let plain_png = std::fs::read(fixture_path("plain.png")).expect("Read plain.png");
        let compressed = image::compress_image(&plain_png, 5).expect("Compress image");
        assert!(compressed.len() <= 6 * 1024);
    }

    #[test]
    fn test_image_resize_pixels() {
        let plain_png = std::fs::read(fixture_path("plain.png")).expect("Read plain.png");
        // Resize to width 200, height 0 (should maintain 4:3 aspect ratio -> 200x150)
        let resized = image::resize_image(&plain_png, 200, 0).expect("Resize image");
        let (w, h) = image::probe_dimensions(&resized).expect("Probe resized");
        assert_eq!((w, h), (200, 150));
    }

    #[test]
    fn test_audio_parse_timestamp() {
        assert_eq!(audio::parse_timestamp("90").unwrap(), 90.0);
        assert_eq!(audio::parse_timestamp("1:30").unwrap(), 90.0);
        assert_eq!(audio::parse_timestamp("01:30.5").unwrap(), 90.5);
        assert_eq!(audio::parse_timestamp("01:00:00").unwrap(), 3600.0);
    }

    #[test]
    fn test_audio_convert_with_ffmpeg() {
        if !audio::is_ffmpeg_available() {
            eprintln!("Skipping audio convert test: ffmpeg not found");
            return;
        }

        let wav_path = fixture_path("sound.wav");
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let out_mp3 = temp_dir.path().join("converted.mp3");

        audio::convert_audio(&wav_path, &out_mp3, Format::Mp3, Some(192)).expect("Convert WAV to MP3");
        assert!(out_mp3.exists());
        let mp3_bytes = std::fs::read(&out_mp3).expect("Read converted mp3");
        assert!(!mp3_bytes.is_empty());
        assert_eq!(refract_core::detect(&mp3_bytes), Some(Format::Mp3));
    }

    #[test]
    fn test_audio_trim_with_ffmpeg() {
        if !audio::is_ffmpeg_available() {
            eprintln!("Skipping audio trim test: ffmpeg not found");
            return;
        }

        let wav_path = fixture_path("sound.wav");
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let out_wav = temp_dir.path().join("trimmed.wav");

        audio::trim_audio(&wav_path, &out_wav, Some("0.2"), Some("0.6")).expect("Trim WAV");
        assert!(out_wav.exists());
        let trimmed_bytes = std::fs::read(&out_wav).expect("Read trimmed wav");
        assert!(!trimmed_bytes.is_empty());
        assert_eq!(refract_core::detect(&trimmed_bytes), Some(Format::Wav));
    }

    #[test]
    fn test_pdf_probe_info() {
        let plain_pdf = std::fs::read(fixture_path("plain.pdf")).expect("Read plain.pdf");
        let info = pdf::probe_pdf(&plain_pdf).expect("Probe PDF");
        assert_eq!(info.page_count, 3);
        assert!(!info.is_encrypted);

        let locked_pdf = std::fs::read(fixture_path("locked.pdf")).expect("Read locked.pdf");
        let locked_info = pdf::probe_pdf(&locked_pdf).expect("Probe locked PDF");
        assert!(locked_info.is_encrypted);
    }

    #[test]
    fn test_pdf_parse_ranges_and_order() {
        let ranges = pdf::parse_ranges("1-2, 3", 3).expect("Parse ranges");
        assert_eq!(ranges, vec![vec![1, 2], vec![3]]);

        let order = pdf::parse_order("3, 1, 2", 3).expect("Parse order");
        assert_eq!(order, vec![3, 1, 2]);

        // Duplicate page should fail
        assert!(pdf::parse_order("3, 3, 1", 3).is_err());
        // Wrong count should fail
        assert!(pdf::parse_order("1, 2", 3).is_err());
    }

    #[test]
    fn test_pdf_compress() {
        let plain_pdf = std::fs::read(fixture_path("plain.pdf")).expect("Read plain.pdf");
        let compressed = pdf::compress_pdf(&plain_pdf).expect("Compress PDF");
        assert!(!compressed.is_empty());
        assert_eq!(refract_core::detect(&compressed), Some(Format::Pdf));
    }

    #[test]
    fn test_pdf_merge() {
        let plain_pdf = std::fs::read(fixture_path("plain.pdf")).expect("Read plain.pdf");
        let merged = pdf::merge_pdfs(&[&plain_pdf, &plain_pdf]).expect("Merge PDFs");
        assert!(!merged.is_empty());
        assert_eq!(refract_core::detect(&merged), Some(Format::Pdf));
    }

    #[test]
    fn test_pdf_reorder() {
        let plain_pdf = std::fs::read(fixture_path("plain.pdf")).expect("Read plain.pdf");
        let original_doc = lopdf::Document::load_mem(&plain_pdf).expect("Load plain.pdf");
        let orig_pages = original_doc.get_pages();
        let p1 = orig_pages[&1];
        let p2 = orig_pages[&2];
        let p3 = orig_pages[&3];

        let reordered = pdf::reorder_pdf(&plain_pdf, "3, 1, 2").expect("Reorder PDF");
        assert!(!reordered.is_empty());
        assert_eq!(refract_core::detect(&reordered), Some(Format::Pdf));

        let reordered_doc = lopdf::Document::load_mem(&reordered).expect("Load reordered PDF");
        let new_pages = reordered_doc.get_pages();
        assert_eq!(new_pages.len(), 3);
        // Verify page order: new page 1 is old page 3, new page 2 is old page 1, new page 3 is old page 2
        assert_eq!(new_pages[&1], p3);
        assert_eq!(new_pages[&2], p1);
        assert_eq!(new_pages[&3], p2);
    }

    #[test]
    fn test_pdf_split() {
        let plain_pdf = std::fs::read(fixture_path("plain.pdf")).expect("Read plain.pdf");
        let parts = pdf::split_pdf(&plain_pdf, "1-2, 3").expect("Split PDF");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "p1-2");
        assert_eq!(parts[1].0, "p3");
        let info1 = pdf::probe_pdf(&parts[0].1).expect("Probe part 1");
        assert_eq!(info1.page_count, 2);
        let info2 = pdf::probe_pdf(&parts[1].1).expect("Probe part 2");
        assert_eq!(info2.page_count, 1);
    }
}
