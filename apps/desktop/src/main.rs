slint::include_modules!();

use refract_core::format::Format;
use refract_engine::{image, pdf, audio, render};
use slint::{Model, VecModel, SharedString, ComponentHandle};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

#[derive(Clone, Debug)]
struct JobRecord {
    id: i32,
    input_path: PathBuf,
    name: String,
    size_bytes: u64,
    tool_id: String,
    tool_label: String,
    status: JobStatus,
    error: String,
    output_path: PathBuf,
    progress: f32,
    is_batch_member: bool,
    batch_leader_id: i32,
}

#[derive(Clone, Debug, PartialEq)]
enum JobStatus {
    Waiting,
    Working,
    Done,
    Failed,
    NeedsPassword,
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", (bytes as f64 / 1024.0).max(1.0).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
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

fn format_primary_extension(fmt: Format) -> &'static str {
    match fmt {
        Format::Jpeg => "jpg",
        Format::Png => "png",
        Format::Webp => "webp",
        Format::Heic => "heic",
        Format::Pdf => "pdf",
        Format::Mp3 => "mp3",
        Format::Wav => "wav",
        Format::M4a => "m4a",
        Format::Ogg => "ogg",
    }
}

fn uniquify_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    for i in 1..10000 {
        let new_name = if ext.is_empty() {
            format!("{} ({})", stem, i)
        } else {
            format!("{} ({}).{}", stem, i, ext)
        };
        let candidate = parent.join(new_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

fn get_settings_path() -> PathBuf {
    let mut base = if cfg!(target_os = "windows") {
        std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Application Support")).unwrap_or_else(|| PathBuf::from("."))
    } else {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
    };
    base.push("Refract");
    std::fs::create_dir_all(&base).ok();
    base.push("settings.json");
    base
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
struct Settings {
    is_dark_mode: bool,
    output_folder: String,
    output_next_to_source: bool,
    last_tool_id: String,
}

fn load_settings() -> Settings {
    let path = get_settings_path();
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        Settings::default()
    }
}

fn save_settings(settings: &Settings) {
    let path = get_settings_path();
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        std::fs::write(&path, json).ok();
    }
}

fn to_refract_err(e: std::io::Error) -> refract_core::error::RefractError {
    refract_core::error::RefractError::with_details(
        refract_core::error::ErrorCode::EngineFailed,
        "I/O error",
        e.to_string(),
    )
}

fn main() -> Result<(), slint::PlatformError> {
    let settings = load_settings();

    let main_window = MainWindow::new()?;

    // Initialize tool specs from Rust
    let tool_specs: Vec<_> = refract_core::tools::ToolId::ALL
        .iter()
        .map(|t| {
            let spec = t.spec();
            let mut tool_spec = slint_generatedMainWindow::ToolSpec {
                id: spec.id.id_str().to_string().into(),
                title: spec.title.to_string().into(),
                tagline: spec.tagline.to_string().into(),
                description: spec.description.to_string().into(),
                domain: spec.domain.to_string().into(),
                input_formats: Rc::new(VecModel::default()).into(),
                output_capability: spec.output_capability.into(),
                engine: spec.engine as i32,
                batch: spec.batch,
                min_files: spec.min_files as i32,
                max_files: spec.max_files.unwrap_or(0) as i32,
                extensions: Rc::new(VecModel::default()).into(),
            };
            // Set input_formats and extensions using the VecModel setter
            let input_formats_model = Rc::new(VecModel::from(
                spec.input_formats.iter().map(|s| (*s).into()).collect::<Vec<SharedString>>()
            ));
            tool_spec.input_formats = input_formats_model.into();
            let extensions_model = Rc::new(VecModel::from(
                spec.extensions.iter().map(|s| (*s).into()).collect::<Vec<SharedString>>()
            ));
            tool_spec.extensions = extensions_model.into();
            tool_spec
        })
        .collect();

    // Group tools by domain
    let mut groups = Vec::new();
    for kind in [refract_core::format::Kind::Image, refract_core::format::Kind::Pdf, refract_core::format::Kind::Audio] {
        let kind_str: SharedString = kind.to_string().into();
        let group_tools: Vec<_> = tool_specs.iter()
            .filter(|t| t.domain == kind_str)
            .cloned()
            .collect();
        if !group_tools.is_empty() {
            groups.push(slint_generatedMainWindow::ToolGroup {
                domain_label: kind_str,
                tools: Rc::new(VecModel::from(group_tools)).into(),
            });
        }
    }

    main_window.set_tool_specs(Rc::new(VecModel::from(tool_specs.clone())).into());
    main_window.set_tool_groups(Rc::new(VecModel::from(groups)).into());

    // Apply saved settings
    main_window.set_is_dark_mode(settings.is_dark_mode);
    main_window.set_output_folder(settings.output_folder.clone().into());
    main_window.set_output_next_to_source(settings.output_next_to_source);
    if !settings.last_tool_id.is_empty() {
        main_window.set_selected_tool_id(settings.last_tool_id.clone().into());
    } else if !tool_specs.is_empty() {
        main_window.set_selected_tool_id(tool_specs[0].id.clone());
    }

    // Set initial current tool properties
    if let Some(first_tool) = tool_specs.first() {
        main_window.set_current_tool_title(first_tool.title.clone());
        main_window.set_current_tool_tagline(first_tool.tagline.clone());
        main_window.set_current_tool_description(first_tool.description.clone());
        main_window.set_current_tool_domain(first_tool.domain.clone());
        main_window.set_current_tool_extensions(first_tool.extensions.clone());
        main_window.set_current_tool_extensions_joined(first_tool.extensions.iter().map(|s| s.to_string()).collect::<Vec<String>>().join(", ").into());
        main_window.set_current_tool_batch(first_tool.batch);
    }

    // Engine availability
    main_window.set_ffmpeg_available(audio::is_ffmpeg_available());
    main_window.set_pdf_renderer_available(render::has_renderer());

    // Shared state
    let jobs_store: Arc<Mutex<Vec<JobRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let next_id = Arc::new(Mutex::new(1i32));
    let is_busy = Arc::new(AtomicBool::new(false));
    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>();

    // UI event loop to handle progress updates from worker thread
    let ui_weak = main_window.as_weak();
    slint::invoke_from_event_loop({
        let ui_weak = ui_weak.clone();
        move || {
            let ui_weak = ui_weak.clone();
            std::thread::spawn(move || {
                while let Ok(event) = ui_rx.recv() {
                    let _ = slint::invoke_from_event_loop({
                        let ui_weak = ui_weak.clone();
                        move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                handle_ui_event(&ui, event);
                            }
                        }
                    });
                }
            });
        }
    }).map_err(|e| slint::PlatformError::from(e.to_string()))?;

    // Callbacks
    setup_pick_files(&main_window, &jobs_store, &next_id, &ui_tx);
    setup_pick_output_folder(&main_window, &ui_tx);
    setup_clear_jobs(&main_window, &jobs_store, &ui_tx);
    setup_open_output_folder(&main_window);
    setup_start_processing(&main_window, &jobs_store, &is_busy, &ui_tx, &settings);
    setup_retry_job(&main_window, &jobs_store, &is_busy, &ui_tx, &settings);
    setup_remove_job(&main_window, &jobs_store, &ui_tx);
    setup_move_job(&main_window, &jobs_store, &ui_tx);
    setup_toggle_theme(&main_window, &settings);
    setup_tool_changed(&main_window, &tool_specs, &ui_tx);
    setup_password_handlers(&main_window, &jobs_store, &is_busy, &ui_tx, &settings);

    main_window.run()
}

