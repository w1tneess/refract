//! No-clobber output staging and persistence shared by every native tool.
//!
//! Outputs are written to an exclusively created hidden temp file next to the final
//! destination, then moved into place with `persist_noclobber` (Windows `MoveFileExW`
//! without `MOVEFILE_REPLACE_EXISTING`, Unix `link` + `unlink`). A destination that
//! appears at any point, including between name resolution and the move, is never
//! overwritten: the move fails atomically and the next numbered name is tried.
//! Temp files are owned [`TempPath`]s, so error and cancellation paths remove only
//! files this module created.

use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use refract_core::naming::suffixed_filename;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use tempfile::{Builder as TempFileBuilder, TempPath};

/// Highest numbered suffix tried (`name (2)` .. `name (10000)`) before giving up.
pub const DEFAULT_MAX_SUFFIX_ATTEMPTS: usize = 9_999;

const TEMP_PREFIX: &str = ".refract-";

fn engine_err(message: impl Into<String>, details: impl Into<String>) -> RefractError {
    RefractError::with_details(ErrorCode::EngineFailed, message, details)
}

fn dir_or_current(dir: &Path) -> &Path {
    if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    }
}

fn ext_suffix(ext: &str) -> String {
    let clean = ext.trim_start_matches('.');
    if clean.is_empty() {
        String::new()
    } else {
        format!(".{}", clean)
    }
}

/// Exclusively create an empty hidden temp file in `dir` (`O_CREAT | O_EXCL` / `CREATE_NEW`),
/// keeping `.ext` so tools that pick a muxer from the extension (FFmpeg) behave correctly.
///
/// The file handle is closed before returning so external processes can write to it on Windows.
pub fn create_sibling_temp(dir: &Path, prefix: &str, ext: &str) -> Result<TempPath, RefractError> {
    let named = TempFileBuilder::new()
        .prefix(prefix)
        .suffix(&ext_suffix(ext))
        .tempfile_in(dir_or_current(dir))
        .map_err(|e| engine_err("Failed to allocate a temporary output file", e.to_string()))?;
    Ok(named.into_temp_path())
}

/// Write `bytes` into a new exclusively created temp file in `dir` and flush it to disk.
pub fn stage_bytes(dir: &Path, ext: &str, bytes: &[u8]) -> Result<TempPath, RefractError> {
    let mut named = TempFileBuilder::new()
        .prefix(TEMP_PREFIX)
        .suffix(&ext_suffix(ext))
        .tempfile_in(dir_or_current(dir))
        .map_err(|e| engine_err("Failed to allocate a temporary output file", e.to_string()))?;
    named
        .write_all(bytes)
        .and_then(|_| named.as_file().sync_all())
        .map_err(|e| engine_err("Failed to write temporary output file", e.to_string()))?;
    Ok(named.into_temp_path())
}

/// Move `temp` to exactly `output`, failing (without overwriting) if `output` exists.
/// On any error the temp file is removed and `output` is left untouched.
pub fn persist_noclobber(temp: TempPath, output: &Path) -> Result<(), RefractError> {
    match temp.persist_noclobber(output) {
        Ok(()) => Ok(()),
        Err(err) => {
            drop(err.path);
            if err.error.kind() == ErrorKind::AlreadyExists {
                Err(engine_err(
                    format!(
                        "Output file already exists and will not be overwritten: {}",
                        output.display()
                    ),
                    "destination_exists",
                ))
            } else {
                Err(engine_err(
                    "Failed to finalize output file",
                    err.error.to_string(),
                ))
            }
        }
    }
}

/// Move `temp` into `dir` as `desired_name`, or `name (2).ext`, `name (3).ext`, ... when taken.
///
/// Each attempt is a single atomic no-clobber move, so an existing file is never replaced even
/// if it is created concurrently. Returns the path actually written. After `max_attempts`
/// numbered names are taken, fails and removes the temp file.
pub fn persist_unique(
    temp: TempPath,
    dir: &Path,
    desired_name: &str,
    max_attempts: usize,
) -> Result<PathBuf, RefractError> {
    persist_unique_cancellable(temp, dir, desired_name, max_attempts, &CancelToken::none())
}

