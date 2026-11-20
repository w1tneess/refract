use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use std::collections::{HashMap, HashSet};

/// Standard PDF page sizes in points (1 pt = 1/72 inch), matching Web (`packages/engine-wasm/src/codecs/pdf.ts`).
pub const A4_POINTS: (f64, f64) = (595.28, 841.89);
pub const LETTER_POINTS: (f64, f64) = (612.0, 792.0);

/// Page sizing mode for `images-to-pdf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfPageSize {
    A4,
    Letter,
    Fit,
}

impl PdfPageSize {
    /// Parse a canonical page size string (`"a4"`, `"letter"`, `"fit"`).
    /// Rejects unknown values cleanly rather than silently replacing them with defaults.
    pub fn parse(raw: &str) -> Result<Self, RefractError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "a4" => Ok(Self::A4),
            "letter" => Ok(Self::Letter),
            "fit" => Ok(Self::Fit),
            other => Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Unsupported PDF page size '{}'. Use 'a4', 'letter', or 'fit'.",
                    other
                ),
            )),
        }
    }
}

/// In-memory password wrapper that redacts `Debug` output and zeroes its buffer on `Drop`.
pub struct SecretPassword {
    bytes: Vec<u8>,
}

impl SecretPassword {
    pub fn new(password: impl Into<String>) -> Self {
        Self {
            bytes: password.into().into_bytes(),
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes).unwrap_or("")
    }

    pub fn clear(&mut self) {
        for b in &mut self.bytes {
            // SAFETY: `b` is a valid mutable reference to an owned byte in `self.bytes`.
            unsafe {
                std::ptr::write_volatile(b, 0);
            }
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
        self.bytes.clear();
    }
}

impl Drop for SecretPassword {
    fn drop(&mut self) {
        self.clear();
    }
}

impl std::fmt::Debug for SecretPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretPassword([REDACTED])")
    }
}

/// One PDF input with its own optional password for multi-document merge.
pub struct PdfMergeInput<'a> {
    pub bytes: &'a [u8],
    pub password: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfInfo {
    pub page_count: u32,
    /// True if the input PDF contains an `/Encrypt` dictionary.
    pub is_encrypted: bool,
    /// True if opening the PDF requires a non-empty user password (false for owner-only PDFs).
    pub requires_password: bool,
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

fn is_password_or_encryption_error(err: &lopdf::Error) -> bool {
    matches!(err, lopdf::Error::Decryption(_)) || {
        let msg = err.to_string().to_lowercase();
        msg.contains("password") || msg.contains("encrypt") || msg.contains("decrypt")
    }
}

/// Strip encryption metadata from an already-decrypted `Document` so saved outputs
/// are written cleanly as unencrypted PDFs.
fn strip_encryption_state(doc: &mut Document) {
    doc.encryption_state = None;
    doc.trailer.remove(b"Encrypt");
}

/// Load a PDF, automatically unlocking owner-only encrypted PDFs with an empty password.
///
/// Uses `Document::load_mem_with_password` so encrypted PDF 1.5+ Object Streams (`ObjStm`)
/// and AES-256 (`AESV3`) documents are decrypted *during* loading rather than after.
///
/// - Unencrypted PDFs open directly.
/// - Owner-only encrypted PDFs (empty user password) open transparently without prompting.
/// - User-encrypted PDFs without a password return [`ErrorCode::NeedsPassword`].
/// - User-encrypted PDFs with an incorrect password return [`ErrorCode::WrongPassword`].
/// - Passwords are never logged or included in error messages.
pub fn load_document(bytes: &[u8], password: Option<&str>) -> Result<Document, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "PDF file is empty (0 bytes)",
        ));
    }

    let candidate = password.unwrap_or("");
    match Document::load_mem_with_password(bytes, candidate) {
        Ok(mut doc) => {
            if doc.is_encrypted() {
                strip_encryption_state(&mut doc);
            }
            Ok(doc)
        }
        Err(e) => {
            if is_password_or_encryption_error(&e) {
                match password {
                    Some(pw) if !pw.is_empty() => Err(RefractError::new(
                        ErrorCode::WrongPassword,
                        "That password did not unlock this PDF.",
                    )),
                    _ => Err(RefractError::new(
                        ErrorCode::NeedsPassword,
                        "This PDF is protected. Enter the password to continue.",
                    )),
                }
            } else {
                Err(RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Failed to parse PDF document",
                    e.to_string(),
                ))
            }
        }
    }
}

/// Decrypt a PDF into standalone unencrypted PDF bytes (used before invoking external rasterizers).
pub fn unlock_pdf_bytes(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>, RefractError> {
    let mut doc = load_document(bytes, password)?;
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to serialize unlocked PDF",
            e.to_string(),
        )
    })?;
    Ok(out)
}