enum UiEvent {
    JobsUpdated(Vec<JobRecord>),
    StatusUpdate(String, f32),
    JobProgress(i32, f32, JobStatus),
    PasswordNeeded(i32, String),
    EngineAvailability(bool, bool),
}

fn job_record_to_entry(job: &JobRecord) -> slint_generatedMainWindow::JobEntry {
    slint_generatedMainWindow::JobEntry {
        id: job.id,
        name: job.name.clone().into(),
        size_text: format_size(job.size_bytes).into(),
        tool_label: job.tool_label.clone().into(),
        status: match job.status {
            JobStatus::Waiting => "Waiting".into(),
            JobStatus::Working => "Working…".into(),
            JobStatus::Done => "Done".into(),
            JobStatus::Failed => "Failed".into(),
            JobStatus::NeedsPassword => "Needs Password".into(),
        },
        error: job.error.clone().into(),
        output_path: job.output_path.to_string_lossy().to_string().into(),
        progress: job.progress,
    }
}

fn handle_ui_event(ui: &MainWindow, event: UiEvent) {
    match event {
        UiEvent::JobsUpdated(jobs) => {
            let entries: Vec<slint_generatedMainWindow::JobEntry> = jobs.iter().map(job_record_to_entry).collect();
            ui.set_jobs(Rc::new(VecModel::from(entries)).into());
            ui.set_jobs_count_str(jobs.len().to_string().into());
        }
        UiEvent::StatusUpdate(text, progress) => {
            ui.set_status_text(text.into());
            ui.set_overall_progress(progress);
        }
        UiEvent::JobProgress(id, progress, status) => {
            if let Some(model) = ui.get_jobs().clone().into() {
                let entries: Vec<slint_generatedMainWindow::JobEntry> = model.iter().collect();
                if let Some(idx) = entries.iter().position(|e| e.id == id) {
                    let mut new_entries = entries;
                    let status_str: slint::SharedString = match status {
                        JobStatus::Waiting => "Waiting".into(),
                        JobStatus::Working => "Working…".into(),
                        JobStatus::Done => "Done".into(),
                        JobStatus::Failed => "Failed".into(),
                        JobStatus::NeedsPassword => "Needs Password".into(),
                    };
                    new_entries[idx].progress = progress;
                    new_entries[idx].status = status_str;
                    ui.set_jobs(Rc::new(VecModel::from(new_entries)).into());
                }
            }
        }
        UiEvent::PasswordNeeded(job_id, prompt) => {
            ui.set_password_job_id(job_id);
            ui.set_password_prompt(prompt.into());
            ui.set_show_password_dialog(true);
        }
        UiEvent::EngineAvailability(ffmpeg, pdf_renderer) => {
            ui.set_ffmpeg_available(ffmpeg);
            ui.set_pdf_renderer_available(pdf_renderer);
        }
    }
}

