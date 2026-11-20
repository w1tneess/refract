slint::include_modules!();

mod pipeline;

use pipeline::preflight::{preflight_inputs_with_passwords, PdfPasswords};
use pipeline::runner::{self, ActiveRun, RunCtx};
use pipeline::secrets::{self, JobSecretStore};
use pipeline::verify::{self, Expect};
use pipeline::{
    plan_execution_units, resolve_recorded_batch_indices, select_batch_retry_indices,
    DesktopToolOptions, JobRequest, PlannedUnit, TrimEnd, UiOptionInput,
};
use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::format::Format;
use refract_core::job::JobState;
use refract_core::naming;
use refract_core::tools::ToolId;
use refract_engine::pdf::SecretPassword;
use refract_engine::{audio, image, pdf, persist, render};
use slint::{ComponentHandle, Model, SharedString, VecModel};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
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
    /// This job's own file is the one a merge batch (or single job) is waiting on a password for.
    awaiting_password: bool,
    /// The canonical 9-state job state; change it only through `set_state` / `start_new_attempt`.
    status: JobState,
    error: String,
    output_path: PathBuf,
    progress: f32,
}

/// Move `job` to `next`, enforcing `JobState::can_transition` (the 9-state table shared with Web).
/// An illegal move is a bug: it panics in debug and test builds, and is ignored in release builds.
fn set_state(job: &mut JobRecord, next: JobState) -> bool {
    let legal = job.status.can_transition(next);
    debug_assert!(
        legal,
        "illegal job state transition {:?} -> {:?} (job {})",
        job.status, next, job.id
    );
    if legal {
        job.status = next;
    }
    legal
}

/// Start a fresh attempt (retry or re-run): terminal states have no outgoing transitions, so a new
/// attempt begins again at `Queued` with the same immutable request, like Web's retry.
fn start_new_attempt(job: &mut JobRecord) {
    debug_assert!(
        matches!(job.status, JobState::Failed | JobState::Canceled),
        "only Failed or Canceled jobs start a new attempt (job {} is {:?})",
        job.id,
        job.status
    );
    job.status = JobState::Queued;
    job.error.clear();
    job.output_path = PathBuf::new();
    job.awaiting_password = false;
    job.progress = 0.0;
}

/// Final state of a run from the furthest stage it reached and its result, per the state table:
/// errors found while checking inputs reject the job; errors after work began fail it.
fn final_state(reached: JobState, result: &Result<PathBuf, RefractError>) -> JobState {
    match result {
        Ok(_) => JobState::Saved,
        Err(e) if e.code.is_cancellation() => JobState::Canceled,
        Err(e) if reached == JobState::Probing && e.code.is_password_error() => {
            JobState::NeedsPassword
        }
        Err(_) if reached == JobState::Probing => JobState::Rejected,
        Err(_) => JobState::Failed,
    }
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
    // Passwords for locked PDFs live here, never in `JobRecord` (which is cloned into UI updates).
    let secret_store = Arc::new(Mutex::new(JobSecretStore::new()));
    let next_id = Arc::new(Mutex::new(1i32));
    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>();
    let shared = Shared {
        jobs: Arc::clone(&jobs_store),
        secrets: Arc::clone(&secret_store),
        active: Arc::new(Mutex::new(ActiveRun::default())),
    };

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
    // One worker runs every job command (Process all, retry, unlock) in order.
    let worker = spawn_worker(shared.clone(), main_window.as_weak(), ui_tx.clone());
    setup_clear_jobs(&main_window, &shared, &ui_tx);
    setup_open_output_folder(&main_window);
    setup_run_controls(&main_window, &shared, &worker, &ui_tx);
    setup_remove_job(&main_window, &shared, &ui_tx);
    setup_move_job(&main_window, &shared, &ui_tx);
    setup_toggle_theme(&main_window);
    setup_tool_changed(&main_window, &tool_specs);
    setup_password_handlers(&main_window, &worker);

    main_window.run()
}

#[allow(dead_code)]
enum UiEvent {
    JobsUpdated(Vec<JobRecord>),
    StatusUpdate(String, f32),
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
            JobState::Queued => "Waiting".into(),
            JobState::Probing | JobState::Running | JobState::Verifying => "Working…".into(),
            JobState::Saved => "Done".into(),
            JobState::Failed | JobState::Rejected => "Failed".into(),
            JobState::NeedsPassword => "Needs Password".into(),
            JobState::Canceled => "Canceled".into(),
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
            awaiting_password: false,
            status: JobState::Queued,
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

/// Clear the queue, keeping jobs that are running (cancel them first to remove them).
fn clear_jobs_guarded(shared: &Shared) -> usize {
    let mut store = shared.jobs.lock().unwrap();
    let active = shared.active.lock().unwrap();
    store.retain(|j| !runner::can_remove_or_move(j.status) || active.is_running(j.id));
    drop(active);
    prune_secrets(&store, &shared.secrets);
    store.len()
}

fn setup_clear_jobs(window: &MainWindow, shared: &Shared, ui_tx: &mpsc::Sender<UiEvent>) {
    let shared = shared.clone();
    let ui_tx = ui_tx.clone();
    window.on_clear_jobs(move || {
        let kept = clear_jobs_guarded(&shared);
        push_ui_event(
            &ui_tx,
            UiEvent::JobsUpdated(shared.jobs.lock().unwrap().clone()),
        );
        let text = if kept == 0 {
            "Ready".to_string()
        } else {
            format!("Kept {kept} running job(s); cancel them to remove them.")
        };
        push_ui_event(&ui_tx, UiEvent::StatusUpdate(text, -1.0));
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

impl PasswordUnlockClaim {
    fn ids(&self) -> Vec<i32> {
        match self {
            PasswordUnlockClaim::Single(job) => vec![job.id],
            PasswordUnlockClaim::MergeBatch { members, .. } => {
                members.iter().map(|j| j.id).collect()
            }
        }
    }
}

/// The recorded batch of `target_id` can't run (a member is missing, moved on or incompatible).
/// Waiting members are rejected through the legal path (`Probing -> Rejected`); members that
/// already ended keep their state and just show the reason.
fn reject_recorded_batch(
    store: &mut [JobRecord],
    target_id: i32,
    recorded_ids: &[i32],
    err: &RefractError,
) {
    for j in store.iter_mut() {
        let belongs_to_same_recorded_batch = !recorded_ids.is_empty()
            && recorded_ids.contains(&j.id)
            && j.last_batch_member_ids == recorded_ids;
        if j.id != target_id && !belongs_to_same_recorded_batch {
            continue;
        }
        if matches!(j.status, JobState::Queued | JobState::NeedsPassword) {
            set_state(j, JobState::Probing);
            set_state(j, JobState::Rejected);
        }
        if !j.status.is_active() && j.status != JobState::Saved {
            j.error = err.to_string();
            j.awaiting_password = false;
            j.progress = -1.0;
        }
    }
}

/// Mark `ids` as starting a run: `Queued`/`NeedsPassword` -> `Probing`.
fn begin_probing(store: &mut [JobRecord], ids: &[i32]) {
    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
        set_state(j, JobState::Probing);
        j.progress = 0.0;
        j.error.clear();
    }
}

/// Run one unit of work for `ids` (already in `Probing`). Stages reported by the work (`Running`,
/// `Verifying`) are applied to the jobs as they happen; the final state comes from the furthest
/// stage reached plus the result (see `final_state`).
fn run_unit(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    ids: &[i32],
    cancel: &CancelToken,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
    work: impl FnOnce(&RunCtx<'_>) -> Result<PathBuf, RefractError>,
) -> Result<PathBuf, RefractError> {
    let on_stage = |stage: JobState| {
        let mut store = jobs_store.lock().unwrap();
        for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
            set_state(j, stage);
        }
        if let Some(tx) = ui_tx {
            push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
        }
    };
    let ctx = RunCtx::new(cancel, &on_stage);
    let result = work(&ctx);
    let mut store = jobs_store.lock().unwrap();
    apply_result_to_jobs(&mut store, ids, &result);
    if let Some(tx) = ui_tx {
        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
    }
    result
}

/// Claim a password-waiting job (or its whole recorded merge batch) for an unlock attempt,
/// moving it to `Probing`. Runs on the single worker, so two unlocks of the same job can't race:
/// the second finds the job no longer in `NeedsPassword` and is refused.
fn try_claim_password_unlock(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    job_id: i32,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) -> Option<Result<PasswordUnlockClaim, RefractError>> {
    let outcome = {
        let mut store = jobs_store.lock().unwrap();
        let target_idx = store.iter().position(|j| j.id == job_id)?;
        let target = store[target_idx].clone();

        if target.status != JobState::NeedsPassword {
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
                |j| j.status == JobState::NeedsPassword,
            ) {
                Ok(indices) => {
                    let mut members = Vec::with_capacity(indices.len());
                    for idx in indices {
                        let id = store[idx].id;
                        begin_probing(&mut store, &[id]);
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
                    reject_recorded_batch(&mut store, job_id, &target.last_batch_member_ids, &err);
                    if let Some(tx) = ui_tx {
                        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
                    }
                    Err(err)
                }
            }
        } else {
            begin_probing(&mut store, &[job_id]);
            let claimed = store[target_idx].clone();
            if let Some(tx) = ui_tx {
                push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
            }
            Ok(PasswordUnlockClaim::Single(claimed))
        }
    };

    Some(outcome)
}

/// Run an unlock with the password the user just entered.
///
/// Single jobs use the password once and drop it (zeroing it). For merge-pdf, the password is
/// stored for the member whose file is awaiting one, then every stored password for the batch is
/// taken out of the store for the attempt (no lock is held while the engine runs). Each member
/// uses its own password; files without one try the others, newest first. Afterwards a password
/// is put back only if its job still awaits one (a wrong one is dropped), via `prune_secrets`.
fn complete_password_unlock_claim(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    secret_store: &Mutex<JobSecretStore>,
    claim: PasswordUnlockClaim,
    entered: SecretPassword,
    cancel: &CancelToken,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) -> Result<PathBuf, RefractError> {
    let (ids, result, taken) = match claim {
        PasswordUnlockClaim::MergeBatch { request, members } => {
            let ids: Vec<i32> = members.iter().map(|j| j.id).collect();
            let key = members
                .iter()
                .find(|j| j.awaiting_password)
                .unwrap_or(&members[0])
                .id;
            let taken = {
                let mut secrets = secret_store.lock().unwrap();
                secrets.insert(key, entered);
                secrets.take(&ids)
            };
            let per_input: Vec<Option<&str>> = taken
                .iter()
                .map(|e| e.as_ref().map(|(_, pw)| pw.as_str()))
                .collect();
            let guesses: Vec<&str> = secrets::newest_first(&taken)
                .into_iter()
                .filter_map(|i| per_input[i])
                .collect();
            let passwords = PdfPasswords {
                all: None,
                per_input: &per_input,
                guesses: &guesses,
            };
            let res = run_unit(jobs_store, &ids, cancel, ui_tx, |ctx| {
                execute_batch_merge_pdf_with_passwords(&members, &request, passwords, ctx)
            });
            drop(guesses);
            drop(per_input);
            (ids, res, taken)
        }
        PasswordUnlockClaim::Single(job) => {
            let ids = vec![job.id];
            let res = run_unit(jobs_store, &ids, cancel, ui_tx, |ctx| {
                process_job_with_password_in(&job, &entered, ctx)
            });
            drop(entered);
            (ids, res, Vec::new())
        }
    };

    let store = jobs_store.lock().unwrap();
    {
        // Lock order everywhere: jobs, then secrets.
        let wrong_index = match &result {
            Err(e) if e.code == ErrorCode::WrongPassword => pdf::input_index_of(e),
            _ => None,
        };
        let mut secrets = secret_store.lock().unwrap();
        for (i, (id, entry)) in ids.iter().zip(taken).enumerate() {
            if let Some(entry) = entry {
                if Some(i) != wrong_index {
                    secrets.restore(*id, entry);
                }
            }
        }
    }
    prune_secrets(&store, secret_store);

    if let Some(tx) = ui_tx {
        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
    }
    result
}

/// Work for the single job worker, run one command at a time.
enum Command {
    /// "Process all": every waiting job, in queue order, grouped where the tool combines files.
    ProcessAll,
    /// A retry prepared on the UI thread: these jobs were moved to a fresh `Queued` attempt.
    Retry(Vec<i32>),
    /// The password the user entered for a job (zeroed when the command is dropped).
    Unlock {
        job_id: i32,
        password: SecretPassword,
    },
}

/// State shared by the UI handlers and the worker.
#[derive(Clone)]
struct Shared {
    jobs: Arc<Mutex<Vec<JobRecord>>>,
    secrets: Arc<Mutex<JobSecretStore>>,
    active: Arc<Mutex<ActiveRun>>,
}

impl Shared {
    #[cfg(test)]
    fn for_store(jobs: &Arc<Mutex<Vec<JobRecord>>>) -> Self {
        Self {
            jobs: Arc::clone(jobs),
            secrets: Arc::new(Mutex::new(JobSecretStore::new())),
            active: Arc::new(Mutex::new(ActiveRun::default())),
        }
    }
}

fn set_running(window: &slint::Weak<MainWindow>, running: bool) {
    let window = window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = window.upgrade() {
            w.set_is_running(running);
        }
    });
}

/// Start the single worker thread. Every run (Process all, retry, unlock) goes through it, so runs
/// never overlap, and each unit's cancel token is registered before its jobs start.
fn spawn_worker(
    shared: Shared,
    window: slint::Weak<MainWindow>,
    ui_tx: mpsc::Sender<UiEvent>,
) -> mpsc::Sender<Command> {
    let (tx, rx) = mpsc::channel::<Command>();
    thread::spawn(move || {
        while let Ok(command) = rx.recv() {
            set_running(&window, true);
            execute_command(&shared, command, Some(&ui_tx));
            set_running(&window, false);
        }
    });
    tx
}

fn execute_command(shared: &Shared, command: Command, ui_tx: Option<&mpsc::Sender<UiEvent>>) {
    match command {
        Command::ProcessAll => run_all_waiting(shared, ui_tx),
        Command::Retry(ids) => run_retry(shared, &ids, ui_tx),
        Command::Unlock { job_id, password } => run_unlock(shared, job_id, password, ui_tx),
    }
}

fn status(ui_tx: Option<&mpsc::Sender<UiEvent>>, text: impl Into<String>, progress: f32) {
    if let Some(tx) = ui_tx {
        push_ui_event(tx, UiEvent::StatusUpdate(text.into(), progress));
    }
}

/// Register `ids` as the running unit, run `work` with its cancel token, then unregister.
fn run_registered(shared: &Shared, ids: &[i32], work: impl FnOnce(&CancelToken)) {
    let token = shared.active.lock().unwrap().start(ids);
    work(&token);
    shared.active.lock().unwrap().finish(ids);
}

/// Fresh copies of `ids` that still exist and are still `Queued` (in `ids` order). Jobs canceled
/// or removed after a run was planned are left out.
fn still_queued(shared: &Shared, ids: &[i32]) -> Vec<JobRecord> {
    let store = shared.jobs.lock().unwrap();
    ids.iter()
        .filter_map(|id| {
            store
                .iter()
                .find(|j| j.id == *id && j.status == JobState::Queued)
                .cloned()
        })
        .collect()
}

fn run_all_waiting(shared: &Shared, ui_tx: Option<&mpsc::Sender<UiEvent>>) {
    let waiting: Vec<JobRecord> = {
        let store = shared.jobs.lock().unwrap();
        store
            .iter()
            .filter(|j| j.status == JobState::Queued)
            .cloned()
            .collect()
    };
    if waiting.is_empty() {
        status(ui_tx, "No pending jobs", -1.0);
        return;
    }
    status(ui_tx, "Starting…", 0.0);

    let planned_units = plan_execution_units(waiting, |j| &j.request);
    let total_units = planned_units.len();

    for (completed, unit) in planned_units.into_iter().enumerate() {
        let progress = completed as f32 / total_units as f32;
        match unit {
            PlannedUnit::Batch {
                tool: tool @ (ToolId::MergePdf | ToolId::ImagesToPdf),
                request,
                items,
            } => {
                let ids: Vec<i32> = items.iter().map(|j| j.id).collect();
                let members = still_queued(shared, &ids);
                if members.is_empty() {
                    continue;
                }
                let ids: Vec<i32> = members.iter().map(|j| j.id).collect();
                let message = if tool == ToolId::MergePdf {
                    format!("Merging {} PDFs…", members.len())
                } else {
                    format!("Compiling {} images into PDF…", members.len())
                };
                run_registered(shared, &ids, |cancel| {
                    {
                        let mut store = shared.jobs.lock().unwrap();
                        for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
                            j.last_batch_member_ids = ids.clone();
                        }
                        begin_probing(&mut store, &ids);
                        if let Some(tx) = ui_tx {
                            push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
                        }
                    }
                    status(ui_tx, message, progress);
                    let _ = run_unit(&shared.jobs, &ids, cancel, ui_tx, |ctx| {
                        if tool == ToolId::MergePdf {
                            execute_batch_merge_pdf_in(&members, &request, ctx)
                        } else {
                            execute_batch_images_to_pdf_in(&members, &request, ctx)
                        }
                    });
                });
            }
            PlannedUnit::Batch { items, .. } => {
                for job in items {
                    run_single(shared, job.id, completed, total_units, ui_tx);
                }
            }
            PlannedUnit::Single(job) => run_single(shared, job.id, completed, total_units, ui_tx),
        }
        status(
            ui_tx,
            format!("Completed {} of {}", completed + 1, total_units),
            (completed + 1) as f32 / total_units as f32,
        );
    }
    status(ui_tx, "All done", 1.0);
}

