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
    if bytes[..check_len].windows(5).any(|w| w == b"%PDF-") {
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
            b"M4A " | b"M4B " | b"mp42" | b"isom" | b"iso2" | b"dash" => return Some(Format::M4a),
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => return Some(Format::Heic),
            _ => {
                // Check compatible brands only within the ftyp box (bounded by box size).
                let raw_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
                let box_end = if raw_size >= 16 {
                    raw_size.min(bytes.len())
                } else {
                    bytes.len().min(64)
                };
                let mut offset = 16;
                while offset + 4 <= box_end {
                    let comp = &bytes[offset..offset + 4];
                    if matches!(comp, b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1") {
                        return Some(Format::Heic);
                    }
                    if matches!(comp, b"M4A " | b"M4B ") {
                        return Some(Format::M4a);
                    }
                    offset += 4;
                }
            }
        }
    }

    // MP3: ID3 tag or MPEG audio frame sync with non-zero layer bits (zero would be ADTS AAC)
    if bytes.starts_with(b"ID3") {
        return Some(Format::Mp3);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 && ((bytes[1] >> 1) & 3) != 0 {
        return Some(Format::Mp3);
    }

    None
}