fn push_ui_event(tx: &mpsc::Sender<UiEvent>, event: UiEvent) {
    tx.send(event).ok();
}

fn setup_pick_files(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    next_id: &Arc<Mutex<i32>>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let next_id = Arc::clone(next_id);
    let ui_tx = ui_tx.clone();

    window.on_pick_files(move || {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };

        let tool_id = window.get_selected_tool_id().to_string();
        let spec_opt = window.get_tool_specs().iter().find(|s| s.id == tool_id);
        let extensions: Vec<String> = spec_opt.as_ref().map(|s| s.extensions.iter().map(|e| e.to_string()).collect()).unwrap_or_default();

        let mut dialog = rfd::FileDialog::new();
        if !extensions.is_empty() {
            dialog = dialog.add_filter("Supported files", &extensions);
        }
        dialog = dialog.add_filter("All files", &["*"]);

        if let Some(files) = dialog.pick_files() {
            let mut store = jobs_store.lock().unwrap();
            let mut id_lock = next_id.lock().unwrap();

            let mut added = Vec::new();
            for path in files {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Unknown".to_string());
                let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let id = *id_lock;
                *id_lock += 1;

                let tool_label = spec_opt.as_ref().map(|s| s.domain.clone()).unwrap_or_else(|| "File".into());

                let record = JobRecord {
                    id,
                    input_path: path,
                    name,
                    size_bytes,
                    tool_id: tool_id.clone(),
                    tool_label: tool_label.to_string(),
                    status: JobStatus::Waiting,
                    error: String::new(),
                    output_path: PathBuf::new(),
                    progress: -1.0,
                    is_batch_member: spec_opt.as_ref().map(|s| s.batch).unwrap_or(false),
                    batch_leader_id: -1,
                };
                added.push(record);
            }

            if let Some(spec) = spec_opt {
                if spec.batch && added.len() > 1 {
                    let leader_id = added[0].id;
                    for job in &mut added {
                        job.batch_leader_id = leader_id;
                    }
                }
            }

            for job in added {
                store.push(job);
            }

            push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
        }
    });
}

fn setup_pick_output_folder(window: &MainWindow, ui_tx: &mpsc::Sender<UiEvent>) {
    let window_weak = window.as_weak();
    let ui_tx = ui_tx.clone();
    window.on_pick_output_folder(move || {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            window.set_output_folder(folder.to_string_lossy().to_string().into());
            window.set_output_next_to_source(false);
            push_ui_event(&ui_tx, UiEvent::StatusUpdate(format!("Output folder: {}", folder.display()), -1.0));
        }
    });
}

