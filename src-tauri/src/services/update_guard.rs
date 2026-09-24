//! Keeps a bad update from stranding anyone.
//!
//! Installing an update moves the running exe aside as `<name>.old`, puts
//! the new one in its place, and launches it on probation. A watchdog — the
//! old exe itself, started with `--update-watchdog <pid>` and no window —
//! waits for the new version to report that its UI loaded. If the new
//! version exits or hangs first, the watchdog puts the old exe back, starts
//! it, and records the rollback so that version isn't offered again.
//!
//! The files, all beside the exe:
//! - `<name>.old`: the previous version, kept until the new one has proven
//!   itself and been restarted once.
//! - `<name>.failed`: a version that was rolled back.
//! - `<name>.update.json`: which update is in flight and how it went.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const WATCHDOG_FLAG: &str = "--update-watchdog";
/// How long a new version gets to show its UI. Generous: a cold WebView2
/// start on a slow machine can take a while.
const PROBATION: Duration = Duration::from_secs(180);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Probation,
    Healthy,
    RolledBack,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UpdateRecord {
    pub from: String,
    pub to: String,
    pub phase: Phase,
}

/// The files beside one exe.
#[derive(Clone, Debug)]
pub struct Layout {
    exe: PathBuf,
}

impl Layout {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        Self { exe: exe.into() }
    }

    /// The layout of the running app. For the watchdog (running as
    /// `<name>.old`) this is still the `<name>.exe` it guards.
    pub fn current() -> Result<Self> {
        Ok(Self::new(std::env::current_exe()?.with_extension("exe")))
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }
    pub fn old(&self) -> PathBuf {
        self.exe.with_extension("old")
    }
    pub fn staged(&self) -> PathBuf {
        self.exe.with_extension("new")
    }
    pub fn failed(&self) -> PathBuf {
        self.exe.with_extension("failed")
    }
    fn record_path(&self) -> PathBuf {
        self.exe.with_extension("update.json")
    }

    pub fn read_record(&self) -> Option<UpdateRecord> {
        let text = fs::read_to_string(self.record_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn write_record(&self, record: &UpdateRecord) -> Result<()> {
        let tmp = self.exe.with_extension("update.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(record)?)?;
        fs::rename(&tmp, self.record_path())?;
        Ok(())
    }

    fn clear_record(&self) {
        let _ = fs::remove_file(self.record_path());
    }
}

/// What the app should tell the user about the last update, if anything.
#[derive(Debug, PartialEq, Eq)]
pub enum StartupNotice {
    /// `failed` didn't start, so this (older) version was put back.
    RolledBack { failed: String },
}

/// Tidies up after earlier updates. Call once at startup, before anything
/// else touches these files.
pub fn on_startup(layout: &Layout, running_version: &str) -> Option<StartupNotice> {
    let _ = fs::remove_file(layout.staged());
    match layout.read_record() {
        // The new version is starting on probation: keep the old exe until
        // the UI loads (see `mark_healthy`).
        Some(record) if record.phase == Phase::Probation => None,
        Some(record) if record.phase == Phase::RolledBack => {
            let _ = fs::remove_file(layout.failed());
            if is_newer(running_version, &record.to) {
                // Moved past the version that failed; nothing to remember.
                layout.clear_record();
                None
            } else {
                Some(StartupNotice::RolledBack { failed: record.to })
            }
        }
        // Proven healthy last run (or no update in flight): the old exe has
        // done its job.
        _ => {
            layout.clear_record();
            let _ = fs::remove_file(layout.old());
            let _ = fs::remove_file(layout.failed());
            None
        }
    }
}

/// Called once the UI has loaded. Ends probation, which the watchdog is
/// waiting for.
pub fn mark_healthy(layout: &Layout, running_version: &str) {
    if let Some(mut record) = layout.read_record() {
        if record.phase == Phase::Probation && record.to == running_version {
            record.phase = Phase::Healthy;
            let _ = layout.write_record(&record);
        }
    }
}

/// A version that was rolled back on this PC: offered only as a manual
/// download, never installed automatically again.
pub fn rolled_back_version(layout: &Layout) -> Option<String> {
    layout
        .read_record()
        .filter(|record| record.phase == Phase::RolledBack)
        .map(|record| record.to)
}

fn is_newer(candidate: &str, than: &str) -> bool {
    match (
        crate::services::updater::parse_version_tag(candidate),
        crate::services::updater::parse_version_tag(than),
    ) {
        (Some(candidate), Some(than)) => candidate > than,
        _ => false,
    }
}

/// Moves the staged exe into place, records the probation, launches the new
/// version and its watchdog. On error the original exe is back in place.
pub fn install(layout: &Layout, from: &str, to: &str) -> Result<()> {
    let _ = fs::remove_file(layout.old());
    fs::rename(layout.exe(), layout.old())
        .context("Could not move the running executable aside. Is the app folder writable?")?;
    if let Err(error) = fs::rename(layout.staged(), layout.exe()) {
        let _ = fs::rename(layout.old(), layout.exe());
        return Err(anyhow::Error::new(error).context("Could not move the new executable into place"));
    }

    layout.write_record(&UpdateRecord {
        from: from.to_string(),
        to: to.to_string(),
        phase: Phase::Probation,
    })?;

    let child = std::process::Command::new(layout.exe())
        .spawn()
        .context("The update installed but the new version failed to launch. Start it manually.")?;

    // Best effort: without the watchdog the update still works, it just
    // can't undo itself.
    let _ = std::process::Command::new(layout.old())
        .arg(WATCHDOG_FLAG)
        .arg(child.id().to_string())
        .spawn();
    Ok(())
}

/// Entry point for `<name>.old --update-watchdog <pid>`. Returns the exit
/// code, or `None` when this isn't a watchdog launch.
pub fn run_watchdog_if_requested() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(WATCHDOG_FLAG) {
        return None;
    }
    let pid: u32 = args.next()?.parse().ok()?;
    let Ok(layout) = Layout::current() else {
        return Some(1);
    };
    Some(watch(&layout, pid))
}

fn watch(layout: &Layout, pid: u32) -> i32 {
    let started = Instant::now();
    loop {
        match layout.read_record() {
            Some(record) if record.phase == Phase::Healthy => return 0,
            Some(record) if record.phase == Phase::Probation => {}
            // Someone else settled it (or the record is gone): stand down.
            _ => return 0,
        }
        let timed_out = started.elapsed() > PROBATION;
        if timed_out || !process::is_running(pid) {
            if timed_out {
                process::terminate(pid);
                std::thread::sleep(Duration::from_secs(2));
            }
            return match roll_back(layout) {
                Ok(()) => {
                    let _ = std::process::Command::new(layout.exe()).spawn();
                    0
                }
                Err(_) => 1,
            };
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Puts the old exe back and records which version failed. Runs from the
/// old exe itself; renaming a running image is allowed on Windows.
pub fn roll_back(layout: &Layout) -> Result<()> {
    let record = layout.read_record().context("no update in flight")?;
    let _ = fs::remove_file(layout.failed());
    // The failed exe may still be exiting; give it a few tries.
    let mut moved = fs::rename(layout.exe(), layout.failed());
    for _ in 0..10 {
        if moved.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
        moved = fs::rename(layout.exe(), layout.failed());
    }
    moved.context("could not move the failed version aside")?;
    fs::rename(layout.old(), layout.exe()).context("could not restore the previous version")?;
    layout.write_record(&UpdateRecord {
        phase: Phase::RolledBack,
        ..record
    })
}

#[cfg(windows)]
mod process {
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        },
    };

    pub fn is_running(pid: u32) -> bool {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
                return false;
            };
            let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
            let _ = CloseHandle(handle);
            running
        }
    }

    pub fn terminate(pid: u32) {
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(handle, 1);
                let _ = CloseHandle(handle);
            }
        }
    }
}