/// Inspect PDF metadata (page count, encryption status, and whether a user password is required).
///
/// Owner-only encrypted PDFs are automatically unlocked with an empty password during probing
/// so `requires_password` is `false` and `page_count` is accurate.
pub fn probe_pdf(bytes: &[u8]) -> Result<PdfInfo, RefractError> {
    if bytes.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "PDF file is empty (0 bytes)",
        ));
    }

    match Document::load_mem_with_password(bytes, "") {
        Ok(doc) => {
            let is_encrypted = doc.is_encrypted() || doc.encryption_state.is_some();
            let page_count = doc.get_pages().len() as u32;
            Ok(PdfInfo {
                page_count,
                is_encrypted,
                requires_password: false,
            })
        }
        Err(e) => {
            if is_password_or_encryption_error(&e) {
                Ok(PdfInfo {
                    page_count: 0,
                    is_encrypted: true,
                    requires_password: true,
                })
            } else {
                Err(RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Failed to parse PDF document",
                    e.to_string(),
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
        requires_password: false,
    })
}

/// Parse page range string into groups of page numbers.
/// Supports `"1-2, 3"`, `"2-"`, `"2-end"`, and empty string (every page as its own group).
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
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid range start: '{}'", start_s.trim()),
                )
            })?;
            let end_trimmed = end_s.trim();
            let end: u32 = if end_trimmed.is_empty() || end_trimmed.eq_ignore_ascii_case("end") {
                total_pages
            } else {
                end_trimmed.parse().map_err(|_| {
                    RefractError::new(
                        ErrorCode::InvalidOption,
                        format!("Invalid range end: '{}'", end_trimmed),
                    )
                })?
            };

            if start < 1 || start > total_pages || end < start || end > total_pages {
                return Err(RefractError::new(
                    ErrorCode::InvalidOption,
                    format!(
                        "Range {}-{} is outside document (1-{})",
                        start, end, total_pages
                    ),
                ));
            }

            groups.push((start..=end).collect());
        } else {
            let page: u32 = part.parse().map_err(|_| {
                RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Invalid page number: '{}'", part),
                )
            })?;
            if page < 1 || page > total_pages {
                return Err(RefractError::new(
                    ErrorCode::InvalidOption,
                    format!("Page {} is outside document (1-{})", page, total_pages),
                ));
            }
            groups.push(vec![page]);
        }
    }

    if groups.is_empty() {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "No valid page ranges found",
        ));
    }

    Ok(groups)
}

/// Parse exact page reorder sequence.
/// Validates 1-to-1 match (all pages `1..=N` appearing exactly once).
pub fn parse_order(order_str: &str, total_pages: u32) -> Result<Vec<u32>, RefractError> {
    if total_pages == 0 {
        return Err(RefractError::new(ErrorCode::DamagedFile, "PDF has 0 pages"));
    }

    let parts: Vec<&str> = order_str
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != total_pages as usize {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "Expected {} pages in order list, got {}",
                total_pages,
                parts.len()
            ),
        ));
    }

    let mut pages = Vec::with_capacity(parts.len());
    let mut seen = HashSet::new();

    for p in parts {
        let page_num: u32 = p.parse().map_err(|_| {
            RefractError::new(
                ErrorCode::InvalidOption,
                format!("Invalid page number: '{}'", p),
            )
        })?;

        if page_num < 1 || page_num > total_pages {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Page {} is out of range 1-{}", page_num, total_pages),
            ));
        }

        if !seen.insert(page_num) {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!("Duplicate page {} in order list", page_num),
            ));
        }

        pages.push(page_num);
    }

    Ok(pages)
}

/// Propagate only the 4 PDF-spec inheritable page attributes (`/MediaBox`, `/CropBox`,
/// `/Resources`, `/Rotate`) from ancestor `/Pages` nodes onto a Page dictionary before
/// reparenting it to a new `/Pages` root. Never copies arbitrary non-inheritable entries.
fn inherit_page_attributes(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    let mut page_dict = doc.get_object(page_id).ok()?.as_dict().ok()?.clone();
    let inheritable: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

    let mut parent_ref = page_dict
        .get(b"Parent")
        .ok()
        .and_then(|o| o.as_reference().ok());
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
        parent_ref = parent_dict
            .get(b"Parent")
            .ok()
            .and_then(|o| o.as_reference().ok());
    }

    Some(page_dict)
}

/// Materialize inherited page attributes onto each selected page before flattening the page tree.
fn materialize_inherited_attributes(
    doc: &mut Document,
    page_ids: &[ObjectId],
    pages_ref: ObjectId,
) {
    let snapshots: Vec<(ObjectId, Dictionary)> = page_ids
        .iter()
        .filter_map(|&pid| inherit_page_attributes(doc, pid).map(|dict| (pid, dict)))
        .collect();

    for (pid, mut dict) in snapshots {
        dict.set("Parent", Object::Reference(pages_ref));
        doc.objects.insert(pid, Object::Dictionary(dict));
    }
}

fn cancellable_stage<T>(
    cancel: &CancelToken,
    stage: impl FnOnce() -> Result<T, RefractError>,
) -> Result<T, RefractError> {
    cancel.check()?;
    let result = stage();
    cancel.check()?;
    result
}

/// Compress a PDF document using object and stream deflating (unlocking with optional password).
pub fn compress_pdf_cancellable(
    bytes: &[u8],
    password: Option<&str>,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    let mut doc = load_document(bytes, password)?;
    cancel.check()?;

    doc.compress();
    let mut out = Vec::new();
    cancel.check()?;
    cancellable_stage(cancel, || {
        doc.save_to(&mut out).map_err(|e| {
            RefractError::with_details(
                ErrorCode::EngineFailed,
                "Failed to save compressed PDF",
                e.to_string(),
            )
        })
    })?;

    cancel.check()?;

    Ok(out)
}

/// Compress a PDF document using object and stream deflating.
pub fn compress_pdf(bytes: &[u8]) -> Result<Vec<u8>, RefractError> {
    compress_pdf_cancellable(bytes, None, &CancelToken::none())
}

