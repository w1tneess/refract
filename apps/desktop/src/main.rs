slint::include_modules!();

mod pipeline;

use pipeline::{
    plan_execution_units, resolve_recorded_batch_indices, select_batch_retry_indices,
    DesktopToolOptions, JobRequest, OutputDestination, PlannedUnit, TrimEnd, UiOptionInput,
};
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use refract_core::tools::ToolId;
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
    tool_label: String,
    request: JobRequest,
    last_batch_member_ids: Vec<i32>,
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

fn format_primary_extension(fmt: Format) -> &'static str {
    fmt.primary_extension()
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
        items
            .iter()
            .map(|s| SharedString::from(*s))
            .collect::<Vec<_>>(),
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

    // Populate defaults and dropdown models from the canonical manifest
    if let Some(tool_id) = ToolId::from_id_str(spec.id.as_str()) {
        if let Ok(defaults) = DesktopToolOptions::default_for(tool_id) {
            match defaults {
                DesktopToolOptions::ConvertImage { target, quality } => {
                    set_combo_model(window, &["jpg", "png", "webp"], target.primary_extension());
                    window.set_opt_int_1(i32::from(quality));
                }
                DesktopToolOptions::CompressImage { target_kb } => {
                    window.set_opt_int_1(target_kb as i32);
                }
                DesktopToolOptions::ResizeImage { width, height, .. } => {
                    window.set_opt_int_1(width.round() as i32);
                    window.set_opt_int_2(height.round() as i32);
                }
                DesktopToolOptions::ImagesToPdf { dpi, quality } => {
                    window.set_opt_int_1(i32::from(quality));
                    window.set_opt_int_2(dpi.round() as i32);
                    window.set_opt_float_1(dpi as f32);
                }
                DesktopToolOptions::PdfToImages { target, dpi } => {
                    set_combo_model(window, &["png", "jpg"], target.primary_extension());
                    window.set_opt_int_1(dpi as i32);
                    window.set_opt_float_1(dpi as f32);
                }
                DesktopToolOptions::SplitPdf { ranges } => {
                    window.set_opt_string_1(ranges.into());
                }
                DesktopToolOptions::ReorderPdf { order } => {
                    window.set_opt_string_1(order.into());
                }
                DesktopToolOptions::ConvertAudio {
                    target,
                    bitrate_kbps,
                } => {
                    set_combo_model(
                        window,
                        &["mp3", "wav", "m4a", "ogg"],
                        target.primary_extension(),
                    );
                    if let Some(br) = bitrate_kbps {
                        window.set_opt_combo_2(br.to_string().into());
                    }
                }
                DesktopToolOptions::TrimAudio {
                    start_secs, end, ..
                } => {
                    let fmt_num = |v: f64| {
                        if v.fract() == 0.0 {
                            format!("{}", v as u64)
                        } else {
                            format!("{}", v)
                        }
                    };
                    window.set_opt_string_1(fmt_num(start_secs).into());
                    let end_str = match end {
                        TrimEnd::EndOfFile => "0".to_string(),
                        TrimEnd::AtSeconds(secs) => fmt_num(secs),
                    };
                    window.set_opt_string_2(end_str.into());
                }
                DesktopToolOptions::MergePdf | DesktopToolOptions::CompressPdf => {}
            }
        }
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

/// Capture and validate an immutable [`JobRequest`] from current UI controls at enqueue time.
fn capture_job_request(window: &MainWindow) -> Result<JobRequest, RefractError> {
    let tool_id = window.get_selected_tool_id().to_string();
    let string_1 = window.get_opt_string_1().to_string();
    let string_2 = window.get_opt_string_2().to_string();
    let combo_1 = window.get_opt_combo_1().to_string();
    let combo_2 = window.get_opt_combo_2().to_string();
    let output_folder = window.get_output_folder().to_string();

    let raw = UiOptionInput {
        string_1: &string_1,
        string_2: &string_2,
        int_1: window.get_opt_int_1(),
        int_2: window.get_opt_int_2(),
        combo_1: &combo_1,
        combo_2: &combo_2,
        resize_unit: None,
        resize_dpi: None,
        resize_target: None,
        trim_bitrate: None,
    };

    JobRequest::from_ui(
        &tool_id,
        &raw,
        window.get_output_next_to_source(),
        &output_folder,
    )
}

fn enqueue_files(
    store: &mut Vec<JobRecord>,
    next_id: &mut i32,
    files: Vec<PathBuf>,
    request: &JobRequest,
) {
    let tool_label = request.tool().label().to_string();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let id = *next_id;
        *next_id += 1;

        store.push(JobRecord {
            id,
            input_path: path,
            name,
            size_bytes,
            tool_label: tool_label.clone(),
            request: request.clone(),
            last_batch_member_ids: Vec::new(),
            status: JobStatus::Waiting,
            error: String::new(),
            output_path: PathBuf::new(),
            progress: -1.0,
        });
    }
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

        let request = match capture_job_request(&window) {
            Ok(req) => req,
            Err(err) => {
                push_ui_event(&ui_tx, UiEvent::StatusUpdate(err.to_string(), -1.0));
                return;
            }
        };

        let tool_id = request.tool().id_str();
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
            enqueue_files(&mut store, &mut id_lock, files, &request);
            push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
            push_ui_event(&ui_tx, UiEvent::StatusUpdate("Ready".into(), -1.0));
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

fn setup_tool_changed(window: &MainWindow, tool_specs: &[slint_generatedMainWindow::ToolSpec]) {
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

enum PasswordUnlockClaim {
    Single(JobRecord),
    MergeBatch {
        request: JobRequest,
        members: Vec<JobRecord>,
    },
}

fn mark_jobs_failed_with_error(
    store: &mut [JobRecord],
    target_id: i32,
    recorded_ids: &[i32],
    err: &RefractError,
) {
    for j in store.iter_mut() {
        let belongs_to_same_recorded_batch = !recorded_ids.is_empty()
            && recorded_ids.contains(&j.id)
            && j.last_batch_member_ids == recorded_ids
            && !matches!(j.status, JobStatus::Done | JobStatus::Working);
        if j.id == target_id || belongs_to_same_recorded_batch {
            j.status = JobStatus::Failed;
            j.error = err.to_string();
            j.progress = -1.0;
        }
    }
}

fn try_claim_password_unlock(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    is_busy: &AtomicBool,
    job_id: i32,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) -> Option<Result<PasswordUnlockClaim, RefractError>> {
    if is_busy.swap(true, Ordering::SeqCst) {
        return None;
    }

    let outcome = {
        let mut store = jobs_store.lock().unwrap();
        let Some(target_idx) = store.iter().position(|j| j.id == job_id) else {
            is_busy.store(false, Ordering::SeqCst);
            return None;
        };
        let target = store[target_idx].clone();

        if target.status != JobStatus::NeedsPassword {
            is_busy.store(false, Ordering::SeqCst);
            return Some(Err(RefractError::new(
                ErrorCode::InvalidOption,
                format!(
                    "Job {} is not awaiting a password (current status: {:?}).",
                    job_id, target.status
                ),
            )));
        }

        if target.request.tool() == ToolId::MergePdf {
            match resolve_recorded_batch_indices(
                &store,
                job_id,
                &target.last_batch_member_ids,
                |j| j.id,
                |j| &j.request,
                |j| j.status == JobStatus::NeedsPassword,
            ) {
                Ok(indices) => {
                    let mut members = Vec::with_capacity(indices.len());
                    for idx in indices {
                        store[idx].status = JobStatus::Working;
                        store[idx].progress = 0.0;
                        store[idx].error.clear();
                        members.push(store[idx].clone());
                    }
                    if let Some(tx) = ui_tx {
                        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
                    }
                    Ok(PasswordUnlockClaim::MergeBatch {
                        request: target.request,
                        members,
                    })
                }
                Err(err) => {
                    mark_jobs_failed_with_error(
                        &mut store,
                        job_id,
                        &target.last_batch_member_ids,
                        &err,
                    );
                    if let Some(tx) = ui_tx {
                        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
                    }
                    is_busy.store(false, Ordering::SeqCst);
                    Err(err)
                }
            }
        } else {
            store[target_idx].status = JobStatus::Working;
            store[target_idx].progress = 0.0;
            store[target_idx].error.clear();
            let claimed = store[target_idx].clone();
            if let Some(tx) = ui_tx {
                push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
            }
            Ok(PasswordUnlockClaim::Single(claimed))
        }
    };

    Some(outcome)
}

fn complete_password_unlock_claim(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    claim: PasswordUnlockClaim,
    password: &str,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) -> Result<PathBuf, RefractError> {
    let (ids, result) = match claim {
        PasswordUnlockClaim::MergeBatch { request, members } => {
            let ids: Vec<i32> = members.iter().map(|j| j.id).collect();
            let res = execute_batch_merge_pdf_with_password(&members, &request, Some(password));
            (ids, res)
        }
        PasswordUnlockClaim::Single(job) => {
            let id = job.id;
            let res = process_job_with_password(&job, password);
            (vec![id], res)
        }
    };

    let mut store = jobs_store.lock().unwrap();
    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
        match &result {
            Ok(out_path) => {
                j.status = JobStatus::Done;
                j.output_path = out_path.clone();
                j.error.clear();
            }
            Err(RefractError {
                code: ErrorCode::WrongPassword,
                ..
            }) => {
                j.status = JobStatus::NeedsPassword;
                j.error = "Wrong password — check it and try again.".to_string();
            }
            Err(RefractError {
                code: ErrorCode::NeedsPassword,
                message,
                ..
            }) => {
                j.status = JobStatus::NeedsPassword;
                j.error = message.clone();
            }
            Err(e) => {
                j.status = JobStatus::Failed;
                j.error = e.to_string();
            }
        }
        j.progress = -1.0;
    }

    if let Some(tx) = ui_tx {
        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
    }
    result
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

        let claim = match try_claim_password_unlock(&jobs_store, &is_busy, job_id, Some(&ui_tx)) {
            Some(Ok(claim)) => claim,
            Some(Err(err)) => {
                push_ui_event(&ui_tx, UiEvent::StatusUpdate(err.to_string(), -1.0));
                return;
            }
            None => return,
        };

        window.set_is_running(true);
        let worker_window = window_weak.clone();
        let worker_jobs = Arc::clone(&jobs_store);
        let worker_busy = Arc::clone(&is_busy);
        let worker_ui_tx = ui_tx.clone();

        thread::spawn(move || {
            push_ui_event(
                &worker_ui_tx,
                UiEvent::StatusUpdate("Unlocking PDF…".into(), 0.0),
            );

            // Uses the job's original immutable `JobRequest` (and recorded batch members for
            // `merge-pdf`) — never recaptures UI options.
            let _ =
                complete_password_unlock_claim(&worker_jobs, claim, &password, Some(&worker_ui_tx));

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

            let planned_units = plan_execution_units(waiting_jobs, |j| &j.request);
            let total_units = planned_units.len();

            for (completed, unit) in planned_units.into_iter().enumerate() {
                match unit {
                    PlannedUnit::Batch {
                        tool: ToolId::MergePdf,
                        request,
                        items,
                    } => {
                        let ids: Vec<i32> = items.iter().map(|j| j.id).collect();
                        {
                            let mut store = worker_jobs.lock().unwrap();
                            for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                                j.status = JobStatus::Working;
                                j.progress = 0.0;
                                j.last_batch_member_ids = ids.clone();
                            }
                            push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                        }
                        push_ui_event(
                            &worker_ui_tx,
                            UiEvent::StatusUpdate(
                                format!("Merging {} PDFs…", items.len()),
                                completed as f32 / total_units as f32,
                            ),
                        );

                        let merge_result = execute_batch_merge_pdf(&items, &request);
                        apply_batch_result(&worker_jobs, &ids, merge_result, &worker_ui_tx);
                    }
                    PlannedUnit::Batch {
                        tool: ToolId::ImagesToPdf,
                        request,
                        items,
                    } => {
                        let ids: Vec<i32> = items.iter().map(|j| j.id).collect();
                        {
                            let mut store = worker_jobs.lock().unwrap();
                            for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                                j.status = JobStatus::Working;
                                j.progress = 0.0;
                                j.last_batch_member_ids = ids.clone();
                            }
                            push_ui_event(&worker_ui_tx, UiEvent::JobsUpdated(store.clone()));
                        }
                        push_ui_event(
                            &worker_ui_tx,
                            UiEvent::StatusUpdate(
                                format!("Compiling {} images into PDF…", items.len()),
                                completed as f32 / total_units as f32,
                            ),
                        );

                        let compile_result = execute_batch_images_to_pdf(&items, &request);
                        apply_batch_result(&worker_jobs, &ids, compile_result, &worker_ui_tx);
                    }
                    PlannedUnit::Batch { items, .. } => {
                        for job in items {
                            run_single_job_in_store(
                                &worker_jobs,
                                &job,
                                completed,
                                total_units,
                                &worker_ui_tx,
                            );
                        }
                    }
                    PlannedUnit::Single(job) => {
                        run_single_job_in_store(
                            &worker_jobs,
                            &job,
                            completed,
                            total_units,
                            &worker_ui_tx,
                        );
                    }
                }

                push_ui_event(
                    &worker_ui_tx,
                    UiEvent::StatusUpdate(
                        format!("Completed {} of {}", completed + 1, total_units),
                        (completed + 1) as f32 / total_units as f32,
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

fn apply_batch_result(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    ids: &[i32],
    result: Result<PathBuf, RefractError>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let mut store = jobs_store.lock().unwrap();
    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
        match &result {
            Ok(path) => {
                j.status = JobStatus::Done;
                j.output_path = path.clone();
                j.error.clear();
            }
            Err(RefractError {
                code: ErrorCode::NeedsPassword,
                message,
                ..
            }) => {
                j.status = JobStatus::NeedsPassword;
                j.error = message.clone();
            }
            Err(e) => {
                j.status = JobStatus::Failed;
                j.error = e.to_string();
            }
        }
        j.progress = -1.0;
    }
    push_ui_event(ui_tx, UiEvent::JobsUpdated(store.clone()));
}

fn run_single_job_in_store(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    job: &JobRecord,
    completed: usize,
    total_units: usize,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    {
        let mut store = jobs_store.lock().unwrap();
        if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
            j.status = JobStatus::Working;
            j.progress = 0.0;
        }
        push_ui_event(ui_tx, UiEvent::JobsUpdated(store.clone()));
    }

    push_ui_event(
        ui_tx,
        UiEvent::StatusUpdate(
            format!(
                "Processing {} of {}: {}",
                completed + 1,
                total_units,
                job.name
            ),
            completed as f32 / total_units as f32,
        ),
    );

    let result = process_job(job);

    {
        let mut store = jobs_store.lock().unwrap();
        if let Some(j) = store.iter_mut().find(|j| j.id == job.id) {
            match result {
                Ok(out_path) => {
                    j.status = JobStatus::Done;
                    j.output_path = out_path;
                    j.error.clear();
                }
                Err(RefractError {
                    code: ErrorCode::NeedsPassword,
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
        push_ui_event(ui_tx, UiEvent::JobsUpdated(store.clone()));
    }
}

fn execute_retry_in_store(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    job_id: i32,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) {
    let members: Vec<JobRecord> = {
        let mut store = jobs_store.lock().unwrap();
        let Some(target) = store.iter().find(|j| j.id == job_id).cloned() else {
            return;
        };
        let recorded_ids = target.last_batch_member_ids.clone();
        match select_batch_retry_indices(
            &store,
            job_id,
            &recorded_ids,
            |j| j.id,
            |j| &j.request,
            |j| matches!(j.status, JobStatus::Failed | JobStatus::Waiting),
        ) {
            Ok(indices) => indices.into_iter().map(|i| store[i].clone()).collect(),
            Err(err) => {
                mark_jobs_failed_with_error(&mut store, job_id, &recorded_ids, &err);
                if let Some(tx) = ui_tx {
                    push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
                }
                return;
            }
        }
    };

    let Some(primary) = members.first().cloned() else {
        return;
    };
    let request = primary.request.clone();
    let ids: Vec<i32> = members.iter().map(|j| j.id).collect();

    {
        let mut store = jobs_store.lock().unwrap();
        for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
            j.status = JobStatus::Working;
            j.progress = 0.0;
            j.error.clear();
            if request.tool().spec().batch {
                j.last_batch_member_ids = ids.clone();
            }
        }
        if let Some(tx) = ui_tx {
            push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
        }
    }

    if let Some(tx) = ui_tx {
        push_ui_event(
            tx,
            UiEvent::StatusUpdate(format!("Retrying: {}", primary.name), 0.0),
        );
    }

    let result = match request.tool() {
        ToolId::MergePdf => execute_batch_merge_pdf(&members, &request),
        ToolId::ImagesToPdf => execute_batch_images_to_pdf(&members, &request),
        _ => process_job(&primary),
    };

    {
        let mut store = jobs_store.lock().unwrap();
        for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
            match &result {
                Ok(out_path) => {
                    j.status = JobStatus::Done;
                    j.output_path = out_path.clone();
                    j.error.clear();
                }
                Err(RefractError {
                    code: ErrorCode::NeedsPassword,
                    message,
                    ..
                }) => {
                    j.status = JobStatus::NeedsPassword;
                    j.error = message.clone();
                }
                Err(e) => {
                    j.status = JobStatus::Failed;
                    j.error = e.to_string();
                }
            }
            j.progress = -1.0;
        }
        if let Some(tx) = ui_tx {
            push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
        }
    }
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
        let worker_window = window_weak.clone();
        let worker_jobs = Arc::clone(&jobs_store);
        let worker_busy = Arc::clone(&is_busy);
        let worker_ui_tx = ui_tx.clone();

        thread::spawn(move || {
            // Re-runs with the job's original immutable `JobRequest` (and compatible batch peers).
            execute_retry_in_store(&worker_jobs, job_id, Some(&worker_ui_tx));

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
    destination: &OutputDestination,
    suffix: &str,
    extension: &str,
) -> PathBuf {
    let parent = destination.resolve_parent(&job.input_path);
    let stem = job
        .input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let out_path = parent.join(format!("{} {}.{}", stem, suffix, extension));
    uniquify_path(&out_path)
}

fn ensure_batch_compatible(jobs: &[JobRecord], request: &JobRequest) -> Result<(), RefractError> {
    if jobs.is_empty() {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Batch job has no input files.",
        ));
    }
    if jobs
        .iter()
        .any(|job| !request.is_batch_compatible_with(&job.request))
    {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Cannot combine jobs with incompatible options or output destinations.",
        ));
    }
    Ok(())
}

fn execute_batch_merge_pdf_with_password(
    jobs: &[JobRecord],
    request: &JobRequest,
    password: Option<&str>,
) -> Result<PathBuf, RefractError> {
    ensure_batch_compatible(jobs, request)?;
    if jobs.len() < 2 {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Merge PDF needs at least 2 compatible PDF files with the same output destination.",
        ));
    }
    let buffers: Vec<Vec<u8>> = jobs
        .iter()
        .map(|j| std::fs::read(&j.input_path).map_err(to_refract_err))
        .collect::<Result<_, _>>()?;
    let slices: Vec<&[u8]> = buffers.iter().map(|b| b.as_slice()).collect();
    let out_bytes = match password {
        Some(pw) => pdf::merge_pdfs_with_password(&slices, pw)?,
        None => pdf::merge_pdfs(&slices)?,
    };
    let out_path = determine_output_path(&jobs[0], request.destination(), "merged", "pdf");
    std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
    Ok(out_path)
}

fn execute_batch_merge_pdf(
    jobs: &[JobRecord],
    request: &JobRequest,
) -> Result<PathBuf, RefractError> {
    execute_batch_merge_pdf_with_password(jobs, request, None)
}

fn execute_batch_images_to_pdf(
    jobs: &[JobRecord],
    request: &JobRequest,
) -> Result<PathBuf, RefractError> {
    ensure_batch_compatible(jobs, request)?;
    let DesktopToolOptions::ImagesToPdf { dpi, quality } = *request.options() else {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Expected ImagesToPdf options for batch compilation.",
        ));
    };
    let buffers: Vec<Vec<u8>> = jobs
        .iter()
        .map(|j| std::fs::read(&j.input_path).map_err(to_refract_err))
        .collect::<Result<_, _>>()?;
    let out_bytes = pdf::images_to_pdf(&buffers, dpi, quality)?;
    let out_path = determine_output_path(&jobs[0], request.destination(), "images", "pdf");
    std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
    Ok(out_path)
}

