slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let main_window = MainWindow::new()?;

    let window_weak = main_window.as_weak();
    main_window.on_pick_files(move || {
        if let Some(window) = window_weak.upgrade() {
            if let Some(files) = rfd::FileDialog::new().pick_files() {
                let count = files.len() as i32;
                window.set_job_count(window.get_job_count() + count);
            }
        }
    });

    let window_weak2 = main_window.as_weak();
    main_window.on_clear_jobs(move || {
        if let Some(window) = window_weak2.upgrade() {
            window.set_job_count(0);
        }
    });

    main_window.run()
}
