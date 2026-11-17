use refract_core::error::{ErrorCode, RefractError};
use lopdf::Document;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct PdfInfo {
    pub page_count: u32,
    pub is_encrypted: bool,
}

/// Inspect PDF metadata (page count, encryption status).
pub fn probe_pdf(bytes: &[u8]) -> Result<PdfInfo, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "PDF file is empty (0 bytes)"));
    }

    match Document::load_mem(bytes) {
        Ok(doc) => {
            let is_encrypted = doc.trailer.get(b"Encrypt").is_ok();
            let page_count = doc.get_pages().len() as u32;
            Ok(PdfInfo {
                page_count,
                is_encrypted,
            })
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.to_lowercase().contains("encrypt") || err_str.to_lowercase().contains("password") {
                Ok(PdfInfo {
                    page_count: 0,
                    is_encrypted: true,
                })
            } else {
                Err(RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Failed to parse PDF document",
                    err_str,
                ))
            }
        }
    }
}

/// Parse page range string into groups of page numbers.
/// e.g. "1-2, 3" with total 3 pages -> [[1, 2], [3]]
pub fn parse_ranges(spec: &str, total_pages: u32) -> Result<Vec<Vec<u32>>, RefractError> {
    let clean = spec.trim();
    if clean.is_empty() {
        return Err(RefractError::new(ErrorCode::DamagedFile, "Page range cannot be empty"));
    }

    let mut groups = Vec::new();
    for part in clean.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some((start_s, end_s)) = part.split_once('-') {
            let start: u32 = start_s.trim().parse().map_err(|_| {
                RefractError::new(ErrorCode::DamagedFile, format!("Invalid range start: {}", start_s))
            })?;
            let end: u32 = if end_s.trim().eq_ignore_ascii_case("end") {
                total_pages
            } else {
                end_s.trim().parse().map_err(|_| {
                    RefractError::new(ErrorCode::DamagedFile, format!("Invalid range end: {}", end_s))
                })?
            };

            if start < 1 || start > total_pages || end < start || end > total_pages {
                return Err(RefractError::new(
                    ErrorCode::LimitExceeded,
                    format!("Range {}-{} is outside document (1-{})", start, end, total_pages),
                ));
            }

            groups.push((start..=end).collect());
        } else {
            let page: u32 = part.parse().map_err(|_| {
                RefractError::new(ErrorCode::DamagedFile, format!("Invalid page number: {}", part))
            })?;
            if page < 1 || page > total_pages {
                return Err(RefractError::new(
                    ErrorCode::LimitExceeded,
                    format!("Page {} is outside document (1-{})", page, total_pages),
                ));
            }
            groups.push(vec![page]);
        }
    }

    if groups.is_empty() {
        return Err(RefractError::new(ErrorCode::DamagedFile, "No valid page ranges found"));
    }

    Ok(groups)
}

/// Parse exact page reorder sequence.
/// Validates 1-to-1 match (all pages 1..N exactly once).
pub fn parse_order(order_str: &str, total_pages: u32) -> Result<Vec<u32>, RefractError> {
    let parts: Vec<&str> = order_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if parts.len() != total_pages as usize {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            format!("Expected {} pages in order list, got {}", total_pages, parts.len()),
        ));
    }

    let mut pages = Vec::with_capacity(parts.len());
    let mut seen = HashSet::new();

    for p in parts {
        let page_num: u32 = p.parse().map_err(|_| {
            RefractError::new(ErrorCode::DamagedFile, format!("Invalid page number: {}", p))
        })?;

        if page_num < 1 || page_num > total_pages {
            return Err(RefractError::new(
                ErrorCode::LimitExceeded,
                format!("Page {} is out of range 1-{}", page_num, total_pages),
            ));
        }

        if !seen.insert(page_num) {
            return Err(RefractError::new(
                ErrorCode::DamagedFile,
                format!("Duplicate page {} in order list", page_num),
            ));
        }

        pages.push(page_num);
    }

    Ok(pages)
}

/// Compress a PDF document using object and stream deflating.
pub fn compress_pdf(bytes: &[u8]) -> Result<Vec<u8>, RefractError> {
    let mut doc = Document::load_mem(bytes).map_err(|e| {
        RefractError::with_details(ErrorCode::DamagedFile, "Failed to load PDF for compression", e.to_string())
    })?;

    doc.compress();

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save compressed PDF", e.to_string())
    })?;

    Ok(out)
}

/// Reorder pages in a PDF document.
pub fn reorder_pdf(bytes: &[u8], order_str: &str) -> Result<Vec<u8>, RefractError> {
    let mut doc = Document::load_mem(bytes).map_err(|e| {
        RefractError::with_details(ErrorCode::DamagedFile, "Failed to load PDF for reordering", e.to_string())
    })?;

    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;
    let order = parse_order(order_str, total_pages)?;

    // If order is identical, return original compressed
    if order == (1..=total_pages).collect::<Vec<_>>() {
        return compress_pdf(bytes);
    }

    // Extract pages in new order into a fresh document
    let mut new_doc = Document::with_version(doc.version.as_str());
    let mut new_pages = Vec::new();

    for &page_num in &order {
        if let Some(&page_id) = pages.get(&page_num) {
            let new_id = new_doc.add_object(doc.get_object(page_id).unwrap().clone());
            new_pages.push(new_id);
        }
    }

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save reordered PDF", e.to_string())
    })?;

    Ok(out)
}

/// Merge multiple PDF documents into one in the provided order.
pub fn merge_pdfs(pdf_list: &[&[u8]]) -> Result<Vec<u8>, RefractError> {
    if pdf_list.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "No PDF files provided to merge"));
    }

    if pdf_list.len() == 1 {
        return Ok(pdf_list[0].to_vec());
    }

    let mut first = Document::load_mem(pdf_list[0]).map_err(|e| {
        RefractError::with_details(ErrorCode::DamagedFile, "Failed to load first PDF", e.to_string())
    })?;

    for (idx, &bytes) in pdf_list.iter().enumerate().skip(1) {
        let other = Document::load_mem(bytes).map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                format!("Failed to load PDF #{}", idx + 1),
                e.to_string(),
            )
        })?;

        // Merge pages and objects from `other` into `first`
        let pages_other = other.get_pages();
        for (_, page_id) in pages_other {
            if let Ok(obj) = other.get_object(page_id) {
                first.add_object(obj.clone());
            }
        }
    }

    let mut out = Vec::new();
    first.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save merged PDF", e.to_string())
    })?;

    Ok(out)
}