fn render_or_extract_pdf_images(
    job: &JobRecord,
    bytes: &[u8],
    target: Format,
    dpi: u32,
    password: Option<&str>,
) -> Result<PathBuf, RefractError> {
    let format = format_primary_extension(target);
    let parent = job.request.destination().resolve_parent(&job.input_path);
    let stem = job
        .input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");

    match render::render_pdf_to_images_with_password_cancellable(
        bytes,
        password,
        dpi,
        format,
        &CancelToken::none(),
    ) {
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
            if render_err.code.is_password_error() {
                return Err(render_err);
            }
            let embedded =
                pdf::extract_embedded_images_with_password(bytes, password, &CancelToken::none())?;
            if embedded.is_empty() {
                return Err(render_err);
            }
            let mut first_written =
                parent.join(format!("{} p0001.{}", stem, embedded[0].extension));
            for (idx, img) in embedded.iter().enumerate() {
                let out_path = parent.join(format!("{} p{:04}.{}", stem, img.page, img.extension));
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

fn process_job(job: &JobRecord) -> Result<PathBuf, RefractError> {
    let destination = job.request.destination();
    match job.request.options() {
        DesktopToolOptions::ConvertImage { target, quality } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = image::convert_image(&bytes, *target, Some(*quality))?;
            let out_path = determine_output_path(
                job,
                destination,
                "converted",
                format_primary_extension(*target),
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        DesktopToolOptions::CompressImage { target_kb } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = image::compress_image(&bytes, *target_kb)?;
            let out_path = determine_output_path(job, destination, "compressed", "jpg");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        opts @ DesktopToolOptions::ResizeImage { .. } => {
            let engine_opts = opts.as_engine_resize_options().ok_or_else(|| {
                RefractError::new(ErrorCode::InvalidOption, "Invalid resize options.")
            })?;
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let (out_bytes, out_fmt) = image::resize_image_with_options(&bytes, &engine_opts)?;
            let out_path = determine_output_path(
                job,
                destination,
                "resized",
                format_primary_extension(out_fmt),
            );
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        DesktopToolOptions::ImagesToPdf { .. } => {
            execute_batch_images_to_pdf(std::slice::from_ref(job), &job.request)
        }
        DesktopToolOptions::PdfToImages { target, dpi } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            render_or_extract_pdf_images(job, &bytes, *target, *dpi, None)
        }
        DesktopToolOptions::CompressPdf => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::compress_pdf(&bytes)?;
            let out_path = determine_output_path(job, destination, "compressed", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        DesktopToolOptions::SplitPdf { ranges } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let parts = pdf::split_pdf(&bytes, ranges)?;
            let parent = destination.resolve_parent(&job.input_path);
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
        DesktopToolOptions::ReorderPdf { order } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::reorder_pdf(&bytes, order)?;
            let out_path = determine_output_path(job, destination, "reordered", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        DesktopToolOptions::MergePdf => Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Merge PDF needs at least 2 compatible PDF files with the same output destination.",
        )),
        DesktopToolOptions::ConvertAudio {
            target,
            bitrate_kbps,
        } => {
            if !audio::is_ffmpeg_available() {
                return Err(RefractError::new(
                    ErrorCode::MissingDependency,
                    "FFmpeg not found. Install FFmpeg and ensure it is on PATH to convert audio.",
                ));
            }
            let out_path = determine_output_path(
                job,
                destination,
                "converted",
                format_primary_extension(*target),
            );
            audio::convert_audio(&job.input_path, &out_path, *target, *bitrate_kbps)?;
            Ok(out_path)
        }
        DesktopToolOptions::TrimAudio {
            start_secs,
            end,
            bitrate_kbps,
        } => {
            if !audio::is_ffmpeg_available() {
                return Err(RefractError::new(
                    ErrorCode::MissingDependency,
                    "FFmpeg not found. Install FFmpeg and ensure it is on PATH to trim audio.",
                ));
            }
            let ext = job
                .input_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp3");
            let out_path = determine_output_path(job, destination, "trimmed", ext);
            let start_arg = format!("{:.3}", start_secs);
            let end_arg = end.to_engine_arg();
            audio::trim_audio_cancellable(
                &job.input_path,
                &out_path,
                Some(start_arg.as_str()),
                end_arg.as_deref(),
                Some(*bitrate_kbps),
                &CancelToken::none(),
            )?;
            Ok(out_path)
        }
    }
}

fn process_job_with_password(job: &JobRecord, password: &str) -> Result<PathBuf, RefractError> {
    let destination = job.request.destination();
    match job.request.options() {
        DesktopToolOptions::CompressPdf => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::compress_pdf_with_password(&bytes, password)?;
            let out_path = determine_output_path(job, destination, "compressed", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        DesktopToolOptions::PdfToImages { target, dpi } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            render_or_extract_pdf_images(job, &bytes, *target, *dpi, Some(password))
        }
        DesktopToolOptions::SplitPdf { ranges } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let parts =
                pdf::split_pdf_with_password(&bytes, ranges, Some(password), &CancelToken::none())?;
            let parent = destination.resolve_parent(&job.input_path);
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
        DesktopToolOptions::ReorderPdf { order } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = pdf::reorder_pdf_with_password(
                &bytes,
                order,
                Some(password),
                &CancelToken::none(),
            )?;
            let out_path = determine_output_path(job, destination, "reordered", "pdf");
            std::fs::write(&out_path, out_bytes).map_err(to_refract_err)?;
            Ok(out_path)
        }
        DesktopToolOptions::MergePdf => Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Merge PDF unlock requires the complete recorded batch.",
        )),
        _ => Err(RefractError::new(
            ErrorCode::UnsupportedFormat,
            "Password not needed for this tool",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::options::parse_format_strict;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

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
    fn test_parse_format_strict() {
        assert_eq!(parse_format_strict("mp3").unwrap(), Format::Mp3);
        assert_eq!(parse_format_strict("wav").unwrap(), Format::Wav);
        assert_eq!(parse_format_strict("webp").unwrap(), Format::Webp);
        assert_eq!(parse_format_strict("png").unwrap(), Format::Png);
        assert_eq!(parse_format_strict("jpg").unwrap(), Format::Jpeg);
        assert_eq!(
            parse_format_strict("unknown").unwrap_err().code,
            ErrorCode::InvalidOption
        );
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
        let req = JobRequest::new(
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Png,
                quality: 90,
            },
            OutputDestination::NextToSource,
        )
        .unwrap();
        let job = JobRecord {
            id: 1,
            input_path: PathBuf::from("/home/user/photo.jpg"),
            name: "photo.jpg".into(),
            size_bytes: 1000,
            tool_label: "Convert image".into(),
            request: req,
            last_batch_member_ids: Vec::new(),
            status: JobStatus::Waiting,
            error: String::new(),
            output_path: PathBuf::new(),
            progress: 0.0,
        };
        let path = determine_output_path(&job, job.request.destination(), "converted", "png");
        assert_eq!(path.file_name().unwrap(), "photo converted.png");
    }

    #[test]
    fn enqueue_captures_immutable_request_unaffected_by_later_ui_changes() {
        let out_dir_a = tempfile::tempdir().unwrap();
        let out_dir_b = tempfile::tempdir().unwrap();
        let mut store = Vec::new();
        let mut next_id = 1;

        // Enqueue PNG->WebP into out_dir_a
        let req_a = JobRequest::from_ui(
            "convert-image",
            &UiOptionInput {
                combo_1: "webp",
                int_1: 80,
                ..Default::default()
            },
            false,
            out_dir_a.path().to_str().unwrap(),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.png")],
            &req_a,
        );

        // Simulate user changing UI to JPEG + out_dir_b and enqueuing another file
        let req_b = JobRequest::from_ui(
            "convert-image",
            &UiOptionInput {
                combo_1: "jpg",
                int_1: 60,
                ..Default::default()
            },
            false,
            out_dir_b.path().to_str().unwrap(),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.png")],
            &req_b,
        );

        // First job still retains its original WebP + out_dir_a snapshot
        assert_eq!(store[0].request, req_a);
        let out_a = process_job(&store[0]).unwrap();
        assert_eq!(out_a.parent(), Some(out_dir_a.path()));
        assert_eq!(
            refract_core::detect(&std::fs::read(&out_a).unwrap()),
            Some(Format::Webp)
        );

        let out_b = process_job(&store[1]).unwrap();
        assert_eq!(out_b.parent(), Some(out_dir_b.path()));
        assert_eq!(
            refract_core::detect(&std::fs::read(&out_b).unwrap()),
            Some(Format::Jpeg)
        );
    }

    #[test]
    fn mixed_tool_queue_preserves_and_executes_distinct_requests() {
        let out_dir = tempfile::tempdir().unwrap();
        let mut store = Vec::new();
        let mut next_id = 1;

        // 1. resize-image with cm + 300 DPI + explicit WebP target
        let resize_req = JobRequest::from_ui(
            "resize-image",
            &UiOptionInput {
                int_1: 2,
                int_2: 0,
                resize_unit: Some("cm"),
                resize_dpi: Some(254.0), // 2 cm @ 254 DPI = 200 px -> 200x150
                resize_target: Some("webp"),
                ..Default::default()
            },
            false,
            out_dir.path().to_str().unwrap(),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.png")],
            &resize_req,
        );

        // 2. split-pdf with canonical empty ranges "" (splits every page of 3-page plain.pdf)
        let split_req = JobRequest::from_ui(
            "split-pdf",
            &UiOptionInput {
                string_1: "",
                ..Default::default()
            },
            false,
            out_dir.path().to_str().unwrap(),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.pdf")],
            &split_req,
        );

        // 3. reorder-pdf with explicit order
        let reorder_req = JobRequest::from_ui(
            "reorder-pdf",
            &UiOptionInput {
                string_1: "3, 1, 2",
                ..Default::default()
            },
            false,
            out_dir.path().to_str().unwrap(),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.pdf")],
            &reorder_req,
        );

        let planned = plan_execution_units(store, |j| &j.request);
        assert_eq!(planned.len(), 3);

        let PlannedUnit::Single(resize_job) = &planned[0] else {
            panic!("expected single resize job");
        };
        let resized_path = process_job(resize_job).unwrap();
        let resized_bytes = std::fs::read(&resized_path).unwrap();
        assert_eq!(refract_core::detect(&resized_bytes), Some(Format::Webp));
        assert_eq!(image::probe_dimensions(&resized_bytes).unwrap(), (200, 150));

        let PlannedUnit::Single(split_job) = &planned[1] else {
            panic!("expected single split job");
        };
        let last_split = process_job(split_job).unwrap();
        assert!(last_split.exists());
        assert!(out_dir.path().join("plain-p1.pdf").exists());
        assert!(out_dir.path().join("plain-p2.pdf").exists());
        assert!(out_dir.path().join("plain-p3.pdf").exists());

        let PlannedUnit::Single(reorder_job) = &planned[2] else {
            panic!("expected single reorder job");
        };
        let reordered_path = process_job(reorder_job).unwrap();
        assert_eq!(
            pdf::probe_pdf(&std::fs::read(&reordered_path).unwrap())
                .unwrap()
                .page_count,
            3
        );
    }

    #[test]
    fn retry_uses_original_request_for_single_and_batch_jobs() {
        let out_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(Vec::new()));
        let mut next_id = 1;

        // 1. Single failed convert-image job targeting WebP in out_dir
        let req_single = JobRequest::new(
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Webp,
                quality: 85,
            },
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("plain.png")],
                &req_single,
            );
            guard[0].status = JobStatus::Failed;
            guard[0].error = "simulated transient failure".into();
        }

        execute_retry_in_store(&store, 1, None);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[0].status, JobStatus::Done);
            assert_eq!(guard[0].output_path.parent(), Some(out_dir.path()));
            assert_eq!(
                refract_core::detect(&std::fs::read(&guard[0].output_path).unwrap()),
                Some(Format::Webp)
            );
        }

        // 2. Failed merge-pdf batch (2 compatible files) retries the full compatible batch
        let req_merge = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("plain.pdf"), fixture_path("5pages.pdf")],
                &req_merge,
            );
            guard[1].status = JobStatus::Failed;
            guard[2].status = JobStatus::Failed;
            guard[1].last_batch_member_ids = vec![2, 3];
            guard[2].last_batch_member_ids = vec![2, 3];
        }

        execute_retry_in_store(&store, 2, None);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[1].status, JobStatus::Done);
            assert_eq!(guard[2].status, JobStatus::Done);
            assert_eq!(guard[1].output_path, guard[2].output_path);
            let merged_bytes = std::fs::read(&guard[1].output_path).unwrap();
            assert_eq!(pdf::probe_pdf(&merged_bytes).unwrap().page_count, 8);
        }

        // 3. Failed images-to-pdf batch retries all compatible members with original DPI/quality
        let req_imgs = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 200.0,
                quality: 92,
            },
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("plain.png"), fixture_path("plain.jpg")],
                &req_imgs,
            );
            guard[3].status = JobStatus::Failed;
            guard[4].status = JobStatus::Failed;
            guard[3].last_batch_member_ids = vec![4, 5];
            guard[4].last_batch_member_ids = vec![4, 5];
        }

        execute_retry_in_store(&store, 4, None);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[3].status, JobStatus::Done);
            assert_eq!(guard[4].status, JobStatus::Done);
            assert_eq!(guard[3].output_path, guard[4].output_path);
            let pdf_bytes = std::fs::read(&guard[3].output_path).unwrap();
            assert_eq!(pdf::probe_pdf(&pdf_bytes).unwrap().page_count, 2);
        }
    }

    #[test]
    fn password_unlock_preserves_original_request_for_compress_split_reorder_and_pdf_to_images() {
        let out_dir = tempfile::tempdir().unwrap();
        let locked_pdf = fixture_path("locked.pdf");

        // 1. compress-pdf unlock honors custom output directory from original request
        let compress_req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        let mut store = Vec::new();
        let mut next_id = 1;
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![locked_pdf.clone()],
            &compress_req,
        );

        assert_eq!(
            process_job(&store[0]).unwrap_err().code,
            ErrorCode::NeedsPassword
        );
        assert_eq!(
            process_job_with_password(&store[0], "wrong-pw")
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        let compressed_out = process_job_with_password(&store[0], "user-pw-123").unwrap();
        assert_eq!(compressed_out.parent(), Some(out_dir.path()));
        assert_eq!(
            refract_core::detect(&std::fs::read(&compressed_out).unwrap()),
            Some(Format::Pdf)
        );

        // 2. split-pdf unlock preserves requested ranges and destination
        let split_req = JobRequest::new(
            ToolId::SplitPdf,
            DesktopToolOptions::SplitPdf {
                ranges: "1-2, 3".into(),
            },
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![locked_pdf.clone()],
            &split_req,
        );
        let split_out = process_job_with_password(&store[1], "user-pw-123").unwrap();
        assert_eq!(split_out.parent(), Some(out_dir.path()));
        assert!(out_dir.path().join("locked-p1-2.pdf").exists());
        assert!(out_dir.path().join("locked-p3.pdf").exists());

        // 3. reorder-pdf unlock preserves requested page order and destination
        let reorder_req = JobRequest::new(
            ToolId::ReorderPdf,
            DesktopToolOptions::ReorderPdf {
                order: "3, 1, 2".into(),
            },
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![locked_pdf.clone()],
            &reorder_req,
        );
        let reorder_out = process_job_with_password(&store[2], "user-pw-123").unwrap();
        assert_eq!(reorder_out.parent(), Some(out_dir.path()));
        assert_eq!(
            pdf::probe_pdf(&std::fs::read(&reorder_out).unwrap())
                .unwrap()
                .page_count,
            3
        );

        // 4. pdf-to-images unlock preserves non-default target format (JPEG) and DPI (240)
        //    rather than hardcoding 150 DPI / PNG; verify unlock + WrongPassword behavior even
        //    on hosts without an external PDF rasterizer.
        let render_req = JobRequest::new(
            ToolId::PdfToImages,
            DesktopToolOptions::PdfToImages {
                target: Format::Jpeg,
                dpi: 240,
            },
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();
        enqueue_files(&mut store, &mut next_id, vec![locked_pdf], &render_req);
        assert_eq!(
            store[3].request.options(),
            &DesktopToolOptions::PdfToImages {
                target: Format::Jpeg,
                dpi: 240,
            }
        );
        assert_eq!(
            process_job_with_password(&store[3], "wrong-pw")
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        match process_job_with_password(&store[3], "user-pw-123") {
            Ok(first_page) => {
                assert_eq!(first_page.parent(), Some(out_dir.path()));
                assert_eq!(first_page.extension().and_then(|e| e.to_str()), Some("jpg"));
            }
            Err(err) => {
                // On hosts without a PDF rasterizer (`mutool`/`pdftoppm`/`pdftocairo`/`gs`),
                // `locked.pdf` has no embedded raster images so MissingDependency is expected —
                // never WrongPassword or NeedsPassword.
                assert_eq!(err.code, ErrorCode::MissingDependency);
            }
        }
    }

    #[test]
    fn batch_execution_rejects_incompatible_requests_and_separates_them_deterministically() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let mut store = Vec::new();
        let mut next_id = 1;

        // Incompatible merge-pdf jobs (different destinations) must NOT be combined
        let merge_a = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(dir_a.path().to_path_buf()),
        )
        .unwrap();
        let merge_b = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(dir_b.path().to_path_buf()),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.pdf")],
            &merge_a,
        );
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("5pages.pdf")],
            &merge_b,
        );

        // Direct batch execution rejects mixed requests
        assert_eq!(
            execute_batch_merge_pdf(&store, &merge_a).unwrap_err().code,
            ErrorCode::InvalidOption
        );

        // Planner separates them into 2 single-file merge units, each rejected cleanly (< 2 files)
        let planned = plan_execution_units(store, |j| &j.request);
        assert_eq!(planned.len(), 2);
        for unit in planned {
            let PlannedUnit::Batch { request, items, .. } = unit else {
                panic!("expected batch unit");
            };
            assert_eq!(items.len(), 1);
            assert_eq!(
                execute_batch_merge_pdf(&items, &request).unwrap_err().code,
                ErrorCode::InvalidOption
            );
        }

        // Contiguous compatible images-to-pdf jobs combine in order, while incompatible jobs flush and stay separate
        let mut img_store = Vec::new();
        let img_a = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 80,
            },
            OutputDestination::Directory(dir_a.path().to_path_buf()),
        )
        .unwrap();
        let img_b = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 300.0,
                quality: 95,
            },
            OutputDestination::Directory(dir_b.path().to_path_buf()),
        )
        .unwrap();
        // Queue: [img_a(png), img_a(webp), img_b(jpg), img_a(png)] -> 3 contiguous units: [2, 1, 1]
        enqueue_files(
            &mut img_store,
            &mut next_id,
            vec![fixture_path("plain.png"), fixture_path("plain.webp")],
            &img_a,
        );
        enqueue_files(
            &mut img_store,
            &mut next_id,
            vec![fixture_path("plain.jpg")],
            &img_b,
        );
        enqueue_files(
            &mut img_store,
            &mut next_id,
            vec![fixture_path("plain.png")],
            &img_a,
        );

        // Direct call rejects mixed batch
        assert_eq!(
            execute_batch_images_to_pdf(&img_store, &img_a)
                .unwrap_err()
                .code,
            ErrorCode::InvalidOption
        );

        let img_units = plan_execution_units(img_store, |j| &j.request);
        assert_eq!(img_units.len(), 3);
        let PlannedUnit::Batch {
            request: req0,
            items: items0,
            ..
        } = &img_units[0]
        else {
            panic!("expected first batch");
        };
        assert_eq!(items0.len(), 2); // contiguous plain.png + plain.webp in original order
        let pdf_a = execute_batch_images_to_pdf(items0, req0).unwrap();
        assert_eq!(pdf_a.parent(), Some(dir_a.path()));
        assert_eq!(
            pdf::probe_pdf(&std::fs::read(&pdf_a).unwrap())
                .unwrap()
                .page_count,
            2
        );

        let PlannedUnit::Batch {
            request: req1,
            items: items1,
            ..
        } = &img_units[1]
        else {
            panic!("expected second batch");
        };
        assert_eq!(items1.len(), 1); // plain.jpg alone at 300 DPI / q95
        let pdf_b = execute_batch_images_to_pdf(items1, req1).unwrap();
        assert_eq!(pdf_b.parent(), Some(dir_b.path()));
        assert_eq!(
            pdf::probe_pdf(&std::fs::read(&pdf_b).unwrap())
                .unwrap()
                .page_count,
            1
        );

        let PlannedUnit::Batch {
            request: req2,
            items: items2,
            ..
        } = &img_units[2]
        else {
            panic!("expected third batch after intervening incompatible job");
        };
        assert_eq!(*req2, img_a);
        assert_eq!(items2.len(), 1); // trailing img_a was NOT hoisted ahead of img_b
        let pdf_c = execute_batch_images_to_pdf(items2, req2).unwrap();
        assert_eq!(pdf_c.parent(), Some(dir_a.path()));
        assert_eq!(
            pdf::probe_pdf(&std::fs::read(&pdf_c).unwrap())
                .unwrap()
                .page_count,
            1
        );
    }

    fn write_marker_pdf(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let plain_png = std::fs::read(fixture_path("plain.png")).unwrap();
        let resized = image::resize_image(&plain_png, width, height).unwrap();
        let pdf_bytes = pdf::images_to_pdf(&[resized], 150.0, 90).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, pdf_bytes).unwrap();
        path
    }

    fn page_embedded_image_dims(pdf_bytes: &[u8], page: u32) -> (u32, u32) {
        let parts = pdf::split_pdf(pdf_bytes, &page.to_string()).unwrap();
        assert_eq!(parts.len(), 1);
        let embedded = pdf::extract_embedded_images(&parts[0].1).unwrap();
        assert!(
            !embedded.is_empty(),
            "expected embedded marker image on page {}",
            page
        );
        image::probe_dimensions(&embedded[0].bytes).unwrap()
    }

    #[test]
    fn password_unlock_claim_prevents_concurrent_unlock_races() {
        let store = Arc::new(Mutex::new(Vec::new()));
        let is_busy = AtomicBool::new(false);
        let req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        let mut next_id = 1;
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("locked.pdf")],
                &req,
            );
            guard[0].status = JobStatus::NeedsPassword;
        }

        // Unknown job releases busy flag cleanly
        assert!(try_claim_password_unlock(&store, &is_busy, 999, None).is_none());
        assert!(!is_busy.load(Ordering::SeqCst));

        // First unlock claims busy flag and preserves immutable request
        let claimed = try_claim_password_unlock(&store, &is_busy, 1, None)
            .unwrap()
            .unwrap();
        match claimed {
            PasswordUnlockClaim::Single(job) => assert_eq!(job.request, req),
            PasswordUnlockClaim::MergeBatch { .. } => panic!("expected Single claim"),
        }
        assert!(is_busy.load(Ordering::SeqCst));

        // Second concurrent unlock attempt is rejected without racing
        assert!(try_claim_password_unlock(&store, &is_busy, 1, None).is_none());
    }

    #[test]
    fn retry_merge_pdf_batch_preserves_recorded_input_order_after_ui_reorder() {
        let work_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let pdf_1 = write_marker_pdf(work_dir.path(), "first.pdf", 64, 48);
        let pdf_2 = write_marker_pdf(work_dir.path(), "second.pdf", 96, 48);
        let pdf_3 = write_marker_pdf(work_dir.path(), "third.pdf", 128, 48);

        let merge_req = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();

        let store = Arc::new(Mutex::new(Vec::new()));
        let mut next_id = 1;
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![pdf_1, pdf_2, pdf_3],
                &merge_req,
            );
            // Record original batch order [1, 2, 3] and simulate failed batch
            for j in guard.iter_mut() {
                j.status = JobStatus::Failed;
                j.last_batch_member_ids = vec![1, 2, 3];
            }
            // Reorder the job store in UI to [3, 1, 2]
            guard.rotate_right(1);
            assert_eq!(
                guard.iter().map(|j| j.id).collect::<Vec<_>>(),
                vec![3, 1, 2]
            );
        }

        // Retry via job 2 (now last in UI order)
        execute_retry_in_store(&store, 2, None);

        let guard = store.lock().unwrap();
        assert!(guard.iter().all(|j| j.status == JobStatus::Done));
        let out_path = guard[0].output_path.clone();
        assert!(guard.iter().all(|j| j.output_path == out_path));
        assert_eq!(out_path.parent(), Some(out_dir.path()));
        // Output name comes from recorded first input (`first.pdf`), not reordered `third.pdf`
        assert_eq!(out_path.file_name().unwrap(), "first merged.pdf");

        let merged_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(pdf::probe_pdf(&merged_bytes).unwrap().page_count, 3);
        assert_eq!(page_embedded_image_dims(&merged_bytes, 1), (64, 48));
        assert_eq!(page_embedded_image_dims(&merged_bytes, 2), (96, 48));
        assert_eq!(page_embedded_image_dims(&merged_bytes, 3), (128, 48));
    }

    #[test]
    fn retry_merge_pdf_batch_fails_explicitly_when_recorded_member_is_missing() {
        let out_dir = tempfile::tempdir().unwrap();
        let merge_req = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();

        let store = Arc::new(Mutex::new(Vec::new()));
        let mut next_id = 1;
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![
                    fixture_path("plain.pdf"),
                    fixture_path("5pages.pdf"),
                    fixture_path("plain.pdf"),
                ],
                &merge_req,
            );
            for j in guard.iter_mut() {
                j.status = JobStatus::Failed;
                j.last_batch_member_ids = vec![1, 2, 3];
            }
            // Delete member 2 before retrying
            guard.retain(|j| j.id != 2);
        }

        execute_retry_in_store(&store, 1, None);

        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 2);
        for j in guard.iter() {
            assert_eq!(j.status, JobStatus::Failed);
            assert!(
                j.error.contains("missing") || j.error.contains("removed"),
                "unexpected error: {}",
                j.error
            );
            assert!(j.output_path.as_os_str().is_empty());
        }
        assert!(std::fs::read_dir(out_dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn password_unlock_merge_pdf_batch_handles_same_password_wrong_password_destination_and_order()
    {
        let work_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let locked_bytes = std::fs::read(fixture_path("locked.pdf")).unwrap();
        let enc_1 = work_dir.path().join("enc_first.pdf");
        let enc_2 = work_dir.path().join("enc_second.pdf");
        std::fs::write(&enc_1, &locked_bytes).unwrap();
        std::fs::write(&enc_2, &locked_bytes).unwrap();

        let merge_req = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();

        let store = Arc::new(Mutex::new(Vec::new()));
        let is_busy = AtomicBool::new(false);
        let mut next_id = 1;
        {
            let mut guard = store.lock().unwrap();
            enqueue_files(&mut guard, &mut next_id, vec![enc_1, enc_2], &merge_req);
            for j in guard.iter_mut() {
                j.last_batch_member_ids = vec![1, 2];
            }
        }

        // Initial unauthenticated merge returns NeedsPassword
        {
            let snapshot = store.lock().unwrap().clone();
            let err = execute_batch_merge_pdf(&snapshot, &merge_req).unwrap_err();
            assert_eq!(err.code, ErrorCode::NeedsPassword);
            let mut guard = store.lock().unwrap();
            for j in guard.iter_mut() {
                j.status = JobStatus::NeedsPassword;
                j.error = err.message.clone();
            }
            // Reorder the UI store to [2, 1] before unlocking
            guard.swap(0, 1);
            assert_eq!(guard.iter().map(|j| j.id).collect::<Vec<_>>(), vec![2, 1]);
        }

        // 1. Incorrect password keeps all members in NeedsPassword and releases busy flag
        let wrong_claim = try_claim_password_unlock(&store, &is_busy, 2, None)
            .unwrap()
            .unwrap();
        let wrong_res = complete_password_unlock_claim(&store, wrong_claim, "wrong-pw", None);
        is_busy.store(false, Ordering::SeqCst);
        assert_eq!(wrong_res.unwrap_err().code, ErrorCode::WrongPassword);
        {
            let guard = store.lock().unwrap();
            for j in guard.iter() {
                assert_eq!(j.status, JobStatus::NeedsPassword);
                assert!(j.error.contains("Wrong password"));
                assert!(j.output_path.as_os_str().is_empty());
            }
        }
        assert!(std::fs::read_dir(out_dir.path()).unwrap().next().is_none());

        // 2. Correct password unlocks full batch in recorded order [1, 2] into original destination
        let right_claim = try_claim_password_unlock(&store, &is_busy, 2, None)
            .unwrap()
            .unwrap();
        let out_path =
            complete_password_unlock_claim(&store, right_claim, "user-pw-123", None).unwrap();
        is_busy.store(false, Ordering::SeqCst);

        assert_eq!(out_path.parent(), Some(out_dir.path()));
        // Derived from recorded first member `enc_first.pdf`, NOT reordered `enc_second.pdf`
        assert_eq!(out_path.file_name().unwrap(), "enc_first merged.pdf");
        let merged_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(pdf::probe_pdf(&merged_bytes).unwrap().page_count, 6);

        let guard = store.lock().unwrap();
        for j in guard.iter() {
            assert_eq!(j.status, JobStatus::Done);
            assert_eq!(j.output_path, out_path);
            assert!(j.error.is_empty());
        }
    }

    #[test]
    fn password_unlock_merge_pdf_batch_preserves_exact_page_order_after_ui_reorder() {
        let work_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let locked_bytes = std::fs::read(fixture_path("locked.pdf")).unwrap();
        let enc_1 = work_dir.path().join("enc_a.pdf");
        let enc_2 = work_dir.path().join("enc_b.pdf");
        std::fs::write(&enc_1, &locked_bytes).unwrap();
        std::fs::write(&enc_2, &locked_bytes).unwrap();
        let marker_a = write_marker_pdf(work_dir.path(), "marker_a.pdf", 64, 48);
        let marker_b = write_marker_pdf(work_dir.path(), "marker_b.pdf", 128, 48);

        let merge_req = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        )
        .unwrap();

        let store = Arc::new(Mutex::new(Vec::new()));
        let is_busy = AtomicBool::new(false);
        let mut next_id = 1;
        {
            let mut guard = store.lock().unwrap();
            // Original recorded order: [enc_a (3p), marker_a (1p, 64x48), enc_b (3p), marker_b (1p, 128x48)]
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![enc_1, marker_a, enc_2, marker_b],
                &merge_req,
            );
            for j in guard.iter_mut() {
                j.status = JobStatus::NeedsPassword;
                j.last_batch_member_ids = vec![1, 2, 3, 4];
            }
            // Reverse UI store to [4, 3, 2, 1]
            guard.reverse();
        }

        let claim = try_claim_password_unlock(&store, &is_busy, 3, None)
            .unwrap()
            .unwrap();
        let out_path = complete_password_unlock_claim(&store, claim, "user-pw-123", None).unwrap();
        is_busy.store(false, Ordering::SeqCst);

        let merged_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(pdf::probe_pdf(&merged_bytes).unwrap().page_count, 8);
        // Page 4 must be marker_a (64x48) and page 8 must be marker_b (128x48)
        assert_eq!(page_embedded_image_dims(&merged_bytes, 4), (64, 48));
        assert_eq!(page_embedded_image_dims(&merged_bytes, 8), (128, 48));

        let guard = store.lock().unwrap();
        assert!(guard.iter().all(|j| j.status == JobStatus::Done));
        assert!(guard.iter().all(|j| j.output_path == out_path));
    }

    #[test]
    fn password_unlock_merge_pdf_batch_rejects_incompatible_and_missing_members() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let req_a = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(dir_a.path().to_path_buf()),
        )
        .unwrap();
        let req_b = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(dir_b.path().to_path_buf()),
        )
        .unwrap();

        // 1. Incompatible batch member request fails cleanly, marks BOTH batch members Failed
        //    (neither stays stuck in NeedsPassword), and leaves unrelated jobs untouched.
        let store_incompat = Arc::new(Mutex::new(Vec::new()));
        let is_busy = AtomicBool::new(false);
        let mut next_id = 1;
        let unrelated_req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::NextToSource,
        )
        .unwrap();
        {
            let mut guard = store_incompat.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("locked.pdf")],
                &req_a,
            );
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("locked.pdf")],
                &req_b,
            );
            enqueue_files(
                &mut guard,
                &mut next_id,
                vec![fixture_path("locked.pdf")],
                &unrelated_req,
            );
            for j in &mut guard[..2] {
                j.status = JobStatus::NeedsPassword;
                j.last_batch_member_ids = vec![1, 2];
            }
            guard[2].status = JobStatus::NeedsPassword;
        }

        let err = try_claim_password_unlock(&store_incompat, &is_busy, 1, None)
            .unwrap()
            .err()
            .expect("expected incompatible batch error");
        assert_eq!(err.code, ErrorCode::InvalidOption);
        assert!(!is_busy.load(Ordering::SeqCst));
        {
            let guard = store_incompat.lock().unwrap();
            assert_eq!(guard[0].status, JobStatus::Failed);
            assert!(guard[0].error.contains("incompatible"));
            assert_eq!(guard[1].status, JobStatus::Failed);
            assert!(guard[1].error.contains("incompatible"));
            // Unrelated job 3 must stay untouched in NeedsPassword
            assert_eq!(guard[2].status, JobStatus::NeedsPassword);
            assert!(guard[2].error.is_empty());
        }

        // 2. Missing batch member fails cleanly, updates remaining member, and writes no file
        let store_missing = Arc::new(Mutex::new(Vec::new()));
        let mut next_id_2 = 1;
        {
            let mut guard = store_missing.lock().unwrap();
            enqueue_files(
                &mut guard,
                &mut next_id_2,
                vec![fixture_path("locked.pdf"), fixture_path("locked.pdf")],
                &req_a,
            );
            for j in guard.iter_mut() {
                j.status = JobStatus::NeedsPassword;
                j.last_batch_member_ids = vec![1, 2];
            }
            guard.retain(|j| j.id != 2);
        }

        let missing_err = try_claim_password_unlock(&store_missing, &is_busy, 1, None)
            .unwrap()
            .err()
            .expect("expected missing member error");
        assert_eq!(missing_err.code, ErrorCode::InvalidOption);
        assert!(!is_busy.load(Ordering::SeqCst));
        {
            let guard = store_missing.lock().unwrap();
            assert_eq!(guard[0].status, JobStatus::Failed);
            assert!(
                guard[0].error.contains("missing") || guard[0].error.contains("removed"),
                "unexpected error: {}",
                guard[0].error
            );
        }
        assert!(std::fs::read_dir(dir_a.path()).unwrap().next().is_none());

        // 3. Password unlock refuses targets not in NeedsPassword without mutating their status
        for non_unlock_status in [
            JobStatus::Waiting,
            JobStatus::Working,
            JobStatus::Done,
            JobStatus::Failed,
        ] {
            let store_state = Arc::new(Mutex::new(Vec::new()));
            let mut next_id_3 = 1;
            {
                let mut guard = store_state.lock().unwrap();
                enqueue_files(
                    &mut guard,
                    &mut next_id_3,
                    vec![fixture_path("locked.pdf")],
                    &unrelated_req,
                );
                guard[0].status = non_unlock_status.clone();
            }
            let status_err = try_claim_password_unlock(&store_state, &is_busy, 1, None)
                .unwrap()
                .err()
                .expect("expected invalid unlock state error");
            assert_eq!(status_err.code, ErrorCode::InvalidOption);
            assert!(!is_busy.load(Ordering::SeqCst));
            assert_eq!(store_state.lock().unwrap()[0].status, non_unlock_status);
        }
    }
}