/// Compress a PDF document, unlocking it with `password` when needed.
pub fn compress_pdf_with_password(bytes: &[u8], password: &str) -> Result<Vec<u8>, RefractError> {
    compress_pdf_cancellable(bytes, Some(password), &CancelToken::none())
}

/// Reorder pages in a PDF document with optional password and cancellation support.
///
/// Preserves inherited `/MediaBox`, `/CropBox`, `/Resources`, and `/Rotate` attributes when
/// flattening nested `/Pages` trees, and preserves unlocked state on identity order.
pub fn reorder_pdf_with_password(
    bytes: &[u8],
    order_str: &str,
    password: Option<&str>,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    let mut doc = load_document(bytes, password)?;
    cancel.check()?;

    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;
    let order = parse_order(order_str, total_pages)?;

    // Identity-order fast path: compress and save the already-unlocked `doc` directly
    // so password-protected documents are never reloaded without their password.
    if order == (1..=total_pages).collect::<Vec<_>>() {
        cancel.check()?;
        doc.compress();
        let mut out = Vec::new();
        cancel.check()?;
        cancellable_stage(cancel, || {
            doc.save_to(&mut out).map_err(|e| {
                RefractError::with_details(
                    ErrorCode::EngineFailed,
                    "Failed to save reordered PDF",
                    e.to_string(),
                )
            })
        })?;
        cancel.check()?;
        return Ok(out);
    }

    let pages_ref = doc
        .catalog()
        .map_err(|e| {
            RefractError::with_details(ErrorCode::DamagedFile, "Missing PDF catalog", e.to_string())
        })?
        .get(b"Pages")
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Missing PDF Pages reference",
                e.to_string(),
            )
        })?
        .as_reference()
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Invalid PDF Pages reference",
                e.to_string(),
            )
        })?;

    let ordered_ids: Vec<ObjectId> = order.iter().map(|&p| pages[&p]).collect();
    let new_kids: Vec<Object> = ordered_ids
        .iter()
        .map(|&id| Object::Reference(id))
        .collect();

    // Propagate inherited page attributes before reparenting pages directly under root `/Pages`.
    materialize_inherited_attributes(&mut doc, &ordered_ids, pages_ref);

    let pages_dict = doc
        .get_object_mut(pages_ref)
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Failed to get Pages object",
                e.to_string(),
            )
        })?
        .as_dict_mut()
        .map_err(|e| {
            RefractError::with_details(
                ErrorCode::DamagedFile,
                "Pages object is not a dictionary",
                e.to_string(),
            )
        })?;

    pages_dict.set("Kids", Object::Array(new_kids));
    pages_dict.set("Count", Object::Integer(total_pages as i64));

    let mut out = Vec::new();
    cancel.check()?;
    cancellable_stage(cancel, || {
        doc.save_to(&mut out).map_err(|e| {
            RefractError::with_details(
                ErrorCode::EngineFailed,
                "Failed to save reordered PDF",
                e.to_string(),
            )
        })
    })?;

    cancel.check()?;

    Ok(out)
}

/// Reorder pages in a PDF document.
pub fn reorder_pdf(bytes: &[u8], order_str: &str) -> Result<Vec<u8>, RefractError> {
    reorder_pdf_with_password(bytes, order_str, None, &CancelToken::none())
}

/// Split a PDF document into multiple PDFs by page ranges, with optional password and cancellation.
/// Empty `ranges_spec` splits every page into its own PDF (matching Web).
/// Preserves inherited `/MediaBox`, `/CropBox`, `/Resources`, and `/Rotate` attributes.
pub fn split_pdf_with_password(
    bytes: &[u8],
    ranges_spec: &str,
    password: Option<&str>,
    cancel: &CancelToken,
) -> Result<Vec<(String, Vec<u8>)>, RefractError> {
    cancel.check()?;
    let doc = load_document(bytes, password)?;
    cancel.check()?;

    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;
    let groups = parse_ranges(ranges_spec, total_pages)?;
    let mut outputs = Vec::with_capacity(groups.len());

    for group in groups {
        cancel.check()?;
        let mut group_doc = doc.clone();
        let pages_ref = group_doc
            .catalog()
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Missing PDF catalog",
                    e.to_string(),
                )
            })?
            .get(b"Pages")
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Missing PDF Pages reference",
                    e.to_string(),
                )
            })?
            .as_reference()
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Invalid PDF Pages reference",
                    e.to_string(),
                )
            })?;

        let group_ids: Vec<ObjectId> = group.iter().map(|&p| pages[&p]).collect();
        let group_kids: Vec<Object> = group_ids.iter().map(|&id| Object::Reference(id)).collect();

        // Materialize inherited attributes BEFORE reparenting & pruning ancestor `/Pages` nodes.
        materialize_inherited_attributes(&mut group_doc, &group_ids, pages_ref);

        let pages_dict = group_doc
            .get_object_mut(pages_ref)
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Failed to get Pages object",
                    e.to_string(),
                )
            })?
            .as_dict_mut()
            .map_err(|e| {
                RefractError::with_details(
                    ErrorCode::DamagedFile,
                    "Pages object is not a dictionary",
                    e.to_string(),
                )
            })?;

        pages_dict.set("Kids", Object::Array(group_kids));
        pages_dict.set("Count", Object::Integer(group.len() as i64));

        // Remove unreferenced pages and their resources so split files only carry their own pages.
        group_doc.prune_objects();
        group_doc.renumber_objects();

        let mut out = Vec::new();
        cancel.check()?;
        cancellable_stage(cancel, || {
            group_doc.save_to(&mut out).map_err(|e| {
                RefractError::with_details(
                    ErrorCode::EngineFailed,
                    "Failed to save split PDF",
                    e.to_string(),
                )
            })
        })?;

        let label = if group.len() == 1 {
            format!("p{}", group[0])
        } else {
            format!("p{}-{}", group[0], group.last().unwrap())
        };
        outputs.push((label, out));
    }

    cancel.check()?;

    Ok(outputs)
}

