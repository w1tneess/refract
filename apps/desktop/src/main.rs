slint::include_modules!();

use refract_core::format::Format;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug)]
struct JobRecord {
    id: i32,
    input_paths: Vec<PathBuf>,
    name: String,
    size_bytes: u64,
    tool: String,
    status: String,
    error: String,
    output_path: String,
}

#[derive(Clone, Debug)]
struct JobOptions {
    image_format: String,
    image_quality: u8,
    target_kb: u32,
    resize_w: u32,
    resize_h: u32,
    audio_format: String,
    audio_bitrate: u32,
    trim_start: String,
    trim_end: String,
    pdf_ranges: String,
    pdf_order: String,
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 KB".to_string();
    }
    if bytes < 1024 * 1024 {
        format!("{} KB", (bytes as f64 / 1024.0).max(1.0).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn get_filter_for_tool(tool: &str) -> (&'static str, &'static [&'static str]) {
    match tool {
        "Convert audio" | "Trim audio" => ("Audio files", &["mp3", "wav", "m4a", "ogg"]),
        "Merge PDF" | "Split PDF" | "Reorder PDF" | "Compress PDF" => ("PDF documents", &["pdf"]),
        _ => ("Image files", &["jpg", "jpeg", "png", "webp", "heic"]),
    }
}

fn map_format_str(fmt: &str) -> Format {
    match fmt.to_lowercase().as_str() {
        "png" => Format::Png,
        "webp" => Format::Webp,
        "mp3" => Format::Mp3,
        "wav" => Format::Wav,
        "m4a" => Format::M4a,
        "ogg" => Format::Ogg,
        "pdf" => Format::Pdf,
        "heic" => Format::Heic,
        _ => Format::Jpeg,
    }
}

/// Build a non-colliding output path in `dir` (appending ` (2)`, ` (3)`, ... if the file already exists).
fn unique_output_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{}.{}", base, ext));
    if !first.exists() {
        return first;
    }
    for n in 2..10_000u32 {
        let candidate = dir.join(format!("{} ({}).{}", base, n, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

fn main() -> Result<(), slint::PlatformError> {
    let main_window = MainWindow::new()?;

    let jobs_store: Arc<Mutex<Vec<JobRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let next_id = Arc::new(Mutex::new(1i32));
    let is_busy = Arc::new(AtomicBool::new(false));

    // Callback: Pick files
    {
        let window_weak = main_window.as_weak();
        let jobs_store = Arc::clone(&jobs_store);
        let next_id = Arc::clone(&next_id);

        main_window.on_pick_files(move || {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };

            let tool = window.get_selected_tool().to_string();
            let (filter_name, extensions) = get_filter_for_tool(&tool);

            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.add_filter(filter_name, extensions);

            if let Some(files) = dialog.pick_files() {
                if files.is_empty() {
                    return;
                }
                let mut store = jobs_store.lock().unwrap();
                let mut id_lock = next_id.lock().unwrap();

                if tool == "Merge PDF" {
                    // Combine multi-selected PDFs into a single Merge PDF job (or append to an existing waiting Merge PDF job).
                    let total_size: u64 = files
                        .iter()
                        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
                        .sum();
                    if let Some(existing) = store.iter_mut().find(|j| j.tool == "Merge PDF" && j.status == "Waiting") {
                        existing.input_paths.extend(files);
                        existing.size_bytes += total_size;
                        let first_name = existing.input_paths[0]
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "document.pdf".to_string());
                        let count = existing.input_paths.len();
                        existing.name = if count > 1 {
                            format!("{} + {} more", first_name, count - 1)
                        } else {
                            first_name
                        };
                    } else {
                        let first_name = files[0]
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "document.pdf".to_string());
                        let name = if files.len() > 1 {
                            format!("{} + {} more", first_name, files.len() - 1)
                        } else {
                            first_name
                        };
                        let id = *id_lock;
                        *id_lock += 1;
                        store.push(JobRecord {
                            id,
                            input_paths: files,
                            name,
                            size_bytes: total_size,
                            tool: tool.clone(),
                            status: "Waiting".to_string(),
                            error: String::new(),
                            output_path: String::new(),
                        });
                    }
                } else {
                    for path in files {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Unknown".to_string());
                        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let id = *id_lock;
                        *id_lock += 1;

                        store.push(JobRecord {
                            id,
                            input_paths: vec![path],
                            name,
                            size_bytes,
                            tool: tool.clone(),
                            status: "Waiting".to_string(),
                            error: String::new(),
                            output_path: String::new(),
                        });
                    }
                }

                update_ui_jobs(&window, &store);
            }
        });
    }

    // Callback: Clear jobs
    {
        let window_weak = main_window.as_weak();
        let jobs_store = Arc::clone(&jobs_store);

        main_window.on_clear_jobs(move || {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };

            let mut store = jobs_store.lock().unwrap();
            store.clear();
            update_ui_jobs(&window, &store);
        });
    }

    // Callback: Open output folder
    main_window.on_open_output_folder(|path_str| {
        let p = Path::new(path_str.as_str());
        let to_open = if p.is_file() {
            p.parent().unwrap_or(p)
        } else {
            p
        };
        let _ = open::that_detached(to_open);
    });

    // Callback: Start processing
    {
        let window_weak = main_window.as_weak();
        let jobs_store = Arc::clone(&jobs_store);
        let is_busy = Arc::clone(&is_busy);

        main_window.on_start_processing(move || {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };

            if is_busy.swap(true, Ordering::SeqCst) {
                return;
            }

            window.set_is_running(true);

            // Snapshot options from UI
            let opts = JobOptions {
                image_format: window.get_opt_image_format().to_string(),
                image_quality: window.get_opt_image_quality().clamp(1, 100) as u8,
                target_kb: window.get_opt_target_kb().trim().parse().unwrap_or(200),
                resize_w: window.get_opt_resize_width().trim().parse().unwrap_or(800),
                resize_h: window.get_opt_resize_height().trim().parse().unwrap_or(0),
                audio_format: window.get_opt_audio_format().to_string(),
                audio_bitrate: window.get_opt_audio_bitrate().trim().parse().unwrap_or(192),
                trim_start: window.get_opt_trim_start().to_string(),
                trim_end: window.get_opt_trim_end().to_string(),
                pdf_ranges: window.get_opt_pdf_ranges().to_string(),
                pdf_order: window.get_opt_pdf_order().to_string(),
            };

            let worker_window = window_weak.clone();
            let worker_jobs = Arc::clone(&jobs_store);
            let worker_busy = Arc::clone(&is_busy);

            thread::spawn(move || {
                let jobs_to_run = {
                    let store = worker_jobs.lock().unwrap();
                    store.clone()
                };

                for job in jobs_to_run {
                    if job.status == "Done" {
                        continue;
                    }

                    // Set job to Working and clear any previous error
                    {
                        let mut store = worker_jobs.lock().unwrap();
                        if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                            j.status = "Working".to_string();
                            j.error.clear();
                        }
                        let snapshot = store.clone();
                        let w_weak = worker_window.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_weak.upgrade() {
                                update_ui_jobs(&w, &snapshot);
                            }
                        });
                    }

                    // Execute operation
                    let result = process_job(&job, &opts);

                    // Update result
                    {
                        let mut store = worker_jobs.lock().unwrap();
                        if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                            match result {
                                Ok(out_path) => {
                                    j.status = "Done".to_string();
                                    j.error.clear();
                                    j.output_path = out_path.to_string_lossy().to_string();
                                }
                                Err(err) => {
                                    j.status = "Failed".to_string();
                                    j.error = err;
                                }
                            }
                        }

                        let snapshot = store.clone();
                        let w_weak = worker_window.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_weak.upgrade() {
                                update_ui_jobs(&w, &snapshot);
                            }
                        });
                    }
                }

                worker_busy.store(false, Ordering::SeqCst);
                let w_weak = worker_window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = w_weak.upgrade() {
                        w.set_is_running(false);
                    }
                });
            });
        });
    }

    main_window.run()
}

