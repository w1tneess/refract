pub mod detect;
pub mod error;
pub mod format;
pub mod generated_manifest;
pub mod job;
pub mod tools;

pub use detect::detect;
pub use error::{ErrorCode, RefractError};
pub use format::{Format, Kind};
pub use generated_manifest::{
    limits_for_key, manifest_for, LimitsSpec, ManifestOptionSpec, OptionChoice, OptionDefault,
    OptionShowIf, OptionType, Platform, PlatformAvailability, ToolEngine, ToolManifestEntry,
    FORMAT_SUPPORT, LIMITS_MANIFEST, MANIFEST_VERSION, TOOL_MANIFEST,
};
pub use job::{Job, JobState};
pub use tools::ToolId;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_jpeg() {
        let bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect(&bytes), Some(Format::Jpeg));
    }

    #[test]
    fn test_detect_png() {
        let bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect(&bytes), Some(Format::Png));
    }

    #[test]
    fn test_detect_pdf() {
        let bytes = b"%PDF-1.7\n%some content";
        assert_eq!(detect(bytes), Some(Format::Pdf));
    }

    #[test]
    fn test_detect_webp() {
        let mut bytes = b"RIFF....WEBPVP8 ".to_vec();
        bytes[4..8].copy_from_slice(&12u32.to_le_bytes());
        assert_eq!(detect(&bytes), Some(Format::Webp));
    }

    #[test]
    fn test_detect_wav() {
        let mut bytes = b"RIFF....WAVEfmt ".to_vec();
        bytes[4..8].copy_from_slice(&12u32.to_le_bytes());
        assert_eq!(detect(&bytes), Some(Format::Wav));
    }

    #[test]
    fn test_detect_ogg() {
        let bytes = b"OggS\x00\x02";
        assert_eq!(detect(bytes), Some(Format::Ogg));
    }

    #[test]
    fn test_detect_heic() {
        let mut bytes = vec![0; 12];
        bytes[0..4].copy_from_slice(&12u32.to_be_bytes());
        bytes[4..8].copy_from_slice(b"ftyp");
        bytes[8..12].copy_from_slice(b"heic");
        assert_eq!(detect(&bytes), Some(Format::Heic));
    }

    #[test]
    fn test_detect_mp3_vs_adts_aac() {
        // MPEG-1 Layer III frame sync -> MP3
        assert_eq!(detect(&[0xFF, 0xFB, 0x90, 0x00]), Some(Format::Mp3));
        // ADTS AAC (layer bits == 0) must NOT be misdetected as MP3
        assert_eq!(detect(&[0xFF, 0xF1, 0x50, 0x80]), None);
    }

    #[test]
    fn test_detect_m4a_brands_and_bounded_ftyp() {
        let mut bytes = vec![0; 24];
        bytes[0..4].copy_from_slice(&16u32.to_be_bytes()); // ftyp box ends at 16
        bytes[4..8].copy_from_slice(b"ftyp");
        bytes[8..12].copy_from_slice(b"avif");
        // "heic" placed AFTER the ftyp box boundary must not be scanned
        bytes[16..20].copy_from_slice(b"heic");
        assert_eq!(detect(&bytes), None);

        let mut m4b = vec![0; 12];
        m4b[0..4].copy_from_slice(&12u32.to_be_bytes());
        m4b[4..8].copy_from_slice(b"ftyp");
        m4b[8..12].copy_from_slice(b"M4B ");
        assert_eq!(detect(&m4b), Some(Format::M4a));
    }
}