/// Split a PDF document into multiple PDFs by page ranges.
pub fn split_pdf(bytes: &[u8], ranges_spec: &str) -> Result<Vec<(String, Vec<u8>)>, RefractError> {
    split_pdf_with_password(bytes, ranges_spec, None, &CancelToken::none())
}

/// Merge multiple PDF documents with independent per-input passwords and cancellation support.
///
/// Preserves exact input order. When any input requires or fails a password, returns
/// `NeedsPassword` or `WrongPassword` with `details = "input_index=<0-based>"` so callers
/// know which input needs unlocking without exposing any password text.
pub fn merge_pdfs_with_inputs(
    inputs: &[PdfMergeInput<'_>],
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    if inputs.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "No PDF files provided to merge",
        ));
    }

    if inputs.len() == 1 {
        let mut doc = load_document(inputs[0].bytes, inputs[0].password)
            .map_err(|e| tag_merge_input_error(e, 0))?;
        let mut out = Vec::new();
        cancel.check()?;
        cancellable_stage(cancel, || {
            doc.save_to(&mut out).map_err(|e| {
                RefractError::with_details(
                    ErrorCode::EngineFailed,
                    "Failed to save merged PDF",
                    e.to_string(),
                )
            })
        })?;
        cancel.check()?;
        return Ok(out);
    }

    let mut target = Document::with_version("1.5");
    let mut kids: Vec<Object> = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        cancel.check()?;
        let doc = load_document(input.bytes, input.password)
            .map_err(|e| tag_merge_input_error(e, index))?;
        merge_document_into(&mut target, &doc, &mut kids)?;
    }

    cancel.check()?;
    finalize_merged(&mut target, kids)?;

    let mut out = Vec::new();
    cancel.check()?;
    cancellable_stage(cancel, || {
        target.save_to(&mut out).map_err(|e| {
            RefractError::with_details(
                ErrorCode::EngineFailed,
                "Failed to save merged PDF",
                e.to_string(),
            )
        })
    })?;

    cancel.check()?;

    Ok(out)
}

const INPUT_INDEX_TAG: &str = "input_index=";

/// Error `details` naming which input (0-based) of a multi-file job a password error is about.
pub fn input_index_details(index: usize) -> String {
    format!("{}{}", INPUT_INDEX_TAG, index)
}

/// Which input (0-based) a password error is about, if it was tagged with [`input_index_details`].
pub fn input_index_of(err: &RefractError) -> Option<usize> {
    err.details
        .as_deref()?
        .strip_prefix(INPUT_INDEX_TAG)?
        .parse()
        .ok()
}

fn tag_merge_input_error(err: RefractError, index: usize) -> RefractError {
    match err.code {
        ErrorCode::NeedsPassword => RefractError::with_details(
            ErrorCode::NeedsPassword,
            format!(
                "Document #{} is protected. Enter its password to continue.",
                index + 1
            ),
            input_index_details(index),
        ),
        ErrorCode::WrongPassword => RefractError::with_details(
            ErrorCode::WrongPassword,
            format!("That password did not unlock document #{}.", index + 1),
            input_index_details(index),
        ),
        _ => err,
    }
}

/// Merge multiple PDF documents into one, preserving the given order.
pub fn merge_pdfs(pdf_list: &[&[u8]]) -> Result<Vec<u8>, RefractError> {
    let inputs: Vec<PdfMergeInput<'_>> = pdf_list
        .iter()
        .map(|&bytes| PdfMergeInput {
            bytes,
            password: None,
        })
        .collect();
    merge_pdfs_with_inputs(&inputs, &CancelToken::none())
}

