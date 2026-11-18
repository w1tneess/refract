use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use refract_core::error::{ErrorCode, RefractError};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct PdfInfo {
    pub page_count: u32,
    pub is_encrypted: bool,
}

/// An image that was embedded in a PDF page.
#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    /// 1-based page number the image was found on.
    pub page: u32,
    /// Encoded image bytes (JPEG or PNG).
    pub bytes: Vec<u8>,
    /// File extension matching the encoded bytes.
    pub extension: &'static str,
}

/// Load a PDF, decrypting it when a password is supplied.
///
/// Without a password, encrypted documents fail with [`ErrorCode::NeedsPassword`]
/// so the UI can ask the user for one.
pub fn load_document(bytes: &[u8], password: Option<&str>) -> Result<Document, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "PDF file is empty (0 bytes)"));
    }

    let mut doc = Document::load_mem(bytes).map_err(|e| {
        let message = e.to_string();
        let lowered = message.to_lowercase();
        if lowered.contains("encrypt") || lowered.contains("password") {
            RefractError::new(
                ErrorCode::NeedsPassword,
                "This PDF is protected. Enter the password to continue.",
            )
        } else {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Failed to parse PDF document",
                message,
            )
        }
    })?;

    if doc.is_encrypted() {
        let password = match password {
            Some(p) if !p.is_empty() => p,
            _ => {
                return Err(RefractError::new(
                    ErrorCode::NeedsPassword,
                    "This PDF is protected. Enter the password to continue.",
                ))
            }
        };

        doc.authenticate_password(password).map_err(|_| {
            RefractError::new(
                ErrorCode::WrongPassword,
                "That password did not unlock this PDF.",
            )
        })?;

        doc.decrypt(password).map_err(|e| {
            RefractError::with_details(
                ErrorCode::WrongPassword,
                "That password did not unlock this PDF.",
                e.to_string(),
            )
        })?;
    }

    Ok(doc)
}

/// Inspect PDF metadata (page count, encryption status).
pub fn probe_pdf(bytes: &[u8]) -> Result<PdfInfo, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "PDF file is empty (0 bytes)"));
    }

    match Document::load_mem(bytes) {
        Ok(doc) => {
            let is_encrypted = doc.is_encrypted();
            let page_count = doc.get_pages().len() as u32;
            Ok(PdfInfo {
                page_count,
                is_encrypted,
            })
        }
        Err(e) => {
            let err_str = e.to_string();
            let lowered = err_str.to_lowercase();
            if lowered.contains("encrypt") || lowered.contains("password") {
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

/// Inspect PDF metadata, unlocking it with `password` when needed.
pub fn probe_pdf_with_password(bytes: &[u8], password: &str) -> Result<PdfInfo, RefractError> {
    let doc = load_document(bytes, Some(password))?;
    Ok(PdfInfo {
        page_count: doc.get_pages().len() as u32,
        is_encrypted: doc.is_encrypted(),
    })
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
    let mut doc = load_document(bytes, None)?;

    doc.compress();

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save compressed PDF", e.to_string())
    })?;

    Ok(out)
}

/// Compress a PDF document, unlocking it with `password` when needed.
pub fn compress_pdf_with_password(bytes: &[u8], password: &str) -> Result<Vec<u8>, RefractError> {
    let mut doc = load_document(bytes, Some(password))?;

    doc.compress();

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save compressed PDF", e.to_string())
    })?;

    Ok(out)
}

/// Reorder pages in a PDF document.
pub fn reorder_pdf(bytes: &[u8], order_str: &str) -> Result<Vec<u8>, RefractError> {
    let mut doc = load_document(bytes, None)?;

    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;
    let order = parse_order(order_str, total_pages)?;

    // If the requested order already matches the document, only compress.
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

    let new_kids: Vec<Object> = order
        .iter()
        .map(|&p| Object::Reference(pages[&p]))
        .collect();

    for &page_num in &order {
        let page_id = pages[&page_num];
        if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            page_dict.set("Parent", Object::Reference(pages_ref));
        }
    }

    let pages_dict = doc
        .get_object_mut(pages_ref)
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to get Pages object", e.to_string()))?
        .as_dict_mut()
        .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Pages object is not a dictionary", e.to_string()))?;

    pages_dict.set("Kids", Object::Array(new_kids));
    pages_dict.set("Count", Object::Integer(total_pages as i64));

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save reordered PDF", e.to_string())
    })?;

    Ok(out)
}