fn run_single(
    shared: &Shared,
    id: i32,
    completed: usize,
    total_units: usize,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) {
    let Some(job) = still_queued(shared, &[id]).pop() else {
        return;
    };
    run_registered(shared, &[id], |cancel| {
        run_single_job_in_store(&shared.jobs, &job, completed, total_units, cancel, ui_tx);
    });
}

fn run_single_job_in_store(
    jobs_store: &Arc<Mutex<Vec<JobRecord>>>,
    job: &JobRecord,
    completed: usize,
    total_units: usize,
    cancel: &CancelToken,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) {
    {
        let mut store = jobs_store.lock().unwrap();
        begin_probing(&mut store, &[job.id]);
        if let Some(tx) = ui_tx {
            push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
        }
    }
    status(
        ui_tx,
        format!(
            "Processing {} of {}: {}",
            completed + 1,
            total_units,
            job.name
        ),
        completed as f32 / total_units as f32,
    );
    let _ = run_unit(jobs_store, &[job.id], cancel, ui_tx, |ctx| {
        process_job_in(job, ctx)
    });
}

/// Prepare a retry on the UI thread: the job (and, for merge/images-to-pdf, its recorded batch)
/// starts a fresh `Queued` attempt with its original immutable request. Only `Failed` and
/// `Canceled` jobs can be retried (`can_retry`); returns the ids for `Command::Retry`.
fn prepare_retry(store: &mut [JobRecord], job_id: i32) -> Result<Vec<i32>, RefractError> {
    let target = store
        .iter()
        .find(|j| j.id == job_id)
        .cloned()
        .ok_or_else(|| RefractError::new(ErrorCode::InvalidOption, "That job no longer exists."))?;
    if !runner::can_retry(target.status) {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            format!(
                "'{}' can't be retried ({}).",
                target.name,
                target.status.label()
            ),
        ));
    }
    let recorded_ids = target.last_batch_member_ids.clone();
    let indices = match select_batch_retry_indices(
        store,
        job_id,
        &recorded_ids,
        |j| j.id,
        |j| &j.request,
        |j| runner::can_retry(j.status),
    ) {
        Ok(indices) => indices,
        Err(err) => {
            reject_recorded_batch(store, job_id, &recorded_ids, &err);
            return Err(err);
        }
    };
    let ids: Vec<i32> = indices.iter().map(|&i| store[i].id).collect();
    let batch = target.request.tool().spec().batch;
    for &i in &indices {
        start_new_attempt(&mut store[i]);
        if batch {
            store[i].last_batch_member_ids = ids.clone();
        }
    }
    Ok(ids)
}

fn run_retry(shared: &Shared, ids: &[i32], ui_tx: Option<&mpsc::Sender<UiEvent>>) {
    let members = still_queued(shared, ids);
    let Some(primary) = members.first().cloned() else {
        return;
    };
    let request = primary.request.clone();
    let member_ids: Vec<i32> = members.iter().map(|j| j.id).collect();

    // A merge/images-to-pdf retry runs exactly its recorded files or not at all.
    if request.tool().spec().batch && member_ids.len() != ids.len() {
        let err = RefractError::new(
            ErrorCode::InvalidOption,
            "Some files of this batch were canceled or removed before it could run.",
        );
        let mut store = shared.jobs.lock().unwrap();
        begin_probing(&mut store, &member_ids);
        apply_result_to_jobs(&mut store, &member_ids, &Err(err));
        if let Some(tx) = ui_tx {
            push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
        }
        return;
    }

    run_registered(shared, &member_ids, |cancel| {
        {
            let mut store = shared.jobs.lock().unwrap();
            begin_probing(&mut store, &member_ids);
            if let Some(tx) = ui_tx {
                push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
            }
        }
        status(ui_tx, format!("Retrying: {}", primary.name), 0.0);
        let _ = run_unit(
            &shared.jobs,
            &member_ids,
            cancel,
            ui_tx,
            |ctx| match request.tool() {
                ToolId::MergePdf => execute_batch_merge_pdf_in(&members, &request, ctx),
                ToolId::ImagesToPdf => execute_batch_images_to_pdf_in(&members, &request, ctx),
                _ => process_job_in(&primary, ctx),
            },
        );
    });
    status(ui_tx, "Ready", -1.0);
}

/// Test helper: prepare and run a retry in one step, as clicking Retry then the worker would.
#[cfg(test)]
fn execute_retry_in_store(jobs_store: &Arc<Mutex<Vec<JobRecord>>>, job_id: i32) {
    let shared = Shared::for_store(jobs_store);
    let prepared = prepare_retry(&mut jobs_store.lock().unwrap(), job_id);
    if let Ok(ids) = prepared {
        execute_command(&shared, Command::Retry(ids), None);
    }
}