#[cfg(not(windows))]
mod process {
    pub fn is_running(_pid: u32) -> bool {
        false
    }
    pub fn terminate(_pid: u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("AppleCrap Alpha.exe"));
        (dir, layout)
    }

    fn touch(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    fn probation(layout: &Layout) {
        layout
            .write_record(&UpdateRecord {
                from: "0.4.0-alpha.1".into(),
                to: "0.5.0-beta.1".into(),
                phase: Phase::Probation,
            })
            .unwrap();
    }

    #[test]
    fn keeps_the_old_version_during_probation() {
        let (_dir, layout) = layout();
        touch(&layout.old(), "old");
        probation(&layout);
        assert_eq!(on_startup(&layout, "0.5.0-beta.1"), None);
        assert!(layout.old().exists(), "old exe must survive probation");
    }

    #[test]
    fn healthy_update_is_cleaned_up_on_the_next_start() {
        let (_dir, layout) = layout();
        touch(&layout.old(), "old");
        probation(&layout);
        mark_healthy(&layout, "0.5.0-beta.1");
        assert_eq!(layout.read_record().unwrap().phase, Phase::Healthy);
        on_startup(&layout, "0.5.0-beta.1");
        assert!(!layout.old().exists());
        assert!(layout.read_record().is_none());
    }

    #[test]
    fn only_the_new_version_can_end_probation() {
        let (_dir, layout) = layout();
        probation(&layout);
        mark_healthy(&layout, "0.4.0-alpha.1");
        assert_eq!(layout.read_record().unwrap().phase, Phase::Probation);
    }

    #[test]
    fn roll_back_restores_the_old_exe_and_remembers_the_bad_version() {
        let (_dir, layout) = layout();
        touch(layout.exe(), "new");
        touch(&layout.old(), "old");
        probation(&layout);

        roll_back(&layout).unwrap();
        assert_eq!(fs::read_to_string(layout.exe()).unwrap(), "old");
        assert_eq!(fs::read_to_string(layout.failed()).unwrap(), "new");
        assert_eq!(rolled_back_version(&layout).as_deref(), Some("0.5.0-beta.1"));

        // The restored version starts, says what happened, and drops the
        // failed exe — but keeps remembering the version.
        assert_eq!(
            on_startup(&layout, "0.4.0-alpha.1"),
            Some(StartupNotice::RolledBack { failed: "0.5.0-beta.1".into() })
        );
        assert!(!layout.failed().exists());
        assert_eq!(rolled_back_version(&layout).as_deref(), Some("0.5.0-beta.1"));

        // Once a newer version is running, the memory is dropped.
        assert_eq!(on_startup(&layout, "0.5.1-beta.1"), None);
        assert!(layout.read_record().is_none());
    }

    #[test]
    fn install_keeps_the_original_when_the_new_exe_is_missing() {
        let (_dir, layout) = layout();
        touch(layout.exe(), "current");
        assert!(install(&layout, "0.4.0-alpha.1", "0.5.0-beta.1").is_err());
        assert_eq!(fs::read_to_string(layout.exe()).unwrap(), "current");
    }

    /// `install` starts the watchdog from `<name>.old`; Windows must run an
    /// exe whatever its extension when it's launched directly.
    #[cfg(windows)]
    #[test]
    fn an_exe_named_old_still_launches() {
        let (_dir, layout) = layout();
        fs::copy(r"C:\Windows\System32\hostname.exe", layout.old()).unwrap();
        let status = std::process::Command::new(layout.old())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn a_missing_process_is_not_running() {
        assert!(!process::is_running(u32::MAX - 1));
    }
}