/// Split a PDF document into multiple PDFs by page ranges.
/// e.g. "1-2, 3" creates two PDFs: one with pages 1-2, one with page 3.
pub fn split_pdf(bytes: &[u8], ranges_spec: &str) -> Result<Vec<(String, Vec<u8>)>, RefractError> {
    let doc = load_document(bytes, None)?;

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

        let group_kids: Vec<Object> = group
            .iter()
            .map(|&p| Object::Reference(pages[&p]))
            .collect();

        for &page_num in &group {
            let page_id = pages[&page_num];
            if let Ok(page_dict) = group_doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                page_dict.set("Parent", Object::Reference(pages_ref));
            }
        }

        let pages_dict = group_doc
            .get_object_mut(pages_ref)
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Failed to get Pages object", e.to_string()))?
            .as_dict_mut()
            .map_err(|e| RefractError::with_details(ErrorCode::DamagedFile, "Pages object is not a dictionary", e.to_string()))?;

        pages_dict.set("Kids", Object::Array(group_kids));
        pages_dict.set("Count", Object::Integer(group.len() as i64));

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

/// Merge multiple PDF documents into one, preserving the given order.
pub fn merge_pdfs(pdf_list: &[&[u8]]) -> Result<Vec<u8>, RefractError> {
    if pdf_list.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "No PDF files provided to merge"));
    }

    if pdf_list.len() == 1 {
        return Ok(pdf_list[0].to_vec());
    }

    let mut target = Document::with_version("1.5");
    let mut kids: Vec<Object> = Vec::new();

    for (index, bytes) in pdf_list.iter().enumerate() {
        let doc = load_document(bytes, None).map_err(|e| match e.code {
            ErrorCode::NeedsPassword => RefractError::with_details(
                ErrorCode::NeedsPassword,
                format!("Document #{} is protected. Enter its password to continue.", index + 1),
                e.message,
            ),
            _ => e,
        })?;

        merge_document_into(&mut target, &doc, &mut kids)?;
    }

    finalize_merged(&mut target, kids)?;

    let mut out = Vec::new();
    target.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save merged PDF", e.to_string())
    })?;

    Ok(out)
}

/// Merge multiple PDF documents, unlocking every document with `password`.
pub fn merge_pdfs_with_password(pdf_list: &[&[u8]], password: &str) -> Result<Vec<u8>, RefractError> {
    if pdf_list.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "No PDF files provided to merge"));
    }

    let mut target = Document::with_version("1.5");
    let mut kids: Vec<Object> = Vec::new();

    for bytes in pdf_list {
        let doc = load_document(bytes, Some(password))?;
        merge_document_into(&mut target, &doc, &mut kids)?;
    }

    finalize_merged(&mut target, kids)?;

    let mut out = Vec::new();
    target.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save merged PDF", e.to_string())
    })?;

    Ok(out)
}

/// Copy every object of `doc` into `target`, remapping object ids, and collect
/// the copied page references in document order.
fn merge_document_into(
    target: &mut Document,
    doc: &Document,
    kids: &mut Vec<Object>,
) -> Result<(), RefractError> {
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "One of the documents has no pages and cannot be merged.",
        ));
    }

    // Reserve a fresh object id for every object of the source document.
    let mut id_map: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(doc.objects.len());
    for old_id in doc.objects.keys() {
        let new_id = target.new_object_id();
        id_map.insert(*old_id, new_id);
    }

    for (old_id, object) in doc.objects.iter() {
        // The source page tree root and catalog are replaced by the merged ones.
        // Outline trees are dropped because they point at the old catalog.
        if matches!(
            object.type_name(),
            Ok(b"Catalog") | Ok(b"Pages") | Ok(b"Outlines") | Ok(b"Outline")
        ) {
            continue;
        }

        let new_object = remap_object(object, &id_map);
        target.objects.insert(id_map[old_id], new_object);
    }

    for (_, page_id) in pages.iter() {
        if let Some(new_id) = id_map.get(page_id) {
            kids.push(Object::Reference(*new_id));
        }
    }

    Ok(())
}

