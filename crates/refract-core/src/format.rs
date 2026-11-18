use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Pdf,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Heic,
    Jpeg,
    Png,
    Webp,
    Pdf,
    Mp3,
    Wav,
    M4a,
    Ogg,
}

impl Format {
    pub const ALL: &'static [Format] = &[
        Format::Heic,
        Format::Jpeg,
        Format::Png,
        Format::Webp,
        Format::Pdf,
        Format::Mp3,
        Format::Wav,
        Format::M4a,
        Format::Ogg,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Format::Heic => "HEIC",
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
            Format::Pdf => "PDF",
            Format::Mp3 => "MP3",
            Format::Wav => "WAV",
            Format::M4a => "M4A",
            Format::Ogg => "OGG",
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Format::Heic | Format::Jpeg | Format::Png | Format::Webp => Kind::Image,
            Format::Pdf => Kind::Pdf,
            Format::Mp3 | Format::Wav | Format::M4a | Format::Ogg => Kind::Audio,
        }
    }

    /// Map a lowercase file extension (without dot) to a format.
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "heic" | "heif" => Some(Format::Heic),
            "jpg" | "jpeg" | "jpe" => Some(Format::Jpeg),
            "png" => Some(Format::Png),
            "webp" => Some(Format::Webp),
            "pdf" => Some(Format::Pdf),
            "mp3" => Some(Format::Mp3),
            "wav" | "wave" => Some(Format::Wav),
            "m4a" | "mp4" => Some(Format::M4a),
            "ogg" | "oga" => Some(Format::Ogg),
            _ => None,
        }
    }

    /// Map a format label ("JPEG", "png", ...) to a format.
    pub fn from_label(label: &str) -> Option<Format> {
        match label.trim().to_ascii_lowercase().as_str() {
            "heic" | "heif" => Some(Format::Heic),
            "jpg" | "jpeg" => Some(Format::Jpeg),
            "png" => Some(Format::Png),
            "webp" => Some(Format::Webp),
            "pdf" => Some(Format::Pdf),
            "mp3" => Some(Format::Mp3),
            "wav" => Some(Format::Wav),
            "m4a" => Some(Format::M4a),
            "ogg" => Some(Format::Ogg),
            _ => None,
        }
    }

    pub fn primary_extension(&self) -> &'static str {
        match self {
            Format::Heic => "heic",
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Pdf => "pdf",
            Format::Mp3 => "mp3",
            Format::Wav => "wav",
            Format::M4a => "m4a",
            Format::Ogg => "ogg",
        }
    }

    pub fn mime(&self) -> &'static str {
        match self {
            Format::Heic => "image/heic",
            Format::Jpeg => "image/jpeg",
            Format::Png => "image/png",
            Format::Webp => "image/webp",
            Format::Pdf => "application/pdf",
            Format::Mp3 => "audio/mpeg",
            Format::Wav => "audio/wav",
            Format::M4a => "audio/mp4",
            Format::Ogg => "audio/ogg",
        }
    }
}

impl Kind {
    pub const ALL: &'static [Kind] = &[Kind::Image, Kind::Pdf, Kind::Audio];

    /// Section heading used by the UI.
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Image => "Images",
            Kind::Pdf => "PDF",
            Kind::Audio => "Audio",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}