fn setup_clear_jobs(window: &MainWindow, jobs_store: &Arc<Mutex<Vec<JobRecord>>>, ui_tx: &mpsc::Sender<UiEvent>) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let ui_tx = ui_tx.clone();

    window.on_clear_jobs(move || {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let mut store = jobs_store.lock().unwrap();
        store.clear();
        push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
    });
}

fn setup_open_output_folder(window: &MainWindow) {
    window.on_open_output_folder(move |path_str| {
        let p = Path::new(path_str.as_str());
        let to_open = if p.is_file() {
            p.parent().unwrap_or(p)
        } else {
            p
        };
        let _ = open::that_detached(to_open);
    });
}

fn setup_toggle_theme(window: &MainWindow, settings: &Settings) {
    let window_weak = window.as_weak();
    let mut settings = settings.clone();

    window.on_toggle_theme(move || {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let new_mode = !window.get_is_dark_mode();
        window.set_is_dark_mode(new_mode);
        settings.is_dark_mode = new_mode;
        save_settings(&settings);
    });
}

fn setup_tool_changed(window: &MainWindow, tool_specs: &[slint_generatedMainWindow::ToolSpec], ui_tx: &mpsc::Sender<UiEvent>) {
    let window_weak = window.as_weak();
    let ui_tx = ui_tx.clone();
    let tool_specs = tool_specs.to_vec();

    window.on_tool_changed(move |tool_id: SharedString| {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let tool_id_str = tool_id.to_string();

        if let Some(spec) = tool_specs.iter().find(|s| s.id == tool_id_str) {
            window.set_current_tool_title(spec.title.clone());
            window.set_current_tool_tagline(spec.tagline.clone());
            window.set_current_tool_description(spec.description.clone());
            window.set_current_tool_domain(spec.domain.clone());
            window.set_current_tool_extensions(spec.extensions.clone());
            window.set_current_tool_extensions_joined(spec.extensions.iter().map(|s| s.to_string()).collect::<Vec<String>>().join(", ").into());
            window.set_current_tool_batch(spec.batch);
        }

        let mut settings = load_settings();
        settings.last_tool_id = tool_id_str;
        save_settings(&settings);
    });
}

fn setup_password_handlers(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    is_busy: &Arc<AtomicBool>,
    ui_tx: &mpsc::Sender<UiEvent>,
    _settings: &Settings,
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let is_busy = Arc::clone(is_busy);
    let ui_tx = ui_tx.clone();

    window.on_password_entered(move |job_id, password| {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };

        let job_opt = {
            let store = jobs_store.lock().unwrap();
            store.iter().find(|j| j.id == job_id).cloned()
        };

        if let Some(job) = job_opt {
            let worker_window = window_weak.clone();
            let worker_jobs = Arc::clone(&jobs_store);
            let worker_busy = Arc::clone(&is_busy);
            let worker_ui_tx = ui_tx.clone();

            thread::spawn(move || {
                worker_busy.store(true, Ordering::SeqCst);
                push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate("Unlocking PDF…".into(), 0.0));

                let result = process_job_with_password(&job, &password);

                let mut store = worker_jobs.lock().unwrap();
                if let Some(j) = store.iter_mut().find(|j| j.id == job_id) {
                    match result {
                        Ok(out_path) => {
                            j.status = JobStatus::Done;
                            j.output_path = out_path;
                            j.error.clear();
                        }
                        Err(refract_core::error::RefractError { code: refract_core::error::ErrorCode::WrongPassword, .. }) => {
                            j.status = JobStatus::NeedsPassword;
                            j.error = "Wrong password".to_string();
                        }
                        Err(e) => {
                            j.status = JobStatus::Failed;
                            j.error = e.to_string();
                        }
                    }
                    j.progress = -1.0;
                }

                push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                worker_busy.store(false, Ordering::SeqCst);
                push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate("Ready".into(), -1.0));
            });
        }
    });
}