/// Build the shared page tree and catalog for a merged document.
fn finalize_merged(target: &mut Document, kids: Vec<Object>) -> Result<(), RefractError> {
    if kids.is_empty() {
        return Err(RefractError::new(
            ErrorCode::DamagedFile,
            "The selected documents contain no pages.",
        ));
    }

    let page_count = kids.len();
    let page_ids: Vec<ObjectId> = kids
        .iter()
        .filter_map(|kid| kid.as_reference().ok())
        .collect();

    let pages_id = target.new_object_id();
    let pages_dict = dictionary! {
        "Type" => "Pages",
        "Kids" => kids,
        "Count" => page_count as i64,
    };
    target.objects.insert(pages_id, Object::Dictionary(pages_dict));

    // Every copied page now points at the merged page tree instead of its old
    // parent, and is explicitly tagged as a page so viewers and lopdf agree.
    for page_id in page_ids {
        if let Ok(page) = target.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference(pages_id));
        }
    }

    let catalog_id = target.new_object_id();
    let catalog_dict = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    target.objects.insert(catalog_id, Object::Dictionary(catalog_dict));

    target.trailer.set("Root", Object::Reference(catalog_id));
    target.trailer.remove(b"Encrypt");
    target.trailer.remove(b"Info");

    Ok(())
}

/// Rewrite every indirect reference inside `object` using `id_map`.
fn remap_object(object: &Object, id_map: &HashMap<ObjectId, ObjectId>) -> Object {
    match object {
        Object::Reference(id) => match id_map.get(id) {
            Some(new_id) => Object::Reference(*new_id),
            None => object.clone(),
        },
        Object::Array(items) => {
            Object::Array(items.iter().map(|item| remap_object(item, id_map)).collect())
        }
        Object::Dictionary(dict) => {
            let mut new_dict = Dictionary::new();
            for (key, value) in dict.iter() {
                new_dict.set(key.clone(), remap_object(value, id_map));
            }
            Object::Dictionary(new_dict)
        }
        Object::Stream(stream) => {
            let mut new_dict = Dictionary::new();
            for (key, value) in stream.dict.iter() {
                new_dict.set(key.clone(), remap_object(value, id_map));
            }
            Object::Stream(Stream::new(new_dict, stream.content.clone()))
        }
        other => other.clone(),
    }
}

/// Build a PDF document from a list of encoded images (JPEG, PNG or WebP).
///
/// One page is created per image. Pages are sized from the pixel dimensions of
/// the source image and the requested DPI, so a 3000x2000 photo at 150 DPI
/// becomes a 1440x960 point page.
pub fn images_to_pdf(images: &[Vec<u8>], dpi: f64, quality: u8) -> Result<Vec<u8>, RefractError> {
    if images.is_empty() {
        return Err(RefractError::new(ErrorCode::EmptyFile, "No images were provided"));
    }

    let dpi = if dpi <= 0.0 { 150.0 } else { dpi };

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::new();

    for bytes in images {
        let decoded = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Could not read an image for the PDF",
                    e.to_string(),
                )
            })?
            .decode()
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Could not decode an image for the PDF",
                    e.to_string(),
                )
            })?;

        let (width_px, height_px) = image::GenericImageView::dimensions(&decoded);
        let jpeg = crate::image::encode_jpeg_flattened(&decoded, quality)?;

        let image_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width_px as i64,
                "Height" => height_px as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ));

        let page_width = (width_px as f64 * 72.0 / dpi).round().max(1.0);
        let page_height = (height_px as f64 * 72.0 / dpi).round().max(1.0);

        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! {
                "Im0" => image_id,
            },
        });

        let content = format!(
            "q {:.2} 0 0 {:.2} 0 0 cm /Im0 Do Q",
            page_width, page_height
        );
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));

        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                0.into(),
                0.into(),
                (page_width.round() as i64).into(),
                (page_height.round() as i64).into(),
            ],
            "Resources" => resources_id,
            "Contents" => content_id,
        });

        kids.push(Object::Reference(page_id));
    }

    let pages_dict = dictionary! {
        "Type" => "Pages",
        "Kids" => kids.clone(),
        "Count" => kids.len() as i64,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(ErrorCode::EngineFailed, "Failed to save the PDF", e.to_string())
    })?;

    Ok(out)
}