pub fn persist_unique_cancellable(
    temp: TempPath,
    dir: &Path,
    desired_name: &str,
    max_attempts: usize,
    cancel: &CancelToken,
) -> Result<PathBuf, RefractError> {
    let dir = dir_or_current(dir);
    let mut temp = temp;
    let candidates = std::iter::once(desired_name.to_string())
        .chain((2..=max_attempts + 1).map(|n| suffixed_filename(desired_name, n)));

    for name in candidates {
        cancel.check()?;
        let candidate = dir.join(&name);
        match temp.persist_noclobber(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.error.kind() == ErrorKind::AlreadyExists => temp = err.path,
            Err(err) => {
                drop(err.path);
                return Err(engine_err(
                    format!("Failed to save output file {}", candidate.display()),
                    err.error.to_string(),
                ));
            }
        }
    }

    Err(engine_err(
        format!(
            "Could not find an unused output file name for '{}' after {} attempts.",
            desired_name, max_attempts
        ),
        "destination_exhausted",
    ))
}

fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => "",
    }
}

/// Stage and save one output in `dir` as `desired_name` (numbered if taken).
/// Cancellation is checked after staging and before the file becomes visible.
pub fn save_output(
    dir: &Path,
    desired_name: &str,
    bytes: &[u8],
    cancel: &CancelToken,
) -> Result<PathBuf, RefractError> {
    let mut batch = StagedOutputBatch::new(dir);
    batch.stage(desired_name, bytes)?;
    let mut saved = batch.commit(cancel)?;
    Ok(saved.remove(0))
}

// Quarantine before inspecting identity: never check a public name and then unlink it.
fn rollback_owned(committed: &[(PathBuf, same_file::Handle)], dir: &Path) -> Vec<String> {
    let recovery = match tempfile::Builder::new()
        .prefix(".refract-recovery-")
        .tempdir_in(dir)
    {
        Ok(dir) => dir,
        Err(e) => {
            return committed
                .iter()
                .map(|(p, _)| format!("{} (quarantine unavailable: {e})", p.display()))
                .collect()
        }
    };
    let mut failures = Vec::new();
    for (index, (path, owner)) in committed.iter().enumerate() {
        let held = recovery.path().join(index.to_string());
        match std::fs::rename(path, &held) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        }
        let ours = std::fs::symlink_metadata(&held).is_ok_and(|m| m.is_file())
            && same_file::Handle::from_path(&held).is_ok_and(|h| h == *owner);
        if ours {
            if let Err(e) = std::fs::remove_file(&held) {
                failures.push(format!(
                    "{} (owned output quarantined at {}): {e}",
                    path.display(),
                    held.display()
                ));
            }
        } else {
            // A concurrent replacement belongs to someone else. Restore without clobbering.
            match std::fs::hard_link(&held, path) {
                Ok(()) => {
                    if let Err(e) = std::fs::remove_file(&held) {
                        failures.push(format!("{}: {e}", held.display()));
                    }
                }
                Err(e) => failures.push(format!(
                    "Replacement for {} retained at {}: {e}",
                    path.display(),
                    held.display()
                )),
            }
        }
    }
    if !failures.is_empty() {
        let _ = recovery.keep();
    }
    failures
}

/// A set of outputs that are saved all together or not at all (`split-pdf`, `pdf-to-images`).
///
/// Every output is first written to its own temp file. [`commit`](Self::commit) then moves them
/// into place in order; if any move fails or the job is canceled mid-way, outputs already moved by
/// this batch are removed after ownership verification. Cleanup failures retain private recovery
/// files and ownership anchors, reported with the original error for safe recovery.
pub struct StagedOutputBatch {
    dir: PathBuf,
    staged: Vec<(TempPath, String)>,
    max_attempts: usize,
}