fn run_unlock(
    shared: &Shared,
    job_id: i32,
    password: SecretPassword,
    ui_tx: Option<&mpsc::Sender<UiEvent>>,
) {
    let claim = match try_claim_password_unlock(&shared.jobs, job_id, ui_tx) {
        Some(Ok(claim)) => claim,
        Some(Err(err)) => {
            let store = shared.jobs.lock().unwrap();
            prune_secrets(&store, &shared.secrets);
            status(ui_tx, err.to_string(), -1.0);
            return;
        }
        None => return,
    };
    let ids = claim.ids();
    status(ui_tx, "Unlocking PDF…", 0.0);
    // The claim already moved the jobs to Probing; a cancel in that moment is kept by
    // `ActiveRun` and applied as soon as the unit registers here.
    run_registered(shared, &ids, |cancel| {
        // Uses the job's original immutable `JobRequest` (and recorded batch members for
        // `merge-pdf`) — never recaptures UI options.
        let _ = complete_password_unlock_claim(
            &shared.jobs,
            &shared.secrets,
            claim,
            password,
            cancel,
            ui_tx,
        );
    });
    status(ui_tx, "Ready", -1.0);
}

/// Cancel a job. Waiting and password-waiting jobs (with their recorded batch) end `Canceled` at
/// once; a running job's unit is stopped through its cancel token and ends `Canceled`, leaving no
/// output. Passwords of canceled jobs are dropped.
fn cancel_job(shared: &Shared, job_id: i32, ui_tx: Option<&mpsc::Sender<UiEvent>>) {
    let mut store = shared.jobs.lock().unwrap();
    let Some(job) = store.iter().find(|j| j.id == job_id).cloned() else {
        return;
    };
    if !runner::can_cancel(job.status) {
        return;
    }
    match job.status {
        JobState::Queued | JobState::NeedsPassword => {
            let recorded = &job.last_batch_member_ids;
            let in_same_batch = |j: &JobRecord| {
                j.id == job_id
                    || (job.request.tool().spec().batch
                        && !recorded.is_empty()
                        && &j.last_batch_member_ids == recorded
                        && matches!(j.status, JobState::Queued | JobState::NeedsPassword))
            };
            for j in store.iter_mut().filter(|j| in_same_batch(j)) {
                set_state(j, JobState::Canceled);
                j.error = "Canceled.".into();
                j.awaiting_password = false;
                j.progress = -1.0;
            }
            prune_secrets(&store, &shared.secrets);
        }
        _ => shared.active.lock().unwrap().cancel(job_id),
    }
    if let Some(tx) = ui_tx {
        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
    }
}

/// Stop the running unit and cancel every waiting job. Password-waiting jobs are left alone.
fn cancel_all(shared: &Shared, ui_tx: Option<&mpsc::Sender<UiEvent>>) {
    let mut store = shared.jobs.lock().unwrap();
    for j in store.iter_mut().filter(|j| j.status == JobState::Queued) {
        set_state(j, JobState::Canceled);
        j.error = "Canceled.".into();
        j.progress = -1.0;
    }
    shared.active.lock().unwrap().cancel_current();
    if let Some(tx) = ui_tx {
        push_ui_event(tx, UiEvent::JobsUpdated(store.clone()));
    }
}

fn setup_run_controls(
    window: &MainWindow,
    shared: &Shared,
    worker: &mpsc::Sender<Command>,
    ui_tx: &mpsc::Sender<UiEvent>,
) {
    let tx = worker.clone();
    window.on_start_processing(move || {
        tx.send(Command::ProcessAll).ok();
    });

    let tx = worker.clone();
    let retry_shared = shared.clone();
    let retry_ui = ui_tx.clone();
    window.on_retry_job(move |job_id| {
        let mut store = retry_shared.jobs.lock().unwrap();
        match prepare_retry(&mut store, job_id) {
            Ok(ids) => {
                push_ui_event(&retry_ui, UiEvent::JobsUpdated(store.clone()));
                tx.send(Command::Retry(ids)).ok();
            }
            Err(err) => {
                push_ui_event(&retry_ui, UiEvent::JobsUpdated(store.clone()));
                push_ui_event(&retry_ui, UiEvent::StatusUpdate(err.to_string(), -1.0));
            }
        }
    });

    let cancel_shared = shared.clone();
    let cancel_ui = ui_tx.clone();
    window.on_cancel_job(move |job_id| cancel_job(&cancel_shared, job_id, Some(&cancel_ui)));

    let cancel_all_shared = shared.clone();
    let cancel_all_ui = ui_tx.clone();
    window.on_cancel_all(move || cancel_all(&cancel_all_shared, Some(&cancel_all_ui)));
}

fn setup_password_handlers(window: &MainWindow, worker: &mpsc::Sender<Command>) {
    let window_weak_show = window.as_weak();
    window.on_show_password_dialog_cb(move |job_id, prompt| {
        if let Some(window) = window_weak_show.upgrade() {
            window.set_password_job_id(job_id);
            window.set_password_prompt(prompt);
            window.set_password_input("".into());
            window.set_show_password_dialog(true);
        }
    });

    let tx = worker.clone();
    window.on_password_entered(move |job_id, password| {
        // Copy out of the UI string at once; from here on it is only a zeroed-on-drop secret.
        // The UI toolkit's own copy of the typed text is outside Refract's control.
        let password_secret = SecretPassword::new(password.as_str());
        drop(password);
        tx.send(Command::Unlock {
            job_id,
            password: password_secret,
        })
        .ok();
    });
}

/// Record a run's result on the jobs it covered. The final state comes from the furthest stage
/// each job reached plus the result (`final_state`), through the checked `set_state`.
/// `ids` are in input order, so a password error's `input_index` names the member whose file is
/// locked: every member then waits for a password with the same prompt (naming that file), and
/// only that member is marked `awaiting_password`. This is the only place results are applied.
fn apply_result_to_jobs(
    store: &mut [JobRecord],
    ids: &[i32],
    result: &Result<PathBuf, RefractError>,
) {
    let locked_id = match result {
        Err(e) if e.code.is_password_error() => {
            let index = pdf::input_index_of(e).unwrap_or(0);
            ids.get(index).or(ids.first()).copied()
        }
        _ => None,
    };
    for j in store.iter_mut().filter(|j| ids.contains(&j.id)) {
        let next = final_state(j.status, result);
        set_state(j, next);
        j.awaiting_password = false;
        match result {
            Ok(path) => {
                j.output_path = path.clone();
                j.error.clear();
            }
            Err(e) if next == JobState::NeedsPassword => {
                j.error = e.message.clone();
                j.awaiting_password = Some(j.id) == locked_id;
            }
            Err(e) => j.error = e.to_string(),
        }
        j.progress = -1.0;
    }
}

/// The secret store's lifetime rule: keep a password only while its job still awaits one.
fn prune_secrets(store: &[JobRecord], secrets: &Mutex<JobSecretStore>) {
    secrets.lock().unwrap().retain_jobs(|id| {
        store
            .iter()
            .any(|j| j.id == id && j.status == JobState::NeedsPassword)
    });
}

/// Remove a job unless it is running. Returns false (and changes nothing) for a running job.
fn remove_job_guarded(shared: &Shared, job_id: i32) -> bool {
    let mut store = shared.jobs.lock().unwrap();
    let running = store
        .iter()
        .any(|j| j.id == job_id && !runner::can_remove_or_move(j.status))
        || shared.active.lock().unwrap().is_running(job_id);
    if running {
        return false;
    }
    store.retain(|j| j.id != job_id);
    prune_secrets(&store, &shared.secrets);
    true
}

fn setup_remove_job(window: &MainWindow, shared: &Shared, ui_tx: &mpsc::Sender<UiEvent>) {
    let shared = shared.clone();
    let ui_tx = ui_tx.clone();
    window.on_remove_job(move |job_id| {
        if !remove_job_guarded(&shared, job_id) {
            push_ui_event(
                &ui_tx,
                UiEvent::StatusUpdate("Cancel this job before removing it.".into(), -1.0),
            );
            return;
        }
        push_ui_event(
            &ui_tx,
            UiEvent::JobsUpdated(shared.jobs.lock().unwrap().clone()),
        );
    });
}

/// Move a job one place up or down, unless it or its neighbour is running.
fn move_job_guarded(store: &mut [JobRecord], job_id: i32, up: bool) -> bool {
    let Some(idx) = store.iter().position(|j| j.id == job_id) else {
        return false;
    };
    let other = if up {
        idx.checked_sub(1)
    } else {
        Some(idx + 1).filter(|&o| o < store.len())
    };
    match other {
        Some(o)
            if runner::can_remove_or_move(store[idx].status)
                && runner::can_remove_or_move(store[o].status) =>
        {
            store.swap(idx, o);
            true
        }
        _ => false,
    }
}

fn setup_move_job(window: &MainWindow, shared: &Shared, ui_tx: &mpsc::Sender<UiEvent>) {
    for up in [true, false] {
        let shared = shared.clone();
        let ui_tx = ui_tx.clone();
        let handler = move |job_id: i32| {
            let mut store = shared.jobs.lock().unwrap();
            move_job_guarded(&mut store, job_id, up);
            push_ui_event(&ui_tx, UiEvent::JobsUpdated(store.clone()));
        };
        if up {
            window.on_move_job_up(handler);
        } else {
            window.on_move_job_down(handler);
        }
    }
}

fn input_file_name(job: &JobRecord) -> String {
    job.input_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string())
}

/// Canonical manifest output name for `job`'s single output (e.g. `photo (converted).png`).
fn output_file_name(job: &JobRecord, extension: &str) -> String {
    naming::output_name_for_tool(
        job.request.tool(),
        &input_file_name(job),
        extension,
        &mut HashSet::new(),
    )
}

fn output_dir(job: &JobRecord) -> PathBuf {
    job.request
        .destination()
        .resolve_parent(&job.input_path)
        .to_path_buf()
}

/// Check every output against its expectation, then save them all together or none.
/// Names are numbered if taken; nothing is ever overwritten. Returns saved paths in order.
/// This is the only path by which Desktop saves image and PDF outputs. Reports `Verifying` first;
/// a cancel before or during the save leaves no output (the commit rolls back what it moved).
fn save_checked_outputs(
    job: &JobRecord,
    outputs: &[(String, &[u8])],
    expects: &[Expect],
    ctx: &RunCtx<'_>,
) -> Result<Vec<PathBuf>, RefractError> {
    ctx.begin_verify()?;
    let bytes: Vec<&[u8]> = outputs.iter().map(|(_, b)| *b).collect();
    verify::verify_all(&bytes, expects)?;
    let mut batch = persist::StagedOutputBatch::new(&output_dir(job));
    for (name, bytes) in outputs {
        batch.stage(name, bytes)?;
    }
    batch.commit(ctx.cancel)
}

/// Check and save one output under the canonical manifest name.
fn save_single_output(
    job: &JobRecord,
    extension: &str,
    bytes: &[u8],
    expect: Expect,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    let name = output_file_name(job, extension);
    let mut saved = save_checked_outputs(job, &[(name, bytes)], &[expect], ctx)?;
    Ok(saved.remove(0))
}

