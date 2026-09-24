//! Notices crashes so they can be reported instead of guessed at.
//!
//! - A panic hook writes the panic (message, location, backtrace) to
//!   `runtime.log` and `last-crash.json` in the data folder.
//! - `session.lock` exists while the app runs and is removed on a clean
//!   exit, so the next start can tell whether the last one ended abruptly.
//!
//! A panic inside a background task doesn't end the app (the task just
//! stops), so the hook also flags it for the running app to surface.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

use serde::{Deserialize, Serialize};

const LOCK_FILE: &str = "session.lock";
const CRASH_FILE: &str = "last-crash.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CrashRecord {
    pub at: String,
    pub version: String,
    pub message: String,
    pub location: String,
}

/// How the previous run ended.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PreviousSession {
    /// It didn't exit normally (crash, force-quit, power loss, or Windows
    /// shutting down underneath it).
    pub unclean: bool,
    /// A panic it recorded, if any.
    pub crash: Option<CrashRecord>,
}

static PANICKED: AtomicBool = AtomicBool::new(false);
static LAST_PANIC: Mutex<Option<CrashRecord>> = Mutex::new(None);
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Installs the panic hook. Call once the data folder is known.
pub fn install(data_dir: &Path, version: &str) {
    if DATA_DIR.set(data_dir.to_path_buf()).is_err() {
        return;
    }
    let version = version.to_string();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| text.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        let location = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()))
            .unwrap_or_default();
        let record = CrashRecord {
            at: chrono::Utc::now().to_rfc3339(),
            version: version.clone(),
            message,
            location,
        };
        if let Some(dir) = DATA_DIR.get() {
            write_record(dir, &record);
        }
        if let Ok(mut last) = LAST_PANIC.lock() {
            *last = Some(record);
        }
        PANICKED.store(true, Ordering::SeqCst);
        previous(info);
    }));
}

fn write_record(dir: &Path, record: &CrashRecord) {
    let thread = std::thread::current();
    let backtrace = std::backtrace::Backtrace::force_capture();
    if let Ok(mut log) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("runtime.log"))
    {
        let _ = writeln!(
            log,
            "[PANIC] {} v{} thread '{}' panicked at {}: {}\n{backtrace}",
            record.at,
            record.version,
            thread.name().unwrap_or("unnamed"),
            record.location,
            record.message,
        );
    }
    if let Ok(json) = serde_json::to_vec_pretty(record) {
        let _ = fs::write(dir.join(CRASH_FILE), json);
    }
}

/// A panic caught since the last call, if any (the app kept running).
pub fn take_runtime_panic() -> Option<CrashRecord> {
    if !PANICKED.swap(false, Ordering::SeqCst) {
        return None;
    }
    LAST_PANIC.lock().ok().and_then(|mut last| last.take())
}

/// Reads how the last run ended and marks this one as running.
pub fn begin_session(data_dir: &Path) -> PreviousSession {
    let lock = data_dir.join(LOCK_FILE);
    let crash_path = data_dir.join(CRASH_FILE);
    let unclean = lock.exists();
    let crash = fs::read_to_string(&crash_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let _ = fs::remove_file(&crash_path);
    let _ = fs::write(&lock, std::process::id().to_string());
    PreviousSession { unclean, crash }
}

/// Marks a clean exit.
pub fn end_session(data_dir: &Path) {
    let _ = fs::remove_file(data_dir.join(LOCK_FILE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_exit_leaves_nothing_to_report() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(begin_session(dir.path()), PreviousSession::default());
        end_session(dir.path());
        assert_eq!(begin_session(dir.path()), PreviousSession::default());
    }

    #[test]
    fn an_abrupt_exit_is_noticed_once() {
        let dir = tempfile::tempdir().unwrap();
        begin_session(dir.path());
        // No end_session: the process died.
        let record = CrashRecord {
            at: "2026-09-24T12:00:00Z".into(),
            version: "0.5.0-beta.1".into(),
            message: "boom".into(),
            location: "src/app/queue.rs:10".into(),
        };
        write_record(dir.path(), &record);

        let previous = begin_session(dir.path());
        assert!(previous.unclean);
        assert_eq!(previous.crash, Some(record));

        // Reported once; the next run starts fresh.
        end_session(dir.path());
        assert_eq!(begin_session(dir.path()), PreviousSession::default());
        let log = fs::read_to_string(dir.path().join("runtime.log")).unwrap();
        assert!(log.contains("[PANIC]") && log.contains("boom"));
    }
}