/// Merge multiple PDF documents, supplying `password` for every encrypted document.
pub fn merge_pdfs_with_password(
    pdf_list: &[&[u8]],
    password: &str,
) -> Result<Vec<u8>, RefractError> {
    let inputs: Vec<PdfMergeInput<'_>> = pdf_list
        .iter()
        .map(|&bytes| PdfMergeInput {
            bytes,
            password: Some(password),
        })
        .collect();
    merge_pdfs_with_inputs(&inputs, &CancelToken::none())
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

    for page_id in pages.values() {
        if let Some(new_id) = id_map.get(page_id) {
            if let Some(inherited) = inherit_page_attributes(doc, *page_id) {
                target.objects.insert(
                    *new_id,
                    remap_object(&Object::Dictionary(inherited), &id_map),
                );
            }
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
    target
        .objects
        .insert(pages_id, Object::Dictionary(pages_dict));

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
    target
        .objects
        .insert(catalog_id, Object::Dictionary(catalog_dict));

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
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|item| remap_object(item, id_map))
                .collect(),
        ),
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

/// Build a PDF document from a list of encoded images (JPEG, PNG or WebP) with
/// configurable page size (`A4`, `Letter`, or `Fit`), DPI, JPEG quality, and cancellation.
pub fn images_to_pdf_with_options(
    images: &[Vec<u8>],
    page_size: PdfPageSize,
    dpi: f64,
    quality: u8,
    cancel: &CancelToken,
) -> Result<Vec<u8>, RefractError> {
    cancel.check()?;
    if images.is_empty() {
        return Err(RefractError::new(
            ErrorCode::EmptyFile,
            "No images were provided",
        ));
    }
    if !dpi.is_finite() || dpi <= 0.0 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "DPI must be a positive finite number",
        ));
    }

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::with_capacity(images.len());

    for bytes in images {
        cancel.check()?;
        let decoded = crate::image::decode_image_cancellable(bytes, cancel)?;
        cancel.check()?;
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

        let (page_w, page_h, draw_x, draw_y, draw_w, draw_h) = match page_size {
            PdfPageSize::Fit => {
                let pw = (width_px as f64 * 72.0 / dpi).max(1.0);
                let ph = (height_px as f64 * 72.0 / dpi).max(1.0);
                (pw, ph, 0.0, 0.0, pw, ph)
            }
            PdfPageSize::A4 | PdfPageSize::Letter => {
                let (pw, ph) = if page_size == PdfPageSize::A4 {
                    A4_POINTS
                } else {
                    LETTER_POINTS
                };
                let iw = width_px.max(1) as f64;
                let ih = height_px.max(1) as f64;
                let scale = (pw / iw).min(ph / ih);
                let dw = iw * scale;
                let dh = ih * scale;
                let x = (pw - dw) / 2.0;
                let y = (ph - dh) / 2.0;
                (pw, ph, x, y, dw, dh)
            }
        };

        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! {
                "Im0" => image_id,
            },
        });

        let content = format!(
            "q {:.2} 0 0 {:.2} {:.2} {:.2} cm /Im0 Do Q",
            draw_w, draw_h, draw_x, draw_y
        );
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));

        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_w as f32),
                Object::Real(page_h as f32),
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
    cancel.check()?;
    cancellable_stage(cancel, || {
        doc.save_to(&mut out).map_err(|e| {
            RefractError::with_details(
                ErrorCode::EngineFailed,
                "Failed to save the PDF",
                e.to_string(),
            )
        })
    })?;

    cancel.check()?;

    Ok(out)
}

/// Build a PDF document from a list of encoded images using `Fit` page sizing at `dpi`.
pub fn images_to_pdf(images: &[Vec<u8>], dpi: f64, quality: u8) -> Result<Vec<u8>, RefractError> {
    let dpi = if dpi <= 0.0 { 150.0 } else { dpi };
    images_to_pdf_with_options(images, PdfPageSize::Fit, dpi, quality, &CancelToken::none())
}

/// Extract the images that are embedded in a PDF, page by page, with password & cancellation support.
///
/// Resolves inherited `/Resources` dictionaries so images declared on parent `/Pages` nodes are also found.
pub fn extract_embedded_images_with_password(
    bytes: &[u8],
    password: Option<&str>,
    cancel: &CancelToken,
) -> Result<Vec<EmbeddedImage>, RefractError> {
    cancel.check()?;
    let doc = load_document(bytes, password)?;
    let mut results = Vec::new();

    for (page_number, page_id) in doc.get_pages() {
        cancel.check()?;
        let Some(page) = inherit_page_attributes(&doc, page_id) else {
            continue;
        };

        let resources = match doc.get_dict_in_dict(&page, b"Resources") {
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

            cancel.check()?;
            if let Some(extracted) = cancellable_stage(cancel, || Ok(decode_image_stream(stream)))?
            {
                results.push(EmbeddedImage {
                    page: page_number,
                    bytes: extracted.0,
                    extension: extracted.1,
                });
            }
        }
    }

    cancel.check()?;

    Ok(results)
}