fn setup_start_processing(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    is_busy: &Arc<AtomicBool>,
    ui_tx: &mpsc::Sender<UiEvent>,
    _settings: &Settings,
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let is_busy = Arc::clone(is_busy);
    let ui_tx = ui_tx.clone();

    window.on_start_processing(move || {
        if is_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => {
                is_busy.store(false, Ordering::SeqCst);
                return;
            }
        };

        window.set_is_running(true);
        push_ui_event(&ui_tx, UiEvent::StatusUpdate("Starting…".into(), 0.0));

        let tool_id = window.get_selected_tool_id().to_string();
        let opt_string_1 = window.get_opt_string_1().to_string();
        let opt_string_2 = window.get_opt_string_2().to_string();
        let opt_int_1 = window.get_opt_int_1() as u32;
        let opt_int_2 = window.get_opt_int_2() as u32;
        let opt_float_1 = window.get_opt_float_1() as f64;
        let opt_combo_1 = window.get_opt_combo_1().to_string();
        let output_next_to_source = window.get_output_next_to_source();
        let output_folder = if output_next_to_source {
            None
        } else {
            let folder = window.get_output_folder().to_string();
            if folder.is_empty() { None } else { Some(PathBuf::from(folder)) }
        };

        let worker_window = window_weak.clone();
        let worker_jobs = Arc::clone(&jobs_store);
        let worker_busy = Arc::clone(&is_busy);
        let worker_ui_tx = ui_tx.clone();

        thread::spawn(move || {
            let jobs_to_run = {
                let store = worker_jobs.lock().unwrap();
                let mut seen_batch = std::collections::HashSet::new();
                store.iter()
                    .filter(|j| j.status == JobStatus::Waiting)
                    .filter(|j| {
                        if j.is_batch_member {
                            if seen_batch.contains(&j.batch_leader_id) {
                                false
                            } else {
                                seen_batch.insert(j.batch_leader_id);
                                true
                            }
                        } else {
                            true
                        }
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            };

            if jobs_to_run.is_empty() {
                worker_busy.store(false, Ordering::SeqCst);
                push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate("No pending jobs".into(), -1.0));
                let _ = slint::invoke_from_event_loop({
                    let worker_window = worker_window.clone();
                    move || {
                        if let Some(w) = worker_window.upgrade() {
                            w.set_is_running(false);
                        }
                    }
                });
                return;
            }

            let total_jobs = jobs_to_run.len();
            let mut completed = 0;

            for job in jobs_to_run {
                if job.status != JobStatus::Waiting {
                    continue;
                }

                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                        j.status = JobStatus::Working;
                        j.progress = 0.0;
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }

                push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate(
                    format!("Processing {} of {}: {}", completed + 1, total_jobs, job.name),
                    (completed as f32) / (total_jobs as f32),
                ));

                let result = process_job(
                    &job,
                    &tool_id,
                    &opt_string_1,
                    &opt_string_2,
                    opt_int_1,
                    opt_int_2,
                    opt_float_1,
                    &opt_combo_1,
                    output_next_to_source,
                    output_folder.as_deref(),
                    &worker_ui_tx,
                );

                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                        match result {
                            Ok(out_path) => {
                                j.status = JobStatus::Done;
                                j.output_path = out_path;
                                j.error.clear();
                            }
                            Err(refract_core::error::RefractError { code: refract_core::error::ErrorCode::NeedsPassword, message, .. }) => {
                                j.status = JobStatus::NeedsPassword;
                                j.error = message;
                            }
                            Err(e) => {
                                j.status = JobStatus::Failed;
                                j.error = e.to_string();
                            }
                        }
                        j.progress = -1.0;
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }

                completed += 1;
                push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate(
                    format!("Completed {} of {}", completed, total_jobs),
                    (completed as f32) / (total_jobs as f32),
                ));
            }

            worker_busy.store(false, Ordering::SeqCst);
            push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate("All done".into(), 1.0));
            let _ = slint::invoke_from_event_loop({
                let worker_window = worker_window.clone();
                move || {
                    if let Some(w) = worker_window.upgrade() {
                        w.set_is_running(false);
                    }
                }
            });
        });
    });
}

