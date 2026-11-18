slint::include_modules!();

use refract_core::format::Format;
use refract_engine::{audio, image, pdf, render};
use slint::{ComponentHandle, Model, SharedString, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
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
    if bytes == 0 {
        return "0 KB".to_string();
    }
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
        "mp3" => Format::Mp3,
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
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|h| h.join("Library/Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
    };
    base.push("Refract");
    std::fs::create_dir_all(&base).ok();
    base.push("settings.json");
    base
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    is_dark_mode: bool,
    output_folder: String,
    output_next_to_source: bool,
    last_tool_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            is_dark_mode: true,
            output_folder: String::new(),
            output_next_to_source: true,
            last_tool_id: String::new(),
        }
    }
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

fn set_combo_model(window: &MainWindow, items: &[&str], selected: &str) {
    let model = Rc::new(VecModel::from(
        items.iter().map(|s| SharedString::from(*s)).collect::<Vec<_>>(),
    ));
    window.set_opt_combo_1_model(model.into());
    window.set_opt_combo_1(selected.into());
}

fn apply_tool_selection(
    window: &MainWindow,
    tool_specs: &[slint_generatedMainWindow::ToolSpec],
    tool_id_str: &str,
) {
    let spec = tool_specs
        .iter()
        .find(|s| s.id == tool_id_str)
        .or_else(|| tool_specs.first());

    let Some(spec) = spec else {
        return;
    };

    window.set_selected_tool_id(spec.id.clone());
    window.set_current_tool_title(spec.title.clone());
    window.set_current_tool_tagline(spec.tagline.clone());
    window.set_current_tool_description(spec.description.clone());
    window.set_current_tool_domain(spec.domain.clone());
    window.set_current_tool_extensions(spec.extensions.clone());
    window.set_current_tool_extensions_joined(
        spec.extensions
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
            .join(", ")
            .into(),
    );
    window.set_current_tool_batch(spec.batch);

    // Populate sensible defaults and dropdown models per tool
    match spec.id.as_str() {
        "convert-image" => {
            set_combo_model(window, &["jpg", "png", "webp"], "jpg");
            window.set_opt_int_1(90);
        }
        "compress-image" => {
            window.set_opt_int_1(200);
        }
        "resize-image" => {
            window.set_opt_int_1(1920);
            window.set_opt_int_2(0);
        }
        "images-to-pdf" => {
            window.set_opt_int_1(85);
            window.set_opt_int_2(150);
            window.set_opt_float_1(150.0);
        }
        "pdf-to-images" => {
            set_combo_model(window, &["png", "jpg"], "png");
            window.set_opt_int_1(150);
            window.set_opt_float_1(150.0);
        }
        "split-pdf" => {
            window.set_opt_string_1("1-2, 3".into());
        }
        "reorder-pdf" => {
            window.set_opt_string_1("1, 2".into());
        }
        "convert-audio" => {
            set_combo_model(window, &["mp3", "wav", "m4a", "ogg"], "mp3");
            window.set_opt_combo_2("192".into());
        }
        "trim-audio" => {
            window.set_opt_string_1("0".into());
            window.set_opt_string_2("30".into());
        }
        _ => {}
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let settings = load_settings();

    let main_window = MainWindow::new()?;

    // Initialize tool specs from Rust
    let tool_specs: Vec<_> = refract_core::tools::ToolId::ALL
        .iter()
        .map(|t| {
            let spec = t.spec();
            let input_formats_model = Rc::new(VecModel::from(
                spec.input_formats
                    .iter()
                    .map(|s| (*s).into())
                    .collect::<Vec<SharedString>>(),
            ));
            let extensions_model = Rc::new(VecModel::from(
                spec.extensions
                    .iter()
                    .map(|s| (*s).into())
                    .collect::<Vec<SharedString>>(),
            ));
            slint_generatedMainWindow::ToolSpec {
                id: spec.id.id_str().to_string().into(),
                title: spec.title.to_string().into(),
                tagline: spec.tagline.to_string().into(),
                description: spec.description.to_string().into(),
                domain: spec.domain.to_string().into(),
                input_formats: input_formats_model.into(),
                output_capability: spec.output_capability.into(),
                engine: spec.engine as i32,
                batch: spec.batch,
                min_files: spec.min_files as i32,
                max_files: spec.max_files.unwrap_or(0) as i32,
                extensions: extensions_model.into(),
            }
        })
        .collect();

    // Group tools by domain
    let mut groups = Vec::new();
    for kind in [
        refract_core::format::Kind::Image,
        refract_core::format::Kind::Pdf,
        refract_core::format::Kind::Audio,
    ] {
        let kind_str: SharedString = kind.to_string().into();
        let group_tools: Vec<_> = tool_specs
            .iter()
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

    let initial_tool = if !settings.last_tool_id.is_empty() {
        settings.last_tool_id.clone()
    } else {
        tool_specs
            .first()
            .map(|t| t.id.to_string())
            .unwrap_or_default()
    };
    apply_tool_selection(&main_window, &tool_specs, &initial_tool);

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
    })
    .map_err(|e| slint::PlatformError::from(e.to_string()))?;

    // Callbacks
    setup_pick_files(&main_window, &jobs_store, &next_id, &ui_tx);
    setup_pick_output_folder(&main_window, &ui_tx);
    setup_clear_jobs(&main_window, &jobs_store, &ui_tx);
    setup_open_output_folder(&main_window);
    setup_start_processing(&main_window, &jobs_store, &is_busy, &ui_tx);
    setup_retry_job(&main_window, &jobs_store, &is_busy, &ui_tx);
    setup_remove_job(&main_window, &jobs_store, &ui_tx);
    setup_move_job(&main_window, &jobs_store, &ui_tx);
    setup_toggle_theme(&main_window);
    setup_tool_changed(&main_window, &tool_specs);
    setup_password_handlers(&main_window, &jobs_store, &is_busy, &ui_tx);

    main_window.run()
}