/// Preflight every input, then report `Running` (or stop if the run was canceled meanwhile).
/// All execution paths go through here, so the stage is reported in exactly one place.
fn preflight_then_work(
    request: &JobRequest,
    paths: &[&Path],
    passwords: PdfPasswords<'_>,
    ctx: &RunCtx<'_>,
) -> Result<pipeline::preflight::PreflightReport, RefractError> {
    let report = preflight_inputs_with_passwords(request, paths, passwords)?;
    ctx.begin_work()?;
    Ok(report)
}

fn single_password(password: Option<&str>) -> PdfPasswords<'_> {
    PdfPasswords {
        all: password,
        ..Default::default()
    }
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

/// Merge `jobs` in order. Each locked file is opened with exactly the password preflight found
/// for it (its own, or another one entered for the batch); open files get no password at all.
fn execute_batch_merge_pdf_with_passwords(
    jobs: &[JobRecord],
    request: &JobRequest,
    passwords: PdfPasswords<'_>,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    ensure_batch_compatible(jobs, request)?;
    let paths: Vec<&Path> = jobs.iter().map(|j| j.input_path.as_path()).collect();
    let report = preflight_then_work(request, &paths, passwords, ctx)?;
    let inputs: Vec<(Vec<u8>, u32, Option<&str>)> = report
        .inputs
        .into_iter()
        .enumerate()
        .map(|(i, input)| {
            let password = passwords.for_source(i, input.unlocked_with);
            pdf_input_bytes_and_pages(input).map(|(bytes, pages)| (bytes, pages, password))
        })
        .collect::<Result<_, _>>()?;
    let total_pages: u32 = inputs.iter().map(|(_, pages, _)| pages).sum();
    let merge_inputs: Vec<pdf::PdfMergeInput<'_>> = inputs
        .iter()
        .map(|(bytes, _, password)| pdf::PdfMergeInput {
            bytes,
            password: *password,
        })
        .collect();
    let out_bytes = pdf::merge_pdfs_with_inputs(&merge_inputs, ctx.cancel)?;
    // Name comes from the first recorded member; destination is shared (checked by ensure_batch_compatible).
    save_single_output(
        &jobs[0],
        "pdf",
        &out_bytes,
        Expect::format(Format::Pdf).pages(total_pages),
        ctx,
    )
}

fn execute_batch_merge_pdf_in(
    jobs: &[JobRecord],
    request: &JobRequest,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    execute_batch_merge_pdf_with_passwords(jobs, request, PdfPasswords::default(), ctx)
}

fn execute_batch_images_to_pdf_in(
    jobs: &[JobRecord],
    request: &JobRequest,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    ensure_batch_compatible(jobs, request)?;
    let DesktopToolOptions::ImagesToPdf { dpi, quality } = *request.options() else {
        return Err(RefractError::new(
            ErrorCode::InvalidOption,
            "Expected ImagesToPdf options for batch compilation.",
        ));
    };
    let paths: Vec<&Path> = jobs.iter().map(|j| j.input_path.as_path()).collect();
    preflight_then_work(request, &paths, PdfPasswords::default(), ctx)?;
    let buffers: Vec<Vec<u8>> = jobs
        .iter()
        .map(|j| std::fs::read(&j.input_path).map_err(to_refract_err))
        .collect::<Result<_, _>>()?;
    let out_bytes =
        pdf::images_to_pdf_with_options(&buffers, pdf::PdfPageSize::Fit, dpi, quality, ctx.cancel)?;
    let pages = u32::try_from(buffers.len()).unwrap_or(u32::MAX);
    save_single_output(
        &jobs[0],
        "pdf",
        &out_bytes,
        Expect::format(Format::Pdf).pages(pages),
        ctx,
    )
}

fn render_or_extract_pdf_images(
    job: &JobRecord,
    bytes: &[u8],
    page_count: u32,
    target: Format,
    dpi: u32,
    password: Option<&str>,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    let format = format_primary_extension(target);
    let input_name = input_file_name(job);
    let stem = naming::stem_of(&input_name);

    // Entry names follow the manifest's pdf-to-images `entryTemplate` (`{name}-p{page}.{ext}`).
    let (outputs, expects): (Vec<(String, Vec<u8>)>, Vec<Expect>) =
        match render::render_pdf_to_images_with_password_cancellable(
            bytes, password, dpi, format, ctx.cancel,
        ) {
            Ok(pages) => {
                let outputs: Vec<(String, Vec<u8>)> = pages
                    .into_iter()
                    .map(|(page_num, page_bytes, ext)| {
                        (format!("{}-p{}.{}", stem, page_num, ext), page_bytes)
                    })
                    .collect();
                // Rendering produces one image per page, in the requested format.
                (outputs, vec![Expect::format(target); page_count as usize])
            }
            Err(render_err) => {
                if render_err.code.is_password_error() || render_err.code.is_cancellation() {
                    return Err(render_err);
                }
                let embedded =
                    pdf::extract_embedded_images_with_password(bytes, password, ctx.cancel)?;
                if embedded.is_empty() {
                    return Err(render_err);
                }
                let expects = vec![Expect::extracted_image(); embedded.len()];
                let outputs = embedded
                    .into_iter()
                    .map(|img| {
                        (
                            format!("{}-p{}.{}", stem, img.page, img.extension),
                            img.bytes,
                        )
                    })
                    .collect();
                (outputs, expects)
            }
        };

    let refs: Vec<(String, &[u8])> = outputs
        .iter()
        .map(|(name, b)| (name.clone(), b.as_slice()))
        .collect();
    let mut saved = save_checked_outputs(job, &refs, &expects, ctx)?;
    Ok(saved.remove(0))
}

/// Check and save every split part together (`{name}-p{range}.pdf`, the manifest
/// `entryTemplate`), or none. Each part must have its range's page count.
fn save_split_parts(
    job: &JobRecord,
    ranges: &str,
    page_count: u32,
    parts: Vec<(String, Vec<u8>)>,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    let expects: Vec<Expect> = pdf::parse_ranges(ranges, page_count)?
        .iter()
        .map(|range| Expect::format(Format::Pdf).pages(range.len() as u32))
        .collect();
    let input_name = input_file_name(job);
    let stem = naming::stem_of(&input_name);
    let refs: Vec<(String, &[u8])> = parts
        .iter()
        .map(|(label, b)| (format!("{}-{}.pdf", stem, label), b.as_slice()))
        .collect();
    let saved = save_checked_outputs(job, &refs, &expects, ctx)?;
    Ok(saved
        .into_iter()
        .last()
        .expect("commit never returns an empty list"))
}

fn pdf_input_bytes_and_pages(
    input: pipeline::preflight::ValidatedInput,
) -> Result<(Vec<u8>, u32), RefractError> {
    match (input.pdf_bytes, input.page_count) {
        (Some(bytes), Some(pages)) => Ok((bytes, pages)),
        _ => Err(RefractError::new(
            ErrorCode::DamagedFile,
            "PDF input bytes were unavailable after preflight.",
        )),
    }
}

fn take_preflight_pdf(
    mut report: pipeline::preflight::PreflightReport,
) -> Result<(Vec<u8>, u32), RefractError> {
    let input = report.inputs.pop().ok_or_else(|| {
        RefractError::new(
            ErrorCode::DamagedFile,
            "PDF input bytes were unavailable after preflight.",
        )
    })?;
    pdf_input_bytes_and_pages(input)
}

fn process_job_in(job: &JobRecord, ctx: &RunCtx<'_>) -> Result<PathBuf, RefractError> {
    match job.request.options() {
        DesktopToolOptions::ImagesToPdf { .. } => {
            return execute_batch_images_to_pdf_in(std::slice::from_ref(job), &job.request, ctx);
        }
        DesktopToolOptions::MergePdf => {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                "Merge PDF needs at least 2 compatible PDF files with the same output destination.",
            ));
        }
        _ => {}
    }

    let report = preflight_then_work(
        &job.request,
        &[job.input_path.as_path()],
        PdfPasswords::default(),
        ctx,
    )?;
    let detected_format = report.inputs[0].format;
    let cancel = ctx.cancel;

    match job.request.options() {
        DesktopToolOptions::ConvertImage { target, quality } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            // Displayed size (EXIF orientation applied), which the converted image keeps.
            let (w, h) = image::probe_dimensions(&bytes)?;
            let out_bytes =
                image::convert_image_cancellable(&bytes, *target, Some(*quality), cancel)?;
            save_single_output(
                job,
                format_primary_extension(*target),
                &out_bytes,
                Expect::format(*target).dimensions(w, h),
                ctx,
            )
        }
        DesktopToolOptions::CompressImage { target_kb } => {
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let out_bytes = image::compress_image_cancellable(&bytes, *target_kb, cancel)?;
            // The engine may scale the image down to reach the target, so only size is fixed.
            let max_bytes = u64::from(*target_kb) * 1024;
            save_single_output(
                job,
                "jpg",
                &out_bytes,
                Expect::format(Format::Jpeg).max_bytes(max_bytes),
                ctx,
            )
        }
        opts @ DesktopToolOptions::ResizeImage { .. } => {
            let engine_opts = opts.as_engine_resize_options().ok_or_else(|| {
                RefractError::new(ErrorCode::InvalidOption, "Invalid resize options.")
            })?;
            let bytes = std::fs::read(&job.input_path).map_err(to_refract_err)?;
            let (src_w, src_h) = image::probe_dimensions(&bytes)?;
            let (w, h) = image::resolve_resize_dimensions(src_w, src_h, &engine_opts)?;
            let (out_bytes, out_fmt) =
                image::resize_image_with_options_cancellable(&bytes, &engine_opts, cancel)?;
            save_single_output(
                job,
                format_primary_extension(out_fmt),
                &out_bytes,
                Expect::format(out_fmt).dimensions(w, h),
                ctx,
            )
        }
        DesktopToolOptions::PdfToImages { target, dpi } => {
            let (bytes, pages) = take_preflight_pdf(report)?;
            render_or_extract_pdf_images(job, &bytes, pages, *target, *dpi, None, ctx)
        }
        DesktopToolOptions::CompressPdf => {
            let (bytes, pages) = take_preflight_pdf(report)?;
            let out_bytes = pdf::compress_pdf_cancellable(&bytes, None, cancel)?;
            save_single_output(
                job,
                "pdf",
                &out_bytes,
                Expect::format(Format::Pdf).pages(pages),
                ctx,
            )
        }
        DesktopToolOptions::SplitPdf { ranges } => {
            let (bytes, pages) = take_preflight_pdf(report)?;
            let parts = pdf::split_pdf_with_password(&bytes, ranges, None, cancel)?;
            save_split_parts(job, ranges, pages, parts, ctx)
        }
        DesktopToolOptions::ReorderPdf { order } => {
            let (bytes, pages) = take_preflight_pdf(report)?;
            let out_bytes = pdf::reorder_pdf_with_password(&bytes, order, None, cancel)?;
            save_single_output(
                job,
                "pdf",
                &out_bytes,
                Expect::format(Format::Pdf).pages(pages),
                ctx,
            )
        }
        DesktopToolOptions::ConvertAudio {
            target,
            bitrate_kbps,
        } => {
            let dir = output_dir(job);
            let name = output_file_name(job, format_primary_extension(*target));
            // convert-audio does not require ffprobe, and FFmpeg may convert inputs ffprobe can't
            // measure; in either case the length check is skipped and only the type checks run.
            let input_secs = if audio::is_ffprobe_available() {
                audio::probe_duration(&job.input_path).ok()
            } else {
                None
            };
            let mut expect = Expect::format(*target);
            if let Some(secs) = input_secs {
                expect = expect.duration(secs);
            }
            let probe: verify::DurationProbe = &audio::probe_duration;
            let check = |out: &Path| {
                ctx.begin_verify()?;
                verify::verify_audio_file(out, &expect, input_secs.is_some().then_some(probe))
            };
            audio::convert_audio_to(
                &job.input_path,
                audio::AudioOutput::Unique {
                    dir: &dir,
                    name: &name,
                },
                *target,
                *bitrate_kbps,
                cancel,
                &check,
            )
        }
        DesktopToolOptions::TrimAudio {
            start_secs,
            end,
            bitrate_kbps,
        } => {
            let dir = output_dir(job);
            let name = output_file_name(job, format_primary_extension(detected_format));
            let start_arg = format!("{:.3}", start_secs);
            let end_arg = end.to_engine_arg();
            // Same range resolution the engine uses, so "end of file" means the same length.
            let range = audio::resolve_trim_range(
                Some(start_arg.as_str()),
                end_arg.as_deref(),
                audio::probe_duration_cancellable(&job.input_path, cancel)?,
            )?;
            let expect = Expect::format(detected_format).duration(range.expected_duration_secs);
            let probe: verify::DurationProbe = &audio::probe_duration;
            let check = |out: &Path| {
                ctx.begin_verify()?;
                verify::verify_audio_file(out, &expect, Some(probe))
            };
            audio::trim_audio_to(
                &job.input_path,
                audio::AudioOutput::Unique {
                    dir: &dir,
                    name: &name,
                },
                Some(start_arg.as_str()),
                end_arg.as_deref(),
                Some(*bitrate_kbps),
                cancel,
                &check,
            )
        }
        DesktopToolOptions::ImagesToPdf { .. } | DesktopToolOptions::MergePdf => unreachable!(),
    }
}