/// Extract embedded images from a PDF (auto-unlocks owner-only PDFs).
pub fn extract_embedded_images(bytes: &[u8]) -> Result<Vec<EmbeddedImage>, RefractError> {
    extract_embedded_images_with_password(bytes, None, &CancelToken::none())
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

    let expected = width as usize * height as usize * channels;
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
    use std::path::PathBuf;

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        std::fs::read(manifest_dir.join("../../fixtures").join(name))
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", name, e))
    }

    /// Build a minimal valid PDF whose pages have distinct MediaBox widths (`widths`),
    /// making page ordering directly observable after merge/split/reorder.
    fn make_sized_pdf(widths: &[i64]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for &w in widths {
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), w.into(), 400.into()],
            });
            kids.push(Object::Reference(page_id));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => widths.len() as i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save synthetic PDF");
        out
    }

    /// Encrypt a synthetic PDF with `user_pw` and `owner_pw` using `lopdf` V2 (RC4-128).
    fn encrypt_pdf(bytes: &[u8], user_pw: &str, owner_pw: &str) -> Vec<u8> {
        let mut doc = Document::load_mem(bytes).expect("load synthetic PDF");
        let id_bytes = format!("refract-test-id-{}", user_pw).into_bytes();
        doc.trailer.set(
            "ID",
            vec![
                Object::String(id_bytes.clone(), lopdf::StringFormat::Hexadecimal),
                Object::String(id_bytes, lopdf::StringFormat::Hexadecimal),
            ],
        );
        let version = EncryptionVersion::V2 {
            document: &doc,
            owner_password: owner_pw,
            user_password: user_pw,
            key_length: 128,
            permissions: Permissions::all(),
        };
        let state = EncryptionState::try_from(version).expect("build encryption state");
        doc.encrypt(&state).expect("encrypt synthetic PDF");
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save encrypted synthetic PDF");
        out
    }

    /// Build a 2-page PDF with a nested `/Pages` tree where an intermediate `/Pages` node
    /// defines `/MediaBox`, `/CropBox`, `/Resources`, `/Rotate`, and a non-inheritable key.
    fn make_nested_pages_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let root_pages_id = doc.new_object_id();
        let mid_pages_id = doc.new_object_id();

        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });

        let page1_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => mid_pages_id,
        });
        let page2_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => mid_pages_id,
        });

        doc.objects.insert(
            mid_pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_pages_id,
                "Kids" => vec![Object::Reference(page1_id), Object::Reference(page2_id)],
                "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 500.into(), 700.into()],
                "CropBox" => vec![10.into(), 10.into(), 490.into(), 690.into()],
                "Resources" => resources_id,
                "Rotate" => 90,
                "NonInheritableParentTag" => "do-not-copy",
            }),
        );

        doc.objects.insert(
            root_pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(mid_pages_id)],
                "Count" => 2,
            }),
        );

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => root_pages_id,
        });
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save nested pages PDF");
        out
    }

    fn page_mediabox_wh(doc: &Document, page_num: u32) -> (f64, f64) {
        let pages = doc.get_pages();
        let pid = pages[&page_num];
        let dict = inherit_page_attributes(doc, pid).expect("page dict");
        let arr = dict
            .get(b"MediaBox")
            .expect("MediaBox")
            .as_array()
            .expect("MediaBox array");
        let num = |o: &Object| match o {
            Object::Integer(i) => *i as f64,
            Object::Real(f) => *f as f64,
            other => panic!("unexpected MediaBox value {:?}", other),
        };
        (num(&arr[2]) - num(&arr[0]), num(&arr[3]) - num(&arr[1]))
    }

    #[test]
    fn owner_only_pdf_opens_without_password_prompt_across_operations() {
        let owner_only = fixture_bytes("owner-only.pdf");

        let info = probe_pdf(&owner_only).expect("probe owner-only.pdf");
        assert!(info.is_encrypted);
        assert!(
            !info.requires_password,
            "owner-only PDF must not require a user password"
        );
        assert_eq!(info.page_count, 3);

        // All PDF operations should succeed without prompting for a password.
        let compressed = compress_pdf(&owner_only).expect("compress owner-only.pdf");
        let c_info = probe_pdf(&compressed).expect("probe compressed owner-only");
        assert_eq!(c_info.page_count, 3);
        assert!(!c_info.requires_password);

        let parts = split_pdf(&owner_only, "").expect("split owner-only.pdf empty range");
        assert_eq!(parts.len(), 3);

        let reordered = reorder_pdf(&owner_only, "3, 1, 2").expect("reorder owner-only.pdf");
        assert_eq!(probe_pdf(&reordered).unwrap().page_count, 3);

        let plain = fixture_bytes("plain.pdf");
        let merged = merge_pdfs(&[&owner_only, &plain]).expect("merge owner-only + plain");
        assert_eq!(probe_pdf(&merged).unwrap().page_count, 6);

        let embedded = extract_embedded_images(&owner_only).expect("extract owner-only");
        assert!(embedded.is_empty());
    }

    #[test]
    fn locked_pdf_needs_wrong_and_right_password_across_all_operations() {
        let locked = fixture_bytes("locked.pdf");
        let wrong_pw = "definitely-wrong-secret-xyz";
        let right_pw = "user-pw-123";

        let info = probe_pdf(&locked).expect("probe locked.pdf");
        assert!(info.is_encrypted);
        assert!(info.requires_password);

        // No password -> NeedsPassword across all operations
        assert_eq!(
            compress_pdf(&locked).unwrap_err().code,
            ErrorCode::NeedsPassword
        );
        assert_eq!(
            split_pdf(&locked, "1-2").unwrap_err().code,
            ErrorCode::NeedsPassword
        );
        assert_eq!(
            reorder_pdf(&locked, "3, 1, 2").unwrap_err().code,
            ErrorCode::NeedsPassword
        );
        assert_eq!(
            extract_embedded_images(&locked).unwrap_err().code,
            ErrorCode::NeedsPassword
        );

        // Wrong password -> WrongPassword and password text is NEVER leaked in error messages/details
        let err = compress_pdf_with_password(&locked, wrong_pw).unwrap_err();
        assert_eq!(err.code, ErrorCode::WrongPassword);
        assert!(!err.message.contains(wrong_pw));
        assert!(!err
            .details
            .as_deref()
            .unwrap_or_default()
            .contains(wrong_pw));

        assert_eq!(
            split_pdf_with_password(&locked, "1-2", Some(wrong_pw), &CancelToken::none())
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        assert_eq!(
            reorder_pdf_with_password(&locked, "3, 1, 2", Some(wrong_pw), &CancelToken::none())
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        assert_eq!(
            extract_embedded_images_with_password(&locked, Some(wrong_pw), &CancelToken::none())
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );

        // Right password -> succeeds on retry across every operation
        let compressed = compress_pdf_with_password(&locked, right_pw).expect("compress locked");
        let c_info = probe_pdf(&compressed).unwrap();
        assert_eq!(c_info.page_count, 3);
        assert!(!c_info.is_encrypted);

        let parts =
            split_pdf_with_password(&locked, "1-2, 3", Some(right_pw), &CancelToken::none())
                .expect("split locked");
        assert_eq!(parts.len(), 2);
        assert_eq!(probe_pdf(&parts[0].1).unwrap().page_count, 2);
        assert_eq!(probe_pdf(&parts[1].1).unwrap().page_count, 1);

        // Reorder non-identity AND identity order with password
        let reordered =
            reorder_pdf_with_password(&locked, "3, 1, 2", Some(right_pw), &CancelToken::none())
                .expect("reorder locked");
        assert_eq!(probe_pdf(&reordered).unwrap().page_count, 3);

        let identity =
            reorder_pdf_with_password(&locked, "1, 2, 3", Some(right_pw), &CancelToken::none())
                .expect("identity reorder locked must not bypass password");
        let id_info = probe_pdf(&identity).unwrap();
        assert_eq!(id_info.page_count, 3);
        assert!(!id_info.is_encrypted);

        // Extract embedded images with password succeeds
        let extracted =
            extract_embedded_images_with_password(&locked, Some(right_pw), &CancelToken::none())
                .expect("extract embedded locked");
        assert!(extracted.is_empty());
    }

    #[test]
    fn merge_supports_mixed_and_different_passwords_per_input_and_preserves_order() {
        let plain = make_sized_pdf(&[300, 310]);
        let enc_a_raw = make_sized_pdf(&[400]);
        let enc_b_raw = make_sized_pdf(&[500, 510]);
        let enc_a = encrypt_pdf(&enc_a_raw, "alpha-pw-1", "owner-a");
        let enc_b = encrypt_pdf(&enc_b_raw, "beta-pw-2", "owner-b");
        let owner_only = fixture_bytes("owner-only.pdf");

        // Missing password on index 1 identifies document #2
        let missing_err = merge_pdfs_with_inputs(
            &[
                PdfMergeInput {
                    bytes: &plain,
                    password: None,
                },
                PdfMergeInput {
                    bytes: &enc_a,
                    password: None,
                },
            ],
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(missing_err.code, ErrorCode::NeedsPassword);
        assert_eq!(missing_err.details.as_deref(), Some("input_index=1"));

        // Wrong password on index 2 identifies document #3 without leaking the password
        let wrong_err = merge_pdfs_with_inputs(
            &[
                PdfMergeInput {
                    bytes: &plain,
                    password: None,
                },
                PdfMergeInput {
                    bytes: &enc_a,
                    password: Some("alpha-pw-1"),
                },
                PdfMergeInput {
                    bytes: &enc_b,
                    password: Some("wrong-beta-pw"),
                },
            ],
            &CancelToken::none(),
        )
        .unwrap_err();
        assert_eq!(wrong_err.code, ErrorCode::WrongPassword);
        assert_eq!(wrong_err.details.as_deref(), Some("input_index=2"));
        assert!(!wrong_err.message.contains("wrong-beta-pw"));

        // Retry with distinct per-input passwords + owner-only + unencrypted preserves exact order
        let merged = merge_pdfs_with_inputs(
            &[
                PdfMergeInput {
                    bytes: &plain,
                    password: None,
                },
                PdfMergeInput {
                    bytes: &enc_a,
                    password: Some("alpha-pw-1"),
                },
                PdfMergeInput {
                    bytes: &owner_only,
                    password: None,
                },
                PdfMergeInput {
                    bytes: &enc_b,
                    password: Some("beta-pw-2"),
                },
            ],
            &CancelToken::none(),
        )
        .expect("merge mixed encrypted inputs");

        let merged_doc = Document::load_mem(&merged).expect("parse merged PDF");
        assert!(!merged_doc.is_encrypted());
        // 2 + 1 + 3 + 2 = 8 pages
        assert_eq!(merged_doc.get_pages().len(), 8);
        assert_eq!(page_mediabox_wh(&merged_doc, 1).0, 300.0);
        assert_eq!(page_mediabox_wh(&merged_doc, 2).0, 310.0);
        assert_eq!(page_mediabox_wh(&merged_doc, 3).0, 400.0);
        assert_eq!(page_mediabox_wh(&merged_doc, 7).0, 500.0);
        assert_eq!(page_mediabox_wh(&merged_doc, 8).0, 510.0);
    }

    #[test]
    fn nested_pages_inherited_attributes_preserved_in_split_and_reorder() {
        let nested = make_nested_pages_pdf();

        // Split every page (empty range)
        let split_parts = split_pdf(&nested, "").expect("split nested pages PDF");
        assert_eq!(split_parts.len(), 2);
        for (_, part_bytes) in &split_parts {
            let doc = Document::load_mem(part_bytes).expect("load split part");
            assert_eq!(doc.get_pages().len(), 1);
            let page_id = doc.get_pages()[&1];
            let page_dict = doc.get_dictionary(page_id).expect("page dict");

            assert_eq!(page_mediabox_wh(&doc, 1), (500.0, 700.0));
            assert!(page_dict.has(b"CropBox"), "CropBox must be inherited");
            assert!(page_dict.has(b"Resources"), "Resources must be inherited");
            assert_eq!(
                page_dict.get(b"Rotate").unwrap().as_i64().unwrap(),
                90,
                "Rotate must be inherited"
            );
            assert!(
                !page_dict.has(b"NonInheritableParentTag"),
                "arbitrary parent keys must NOT be copied onto pages"
            );
        }

        // Reorder pages (non-identity)
        let reordered = reorder_pdf(&nested, "2, 1").expect("reorder nested pages PDF");
        let rdoc = Document::load_mem(&reordered).expect("load reordered nested PDF");
        assert_eq!(rdoc.get_pages().len(), 2);
        for page_num in [1u32, 2u32] {
            let page_id = rdoc.get_pages()[&page_num];
            let page_dict = rdoc.get_dictionary(page_id).expect("page dict");
            assert_eq!(page_mediabox_wh(&rdoc, page_num), (500.0, 700.0));
            assert!(page_dict.has(b"CropBox"));
            assert!(page_dict.has(b"Resources"));
            assert_eq!(page_dict.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
            assert!(!page_dict.has(b"NonInheritableParentTag"));
        }
    }

    #[test]
    fn invalid_ranges_orders_empty_and_corrupt_pdfs_rejected_cleanly() {
        assert_eq!(
            parse_ranges("a-b", 5).unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_ranges("4-2", 5).unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_ranges("0", 5).unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_ranges("6", 5).unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_ranges(" , ", 5).unwrap_err().code,
            ErrorCode::InvalidOption
        );

        assert_eq!(
            parse_order("1, 1, 2", 3).unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_order("1, 2", 3).unwrap_err().code,
            ErrorCode::InvalidOption
        );
        assert_eq!(
            parse_order("1, 2, 9", 3).unwrap_err().code,
            ErrorCode::InvalidOption
        );

        assert_eq!(
            load_document(&[], None).unwrap_err().code,
            ErrorCode::EmptyFile
        );
        assert_eq!(
            load_document(b"not a pdf at all", None).unwrap_err().code,
            ErrorCode::DamagedFile
        );
    }

    #[test]
    fn images_to_pdf_supports_a4_letter_and_fit_for_portrait_landscape_and_multi_images() {
        let landscape = fixture_bytes("plain.jpg"); // 400x300
        let portrait = fixture_bytes("rotated.jpg"); // EXIF 6 -> 300x400 upright
        let png = fixture_bytes("plain.png"); // 400x300 RGBA

        // Invalid page size must error (never silently fall back)
        assert_eq!(
            PdfPageSize::parse("tabloid").unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // A4 multi-image
        let a4_bytes = images_to_pdf_with_options(
            &[landscape.clone(), portrait.clone(), png.clone()],
            PdfPageSize::A4,
            150.0,
            85,
            &CancelToken::none(),
        )
        .expect("images_to_pdf A4");
        let a4_doc = Document::load_mem(&a4_bytes).unwrap();
        assert_eq!(a4_doc.get_pages().len(), 3);
        let (w, h) = page_mediabox_wh(&a4_doc, 1);
        assert!((w - A4_POINTS.0).abs() < 0.1 && (h - A4_POINTS.1).abs() < 0.1);

        // Letter multi-image
        let letter_bytes = images_to_pdf_with_options(
            &[landscape.clone(), portrait.clone()],
            PdfPageSize::Letter,
            150.0,
            85,
            &CancelToken::none(),
        )
        .expect("images_to_pdf Letter");
        let letter_doc = Document::load_mem(&letter_bytes).unwrap();
        assert_eq!(letter_doc.get_pages().len(), 2);
        let (lw, lh) = page_mediabox_wh(&letter_doc, 2);
        assert!((lw - LETTER_POINTS.0).abs() < 0.1 && (lh - LETTER_POINTS.1).abs() < 0.1);

        // Fit at 72 DPI preserves exact pixel dimensions (landscape 400x300, rotated portrait 300x400)
        let fit_bytes = images_to_pdf_with_options(
            &[landscape, portrait],
            PdfPageSize::Fit,
            72.0,
            90,
            &CancelToken::none(),
        )
        .expect("images_to_pdf Fit");
        let fit_doc = Document::load_mem(&fit_bytes).unwrap();
        assert_eq!(fit_doc.get_pages().len(), 2);
        assert_eq!(page_mediabox_wh(&fit_doc, 1), (400.0, 300.0));
        assert_eq!(page_mediabox_wh(&fit_doc, 2), (300.0, 400.0));
    }

    #[test]
    fn secret_password_redacts_debug_and_clears_buffer() {
        let mut pw = SecretPassword::new("super-secret-value");
        assert_eq!(pw.as_str(), "super-secret-value");
        assert_eq!(format!("{:?}", pw), "SecretPassword([REDACTED])");
        assert!(!format!("{:?}", pw).contains("super-secret-value"));
        pw.clear();
        assert_eq!(pw.as_str(), "");
    }
    #[test]
    fn cancellation_during_final_stage_overrides_success() {
        let token = CancelToken::new();
        let result = cancellable_stage(&token, || {
            let mut doc = Document::with_version("1.5");
            let mut out = Vec::new();
            doc.save_to(&mut out).unwrap();
            token.cancel();
            Ok(out)
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::Canceled);
    }
}