fn setup_retry_job(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    is_busy: &Arc<AtomicBool>,
    ui_tx: &mpsc::Sender<UiEvent>,
    _settings: &Settings,
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let is_busy = Arc::clone(is_busy);
    let ui_tx = ui_tx.clone();

    window.on_retry_job(move |job_id| {
        if is_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => {
                is_busy.store(false, Ordering::SeqCst);
                return;
            }
        };

        window.set_is_running(true);

        let tool_id = window.get_selected_tool_id().to_string();
        let opt_string_1 = window.get_opt_string_1().to_string();
        let opt_string_2 = window.get_opt_string_2().to_string();
        let opt_int_1 = window.get_opt_int_1() as u32;
        let opt_int_2 = window.get_opt_int_2() as u32;
        let opt_float_1 = window.get_opt_float_1() as f64;
        let opt_combo_1 = window.get_opt_combo_1().to_string();
        let output_next_to_source = window.get_output_next_to_source();
        let output_folder = if output_next_to_source {
            None
        } else {
            let folder = window.get_output_folder().to_string();
            if folder.is_empty() { None } else { Some(PathBuf::from(folder)) }
        };

        let worker_window = window_weak.clone();
        let worker_jobs = Arc::clone(&jobs_store);
        let worker_busy = Arc::clone(&is_busy);
        let worker_ui_tx = ui_tx.clone();

        thread::spawn(move || {
            let job_opt = {
                let store = worker_jobs.lock().unwrap();
                store.iter().find(|j| j.id == job_id).cloned()
            };

            if let Some(job) = job_opt {
                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job_id) {
                        j.status = JobStatus::Working;
                        j.progress = 0.0;
                        j.error.clear();
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }

                push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate(
                    format!("Retrying: {}", job.name),
                    0.0,
                ));

                let result = process_job(
                    &job,
                    &tool_id,
                    &opt_string_1,
                    &opt_string_2,
                    opt_int_1,
                    opt_int_2,
                    opt_float_1,
                    &opt_combo_1,
                    output_next_to_source,
                    output_folder.as_deref(),
                    &worker_ui_tx,
                );

                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job_id) {
                        match result {
                            Ok(out_path) => {
                                j.status = JobStatus::Done;
                                j.output_path = out_path;
                                j.error.clear();
                            }
                            Err(refract_core::error::RefractError { code: refract_core::error::ErrorCode::NeedsPassword, message, .. }) => {
                                j.status = JobStatus::NeedsPassword;
                                j.error = message;
                            }
                            Err(e) => {
                                j.status = JobStatus::Failed;
                                j.error = e.to_string();
                            }
                        }
                        j.progress = -1.0;
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }
            }

            worker_busy.store(false, Ordering::SeqCst);
            push_ui_event(&worker_ui_tx, UiEvent::StatusUpdate("Ready".into(), -1.0));
            let _ = slint::invoke_from_event_loop({
                let worker_window = worker_window.clone();
                move || {
                    if let Some(w) = worker_window.upgrade() {
                        w.set_is_running(false);
                    }
                }
            });
        });
    });
}

fn setup_remove_job(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let ui_tx = ui_tx.clone();

    window.on_remove_job(move |job_id| {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let mut store = jobs_store.lock().unwrap();
        store.retain(|j| j.id != job_id);
        push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
    });
}

fn setup_move_job(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let window_weak_up = window.as_weak();
    let jobs_store_up = Arc::clone(jobs_store);
    let ui_tx_up = ui_tx.clone();

    window.on_move_job_up(move |job_id| {
        let window = match window_weak_up.upgrade() {
            Some(w) => w,
            None => return,
        };
        let mut store = jobs_store_up.lock().unwrap();
        if let Some(idx) = store.iter().position(|j| j.id == job_id) {
            if idx > 0 {
                store.swap(idx, idx - 1);
            }
        }
        push_ui_event(&ui_tx_up, UiEvent::JobsUpdated(store.clone()));
    });

    let window_weak_down = window.as_weak();
    let jobs_store_down = Arc::clone(jobs_store);
    let ui_tx_down = ui_tx.clone();

    window.on_move_job_down(move |job_id| {
        let window = match window_weak_down.upgrade() {
            Some(w) => w,
            None => return,
        };
        let mut store = jobs_store_down.lock().unwrap();
        if let Some(idx) = store.iter().position(|j| j.id == job_id) {
            if idx + 1 < store.len() {
                store.swap(idx, idx + 1);
            }
        }
        push_ui_event(&ui_tx_down, UiEvent::JobsUpdated(store.clone()));
    });
}

fn determine_output_path(
    job: &JobRecord,
    output_next_to_source: bool,
    output_folder: Option<&Path>,
    suffix: &str,
    extension: &str,
) -> PathBuf {
    let parent = if output_next_to_source {
        job.input_path.parent().unwrap_or(Path::new("."))
    } else {
        output_folder.unwrap_or_else(|| Path::new("."))
    };
    let stem = job.input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let out_path = parent.join(format!("{} {}.{}", stem, suffix, extension));
    uniquify_path(&out_path)
}

