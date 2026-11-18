use lopdf::{Dictionary, Document, Object, ObjectId};
use refract_core::error::{ErrorCode, RefractError};
use std::collections::{BTreeMap, HashSet};

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
/// Supports "1-2, 3", "2-", "2-end", and empty string (every page as its own group).
pub fn parse_ranges(spec: &str, total_pages: u32) -> Result<Vec<Vec<u32>>, RefractError> {
    if total_pages == 0 {
        return Err(RefractError::new(ErrorCode::DamagedFile, "PDF has 0 pages"));
    }

    let clean = spec.trim();
    if clean.is_empty() {
        return Ok((1..=total_pages).map(|p| vec![p]).collect());
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
            let end_trimmed = end_s.trim();
            let end: u32 = if end_trimmed.is_empty() || end_trimmed.eq_ignore_ascii_case("end") {
                total_pages
            } else {
                end_trimmed.parse().map_err(|_| {
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

    let pages_ref = doc
        .catalog()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Missing PDF catalog", e.to_string()))?
        .get(b"Pages")
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Missing PDF Pages reference", e.to_string()))?
        .as_reference()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Invalid PDF Pages reference", e.to_string()))?;

    let new_kids: Vec<lopdf::Object> = order
        .iter()
        .map(|&p| lopdf::Object::Reference(pages[&p]))
        .collect();

    for &page_num in &order {
        let page_id = pages[&page_num];
        if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            page_dict.set("Parent", lopdf::Object::Reference(pages_ref));
        }
    }

    let pages_dict = doc
        .get_object_mut(pages_ref)
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to get Pages object", e.to_string()))?
        .as_dict_mut()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Pages object is not a dictionary", e.to_string()))?;

    pages_dict.set("Kids", lopdf::Object::Array(new_kids));
    pages_dict.set("Count", lopdf::Object::Integer(total_pages as i64));

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save reordered PDF", e.to_string())
    })?;

    Ok(out)
}

/// Split a PDF document into multiple PDFs by page ranges.
/// e.g. "1-2, 3" creates two PDFs: one with pages 1-2, one with page 3.
pub fn split_pdf(bytes: &[u8], ranges_spec: &str) -> Result<Vec<(String, Vec<u8>)>, RefractError> {
    let doc = Document::load_mem(bytes).map_err(|e| {
        RefractError::with_details(ErrorCode::DamagedFile, "Failed to load PDF for splitting", e.to_string())
    })?;

    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;
    let groups = parse_ranges(ranges_spec, total_pages)?;
    let mut outputs = Vec::new();

    for group in groups {
        let mut group_doc = doc.clone();
        let pages_ref = group_doc
            .catalog()
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Missing PDF catalog", e.to_string()))?
            .get(b"Pages")
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Missing PDF Pages reference", e.to_string()))?
            .as_reference()
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Invalid PDF Pages reference", e.to_string()))?;

        let group_kids: Vec<lopdf::Object> = group
            .iter()
            .map(|&p| lopdf::Object::Reference(pages[&p]))
            .collect();

        for &page_num in &group {
            let page_id = pages[&page_num];
            if let Ok(page_dict) = group_doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                page_dict.set("Parent", lopdf::Object::Reference(pages_ref));
            }
        }

        let pages_dict = group_doc
            .get_object_mut(pages_ref)
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to get Pages object", e.to_string()))?
            .as_dict_mut()
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Pages object is not a dictionary", e.to_string()))?;

        pages_dict.set("Kids", lopdf::Object::Array(group_kids));
        pages_dict.set("Count", lopdf::Object::Integer(group.len() as i64));

        // Remove unreferenced pages and their resources so split files only carry their own pages.
        group_doc.prune_objects();
        group_doc.renumber_objects();

        let mut out = Vec::new();
        group_doc.save_to(&mut out).map_err(|e| {
            RefractError::with_details(ErrorCode::EngineFailed, "Failed to save split PDF", e.to_string())
        })?;

        let label = if group.len() == 1 {
            format!("p{}", group[0])
        } else {
            format!("p{}-{}", group[0], group.last().unwrap())
        };
        outputs.push((label, out));
    }

    Ok(outputs)
}

/// Propagate inheritable page attributes (MediaBox, CropBox, Resources, Rotate) from ancestor Pages nodes
/// onto a Page dictionary before reparenting it to a new Pages root.
fn inherit_page_attributes(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    let mut page_dict = doc.get_object(page_id).ok()?.as_dict().ok()?.clone();
    let inheritable: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

    let mut parent_ref = page_dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    let mut visited = HashSet::new();
    while let Some(pid) = parent_ref {
        if !visited.insert(pid) {
            break;
        }
        let Ok(parent_dict) = doc.get_object(pid).and_then(|o| o.as_dict()) else {
            break;
        };
        for key in inheritable {
            if !page_dict.has(key) {
                if let Ok(val) = parent_dict.get(key) {
                    page_dict.set(key, val.clone());
                }
            }
        }
        parent_ref = parent_dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }

    Some(page_dict)
}

/// Merge multiple PDF documents into one in the provided order.
pub fn merge_pdfs(pdf_list: &[&[u8]]) -> Result<Vec<u8>, RefractError> {
    if pdf_list.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "No PDF files provided to merge"));
    }

    if pdf_list.len() == 1 {
        return Ok(pdf_list[0].to_vec());
    }

    let mut max_id = 1u32;
    let mut ordered_pages: Vec<(ObjectId, Dictionary)> = Vec::new();
    let mut merged_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();

    for (idx, &bytes) in pdf_list.iter().enumerate() {
        let mut doc = Document::load_mem(bytes).map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                format!("Failed to load PDF #{}", idx + 1),
                e.to_string(),
            )
        })?;

        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        let pages = doc.get_pages();
        for (_, page_id) in pages {
            if let Some(page_dict) = inherit_page_attributes(&doc, page_id) {
                ordered_pages.push((page_id, page_dict));
            }
        }

        for (object_id, object) in doc.objects {
            let skip = object
                .as_dict()
                .ok()
                .and_then(|d| d.get(b"Type").ok())
                .and_then(|t| t.as_name().ok())
                .is_some_and(|name| name == b"Catalog" || name == b"Pages");
            if !skip {
                merged_objects.insert(object_id, object);
            }
        }
    }

    if ordered_pages.is_empty() {
        return Err(RefractError::new(ErrorCode::DamagedFile, "No pages found in PDF documents"));
    }

    let mut document = Document::with_version("1.5");
    document.objects = merged_objects;
    document.max_id = max_id;

    let pages_id = document.new_object_id();
    let mut kids = Vec::with_capacity(ordered_pages.len());

    for (page_id, mut page_dict) in ordered_pages {
        page_dict.set("Parent", Object::Reference(pages_id));
        document.objects.insert(page_id, Object::Dictionary(page_dict));
        kids.push(Object::Reference(page_id));
    }

    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set("Count", Object::Integer(kids.len() as i64));
    pages_dict.set("Kids", Object::Array(kids));
    document.objects.insert(pages_id, Object::Dictionary(pages_dict));

    let catalog_id = document.new_object_id();
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    document.objects.insert(catalog_id, Object::Dictionary(catalog));

    document.trailer.set("Root", Object::Reference(catalog_id));
    document.compress();

    let mut out = Vec::new();
    document.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save merged PDF", e.to_string())
    })?;

    Ok(out)
}
