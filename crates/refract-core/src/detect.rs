use crate::format::Format;

/// Detect format from byte headers (magic numbers).
pub fn detect(bytes: &[u8]) -> Option<Format> {
    if bytes.len() < 4 {
        return None;
    }

    // JPEG: FF D8 FF
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Format::Png);
    }

    // PDF: %PDF- in first 1024 bytes
    let check_len = bytes.len().min(1024);
    if let Some(_) = bytes[..check_len].windows(5).position(|w| w == b"%PDF-") {
        return Some(Format::Pdf);
    }

    // RIFF containers: WEBP or WAV
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        if &bytes[8..12] == b"WEBP" {
            return Some(Format::Webp);
        }
        if &bytes[8..12] == b"WAVE" {
            return Some(Format::Wav);
        }
    }

    // Ogg: OggS
    if bytes.starts_with(b"OggS") {
        return Some(Format::Ogg);
    }

    // MP4/M4A/HEIF box: [4 bytes size][ftyp][4 bytes major brand]
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        match brand {
            b"M4A " | b"mp42" | b"isom" => return Some(Format::M4a),
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => return Some(Format::Heic),
            _ => {
                // Check compatible brands if present
                let mut offset = 16;
                while offset + 4 <= bytes.len() {
                    let comp = &bytes[offset..offset + 4];
                    if comp == b"heic" || comp == b"heix" || comp == b"mif1" {
                        return Some(Format::Heic);
                    }
                    if comp == b"M4A " {
                        return Some(Format::M4a);
                    }
                    offset += 4;
                }
            }
        }
    }

    // MP3: ID3 tag or frame sync (0xFF 0xFB/0xF3/0xF2)
    if bytes.starts_with(b"ID3") {
        return Some(Format::Mp3);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some(Format::Mp3);
    }

    None
}