fn process_job(
    job: &JobRecord,
    tool_id: &str,
    opt_string_1: &str,
    opt_string_2: &str,
    opt_int_1: u32,
    opt_int_2: u32,
    opt_float_1: f64,
    opt_combo_1: &str,
    output_next_to_source: bool,
    output_folder: Option<&Path>,
    _ui_tx: &mpsc::Sender<UiEvent>,
) -> Result<PathBuf, refract_core::error::RefractError> {
    match tool_id {
        "convert-image" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let target_format = map_format_str(opt_combo_1);
            let quality = if target_format == Format::Jpeg { opt_int_1.clamp(1, 100) as u8 } else { 85 };
            let out_bytes = image::convert_image(&bytes, target_format, Some(quality)).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Convert failed", e.to_string()))?;
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "converted", format_primary_extension(target_format));
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "compress-image" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let target_kb = opt_int_1.max(1);
            let out_bytes = image::compress_image(&bytes, target_kb).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Compress failed", e.to_string()))?;
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "compressed", "jpg");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "resize-image" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let width = if opt_int_1 == 0 { 0 } else { opt_int_1 };
            let height = if opt_int_2 == 0 { 0 } else { opt_int_2 };
            let out_bytes = image::resize_image_keep_format(&bytes, width, height, None, Some(opt_int_1.clamp(1, 100) as u8)).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Resize failed", e.to_string()))?;
            let ext = job.input_path.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "resized", ext);
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "images-to-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let dpi = if opt_float_1 <= 0.0 { 150.0 } else { opt_float_1 };
            let quality = opt_int_1.clamp(1, 100) as u8;
            let out_bytes = pdf::images_to_pdf(&[bytes], dpi, quality).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Images to PDF failed", e.to_string()))?;
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "images-to-pdf", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "pdf-to-images" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let dpi = (opt_float_1.clamp(72.0, 600.0)) as u32;
            let format = if opt_combo_1 == "jpg" { "jpg" } else { "png" };

            let rendered = render::render_pdf_to_images(&bytes, dpi, format);

            match rendered {
                Ok(pages) => {
                    let parent = if output_next_to_source {
                        job.input_path.parent().unwrap_or(Path::new("."))
                    } else {
                        output_folder.unwrap_or_else(|| Path::new("."))
                    };
                    let stem = job.input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");

                    for (page_num, page_bytes, ext) in pages {
                        let out_path = parent.join(format!("{} p{:04}.{}", stem, page_num, ext));
                        let unique = uniquify_path(&out_path);
                        std::fs::write(&unique, page_bytes).map_err(to_refract_err)?;
                    }

                    let first_page = parent.join(format!("{} p0001.{}", stem, format));
                    Ok(uniquify_path(&first_page))
                }
                Err(e) => {
                    let embedded = pdf::extract_embedded_images(&bytes).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Extract failed", e.to_string()))?;
                    if embedded.is_empty() {
                        return Err(e);
                    }
                    let parent = if output_next_to_source {
                        job.input_path.parent().unwrap_or(Path::new("."))
                    } else {
                        output_folder.unwrap_or_else(|| Path::new("."))
                    };
                    let stem = job.input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");

                    for img in &embedded {
                        let out_path = parent.join(format!("{} p{:04}.{}", stem, img.page, img.extension));
                        let unique = uniquify_path(&out_path);
                        std::fs::write(&unique, img.bytes.clone()).map_err(to_refract_err)?;
                    }

                    let first = parent.join(format!("{} p0001.{}", stem, embedded[0].extension));
                    Ok(uniquify_path(&first))
                }
            }
        }
        "compress-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::compress_pdf(&bytes).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Compress PDF failed", e.to_string()))?;
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "compressed", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "split-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let ranges = opt_string_1.trim();
            if ranges.is_empty() {
                return Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::DamagedFile, "Page ranges cannot be empty"));
            }
            let parts = pdf::split_pdf(&bytes, ranges).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Split PDF failed", e.to_string()))?;
            let parent = if output_next_to_source {
                job.input_path.parent().unwrap_or(Path::new("."))
            } else {
                output_folder.unwrap_or_else(|| Path::new("."))
            };
            let stem = job.input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");

            let mut last_path = parent.to_path_buf();
            for (label, part_bytes) in parts {
                let out_path = parent.join(format!("{}-{}.pdf", stem, label));
                let unique = uniquify_path(&out_path);
                std::fs::write(&unique, part_bytes).map_err(to_refract_err)?;
                last_path = unique;
            }
            Ok(last_path)
        }
        "reorder-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let order = opt_string_1.trim();
            if order.is_empty() {
                return Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::DamagedFile, "Page order cannot be empty"));
            }
            let out_bytes = pdf::reorder_pdf(&bytes, order).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Reorder PDF failed", e.to_string()))?;
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "reordered", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "merge-pdf" => {
            Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::EngineFailed, "Merge PDF requires batch processing - use Process All button"))
        }
        "convert-audio" => {
            if !audio::is_ffmpeg_available() {
                return Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::EngineFailed, "FFmpeg not found. Please install FFmpeg to convert audio."));
            }
            let target_format = map_format_str(opt_combo_1);
            let bitrate = opt_int_1.clamp(64, 320);
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "converted", format_primary_extension(target_format));
            audio::convert_audio(&job.input_path, &out_path, target_format, Some(bitrate)).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Convert audio failed", e.to_string()))?;
            Ok(out_path)
        }
        "trim-audio" => {
            if !audio::is_ffmpeg_available() {
                return Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::EngineFailed, "FFmpeg not found. Please install FFmpeg to trim audio."));
            }
            let ext = job.input_path.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "trimmed", ext);
            audio::trim_audio(&job.input_path, &out_path, Some(opt_string_1), Some(opt_string_2)).map_err(|e| refract_core::error::RefractError::with_details(refract_core::error::ErrorCode::EngineFailed, "Trim audio failed", e.to_string()))?;
            Ok(out_path)
        }
        _ => Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::UnsupportedFormat, format!("Unknown tool: {}", tool_id)))
    }
}