#[allow(dead_code)]
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
            let entries: Vec<slint_generatedMainWindow::JobEntry> =
                jobs.iter().map(job_record_to_entry).collect();
            ui.set_jobs(Rc::new(VecModel::from(entries)).into());
            ui.set_jobs_count_str(jobs.len().to_string().into());
        }
        UiEvent::StatusUpdate(text, progress) => {
            ui.set_status_text(text.into());
            ui.set_overall_progress(progress);
        }
        UiEvent::JobProgress(id, progress, status) => {
            let model = ui.get_jobs();
            let mut entries: Vec<slint_generatedMainWindow::JobEntry> = model.iter().collect();
            if let Some(idx) = entries.iter().position(|e| e.id == id) {
                let status_str: SharedString = match status {
                    JobStatus::Waiting => "Waiting".into(),
                    JobStatus::Working => "Working…".into(),
                    JobStatus::Done => "Done".into(),
                    JobStatus::Failed => "Failed".into(),
                    JobStatus::NeedsPassword => "Needs Password".into(),
                };
                entries[idx].progress = progress;
                entries[idx].status = status_str;
                ui.set_jobs(Rc::new(VecModel::from(entries)).into());
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
        let Some(window) = window_weak.upgrade() else {
            return;
        };

        let tool_id = window.get_selected_tool_id().to_string();
        let spec_opt = window.get_tool_specs().iter().find(|s| s.id == tool_id);
        let extensions: Vec<String> = spec_opt
            .as_ref()
            .map(|s| s.extensions.iter().map(|e| e.to_string()).collect())
            .unwrap_or_default();

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
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let id = *id_lock;
                *id_lock += 1;

                let tool_label = spec_opt
                    .as_ref()
                    .map(|s| s.title.to_string())
                    .unwrap_or_else(|| "File".to_string());

                added.push(JobRecord {
                    id,
                    input_path: path,
                    name,
                    size_bytes,
                    tool_id: tool_id.clone(),
                    tool_label,
                    status: JobStatus::Waiting,
                    error: String::new(),
                    output_path: PathBuf::new(),
                    progress: -1.0,
                });
            }

            store.extend(added);
            push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
        }
    });
}