/// Extract the images that are embedded in a PDF, page by page.
///
/// This is the built-in fallback for "PDF to images" when no external PDF
/// rasterizer is installed: it pulls the original image data out of the file
/// instead of re-rendering the page.
pub fn extract_embedded_images(bytes: &[u8]) -> Result<Vec<EmbeddedImage>, RefractError> {
    let doc = load_document(bytes, None)?;
    let mut results = Vec::new();

    for (page_number, page_id) in doc.get_pages() {
        let page = match doc.get_dictionary(page_id) {
            Ok(page) => page,
            Err(_) => continue,
        };

        let resources = match doc.get_dict_in_dict(page, b"Resources") {
            Ok(resources) => resources,
            Err(_) => continue,
        };

        let xobjects = match doc.get_dict_in_dict(resources, b"XObject") {
            Ok(xobjects) => xobjects,
            Err(_) => continue,
        };

        for (_, value) in xobjects.iter() {
            let object_id = match value.as_reference() {
                Ok(id) => id,
                Err(_) => continue,
            };
            let stream = match doc.get_object(object_id).and_then(|o| o.as_stream()) {
                Ok(stream) => stream,
                Err(_) => continue,
            };

            let subtype = stream
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|s| s.as_name().ok());
            if subtype != Some(b"Image".as_ref()) {
                continue;
            }

            if let Some(extracted) = decode_image_stream(stream) {
                results.push(EmbeddedImage {
                    page: page_number,
                    bytes: extracted.0,
                    extension: extracted.1,
                });
            }
        }
    }

    Ok(results)
}

/// Decode one embedded image XObject into a standalone image file.
fn decode_image_stream(stream: &Stream) -> Option<(Vec<u8>, &'static str)> {
    let filters = stream_filters(stream);

    if filters.iter().any(|f| f == "DCTDecode") {
        // JPEG data can be written out as-is.
        return Some((stream.content.clone(), "jpg"));
    }

    let is_raw = filters.iter().any(|f| f == "FlateDecode") || filters.is_empty();
    if !is_raw {
        return None;
    }

    let width = stream.dict.get(b"Width").ok()?.as_i64().ok()? as u32;
    let height = stream.dict.get(b"Height").ok()?.as_i64().ok()? as u32;
    if width == 0 || height == 0 {
        return None;
    }

    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .ok()
        .and_then(|b| b.as_i64().ok())
        .unwrap_or(8) as u32;
    if bpc != 8 {
        return None;
    }

    let color_space = stream
        .dict
        .get(b"ColorSpace")
        .ok()
        .map(color_space_name)
        .unwrap_or_else(|| "DeviceRGB".to_string());

    let channels = match color_space.as_str() {
        "DeviceRGB" | "RGB" | "CalRGB" => 3usize,
        "DeviceGray" | "CalGray" | "G" => 1usize,
        "DeviceCMYK" | "CMYK" => 4usize,
        _ => return None,
    };

    let raw = if filters.iter().any(|f| f == "FlateDecode") {
        stream.decompressed_content().ok()?
    } else {
        stream.content.clone()
    };

    let expected = width as usize * height as usize * channels as usize;
    if raw.len() < expected {
        return None;
    }
    let samples = &raw[..expected];

    let dynamic = match channels {
        1 => {
            let mut luma = image::GrayImage::new(width, height);
            for (index, pixel) in luma.pixels_mut().enumerate() {
                pixel[0] = samples[index];
            }
            image::DynamicImage::ImageLuma8(luma)
        }
        3 => {
            let mut rgb = image::RgbImage::new(width, height);
            for (index, pixel) in rgb.pixels_mut().enumerate() {
                pixel[0] = samples[index * 3];
                pixel[1] = samples[index * 3 + 1];
                pixel[2] = samples[index * 3 + 2];
            }
            image::DynamicImage::ImageRgb8(rgb)
        }
        _ => return None,
    };

    let encoded = crate::image::encode_png(&dynamic).ok()?;
    Some((encoded, "png"))
}

/// Read the `Filter` entry of a stream as a list of filter names.
fn stream_filters(stream: &Stream) -> Vec<String> {
    match stream.dict.get(b"Filter") {
        Ok(Object::Array(array)) => array
            .iter()
            .filter_map(|item| item.as_name().ok())
            .map(|name| String::from_utf8_lossy(name).to_string())
            .collect(),
        Ok(Object::Name(name)) => vec![String::from_utf8_lossy(name).to_string()],
        _ => Vec::new(),
    }
}

/// Read the base colour space name of an image XObject.
fn color_space_name(value: &Object) -> String {
    match value {
        Object::Name(name) => String::from_utf8_lossy(name).to_string(),
        Object::Array(array) => array
            .first()
            .and_then(|first| first.as_name().ok())
            .map(|name| String::from_utf8_lossy(name).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
