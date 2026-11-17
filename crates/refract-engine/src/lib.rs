pub mod audio;
pub mod image;
pub mod native;

pub use audio::*;
pub use image::*;
pub use native::*;

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

}