fn setup_pick_output_folder(window: &MainWindow, ui_tx: &mpsc::Sender<UiEvent>) {
    let window_weak = window.as_weak();
    let ui_tx = ui_tx.clone();
    window.on_pick_output_folder(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            let folder_str = folder.to_string_lossy().to_string();
            window.set_output_folder(folder_str.clone().into());
            window.set_output_next_to_source(false);
            let mut settings = load_settings();
            settings.output_folder = folder_str;
            settings.output_next_to_source = false;
            save_settings(&settings);
            push_ui_event(
                &ui_tx,
                UiEvent::StatusUpdate(format!("Output folder: {}", folder.display()), -1.0),
            );
        }
    });
}

fn setup_clear_jobs(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let jobs_store = Arc::clone(jobs_store);
    let ui_tx = ui_tx.clone();

    window.on_clear_jobs(move || {
        let mut store = jobs_store.lock().unwrap();
        store.clear();
        push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
        push_ui_event(&ui_tx, UiEvent::StatusUpdate("Ready".into(), -1.0));
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

fn setup_toggle_theme(window: &MainWindow) {
    let window_weak = window.as_weak();

    window.on_toggle_theme(move || {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        let new_mode = !window.get_is_dark_mode();
        window.set_is_dark_mode(new_mode);
        let mut settings = load_settings();
        settings.is_dark_mode = new_mode;
        save_settings(&settings);
    });
}

fn setup_tool_changed(
    window: &MainWindow,
    tool_specs: &[slint_generatedMainWindow::ToolSpec],
) {
    let window_weak = window.as_weak();
    let tool_specs = tool_specs.to_vec();

    window.on_tool_changed(move |tool_id: SharedString| {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        let tool_id_str = tool_id.to_string();
        apply_tool_selection(&window, &tool_specs, &tool_id_str);

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
) {
    let window_weak_show = window.as_weak();
    window.on_show_password_dialog_cb(move |job_id, prompt| {
        if let Some(window) = window_weak_show.upgrade() {
            window.set_password_job_id(job_id);
            window.set_password_prompt(prompt);
            window.set_password_input("".into());
            window.set_show_password_dialog(true);
        }
    });

    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let is_busy = Arc::clone(is_busy);
    let ui_tx = ui_tx.clone();

    window.on_password_entered(move |job_id, password| {
        let Some(window) = window_weak.upgrade() else {
            return;
        };

        let output_next_to_source = window.get_output_next_to_source();
        let output_folder = if output_next_to_source {
            None
        } else {
            let folder = window.get_output_folder().to_string();
            if folder.is_empty() {
                None
            } else {
                Some(PathBuf::from(folder))
            }
        };

        let job_opt = {
            let store = jobs_store.lock().unwrap();
            store.iter().find(|j| j.id == job_id).cloned()
        };

        if let Some(job) = job_opt {
            let worker_jobs = Arc::clone(&jobs_store);
            let worker_busy = Arc::clone(&is_busy);
            let worker_ui_tx = ui_tx.clone();

            thread::spawn(move || {
                worker_busy.store(true, Ordering::SeqCst);
                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate("Unlocking PDF…".into(), 0.0),
                );

                let result = process_job_with_password(
                    &job,
                    &password,
                    output_next_to_source,
                    output_folder.as_deref(),
                );

                let mut store = worker_jobs.lock().unwrap();
                if let Some(j) = store.iter_mut().find(|j| j.id == job_id) {
                    match result {
                        Ok(out_path) => {
                            j.status = JobStatus::Done;
                            j.output_path = out_path;
                            j.error.clear();
                        }
                        Err(refract_core::error::RefractError {
                            code: refract_core::error::ErrorCode::WrongPassword,
                            ..
                        }) => {
                            j.status = JobStatus::NeedsPassword;
                            j.error = "Wrong password — check it and try again.".to_string();
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

#[derive(Clone)]
struct CapturedOptions {
    opt_string_1: String,
    opt_string_2: String,
    opt_int_1: u32,
    opt_int_2: u32,
    opt_float_1: f64,
    opt_combo_1: String,
    opt_combo_2: String,
    output_next_to_source: bool,
    output_folder: Option<PathBuf>,
}

fn capture_options(window: &MainWindow) -> CapturedOptions {
    let output_next_to_source = window.get_output_next_to_source();
    let output_folder = if output_next_to_source {
        None
    } else {
        let folder = window.get_output_folder().to_string();
        if folder.is_empty() {
            None
        } else {
            Some(PathBuf::from(folder))
        }
    };
    CapturedOptions {
        opt_string_1: window.get_opt_string_1().to_string(),
        opt_string_2: window.get_opt_string_2().to_string(),
        opt_int_1: window.get_opt_int_1().max(0) as u32,
        opt_int_2: window.get_opt_int_2().max(0) as u32,
        opt_float_1: window.get_opt_float_1() as f64,
        opt_combo_1: window.get_opt_combo_1().to_string(),
        opt_combo_2: window.get_opt_combo_2().to_string(),
        output_next_to_source,
        output_folder,
    }
}

fn setup_start_processing(
    window: &MainWindow,
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    is_busy: &Arc<AtomicBool>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let is_busy = Arc::clone(is_busy);
    let ui_tx = ui_tx.clone();

    window.on_start_processing(move || {
        if is_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let Some(window) = window_weak.upgrade() else {
            is_busy.store(false, Ordering::SeqCst);
            return;
        };

        window.set_is_running(true);
        push_ui_event(&ui_tx, UiEvent::StatusUpdate("Starting…".into(), 0.0));

        let opts = capture_options(&window);
        let worker_window = window_weak.clone();
        let worker_jobs = Arc::clone(&jobs_store);
        let worker_busy = Arc::clone(&is_busy);
        let worker_ui_tx = ui_tx.clone();

        thread::spawn(move || {
            let waiting_jobs: Vec<JobRecord> = {
                let store = worker_jobs.lock().unwrap();
                store
                    .iter()
                    .filter(|j| j.status == JobStatus::Waiting)
                    .cloned()
                    .collect()
            };

            if waiting_jobs.is_empty() {
                worker_busy.store(false, Ordering::SeqCst);
                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate("No pending jobs".into(), -1.0),
                );
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

            // Separate batch merge-pdf & images-to-pdf jobs from per-file jobs
            let merge_jobs: Vec<JobRecord> = waiting_jobs
                .iter()
                .filter(|j| j.tool_id == "merge-pdf")
                .cloned()
                .collect();
            let images_to_pdf_jobs: Vec<JobRecord> = waiting_jobs
                .iter()
                .filter(|j| j.tool_id == "images-to-pdf")
                .cloned()
                .collect();
            let single_jobs: Vec<JobRecord> = waiting_jobs
                .into_iter()
                .filter(|j| j.tool_id != "merge-pdf" && j.tool_id != "images-to-pdf")
                .collect();

            let total_units = single_jobs.len()
                + usize::from(!merge_jobs.is_empty())
                + usize::from(!images_to_pdf_jobs.is_empty());
            let mut completed = 0usize;

            // 1. Batch Merge PDF
            if !merge_jobs.is_empty() {
                let ids: Vec<i32> = merge_jobs.iter().map(|j| j.id).collect();
                {
                    let mut store = worker_jobs.lock().unwrap();
                    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                        j.status = JobStatus::Working;
                        j.progress = 0.0;
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }
                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate(
                        format!("Merging {} PDFs…", merge_jobs.len()),
                        completed as f32 / total_units as f32,
                    ),
                );

                let merge_result = if merge_jobs.len() < 2 {
                    Err(refract_core::error::RefractError::new(
                        refract_core::error::ErrorCode::DamagedFile,
                        "Merge PDF needs at least 2 PDF files in the queue.",
                    ))
                } else {
                    let read_res: Result<Vec<Vec<u8>>, _> = merge_jobs
                        .iter()
                        .map(|j| std::fs::read(&j.input_path).map_err(to_refract_err))
                        .collect();
                    read_res.and_then(|buffers| {
                        let slices: Vec<&[u8]> = buffers.iter().map(|b| b.as_slice()).collect();
                        let out_bytes = pdf::merge_pdfs(&slices)?;
                        let out_path = determine_output_path(
                            &merge_jobs[0],
                            opts.output_next_to_source,
                            opts.output_folder.as_deref(),
                            "merged",
                            "pdf",
                        );
                        std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
                        Ok(out_path)
                    })
                };

                {
                    let mut store = worker_jobs.lock().unwrap();
                    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                        match &merge_result {
                            Ok(path) => {
                                j.status = JobStatus::Done;
                                j.output_path = path.clone();
                                j.error.clear();
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
            }

            // 2. Batch Images to PDF
            if !images_to_pdf_jobs.is_empty() {
                let ids: Vec<i32> = images_to_pdf_jobs.iter().map(|j| j.id).collect();
                {
                    let mut store = worker_jobs.lock().unwrap();
                    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                        j.status = JobStatus::Working;
                        j.progress = 0.0;
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }
                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate(
                        format!("Compiling {} images into PDF…", images_to_pdf_jobs.len()),
                        completed as f32 / total_units as f32,
                    ),
                );

                let dpi = if opts.opt_int_2 >= 72 {
                    opts.opt_int_2 as f64
                } else {
                    150.0
                };
                let quality = opts.opt_int_1.clamp(10, 100) as u8;
                let compile_result = images_to_pdf_jobs
                    .iter()
                    .map(|j| std::fs::read(&j.input_path).map_err(to_refract_err))
                    .collect::<Result<Vec<Vec<u8>>, _>>()
                    .and_then(|buffers| {
                        let out_bytes = pdf::images_to_pdf(&buffers, dpi, quality)?;
                        let out_path = determine_output_path(
                            &images_to_pdf_jobs[0],
                            opts.output_next_to_source,
                            opts.output_folder.as_deref(),
                            "images",
                            "pdf",
                        );
                        std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
                        Ok(out_path)
                    });

                {
                    let mut store = worker_jobs.lock().unwrap();
                    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                        match &compile_result {
                            Ok(path) => {
                                j.status = JobStatus::Done;
                                j.output_path = path.clone();
                                j.error.clear();
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
            }

            // 3. Individual jobs
            for job in single_jobs {
                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                        j.status = JobStatus::Working;
                        j.progress = 0.0;
                    }
                    push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                }

                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate(
                        format!("Processing {} of {}: {}", completed + 1, total_units, job.name),
                        completed as f32 / total_units as f32,
                    ),
                );

                let result = process_job(&job, &opts);

                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
                        match result {
                            Ok(out_path) => {
                                j.status = JobStatus::Done;
                                j.output_path = out_path;
                                j.error.clear();
                            }
                            Err(refract_core::error::RefractError {
                                code: refract_core::error::ErrorCode::NeedsPassword,
                                message,
                                ..
                            }) => {
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
                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate(
                        format!("Completed {} of {}", completed, total_units),
                        completed as f32 / total_units as f32,
                    ),
                );
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
) {
    let window_weak = window.as_weak();
    let jobs_store = Arc::clone(jobs_store);
    let is_busy = Arc::clone(is_busy);
    let ui_tx = ui_tx.clone();

    window.on_retry_job(move |job_id| {
        if is_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let Some(window) = window_weak.upgrade() else {
            is_busy.store(false, Ordering::SeqCst);
            return;
        };

        window.set_is_running(true);
        let opts = capture_options(&window);
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

                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate(format!("Retrying: {}", job.name), 0.0),
                );

                let result = process_job(&job, &opts);

                {
                    let mut store = worker_jobs.lock().unwrap();
                    if let Some(j) = store.iter_mut().find(|j| j.id == job_id) {
                        match result {
                            Ok(out_path) => {
                                j.status = JobStatus::Done;
                                j.output_path = out_path;
                                j.error.clear();
                            }
                            Err(refract_core::error::RefractError {
                                code: refract_core::error::ErrorCode::NeedsPassword,
                                message,
                                ..
                            }) => {
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
    let jobs_store = Arc::clone(jobs_store);
    let ui_tx = ui_tx.clone();

    window.on_remove_job(move |job_id| {
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
    let jobs_store_up = Arc::clone(jobs_store);
    let ui_tx_up = ui_tx.clone();

    window.on_move_job_up(move |job_id| {
        let mut store = jobs_store_up.lock().unwrap();
        if let Some(idx) = store.iter().position(|j| j.id == job_id) {
            if idx > 0 {
                store.swap(idx, idx - 1);
            }
        }
        push_ui_event(&ui_tx_up, UiEvent::JobsUpdated(store.clone()));
    });

    let jobs_store_down = Arc::clone(jobs_store);
    let ui_tx_down = ui_tx.clone();

    window.on_move_job_down(move |job_id| {
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
    let stem = job
        .input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let out_path = parent.join(format!("{} {}.{}", stem, suffix, extension));
    uniquify_path(&out_path)
}

fn process_job(
    job: &JobRecord,
    opts: &CapturedOptions,
) -> Result<PathBuf, refract_core::error::RefractError> {
    let output_folder = opts.output_folder.as_deref();
    match job.tool_id.as_str() {
        "convert-image" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let target_format = map_format_str(&opts.opt_combo_1);
            let quality = if target_format == Format::Jpeg {
                opts.opt_int_1.clamp(1, 100) as u8
            } else {
                90
            };
            let out_bytes = image::convert_image(&bytes, target_format, Some(quality))?;
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "converted",
                format_primary_extension(target_format),
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "compress-image" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let target_kb = opts.opt_int_1.max(1);
            let out_bytes = image::compress_image(&bytes, target_kb)?;
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "compressed",
                "jpg",
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "resize-image" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let (out_bytes, out_fmt) =
                image::resize_image_with_format(&bytes, opts.opt_int_1, opts.opt_int_2, None)?;
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "resized",
                format_primary_extension(out_fmt),
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "images-to-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let dpi = if opts.opt_int_2 >= 72 {
                opts.opt_int_2 as f64
            } else if opts.opt_float_1 > 0.0 {
                opts.opt_float_1
            } else {
                150.0
            };
            let quality = opts.opt_int_1.clamp(10, 100) as u8;
            let out_bytes = pdf::images_to_pdf(&[bytes], dpi, quality)?;
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "images-to-pdf",
                "pdf",
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "pdf-to-images" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let dpi = if opts.opt_int_1 >= 72 {
                opts.opt_int_1.clamp(72, 600)
            } else {
                opts.opt_float_1.clamp(72.0, 600.0) as u32
            };
            let format = if opts.opt_combo_1 == "jpg" { "jpg" } else { "png" };
            let parent = if opts.output_next_to_source {
                job.input_path.parent().unwrap_or(Path::new("."))
            } else {
                output_folder.unwrap_or_else(|| Path::new("."))
            };
            let stem = job
                .input_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");

            match render::render_pdf_to_images(&bytes, dpi, format) {
                Ok(pages) => {
                    let mut first_written = parent.join(format!("{} p0001.{}", stem, format));
                    for (idx, (page_num, page_bytes, ext)) in pages.into_iter().enumerate() {
                        let out_path = parent.join(format!("{} p{:04}.{}", stem, page_num, ext));
                        let unique = uniquify_path(&out_path);
                        std::fs::write(&unique, page_bytes).map_err(to_refract_err)?;
                        if idx == 0 {
                            first_written = unique;
                        }
                    }
                    Ok(first_written)
                }
                Err(render_err) => {
                    let embedded = pdf::extract_embedded_images(&bytes)?;
                    if embedded.is_empty() {
                        return Err(render_err);
                    }
                    let mut first_written =
                        parent.join(format!("{} p0001.{}", stem, embedded[0].extension));
                    for (idx, img) in embedded.iter().enumerate() {
                        let out_path =
                            parent.join(format!("{} p{:04}.{}", stem, img.page, img.extension));
                        let unique = uniquify_path(&out_path);
                        std::fs::write(&unique, &img.bytes).map_err(to_refract_err)?;
                        if idx == 0 {
                            first_written = unique;
                        }
                    }
                    Ok(first_written)
                }
            }
        }
        "compress-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::compress_pdf(&bytes)?;
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "compressed",
                "pdf",
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "split-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let ranges = opts.opt_string_1.trim();
            if ranges.is_empty() {
                return Err(refract_core::error::RefractError::new(
                    refract_core::error::ErrorCode::DamagedFile,
                    "Page ranges cannot be empty (e.g. 1-2, 3).",
                ));
            }
            let parts = pdf::split_pdf(&bytes, ranges)?;
            let parent = if opts.output_next_to_source {
                job.input_path.parent().unwrap_or(Path::new("."))
            } else {
                output_folder.unwrap_or_else(|| Path::new("."))
            };
            let stem = job
                .input_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");

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
            let order = opts.opt_string_1.trim();
            if order.is_empty() {
                return Err(refract_core::error::RefractError::new(
                    refract_core::error::ErrorCode::DamagedFile,
                    "Page order cannot be empty (e.g. 3, 1, 2).",
                ));
            }
            let out_bytes = pdf::reorder_pdf(&bytes, order)?;
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "reordered",
                "pdf",
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "merge-pdf" => Err(refract_core::error::RefractError::new(
            refract_core::error::ErrorCode::EngineFailed,
            "Merge PDF combines multiple files — use Process all.",
        )),
        "convert-audio" => {
            if !audio::is_ffmpeg_available() {
                return Err(refract_core::error::RefractError::new(
                    refract_core::error::ErrorCode::EngineFailed,
                    "FFmpeg not found. Install FFmpeg and ensure it is on PATH to convert audio.",
                ));
            }
            let target_format = map_format_str(&opts.opt_combo_1);
            let bitrate = opts
                .opt_combo_2
                .trim()
                .parse::<u32>()
                .unwrap_or(192)
                .clamp(64, 320);
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "converted",
                format_primary_extension(target_format),
            );
            audio::convert_audio(&job.input_path, &out_path, target_format, Some(bitrate))?;
            Ok(out_path)
        }
        "trim-audio" => {
            if !audio::is_ffmpeg_available() {
                return Err(refract_core::error::RefractError::new(
                    refract_core::error::ErrorCode::EngineFailed,
                    "FFmpeg not found. Install FFmpeg and ensure it is on PATH to trim audio.",
                ));
            }
            let ext = job
                .input_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp3");
            let out_path = determine_output_path(
                job,
                opts.output_next_to_source,
                output_folder,
                "trimmed",
                ext,
            );
            let start = (!opts.opt_string_1.trim().is_empty()).then_some(opts.opt_string_1.as_str());
            let end = (!opts.opt_string_2.trim().is_empty()).then_some(opts.opt_string_2.as_str());
            audio::trim_audio(&job.input_path, &out_path, start, end)?;
            Ok(out_path)
        }
        _ => Err(refract_core::error::RefractError::new(
            refract_core::error::ErrorCode::UnsupportedFormat,
            format!("Unknown tool: {}", job.tool_id),
        )),
    }
}

fn process_job_with_password(
    job: &JobRecord,
    password: &str,
    output_next_to_source: bool,
    output_folder: Option<&Path>,
) -> Result<PathBuf, refract_core::error::RefractError> {
    match job.tool_id.as_str() {
        "compress-pdf" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::compress_pdf_with_password(&bytes, password)?;
            let out_path = determine_output_path(
                job,
                output_next_to_source,
                output_folder,
                "compressed",
                "pdf",
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        "pdf-to-images" => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let mut doc = pdf::load_document(&bytes, Some(password))?;
            let mut unlocked = Vec::new();
            doc.save_to(&mut unlocked).map_err(|e| {
                refract_core::error::RefractError::with_details(
                    refract_core::error::ErrorCode::EngineFailed,
                    "Failed to prepare unlocked PDF",
                    e.to_string(),
                )
            })?;
            let parent = if output_next_to_source {
                job.input_path.parent().unwrap_or(Path::new("."))
            } else {
                output_folder.unwrap_or_else(|| Path::new("."))
            };
            let stem = job
                .input_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");

            let pages = render::render_pdf_to_images(&unlocked, 150, "png")?;
            let mut first_written = parent.join(format!("{} p0001.png", stem));
            for (idx, (page_num, page_bytes, ext)) in pages.into_iter().enumerate() {
                let out_path = parent.join(format!("{} p{:04}.{}", stem, page_num, ext));
                let unique = uniquify_path(&out_path);
                std::fs::write(&unique, page_bytes).map_err(to_refract_err)?;
                if idx == 0 {
                    first_written = unique;
                }
            }
            Ok(first_written)
        }
        _ => Err(refract_core::error::RefractError::new(
            refract_core::error::ErrorCode::UnsupportedFormat,
            "Password not needed for this tool",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 KB");
        assert_eq!(format_size(500), "1 KB");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(1536), "2 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1536 * 1024), "1.5 MB");
    }

    #[test]
    fn test_map_format_str() {
        assert_eq!(map_format_str("mp3"), Format::Mp3);
        assert_eq!(map_format_str("wav"), Format::Wav);
        assert_eq!(map_format_str("webp"), Format::Webp);
        assert_eq!(map_format_str("png"), Format::Png);
        assert_eq!(map_format_str("jpg"), Format::Jpeg);
    }

    #[test]
    fn test_uniquify_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base = temp_dir.path().join("test.txt");
        std::fs::write(&base, "a").unwrap();
        let u1 = uniquify_path(&base);
        let fname1 = u1.file_name().unwrap().to_str().unwrap();
        assert_ne!(fname1, "test.txt");
        assert!(fname1.contains("(1)"), "got: {}", fname1);
        std::fs::write(&u1, "b").unwrap();
        let u2 = uniquify_path(&base);
        let fname2 = u2.file_name().unwrap().to_str().unwrap();
        assert_ne!(fname2, "test.txt");
        assert_ne!(fname2, fname1);
        assert!(fname2.contains("(2)"), "got: {}", fname2);
    }

    #[test]
    fn test_determine_output_path() {
        let job = JobRecord {
            id: 1,
            input_path: PathBuf::from("/home/user/photo.jpg"),
            name: "photo.jpg".into(),
            size_bytes: 1000,
            tool_id: "convert-image".into(),
            tool_label: "Convert image".into(),
            status: JobStatus::Waiting,
            error: String::new(),
            output_path: PathBuf::new(),
            progress: 0.0,
        };
        let path = determine_output_path(&job, true, None, "converted", "png");
        assert_eq!(path.file_name().unwrap(), "photo converted.png");
    }
}