impl StagedOutputBatch {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir_or_current(dir).to_path_buf(),
            staged: Vec::new(),
            max_attempts: DEFAULT_MAX_SUFFIX_ATTEMPTS,
        }
    }

    pub fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    pub fn len(&self) -> usize {
        self.staged.len()
    }

    pub fn is_empty(&self) -> bool {
        self.staged.is_empty()
    }

    /// Write `bytes` to a temp file that will later be saved as `desired_name`.
    pub fn stage(&mut self, desired_name: &str, bytes: &[u8]) -> Result<(), RefractError> {
        let temp = stage_bytes(&self.dir, extension_of(desired_name), bytes)?;
        self.staged.push((temp, desired_name.to_string()));
        Ok(())
    }

    /// Save every staged output, returning final paths in staging order.
    /// An empty batch is an error so a job can never finish with no outputs.
    pub fn commit(self, cancel: &CancelToken) -> Result<Vec<PathBuf>, RefractError> {
        if self.staged.is_empty() {
            return Err(engine_err(
                "The tool produced no output files.",
                "no_outputs",
            ));
        }
        cancel.check()?;

        // Keep private hard links as durable ownership evidence for recovery if cleanup fails.
        let ownership = tempfile::Builder::new()
            .prefix(".refract-ownership-")
            .tempdir_in(&self.dir)
            .map_err(|e| engine_err("Cannot prepare batch ownership", e.to_string()))?;
        let mut committed: Vec<(PathBuf, same_file::Handle)> =
            Vec::with_capacity(self.staged.len());
        for (index, (temp, name)) in self.staged.into_iter().enumerate() {
            let anchor = ownership.path().join(index.to_string());
            let owner = std::fs::hard_link(&temp, &anchor)
                .and_then(|()| same_file::Handle::from_path(&anchor))
                .map_err(|e| engine_err("Cannot retain output ownership", e.to_string()));
            let result = if owner.is_err() {
                Err(owner.as_ref().err().unwrap().clone())
            } else if cancel.is_canceled() {
                drop(temp);
                Err(RefractError::canceled())
            } else {
                persist_unique_cancellable(temp, &self.dir, &name, self.max_attempts, cancel)
            };
            match result {
                Ok(path) => committed.push((path, owner.unwrap())),
                Err(err) => {
                    let failures = rollback_owned(&committed, &self.dir);
                    if !failures.is_empty() {
                        let evidence = ownership.keep();
                        return Err(RefractError::with_details(err.code, err.message,
                            format!("{}; rollback incomplete: {}. Ownership anchors: {} (numbered in batch order). Recovery: move each public entry into a private quarantine, compare file identity with its anchor using same_file, and delete only matches; restore replacements without overwriting. Retain anchors until recovery succeeds.", err.details.unwrap_or_default(), failures.join("; "), evidence.display())));
                    }
                    return Err(err);
                }
            }
        }
        Ok(committed.into_iter().map(|(path, _)| path).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn save_output_writes_new_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let out = save_output(dir.path(), "a.pdf", b"new", &CancelToken::none()).unwrap();
        assert_eq!(out, dir.path().join("a.pdf"));
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert_eq!(names_in(dir.path()), vec!["a.pdf"]);
    }

    #[test]
    fn existing_destination_is_preserved_and_numbered_name_used() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.pdf"), b"user").unwrap();
        let out = save_output(dir.path(), "a.pdf", b"new", &CancelToken::none()).unwrap();
        assert_eq!(out, dir.path().join("a (2).pdf"));
        assert_eq!(std::fs::read(dir.path().join("a.pdf")).unwrap(), b"user");
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
    }

    #[test]
    fn destination_created_after_staging_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let temp = stage_bytes(dir.path(), "pdf", b"new").unwrap();
        // Simulates another process creating the file between name choice and the move.
        std::fs::write(dir.path().join("a.pdf"), b"racer").unwrap();
        let out = persist_unique(temp, dir.path(), "a.pdf", 5).unwrap();
        assert_eq!(out, dir.path().join("a (2).pdf"));
        assert_eq!(std::fs::read(dir.path().join("a.pdf")).unwrap(), b"racer");
    }

    #[test]
    fn strict_persist_refuses_existing_destination_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.mp3");
        std::fs::write(&target, b"user").unwrap();
        let temp = stage_bytes(dir.path(), "mp3", b"new").unwrap();
        let err = persist_noclobber(temp, &target).unwrap_err();
        assert_eq!(err.details.as_deref(), Some("destination_exists"));
        assert_eq!(std::fs::read(&target).unwrap(), b"user");
        assert_eq!(names_in(dir.path()), vec!["a.mp3"]);
    }

    #[test]
    fn exhausted_suffixes_fail_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.pdf", "a (2).pdf", "a (3).pdf"] {
            std::fs::write(dir.path().join(name), name).unwrap();
        }
        let temp = stage_bytes(dir.path(), "pdf", b"new").unwrap();
        let err = persist_unique(temp, dir.path(), "a.pdf", 2).unwrap_err();
        assert_eq!(err.details.as_deref(), Some("destination_exhausted"));
        assert_eq!(
            names_in(dir.path()),
            vec!["a (2).pdf", "a (3).pdf", "a.pdf"]
        );
        for name in ["a.pdf", "a (2).pdf", "a (3).pdf"] {
            assert_eq!(
                std::fs::read(dir.path().join(name)).unwrap(),
                name.as_bytes()
            );
        }
    }

    #[test]
    fn batch_commits_all_outputs_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut batch = StagedOutputBatch::new(dir.path());
        batch.stage("doc-p1.png", b"1").unwrap();
        batch.stage("doc-p2.png", b"2").unwrap();
        let saved = batch.commit(&CancelToken::none()).unwrap();
        assert_eq!(
            saved,
            vec![dir.path().join("doc-p1.png"), dir.path().join("doc-p2.png")]
        );
        assert_eq!(names_in(dir.path()), vec!["doc-p1.png", "doc-p2.png"]);
    }

    #[test]
    fn batch_failure_mid_commit_rolls_back_and_keeps_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("doc-p1.pdf"), b"user").unwrap();
        std::fs::write(dir.path().join(".refract-unrelated.pdf"), b"other").unwrap();
        let mut batch = StagedOutputBatch::new(dir.path());
        batch.stage("doc-p1.pdf", b"1").unwrap();
        // Output #2 targets a missing sub-folder, so its move fails after #1 was saved.
        batch.stage("missing/doc-p2.pdf", b"2").unwrap();
        batch.stage("doc-p3.pdf", b"3").unwrap();
        assert!(batch.commit(&CancelToken::none()).is_err());
        assert_eq!(
            names_in(dir.path()),
            vec![".refract-unrelated.pdf", "doc-p1.pdf"]
        );
        assert_eq!(
            std::fs::read(dir.path().join("doc-p1.pdf")).unwrap(),
            b"user"
        );
    }

    #[test]
    fn canceled_batch_saves_nothing_and_removes_temps() {
        let dir = tempfile::tempdir().unwrap();
        let mut batch = StagedOutputBatch::new(dir.path());
        batch.stage("a.pdf", b"1").unwrap();
        batch.stage("b.pdf", b"2").unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = batch.commit(&cancel).unwrap_err();
        assert_eq!(err.code, ErrorCode::Canceled);
        assert!(names_in(dir.path()).is_empty());
    }

    #[test]
    fn empty_batch_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = StagedOutputBatch::new(dir.path())
            .commit(&CancelToken::none())
            .unwrap_err();
        assert_eq!(err.details.as_deref(), Some("no_outputs"));
    }
    #[test]
    fn rollback_preserves_replacements_and_removes_owned_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pdf");
        std::fs::write(&path, b"ours").unwrap();
        let owner = same_file::Handle::from_path(&path).unwrap();
        std::fs::rename(&path, dir.path().join("moved.pdf")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        assert!(rollback_owned(&[(path.clone(), owner)], dir.path()).is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        let owner = same_file::Handle::from_path(&path).unwrap();
        assert!(rollback_owned(&[(path.clone(), owner)], dir.path()).is_empty());
        assert!(!path.exists());
    }
    #[test]
    fn canceled_unique_publication_leaves_no_output() {
        let dir = tempfile::tempdir().unwrap();
        let temp = stage_bytes(dir.path(), "pdf", b"ours").unwrap();
        let token = CancelToken::new();
        token.cancel();
        assert_eq!(
            persist_unique_cancellable(temp, dir.path(), "out.pdf", 5, &token)
                .unwrap_err()
                .code,
            ErrorCode::Canceled
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn rollback_reports_paths_when_quarantine_cannot_be_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pdf");
        std::fs::write(&path, b"ours").unwrap();
        let owner = same_file::Handle::from_path(&path).unwrap();
        let failures = rollback_owned(&[(path.clone(), owner)], &path);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains(path.to_str().unwrap()));
        assert_eq!(std::fs::read(&path).unwrap(), b"ours");
    }
}