fn process_job_with_password(
    job: &JobRecord,
    password: &str,
) -> Result<PathBuf, refract_core::error::RefractError> {
    let tool_id = &job.tool_id;
    let output_next_to_source = true;
    let output_folder: Option<&Path> = None;

    match tool_id.as_str() {
        "compress-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::compress_pdf_with_password(&bytes, password)?;
            let out_path = determine_output_path(job, output_next_to_source, output_folder, "compressed", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "pdf-to-images" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let _ = pdf::load_document(&bytes, Some(password)).map_err(|e| e)?;
            process_job(job, tool_id, "", "", 150, 85, 150.0, "png", output_next_to_source, output_folder, &mpsc::channel().0)
        }
        _ => Err(refract_core::error::RefractError::new(refract_core::error::ErrorCode::UnsupportedFormat, "Password not needed for this tool"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "1 KB");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(1536), "2 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1536 * 1024), "1.5 MB");
    }

    #[test]
    fn test_uniquify_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base = temp_dir.path().join("test.txt");
        std::fs::write(&base, "a").unwrap();
        let u1 = uniquify_path(&base);
        let fname1 = u1.file_name().unwrap().to_str().unwrap();
        // Just verify it's different from original and contains a number
        assert_ne!(fname1, "test.txt", "first unique path should differ from original");
        assert!(fname1.contains('1') || fname1.contains("(1)"), "first unique path should contain a number, got: {}", fname1);
        std::fs::write(&u1, "b").unwrap();
        let u2 = uniquify_path(&base);
        let fname2 = u2.file_name().unwrap().to_str().unwrap();
        assert_ne!(fname2, "test.txt", "second unique path should differ from original");
        assert_ne!(fname2, fname1, "second unique path should differ from first");
        assert!(fname2.contains('2') || fname2.contains("(2)"), "second unique path should contain a number, got: {}", fname2);
    }

    #[test]
    fn test_determine_output_path() {
        let job = JobRecord {
            id: 1,
            input_path: PathBuf::from("/home/user/photo.jpg"),
            name: "photo.jpg".into(),
            size_bytes: 1000,
            tool_id: "convert-image".into(),
            tool_label: "Image".into(),
            status: JobStatus::Waiting,
            error: String::new(),
            output_path: PathBuf::new(),
            progress: 0.0,
            is_batch_member: false,
            batch_leader_id: -1,
        };
        let path = determine_output_path(&job, true, None, "converted", "png");
        assert_eq!(path.file_name().unwrap(), "photo converted.png");
    }
}