fn process_job_with_password_in(
    job: &JobRecord,
    password: &SecretPassword,
    ctx: &RunCtx<'_>,
) -> Result<PathBuf, RefractError> {
    let password = password.as_str();
    match job.request.options() {
        DesktopToolOptions::MergePdf => {
            return Err(RefractError::new(
                ErrorCode::InvalidOption,
                "Merge PDF unlock requires the complete recorded batch.",
            ));
        }
        DesktopToolOptions::CompressPdf
        | DesktopToolOptions::PdfToImages { .. }
        | DesktopToolOptions::SplitPdf { .. }
        | DesktopToolOptions::ReorderPdf { .. } => {}
        _ => {
            return Err(RefractError::new(
                ErrorCode::UnsupportedFormat,
                "Password not needed for this tool",
            ));
        }
    }

    let report = preflight_then_work(
        &job.request,
        &[job.input_path.as_path()],
        single_password(Some(password)),
        ctx,
    )?;
    let (bytes, pages) = take_preflight_pdf(report)?;
    let same_pages = Expect::format(Format::Pdf).pages(pages);
    let cancel = ctx.cancel;

    match job.request.options() {
        DesktopToolOptions::CompressPdf => {
            let out_bytes = pdf::compress_pdf_cancellable(&bytes, Some(password), cancel)?;
            save_single_output(job, "pdf", &out_bytes, same_pages, ctx)
        }
        DesktopToolOptions::PdfToImages { target, dpi } => {
            render_or_extract_pdf_images(job, &bytes, pages, *target, *dpi, Some(password), ctx)
        }
        DesktopToolOptions::SplitPdf { ranges } => {
            let parts = pdf::split_pdf_with_password(&bytes, ranges, Some(password), cancel)?;
            save_split_parts(job, ranges, pages, parts, ctx)
        }
        DesktopToolOptions::ReorderPdf { order } => {
            let out_bytes = pdf::reorder_pdf_with_password(&bytes, order, Some(password), cancel)?;
            save_single_output(job, "pdf", &out_bytes, same_pages, ctx)
        }
        _ => unreachable!(),
    }
}

/// Test helpers: run without a store, a cancel request or stage reporting.
#[cfg(test)]
fn process_job(job: &JobRecord) -> Result<PathBuf, RefractError> {
    let cancel = CancelToken::none();
    process_job_in(job, &RunCtx::new(&cancel, &|_| {}))
}

#[cfg(test)]
fn process_job_with_password(
    job: &JobRecord,
    password: &SecretPassword,
) -> Result<PathBuf, RefractError> {
    let cancel = CancelToken::none();
    process_job_with_password_in(job, password, &RunCtx::new(&cancel, &|_| {}))
}

#[cfg(test)]
fn execute_batch_merge_pdf(
    jobs: &[JobRecord],
    request: &JobRequest,
) -> Result<PathBuf, RefractError> {
    let cancel = CancelToken::none();
    execute_batch_merge_pdf_in(jobs, request, &RunCtx::new(&cancel, &|_| {}))
}