fn update_ui_jobs(window: &MainWindow, records: &[JobRecord]) {
    let entries: Vec<JobEntry> = records
        .iter()
        .map(|r| JobEntry {
            id: r.id,
            name: r.name.clone().into(),
            size_text: format_size(r.size_bytes).into(),
            tool_label: r.tool.clone().into(),
            status: r.status.clone().into(),
            error: r.error.clone().into(),
            output_path: r.output_path.clone().into(),
        })
        .collect();

    let model = Rc::new(slint::VecModel::from(entries));
    window.set_jobs(model.into());
}

fn process_job(job: &JobRecord, opts: &JobOptions) -> Result<PathBuf, String> {
    let first_path = job
        .input_paths
        .first()
        .ok_or_else(|| "Job has no input files".to_string())?;
    let parent = first_path.parent().unwrap_or(Path::new("."));
    let stem = first_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");

    match job.tool.as_str() {
        "Convert image" => {
            let bytes = std::fs::read(first_path).map_err(|e| e.to_string())?;
            let target_format = map_format_str(&opts.image_format);
            let out_bytes = refract_engine::image::convert_image(&bytes, target_format, Some(opts.image_quality))
                .map_err(|e| e.to_string())?;
            let out_path = unique_output_path(parent, &format!("{} (converted)", stem), target_format.primary_extension());
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Compress image" => {
            let bytes = std::fs::read(first_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::image::compress_image(&bytes, opts.target_kb).map_err(|e| e.to_string())?;
            let out_path = unique_output_path(parent, &format!("{} (compressed)", stem), "jpg");
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Resize image" => {
            let bytes = std::fs::read(first_path).map_err(|e| e.to_string())?;
            let (out_bytes, out_fmt) =
                refract_engine::image::resize_image_with_format(&bytes, opts.resize_w, opts.resize_h, None)
                    .map_err(|e| e.to_string())?;
            let out_path = unique_output_path(parent, &format!("{} (resized)", stem), out_fmt.primary_extension());
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Convert audio" => {
            let target_format = map_format_str(&opts.audio_format);
            let out_path = unique_output_path(parent, &format!("{} (converted)", stem), target_format.primary_extension());
            refract_engine::audio::convert_audio(first_path, &out_path, target_format, Some(opts.audio_bitrate))
                .map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Trim audio" => {
            let ext = first_path.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
            let out_path = unique_output_path(parent, &format!("{} (trimmed)", stem), ext);
            let start = (!opts.trim_start.trim().is_empty()).then_some(opts.trim_start.as_str());
            let end = (!opts.trim_end.trim().is_empty()).then_some(opts.trim_end.as_str());
            refract_engine::audio::trim_audio(first_path, &out_path, start, end).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Compress PDF" => {
            let bytes = std::fs::read(first_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::pdf::compress_pdf(&bytes).map_err(|e| e.to_string())?;
            let out_path = unique_output_path(parent, &format!("{} (compressed)", stem), "pdf");
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Split PDF" => {
            let bytes = std::fs::read(first_path).map_err(|e| e.to_string())?;
            let parts = refract_engine::pdf::split_pdf(&bytes, &opts.pdf_ranges).map_err(|e| e.to_string())?;
            let mut last_path = parent.to_path_buf();
            for (label, part_bytes) in parts {
                let out_path = unique_output_path(parent, &format!("{}-{}", stem, label), "pdf");
                std::fs::write(&out_path, part_bytes).map_err(|e| e.to_string())?;
                last_path = out_path;
            }
            Ok(last_path)
        }
        "Reorder PDF" => {
            let bytes = std::fs::read(first_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::pdf::reorder_pdf(&bytes, &opts.pdf_order).map_err(|e| e.to_string())?;
            let out_path = unique_output_path(parent, &format!("{} (reordered)", stem), "pdf");
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Merge PDF" => {
            if job.input_paths.len() < 2 {
                return Err("Merge PDF needs at least 2 PDF files. Select multiple PDFs or add another PDF.".to_string());
            }
            let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(job.input_paths.len());
            for path in &job.input_paths {
                buffers.push(std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?);
            }
            let slices: Vec<&[u8]> = buffers.iter().map(|b| b.as_slice()).collect();
            let out_bytes = refract_engine::pdf::merge_pdfs(&slices).map_err(|e| e.to_string())?;
            let out_path = unique_output_path(parent, &format!("{} (merged)", stem), "pdf");
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        other => Err(format!("Unsupported tool: {}", other)),
    }
}
