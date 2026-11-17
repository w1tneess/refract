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
    input_path: PathBuf,
    name: String,
    size_bytes: u64,
    tool: String,
    status: String,
    error: String,
    output_path: String,
}

fn format_size(bytes: u64) -> String {
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
        "wav" => Format::Wav,
        "m4a" => Format::M4a,
        "ogg" => Format::Ogg,
        "pdf" => Format::Pdf,
        "heic" => Format::Heic,
        _ => Format::Jpeg,
    }
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
                let mut store = jobs_store.lock().unwrap();
                let mut id_lock = next_id.lock().unwrap();

                for path in files {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Unknown".to_string());
                    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let id = *id_lock;
                    *id_lock += 1;

                    store.push(JobRecord {
                        id,
                        input_path: path,
                        name,
                        size_bytes,
                        tool: tool.clone(),
                        status: "Waiting".to_string(),
                        error: String::new(),
                        output_path: String::new(),
                    });
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
            if is_busy.swap(true, Ordering::SeqCst) {
                return;
            }

            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };

            window.set_is_running(true);

            // Snapshot options from UI
            let image_format = window.get_opt_image_format().to_string();
            let image_quality = window.get_opt_image_quality() as u8;
            let target_kb: u32 = window.get_opt_target_kb().parse().unwrap_or(200);
            let resize_w: u32 = window.get_opt_resize_width().parse().unwrap_or(800);
            let resize_h: u32 = window.get_opt_resize_height().parse().unwrap_or(0);
            let audio_format = window.get_opt_audio_format().to_string();
            let audio_bitrate: u32 = window.get_opt_audio_bitrate().parse().unwrap_or(192);
            let trim_start = window.get_opt_trim_start().to_string();
            let trim_end = window.get_opt_trim_end().to_string();
            let pdf_order = window.get_opt_pdf_order().to_string();

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

                    // Set job to Working
                    {
                        let mut store = worker_jobs.lock().unwrap();
                        if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                            j.status = "Working".to_string();
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
                    let result = process_job(
                        &job,
                        &image_format,
                        image_quality,
                        target_kb,
                        resize_w,
                        resize_h,
                        &audio_format,
                        audio_bitrate,
                        &trim_start,
                        &trim_end,
                        &pdf_order,
                    );

                    // Update result
                    {
                        let mut store = worker_jobs.lock().unwrap();
                        if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                            match result {
                                Ok(out_path) => {
                                    j.status = "Done".to_string();
                                    j.output_path = out_path.to_string_lossy().to_string();
                                }
                                Err(err) => {
                                    j.status = "Failed".to_string();
                                    j.error = err.to_string();
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

fn process_job(
    job: &JobRecord,
    image_fmt: &str,
    image_quality: u8,
    target_kb: u32,
    resize_w: u32,
    resize_h: u32,
    audio_fmt: &str,
    audio_bitrate: u32,
    trim_start: &str,
    trim_end: &str,
    pdf_order: &str,
) -> Result<PathBuf, String> {
    let parent = job.input_path.parent().unwrap_or(Path::new("."));
    let stem = job.input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");

    match job.tool.as_str() {
        "Convert image" => {
            let bytes = std::fs::read(&job.input_path).map_err(|e| e.to_string())?;
            let target_format = map_format_str(image_fmt);
            let out_bytes = refract_engine::image::convert_image(&bytes, target_format, Some(image_quality))
                .map_err(|e| e.to_string())?;
            let out_path = parent.join(format!("{} (converted).{}", stem, target_format.primary_extension()));
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Compress image" => {
            let bytes = std::fs::read(&job.input_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::image::compress_image(&bytes, target_kb).map_err(|e| e.to_string())?;
            let out_path = parent.join(format!("{} (compressed).jpg", stem));
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Resize image" => {
            let bytes = std::fs::read(&job.input_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::image::resize_image(&bytes, resize_w, resize_h).map_err(|e| e.to_string())?;
            let out_path = parent.join(format!("{} (resized).jpg", stem));
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Convert audio" => {
            let target_format = map_format_str(audio_fmt);
            let out_path = parent.join(format!("{} (converted).{}", stem, target_format.primary_extension()));
            refract_engine::audio::convert_audio(&job.input_path, &out_path, target_format, Some(audio_bitrate))
                .map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Trim audio" => {
            let ext = job.input_path.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
            let out_path = parent.join(format!("{} (trimmed).{}", stem, ext));
            refract_engine::audio::trim_audio(&job.input_path, &out_path, Some(trim_start), Some(trim_end))
                .map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Compress PDF" => {
            let bytes = std::fs::read(&job.input_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::pdf::compress_pdf(&bytes).map_err(|e| e.to_string())?;
            let out_path = parent.join(format!("{} (compressed).pdf", stem));
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        "Reorder PDF" => {
            let bytes = std::fs::read(&job.input_path).map_err(|e| e.to_string())?;
            let out_bytes = refract_engine::pdf::reorder_pdf(&bytes, pdf_order).map_err(|e| e.to_string())?;
            let out_path = parent.join(format!("{} (reordered).pdf", stem));
            std::fs::write(&out_path, out_bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
        _ => {
            let bytes = std::fs::read(&job.input_path).map_err(|e| e.to_string())?;
            let out_path = parent.join(format!("{} (output).bin", stem));
            std::fs::write(&out_path, bytes).map_err(|e| e.to_string())?;
            Ok(out_path)
        }
    }
}