#[cfg(test)]
fn execute_batch_images_to_pdf(
    jobs: &[JobRecord],
    request: &JobRequest,
) -> Result<PathBuf, RefractError> {
    let cancel = CancelToken::none();
    execute_batch_images_to_pdf_in(jobs, request, &RunCtx::new(&cancel, &|_| {}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::options::{parse_format_strict, OutputDestination};
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

    fn record_for(
        input_path: PathBuf,
        tool: ToolId,
        options: DesktopToolOptions,
        destination: OutputDestination,
    ) -> JobRecord {
        let name = input_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        JobRecord {
            id: 1,
            input_path,
            name,
            size_bytes: 1000,
            tool_label: String::new(),
            request: JobRequest::new(tool, options, destination).unwrap(),
            last_batch_member_ids: Vec::new(),
            awaiting_password: false,
            status: JobState::Queued,
            error: String::new(),
            output_path: PathBuf::new(),
            progress: 0.0,
        }
    }

    #[test]
    fn output_file_names_follow_manifest_templates() {
        let input = PathBuf::from("/home/user/photo.jpg");
        let cases = [
            (
                ToolId::ConvertImage,
                DesktopToolOptions::ConvertImage {
                    target: Format::Png,
                    quality: 90,
                },
                "png",
                "photo (converted).png",
            ),
            (
                ToolId::CompressImage,
                DesktopToolOptions::CompressImage { target_kb: 200 },
                "jpg",
                "photo_compressed.jpg",
            ),
            (
                ToolId::MergePdf,
                DesktopToolOptions::MergePdf,
                "pdf",
                "photo_merged.pdf",
            ),
        ];
        for (tool, options, ext, expected) in cases {
            let job = record_for(
                input.clone(),
                tool,
                options,
                OutputDestination::NextToSource,
            );
            assert_eq!(output_file_name(&job, ext), expected);
        }
    }

    #[test]
    fn process_job_never_overwrites_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("plain.png");
        std::fs::copy(fixture_path("plain.png"), &input).unwrap();
        let existing = dir.path().join("plain (converted).webp");
        std::fs::write(&existing, b"user file").unwrap();

        let job = record_for(
            input,
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Webp,
                quality: 80,
            },
            OutputDestination::NextToSource,
        );
        let out = process_job(&job).unwrap();
        assert_eq!(out, dir.path().join("plain (converted) (2).webp"));
        assert_eq!(std::fs::read(&existing).unwrap(), b"user file");
        assert_eq!(
            refract_core::detect(&std::fs::read(&out).unwrap()),
            Some(Format::Webp)
        );
        let leftover_temps = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".refract-")
            })
            .count();
        assert_eq!(leftover_temps, 0);
    }

    #[test]
    fn split_pdf_numbers_taken_part_names_and_keeps_existing_file() {
        let src = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let input = src.path().join("plain.pdf");
        std::fs::copy(fixture_path("plain.pdf"), &input).unwrap();
        let existing = out_dir.path().join("plain-p1-2.pdf");
        std::fs::write(&existing, b"user file").unwrap();

        let job = record_for(
            input,
            ToolId::SplitPdf,
            DesktopToolOptions::SplitPdf {
                ranges: "1-2, 3".into(),
            },
            OutputDestination::Directory(out_dir.path().to_path_buf()),
        );
        process_job(&job).unwrap();
        assert_eq!(std::fs::read(&existing).unwrap(), b"user file");
        let renamed = std::fs::read(out_dir.path().join("plain-p1-2 (2).pdf")).unwrap();
        assert_eq!(pdf::probe_pdf(&renamed).unwrap().page_count, 2);
        assert!(out_dir.path().join("plain-p3.pdf").exists());
    }

    fn dir_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn pdf_job_in(dir: &Path) -> JobRecord {
        record_for(
            dir.join("doc.pdf"),
            ToolId::SplitPdf,
            DesktopToolOptions::SplitPdf {
                ranges: String::new(),
            },
            OutputDestination::Directory(dir.to_path_buf()),
        )
    }

    #[test]
    fn final_state_follows_the_state_table() {
        let ok: Result<PathBuf, RefractError> = Ok(PathBuf::from("out.png"));
        let err = |code| -> Result<PathBuf, RefractError> { Err(RefractError::new(code, "x")) };
        assert_eq!(final_state(JobState::Verifying, &ok), JobState::Saved);
        for reached in [JobState::Probing, JobState::Running, JobState::Verifying] {
            assert_eq!(
                final_state(reached, &err(ErrorCode::Canceled)),
                JobState::Canceled
            );
        }
        assert_eq!(
            final_state(JobState::Probing, &err(ErrorCode::NeedsPassword)),
            JobState::NeedsPassword
        );
        assert_eq!(
            final_state(JobState::Probing, &err(ErrorCode::LimitExceeded)),
            JobState::Rejected
        );
        assert_eq!(
            final_state(JobState::Running, &err(ErrorCode::EngineFailed)),
            JobState::Failed
        );
        assert_eq!(
            final_state(JobState::Verifying, &err(ErrorCode::VerificationFailed)),
            JobState::Failed
        );
        // Every result is reachable through a legal transition from the stage it comes from.
        for (reached, result) in [
            (JobState::Verifying, ok.clone()),
            (JobState::Probing, err(ErrorCode::WrongPassword)),
            (JobState::Probing, err(ErrorCode::DamagedFile)),
            (JobState::Running, err(ErrorCode::Canceled)),
            (JobState::Running, err(ErrorCode::LimitExceeded)),
        ] {
            assert!(reached.can_transition(final_state(reached, &result)));
        }
    }

    #[test]
    fn cancel_at_running_or_verifying_ends_canceled_with_no_output() {
        for cancel_at in [JobState::Running, JobState::Verifying] {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("plain.png");
            std::fs::copy(fixture_path("plain.png"), &input).unwrap();
            let job = record_for(
                input,
                ToolId::ConvertImage,
                DesktopToolOptions::ConvertImage {
                    target: Format::Webp,
                    quality: 80,
                },
                OutputDestination::NextToSource,
            );
            let store = Arc::new(Mutex::new(vec![job.clone()]));
            begin_probing(&mut store.lock().unwrap(), &[job.id]);

            let cancel = CancelToken::new();
            let seen = std::cell::RefCell::new(Vec::new());
            let res = {
                let on_stage = |s: JobState| {
                    seen.borrow_mut().push(s);
                    // The user presses Cancel the moment the run reaches this stage.
                    if s == cancel_at {
                        cancel.cancel();
                    }
                };
                let ctx = RunCtx::new(&cancel, &on_stage);
                process_job_in(&job, &ctx)
            };
            assert_eq!(res.as_ref().unwrap_err().code, ErrorCode::Canceled);
            {
                let mut guard = store.lock().unwrap();
                // Apply as the runner does, from the stage the run reached.
                let reached = *seen.borrow().last().unwrap();
                set_state(&mut guard[0], JobState::Running);
                if reached == JobState::Verifying {
                    set_state(&mut guard[0], JobState::Verifying);
                }
                apply_result_to_jobs(&mut guard, &[job.id], &res);
                assert_eq!(guard[0].status, JobState::Canceled);
            }
            assert_eq!(dir_names(dir.path()), vec!["plain.png"], "{:?}", cancel_at);
        }
    }

    #[test]
    fn rejected_output_is_never_saved() {
        let dir = tempfile::tempdir().unwrap();
        let job = pdf_job_in(dir.path());
        let cancel = CancelToken::none();
        let err = save_single_output(
            &job,
            "pdf",
            b"%PDF-1.7 truncated",
            Expect::format(Format::Pdf),
            &RunCtx::new(&cancel, &|_| {}),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::VerificationFailed);
        assert!(dir_names(dir.path()).is_empty());
    }

    #[test]
    fn one_bad_output_in_a_set_saves_none_of_them() {
        let dir = tempfile::tempdir().unwrap();
        let job = pdf_job_in(dir.path());
        let good = std::fs::read(fixture_path("plain.pdf")).unwrap();
        let outputs = [
            ("a.pdf".to_string(), good.as_slice()),
            ("b.pdf".to_string(), b"not a pdf".as_slice()),
            ("c.pdf".to_string(), good.as_slice()),
        ];
        let err = save_checked_outputs(
            &job,
            &outputs,
            &[Expect::format(Format::Pdf); 3],
            &RunCtx::new(&CancelToken::none(), &|_| {}),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::VerificationFailed);
        assert_eq!(
            err.details.as_deref(),
            Some("file 2 of 3: it is not a known file type")
        );
        assert!(dir_names(dir.path()).is_empty());
    }

    #[test]
    fn rotated_photo_converts_and_resizes_with_upright_size() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("rotated.jpg");
        std::fs::copy(fixture_path("rotated.jpg"), &input).unwrap();

        let convert = record_for(
            input.clone(),
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Png,
                quality: 90,
            },
            OutputDestination::NextToSource,
        );
        let out = process_job(&convert).unwrap();
        assert_eq!(
            image::probe_dimensions(&std::fs::read(&out).unwrap()).unwrap(),
            (300, 400)
        );

        let resize = record_for(
            input,
            ToolId::ResizeImage,
            DesktopToolOptions::ResizeImage {
                unit: image::ResizeUnit::Px,
                width: 150.0,
                height: 0.0,
                dpi: 96.0,
                target_format: None,
                quality: 85,
            },
            OutputDestination::NextToSource,
        );
        let out = process_job(&resize).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(image::probe_dimensions(&bytes).unwrap(), (150, 200));
        assert!(!image::jpeg_has_exif(&bytes));
    }

    #[test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    fn live_audio_outputs_pass_verification() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("sound.m4a");
        std::fs::copy(fixture_path("sound.m4a"), &input).unwrap();

        for target in [Format::Mp3, Format::Wav, Format::M4a, Format::Ogg] {
            let job = record_for(
                input.clone(),
                ToolId::ConvertAudio,
                DesktopToolOptions::ConvertAudio {
                    target,
                    bitrate_kbps: (target != Format::Wav).then_some(192),
                },
                OutputDestination::NextToSource,
            );
            let out = process_job(&job).unwrap();
            assert_eq!(
                refract_core::detect(&std::fs::read(&out).unwrap()),
                Some(target)
            );
        }
        for end in [TrimEnd::AtSeconds(1.5), TrimEnd::EndOfFile] {
            let job = record_for(
                input.clone(),
                ToolId::TrimAudio,
                DesktopToolOptions::TrimAudio {
                    start_secs: 0.5,
                    end,
                    bitrate_kbps: 192,
                },
                OutputDestination::NextToSource,
            );
            process_job(&job).unwrap();
        }
        assert!(!dir_names(dir.path())
            .iter()
            .any(|n| n.starts_with(".refract-")));
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
            guard[0].status = JobState::Failed;
            guard[0].error = "simulated transient failure".into();
        }

        execute_retry_in_store(&store, 1);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[0].status, JobState::Saved);
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
            guard[1].status = JobState::Failed;
            guard[2].status = JobState::Failed;
            guard[1].last_batch_member_ids = vec![2, 3];
            guard[2].last_batch_member_ids = vec![2, 3];
        }

        execute_retry_in_store(&store, 2);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[1].status, JobState::Saved);
            assert_eq!(guard[2].status, JobState::Saved);
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
            guard[3].status = JobState::Failed;
            guard[4].status = JobState::Failed;
            guard[3].last_batch_member_ids = vec![4, 5];
            guard[4].last_batch_member_ids = vec![4, 5];
        }

        execute_retry_in_store(&store, 4);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[3].status, JobState::Saved);
            assert_eq!(guard[4].status, JobState::Saved);
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
            process_job_with_password(&store[0], &SecretPassword::new("wrong-pw"))
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        let compressed_out =
            process_job_with_password(&store[0], &SecretPassword::new("user-pw-123")).unwrap();
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
        let split_out =
            process_job_with_password(&store[1], &SecretPassword::new("user-pw-123")).unwrap();
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
        let reorder_out =
            process_job_with_password(&store[2], &SecretPassword::new("user-pw-123")).unwrap();
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
            process_job_with_password(&store[3], &SecretPassword::new("wrong-pw"))
                .unwrap_err()
                .code,
            ErrorCode::WrongPassword
        );
        match process_job_with_password(&store[3], &SecretPassword::new("user-pw-123")) {
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
            guard[0].status = JobState::NeedsPassword;
        }

        // Unknown job: nothing to claim.
        assert!(try_claim_password_unlock(&store, 999, None).is_none());

        // First unlock claims the job (now Probing) and preserves the immutable request.
        let claimed = try_claim_password_unlock(&store, 1, None).unwrap().unwrap();
        match claimed {
            PasswordUnlockClaim::Single(job) => assert_eq!(job.request, req),
            PasswordUnlockClaim::MergeBatch { .. } => panic!("expected Single claim"),
        }
        assert_eq!(store.lock().unwrap()[0].status, JobState::Probing);

        // A second unlock of the same job (commands run one at a time on the worker) is refused
        // because the job is no longer waiting for a password, and the job is left as it was.
        let second = try_claim_password_unlock(&store, 1, None).unwrap();
        assert_eq!(second.err().unwrap().code, ErrorCode::InvalidOption);
        assert_eq!(store.lock().unwrap()[0].status, JobState::Probing);
    }

    fn convert_job_in(dir: &Path, id: i32) -> JobRecord {
        let input = dir.join(format!("plain-{id}.png"));
        std::fs::copy(fixture_path("plain.png"), &input).unwrap();
        let mut job = record_for(
            input,
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Webp,
                quality: 80,
            },
            OutputDestination::NextToSource,
        );
        job.id = id;
        job
    }

    #[test]
    fn canceling_a_waiting_job_means_process_all_skips_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(vec![
            convert_job_in(dir.path(), 1),
            convert_job_in(dir.path(), 2),
        ]));
        let shared = Shared::for_store(&store);
        cancel_job(&shared, 2, None);
        assert_eq!(store.lock().unwrap()[1].status, JobState::Canceled);

        execute_command(&shared, Command::ProcessAll, None);
        let guard = store.lock().unwrap();
        assert_eq!(guard[0].status, JobState::Saved);
        assert_eq!(guard[1].status, JobState::Canceled);
        let outputs: Vec<String> = dir_names(dir.path())
            .into_iter()
            .filter(|n| n.contains("converted"))
            .collect();
        assert_eq!(outputs, vec!["plain-1 (converted).webp"]);
    }

    #[test]
    fn cancel_reaches_a_running_job_through_its_units_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(vec![convert_job_in(dir.path(), 1)]));
        let shared = Shared::for_store(&store);
        // The worker has registered the unit and the job is running.
        let token = shared.active.lock().unwrap().start(&[1]);
        begin_probing(&mut store.lock().unwrap(), &[1]);
        cancel_job(&shared, 1, None);
        assert!(token.is_canceled());
        // Running jobs can't be removed or moved, and clear-all keeps them.
        assert!(!remove_job_guarded(&shared, 1));
        assert_eq!(clear_jobs_guarded(&shared), 1);
        store.lock().unwrap().push(convert_job_in(dir.path(), 2));
        assert!(!move_job_guarded(&mut store.lock().unwrap(), 2, true));
    }

    #[test]
    fn canceling_a_password_waiting_merge_cancels_the_batch_and_drops_passwords() {
        let work = tempfile::tempdir().unwrap();
        let a = write_locked_marker_pdf(work.path(), "a.pdf", 101, 51, "alpha-pw");
        let b = write_locked_marker_pdf(work.path(), "b.pdf", 103, 53, "bravo-pw");
        let (fx, _) = MergeFixture::run(vec![a, b], work);
        fx.unlock(1, "alpha-pw").unwrap_err();
        assert!(fx.secrets.lock().unwrap().contains(1));

        let shared = Shared {
            jobs: Arc::clone(&fx.store),
            secrets: Arc::new(Mutex::new(JobSecretStore::new())),
            active: Arc::new(Mutex::new(ActiveRun::default())),
        };
        // Use the fixture's own secret store so pruning is observable.
        std::mem::swap(
            &mut *shared.secrets.lock().unwrap(),
            &mut *fx.secrets.lock().unwrap(),
        );
        cancel_job(&shared, 2, None);
        assert!(fx
            .store
            .lock()
            .unwrap()
            .iter()
            .all(|j| j.status == JobState::Canceled));
        assert!(shared.secrets.lock().unwrap().is_empty());
        assert!(dir_names(fx.out.path()).is_empty());
    }

    #[test]
    fn only_failed_or_canceled_jobs_can_be_retried_and_retry_is_a_fresh_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let mut rejected = convert_job_in(dir.path(), 1);
        rejected.status = JobState::Rejected;
        rejected.error = "bad input".into();
        let mut canceled = convert_job_in(dir.path(), 2);
        canceled.status = JobState::Canceled;
        canceled.error = "Canceled.".into();
        let store = Arc::new(Mutex::new(vec![rejected, canceled]));

        let err = prepare_retry(&mut store.lock().unwrap(), 1).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidOption);
        assert_eq!(store.lock().unwrap()[0].status, JobState::Rejected);

        let ids = prepare_retry(&mut store.lock().unwrap(), 2).unwrap();
        assert_eq!(ids, vec![2]);
        {
            let guard = store.lock().unwrap();
            assert_eq!(guard[1].status, JobState::Queued);
            assert!(guard[1].error.is_empty());
        }
        execute_command(&Shared::for_store(&store), Command::Retry(ids), None);
        assert_eq!(store.lock().unwrap()[1].status, JobState::Saved);
    }

    #[test]
    fn retry_of_a_batch_whose_member_was_canceled_runs_nothing() {
        let work = tempfile::tempdir().unwrap();
        let p1 = write_marker_pdf(work.path(), "one.pdf", 64, 48);
        let p2 = write_marker_pdf(work.path(), "two.pdf", 96, 48);
        let out = tempfile::tempdir().unwrap();
        let request = JobRequest::new(
            ToolId::MergePdf,
            DesktopToolOptions::MergePdf,
            OutputDestination::Directory(out.path().to_path_buf()),
        )
        .unwrap();
        let mut jobs = Vec::new();
        let mut next_id = 1;
        enqueue_files(&mut jobs, &mut next_id, vec![p1, p2], &request);
        for j in jobs.iter_mut() {
            j.status = JobState::Failed;
            j.last_batch_member_ids = vec![1, 2];
        }
        let store = Arc::new(Mutex::new(jobs));
        let shared = Shared::for_store(&store);
        let ids = prepare_retry(&mut store.lock().unwrap(), 1).unwrap();
        assert_eq!(ids, vec![1, 2]);
        // Cancel one member after Retry was clicked: the whole recorded batch is canceled.
        cancel_job(&shared, 2, None);
        execute_command(&shared, Command::Retry(ids), None);
        assert!(store
            .lock()
            .unwrap()
            .iter()
            .all(|j| j.status == JobState::Canceled));
        assert!(dir_names(out.path()).is_empty());
    }

    #[test]
    fn unlock_commands_run_one_after_another_on_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("locked.pdf");
        std::fs::copy(fixture_path("locked.pdf"), &input).unwrap();
        let job = record_for(
            input,
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            OutputDestination::NextToSource,
        );
        let store = Arc::new(Mutex::new(vec![job]));
        let shared = Shared::for_store(&store);
        store.lock().unwrap()[0].status = JobState::NeedsPassword;

        // Two unlocks queued back to back: the first succeeds, the second finds nothing to do.
        let (tx, rx) = mpsc::channel::<Command>();
        for _ in 0..2 {
            tx.send(Command::Unlock {
                job_id: 1,
                password: SecretPassword::new("user-pw-123"),
            })
            .unwrap();
        }
        drop(tx);
        while let Ok(command) = rx.recv() {
            execute_command(&shared, command, None);
        }
        let guard = store.lock().unwrap();
        assert_eq!(guard[0].status, JobState::Saved);
        assert_eq!(dir_names(dir.path()).len(), 2, "one output, written once");
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
                j.status = JobState::Failed;
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
        execute_retry_in_store(&store, 2);

        let guard = store.lock().unwrap();
        assert!(guard.iter().all(|j| j.status == JobState::Saved));
        let out_path = guard[0].output_path.clone();
        assert!(guard.iter().all(|j| j.output_path == out_path));
        assert_eq!(out_path.parent(), Some(out_dir.path()));
        // Output name comes from recorded first input (`first.pdf`), not reordered `third.pdf`
        assert_eq!(out_path.file_name().unwrap(), "first_merged.pdf");

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
                j.status = JobState::Failed;
                j.last_batch_member_ids = vec![1, 2, 3];
            }
            // Delete member 2 before retrying
            guard.retain(|j| j.id != 2);
        }

        execute_retry_in_store(&store, 1);

        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 2);
        for j in guard.iter() {
            assert_eq!(j.status, JobState::Failed);
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
                j.status = JobState::NeedsPassword;
                j.error = err.message.clone();
            }
            // Reorder the UI store to [2, 1] before unlocking
            guard.swap(0, 1);
            assert_eq!(guard.iter().map(|j| j.id).collect::<Vec<_>>(), vec![2, 1]);
        }

        // 1. Incorrect password keeps all members in NeedsPassword and releases busy flag
        let wrong_claim = try_claim_password_unlock(&store, 2, None).unwrap().unwrap();
        let wrong_res = complete_password_unlock_claim(
            &store,
            &Mutex::new(JobSecretStore::new()),
            wrong_claim,
            SecretPassword::new("wrong-pw"),
            &CancelToken::none(),
            None,
        );
        assert_eq!(wrong_res.unwrap_err().code, ErrorCode::WrongPassword);
        {
            let guard = store.lock().unwrap();
            for j in guard.iter() {
                assert_eq!(j.status, JobState::NeedsPassword);
                // The message names the file the wrong password was entered for.
                assert!(
                    j.error.contains("did not unlock 'enc_first.pdf'"),
                    "{}",
                    j.error
                );
                assert!(j.output_path.as_os_str().is_empty());
            }
        }
        assert!(std::fs::read_dir(out_dir.path()).unwrap().next().is_none());

        // 2. Correct password unlocks full batch in recorded order [1, 2] into original destination
        let right_claim = try_claim_password_unlock(&store, 2, None).unwrap().unwrap();
        let out_path = complete_password_unlock_claim(
            &store,
            &Mutex::new(JobSecretStore::new()),
            right_claim,
            SecretPassword::new("user-pw-123"),
            &CancelToken::none(),
            None,
        )
        .unwrap();

        assert_eq!(out_path.parent(), Some(out_dir.path()));
        // Derived from recorded first member `enc_first.pdf`, NOT reordered `enc_second.pdf`
        assert_eq!(out_path.file_name().unwrap(), "enc_first_merged.pdf");
        let merged_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(pdf::probe_pdf(&merged_bytes).unwrap().page_count, 6);

        let guard = store.lock().unwrap();
        for j in guard.iter() {
            assert_eq!(j.status, JobState::Saved);
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
                j.status = JobState::NeedsPassword;
                j.last_batch_member_ids = vec![1, 2, 3, 4];
            }
            // Reverse UI store to [4, 3, 2, 1]
            guard.reverse();
        }

        let claim = try_claim_password_unlock(&store, 3, None).unwrap().unwrap();
        let out_path = complete_password_unlock_claim(
            &store,
            &Mutex::new(JobSecretStore::new()),
            claim,
            SecretPassword::new("user-pw-123"),
            &CancelToken::none(),
            None,
        )
        .unwrap();

        let merged_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(pdf::probe_pdf(&merged_bytes).unwrap().page_count, 8);
        // Page 4 must be marker_a (64x48) and page 8 must be marker_b (128x48)
        assert_eq!(page_embedded_image_dims(&merged_bytes, 4), (64, 48));
        assert_eq!(page_embedded_image_dims(&merged_bytes, 8), (128, 48));

        let guard = store.lock().unwrap();
        assert!(guard.iter().all(|j| j.status == JobState::Saved));
        assert!(guard.iter().all(|j| j.output_path == out_path));
    }

    /// Encrypt `bytes` with user password `user_pw` (RC4-128), like the engine's test helper.
    fn encrypt_pdf(bytes: &[u8], user_pw: &str) -> Vec<u8> {
        use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
        use lopdf::{Document, Object, StringFormat};
        let mut doc = Document::load_mem(bytes).unwrap();
        let id = format!("refract-test-id-{user_pw}").into_bytes();
        doc.trailer.set(
            "ID",
            vec![
                Object::String(id.clone(), StringFormat::Hexadecimal),
                Object::String(id, StringFormat::Hexadecimal),
            ],
        );
        let version = EncryptionVersion::V2 {
            document: &doc,
            owner_password: "owner-secret",
            user_password: user_pw,
            key_length: 128,
            permissions: Permissions::all(),
        };
        let state = EncryptionState::try_from(version).unwrap();
        doc.encrypt(&state).unwrap();
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    fn write_locked_marker_pdf(dir: &Path, name: &str, w: u32, h: u32, pw: &str) -> PathBuf {
        let plain = write_marker_pdf(dir, &format!("plain-{name}"), w, h);
        let locked = encrypt_pdf(&std::fs::read(&plain).unwrap(), pw);
        std::fs::remove_file(&plain).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, locked).unwrap();
        path
    }

    struct MergeFixture {
        _work: tempfile::TempDir,
        out: tempfile::TempDir,
        store: Arc<Mutex<Vec<JobRecord>>>,
        secrets: Mutex<JobSecretStore>,
    }

    impl MergeFixture {
        /// Queue `files` as one merge batch and run it once with no passwords, as "Process all" does.
        fn run(files: Vec<PathBuf>, work: tempfile::TempDir) -> (Self, RefractError) {
            let out = tempfile::tempdir().unwrap();
            let request = JobRequest::new(
                ToolId::MergePdf,
                DesktopToolOptions::MergePdf,
                OutputDestination::Directory(out.path().to_path_buf()),
            )
            .unwrap();
            let mut jobs = Vec::new();
            let mut next_id = 1;
            enqueue_files(&mut jobs, &mut next_id, files, &request);
            let ids: Vec<i32> = jobs.iter().map(|j| j.id).collect();
            for j in jobs.iter_mut() {
                j.last_batch_member_ids = ids.clone();
            }
            // Same path as "Process all": Probing, then the run reports stages and the result.
            begin_probing(&mut jobs, &ids);
            let members = jobs.clone();
            let store = Arc::new(Mutex::new(jobs));
            let result = run_unit(&store, &ids, &CancelToken::none(), None, |ctx| {
                execute_batch_merge_pdf_in(&members, &request, ctx)
            });
            let fixture = Self {
                _work: work,
                out,
                store,
                secrets: Mutex::new(JobSecretStore::new()),
            };
            (fixture, result.unwrap_err())
        }

        /// Enter `password` from row `clicked`, as the dialog does.
        fn unlock(&self, clicked: i32, password: &str) -> Result<PathBuf, RefractError> {
            let claim = try_claim_password_unlock(&self.store, clicked, None)
                .unwrap()
                .unwrap();
            complete_password_unlock_claim(
                &self.store,
                &self.secrets,
                claim,
                SecretPassword::new(password),
                &CancelToken::none(),
                None,
            )
        }

        fn awaiting(&self) -> Vec<i32> {
            let store = self.store.lock().unwrap();
            store
                .iter()
                .filter(|j| j.awaiting_password)
                .map(|j| j.id)
                .collect()
        }

        fn prompts(&self) -> Vec<String> {
            self.store
                .lock()
                .unwrap()
                .iter()
                .map(|j| j.error.clone())
                .collect()
        }
    }

    #[test]
    fn merge_unlocks_files_with_different_passwords_one_at_a_time() {
        let work = tempfile::tempdir().unwrap();
        let a = write_locked_marker_pdf(work.path(), "a.pdf", 101, 51, "alpha-pw");
        let open = write_marker_pdf(work.path(), "open.pdf", 102, 52);
        let b = write_locked_marker_pdf(work.path(), "b.pdf", 103, 53, "bravo-pw");
        let owner_only = work.path().join("owner.pdf");
        std::fs::copy(fixture_path("owner-only.pdf"), &owner_only).unwrap();
        let (fx, first) = MergeFixture::run(vec![a, open, b, owner_only], work);

        // First run: a.pdf is the locked file; every row shows the same prompt naming it.
        assert_eq!(first.code, ErrorCode::NeedsPassword);
        assert_eq!(fx.awaiting(), vec![1]);
        assert!(fx
            .prompts()
            .iter()
            .all(|p| p.contains("'a.pdf' is protected")));

        // Password for a.pdf, entered from a different row: a opens, b.pdf now needs its own.
        let err = fx.unlock(3, "alpha-pw").unwrap_err();
        assert_eq!(err.code, ErrorCode::NeedsPassword, "{}", err.message);
        assert_eq!(fx.awaiting(), vec![3]);
        assert!(fx
            .prompts()
            .iter()
            .all(|p| p.contains("'b.pdf' is protected")));
        assert!(fx.secrets.lock().unwrap().contains(1));

        // Password for b.pdf: the merge completes in queue order and every secret is dropped.
        let out = fx.unlock(3, "bravo-pw").unwrap();
        assert!(fx.secrets.lock().unwrap().is_empty());
        assert!(fx.awaiting().is_empty());
        let merged = std::fs::read(&out).unwrap();
        let pages = pdf::probe_pdf(&merged).unwrap().page_count;
        let owner_pages = pdf::probe_pdf(&std::fs::read(fixture_path("owner-only.pdf")).unwrap())
            .unwrap()
            .page_count;
        assert_eq!(pages, 3 + owner_pages);
        assert_eq!(page_embedded_image_dims(&merged, 1), (101, 51));
        assert_eq!(page_embedded_image_dims(&merged, 2), (102, 52));
        assert_eq!(page_embedded_image_dims(&merged, 3), (103, 53));
        assert_eq!(dir_names(fx.out.path()).len(), 1);
    }

    #[test]
    fn one_password_unlocks_every_file_that_shares_it() {
        let work = tempfile::tempdir().unwrap();
        let a = write_locked_marker_pdf(work.path(), "a.pdf", 101, 51, "same-pw");
        let open = write_marker_pdf(work.path(), "open.pdf", 102, 52);
        let b = write_locked_marker_pdf(work.path(), "b.pdf", 103, 53, "same-pw");
        let (fx, _) = MergeFixture::run(vec![a, open, b], work);
        let out = fx.unlock(1, "same-pw").unwrap();
        let merged = std::fs::read(&out).unwrap();
        assert_eq!(page_embedded_image_dims(&merged, 3), (103, 53));
        assert!(fx.secrets.lock().unwrap().is_empty());
    }

    #[test]
    fn wrong_password_drops_only_that_files_password() {
        let work = tempfile::tempdir().unwrap();
        let a = write_locked_marker_pdf(work.path(), "a.pdf", 101, 51, "alpha-pw");
        let b = write_locked_marker_pdf(work.path(), "b.pdf", 103, 53, "bravo-pw");
        let (fx, _) = MergeFixture::run(vec![a, b], work);

        fx.unlock(1, "alpha-pw").unwrap_err();
        assert_eq!(fx.awaiting(), vec![2]);
        let err = fx.unlock(2, "not-bravo").unwrap_err();
        assert_eq!(err.code, ErrorCode::WrongPassword);
        assert!(err.message.contains("'b.pdf'"));
        // a.pdf's password is kept; the wrong one for b.pdf is gone; b.pdf is still awaited.
        let secrets = fx.secrets.lock().unwrap();
        assert!(secrets.contains(1) && !secrets.contains(2));
        drop(secrets);
        assert_eq!(fx.awaiting(), vec![2]);
        assert!(dir_names(fx.out.path()).is_empty());
    }

    #[test]
    fn removing_or_clearing_jobs_drops_their_passwords() {
        let work = tempfile::tempdir().unwrap();
        let a = write_locked_marker_pdf(work.path(), "a.pdf", 101, 51, "alpha-pw");
        let b = write_locked_marker_pdf(work.path(), "b.pdf", 103, 53, "bravo-pw");
        let c = write_locked_marker_pdf(work.path(), "c.pdf", 104, 54, "charlie-pw");
        let (fx, _) = MergeFixture::run(vec![a, b, c], work);
        fx.unlock(1, "alpha-pw").unwrap_err();
        fx.unlock(2, "bravo-pw").unwrap_err();
        assert!(fx.secrets.lock().unwrap().contains(1));

        // Remove one job (as the remove handler does): only its password goes.
        {
            let mut store = fx.store.lock().unwrap();
            store.retain(|j| j.id != 1);
            prune_secrets(&store, &fx.secrets);
        }
        let secrets = fx.secrets.lock().unwrap();
        assert!(!secrets.contains(1) && secrets.contains(2));
        drop(secrets);

        // Clear all.
        {
            let mut store = fx.store.lock().unwrap();
            store.clear();
            prune_secrets(&store, &fx.secrets);
        }
        assert!(fx.secrets.lock().unwrap().is_empty());
    }

    #[test]
    fn passwords_never_appear_in_debug_output_or_errors() {
        let work = tempfile::tempdir().unwrap();
        let a = write_locked_marker_pdf(work.path(), "a.pdf", 101, 51, "alpha-secret-pw");
        let b = write_locked_marker_pdf(work.path(), "b.pdf", 103, 53, "bravo-secret-pw");
        let (fx, _) = MergeFixture::run(vec![a, b], work);
        let err1 = fx.unlock(1, "alpha-secret-pw").unwrap_err();
        let err2 = fx.unlock(2, "wrong-secret-pw").unwrap_err();
        let shown = format!(
            "{:?} {:?} {} {} {:?} {:?}",
            err1,
            err2,
            err1,
            err2,
            fx.secrets.lock().unwrap(),
            fx.store.lock().unwrap()
        );
        for pw in ["alpha-secret-pw", "bravo-secret-pw", "wrong-secret-pw"] {
            assert!(!shown.contains(pw), "leaked {pw}");
        }
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
                j.status = JobState::NeedsPassword;
                j.last_batch_member_ids = vec![1, 2];
            }
            guard[2].status = JobState::NeedsPassword;
        }

        let err = try_claim_password_unlock(&store_incompat, 1, None)
            .unwrap()
            .err()
            .expect("expected incompatible batch error");
        assert_eq!(err.code, ErrorCode::InvalidOption);
        {
            let guard = store_incompat.lock().unwrap();
            assert_eq!(guard[0].status, JobState::Rejected);
            assert!(guard[0].error.contains("incompatible"));
            assert_eq!(guard[1].status, JobState::Rejected);
            assert!(guard[1].error.contains("incompatible"));
            // Unrelated job 3 must stay untouched in NeedsPassword
            assert_eq!(guard[2].status, JobState::NeedsPassword);
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
                j.status = JobState::NeedsPassword;
                j.last_batch_member_ids = vec![1, 2];
            }
            guard.retain(|j| j.id != 2);
        }

        let missing_err = try_claim_password_unlock(&store_missing, 1, None)
            .unwrap()
            .err()
            .expect("expected missing member error");
        assert_eq!(missing_err.code, ErrorCode::InvalidOption);
        {
            let guard = store_missing.lock().unwrap();
            assert_eq!(guard[0].status, JobState::Rejected);
            assert!(
                guard[0].error.contains("missing") || guard[0].error.contains("removed"),
                "unexpected error: {}",
                guard[0].error
            );
        }
        assert!(std::fs::read_dir(dir_a.path()).unwrap().next().is_none());

        // 3. Password unlock refuses targets not in NeedsPassword without mutating their status
        for non_unlock_status in [
            JobState::Queued,
            JobState::Running,
            JobState::Saved,
            JobState::Failed,
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
                guard[0].status = non_unlock_status;
            }
            let status_err = try_claim_password_unlock(&store_state, 1, None)
                .unwrap()
                .err()
                .expect("expected invalid unlock state error");
            assert_eq!(status_err.code, ErrorCode::InvalidOption);
            assert_eq!(store_state.lock().unwrap()[0].status, non_unlock_status);
        }
    }

    #[test]
    fn desktop_execution_paths_enforce_preflight_before_writing_outputs() {
        let out_dir = tempfile::tempdir().unwrap();
        let missing = out_dir.path().join("missing.png");
        let dest = OutputDestination::Directory(out_dir.path().to_path_buf());

        // 1. Single-file image tool rejects HEIC and missing file via preflight
        let conv_req = JobRequest::new(
            ToolId::ConvertImage,
            DesktopToolOptions::ConvertImage {
                target: Format::Jpeg,
                quality: 85,
            },
            dest.clone(),
        )
        .unwrap();
        let mut store = Vec::new();
        let mut next_id = 1;
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("plain.heic"), missing.clone()],
            &conv_req,
        );
        assert_eq!(
            process_job(&store[0]).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        let miss_err = process_job(&store[1]).unwrap_err();
        assert_eq!(miss_err.code, ErrorCode::DamagedFile);
        assert!(miss_err.message.contains("could not be found"));

        // 2. PDF tool rejects empty file and wrong-format PNG via preflight
        let compress_req = JobRequest::new(
            ToolId::CompressPdf,
            DesktopToolOptions::CompressPdf,
            dest.clone(),
        )
        .unwrap();
        enqueue_files(
            &mut store,
            &mut next_id,
            vec![fixture_path("empty.heic"), fixture_path("plain.png")],
            &compress_req,
        );
        assert_eq!(
            process_job(&store[2]).unwrap_err().code,
            ErrorCode::EmptyFile
        );
        assert_eq!(
            process_job(&store[3]).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );

        // 3. merge-pdf batch with one valid PDF and one unsupported PNG fails without writing output
        let merge_req =
            JobRequest::new(ToolId::MergePdf, DesktopToolOptions::MergePdf, dest.clone()).unwrap();
        let mut merge_store = Vec::new();
        enqueue_files(
            &mut merge_store,
            &mut next_id,
            vec![fixture_path("plain.pdf"), fixture_path("plain.png")],
            &merge_req,
        );
        assert_eq!(
            execute_batch_merge_pdf(&merge_store, &merge_req)
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedFormat
        );

        // 4. images-to-pdf batch with one valid PNG and one missing image fails cleanly
        let imgs_req = JobRequest::new(
            ToolId::ImagesToPdf,
            DesktopToolOptions::ImagesToPdf {
                dpi: 150.0,
                quality: 85,
            },
            dest.clone(),
        )
        .unwrap();
        let mut img_store = Vec::new();
        enqueue_files(
            &mut img_store,
            &mut next_id,
            vec![fixture_path("plain.png"), missing],
            &imgs_req,
        );
        assert_eq!(
            execute_batch_images_to_pdf(&img_store, &imgs_req)
                .unwrap_err()
                .code,
            ErrorCode::DamagedFile
        );

        // 5. Audio tool rejects non-audio file via preflight before spawning FFmpeg
        let audio_req = JobRequest::new(
            ToolId::ConvertAudio,
            DesktopToolOptions::ConvertAudio {
                target: Format::Mp3,
                bitrate_kbps: Some(192),
            },
            dest,
        )
        .unwrap();
        let mut audio_store = Vec::new();
        enqueue_files(
            &mut audio_store,
            &mut next_id,
            vec![fixture_path("plain.pdf")],
            &audio_req,
        );
        assert_eq!(
            process_job(&audio_store[0]).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );

        // Zero files were written to out_dir across all failed preflights
        assert!(std::fs::read_dir(out_dir.path()).unwrap().next().is_none());
    }
}
