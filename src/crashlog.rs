//! A crash log for problems that leave nothing behind: a window opened from the desktop has no
//! terminal, so a panic vanishes and a freeze ends in "Force Quit". This writes `crash.log` in
//! the app's data folder with
//! - panics: message, place, the last step the app announced, and a backtrace;
//! - freezes: a frame of the interface that has not finished after [`STALL`], with the same
//!   last step, and a note when it finally finished.
//!
//! The "last step" is a breadcrumb the app sets before work that can go wrong (reading a
//! dropped file, creating a project, sending a request), so the log says which file or request.
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How long one frame may take before it counts as frozen.
const STALL: Duration = Duration::from_secs(5);
/// The log is moved aside to `crash.log.old` once it grows past this.
const MAX_BYTES: u64 = 512 * 1024;

static LOG: OnceLock<PathBuf> = OnceLock::new();
static STEP: Mutex<String> = Mutex::new(String::new());
static START: OnceLock<Instant> = OnceLock::new();
/// When the frame in progress began, in ms since `START` plus one (0: no frame in progress).
static FRAME_SINCE: AtomicU64 = AtomicU64::new(0);
/// The `FRAME_SINCE` value a freeze was already written for.
static REPORTED: AtomicU64 = AtomicU64::new(0);

fn elapsed_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Where the log is written, once [`install`] has run.
pub fn path() -> Option<&'static Path> {
    LOG.get().map(PathBuf::as_path)
}

/// Starts logging into `dir`: panics are written there, and a watchdog thread watches the frames.
pub fn install(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    if LOG.set(dir.join("crash.log")).is_err() {
        return;
    }
    let _ = elapsed_ms();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        record(&format!(
            "PANIC at {place}: {message}\nlast step: {}\n{}",
            step(),
            std::backtrace::Backtrace::force_capture()
        ));
        previous(info);
    }));
    let _ = std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let since = FRAME_SINCE.load(Ordering::Relaxed);
                if since != 0
                    && REPORTED.load(Ordering::Relaxed) != since
                    && elapsed_ms().saturating_sub(since) >= STALL.as_millis() as u64
                {
                    REPORTED.store(since, Ordering::Relaxed);
                    record(&format!(
                        "NOT RESPONDING: one frame has run for {}s and has not finished\nlast step: {}",
                        elapsed_ms().saturating_sub(since) / 1000,
                        step()
                    ));
                }
            }
        });
}

/// Whether the log has grown since this was last asked, so a problem from the previous run is
/// pointed out once. The size last seen is kept next to the log.
pub fn take_unseen(dir: &Path) -> bool {
    let size = std::fs::metadata(dir.join("crash.log")).map_or(0, |m| m.len());
    let seen = dir.join("crash-seen");
    let known = std::fs::read_to_string(&seen)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    if size == 0 || known == Some(size) {
        return false;
    }
    let _ = std::fs::write(seen, size.to_string());
    true
}

/// Sets the step the app is on, for the log if it panics or freezes during it.
pub fn note(step: impl Into<String>) {
    if let Ok(mut current) = STEP.lock() {
        *current = step.into();
    }
}

fn step() -> String {
    STEP.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Marks one frame of the interface; the watchdog times it until the guard is dropped.
pub struct Frame(u64);

impl Frame {
    pub fn begin() -> Self {
        let since = elapsed_ms();
        FRAME_SINCE.store(since, Ordering::Relaxed);
        Frame(since)
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        FRAME_SINCE.store(0, Ordering::Relaxed);
        if REPORTED.load(Ordering::Relaxed) == self.0 {
            record(&format!(
                "RESPONDING AGAIN: the frame took {}s\nlast step: {}",
                elapsed_ms().saturating_sub(self.0) / 1000,
                step()
            ));
        }
    }
}

/// Appends one entry to the log, if logging was installed.
pub fn record(entry: &str) {
    let Some(path) = LOG.get() else {
        return;
    };
    append(path, &stamp(SystemTime::now()), entry);
}

fn append(path: &Path, time: &str, entry: &str) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            file,
            "[{time}] v{} {}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            entry.trim_end()
        );
    }
}

/// `2026-10-19 09:56:52 UTC`
fn stamp(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(stamp(UNIX_EPOCH), "1970-01-01 00:00:00 UTC");
        let t = UNIX_EPOCH + Duration::from_secs(1_792_403_812);
        assert_eq!(stamp(t), "2026-10-19 09:56:52 UTC");
        let leap = UNIX_EPOCH + Duration::from_secs(1_709_164_800 + 86_399);
        assert_eq!(stamp(leap), "2024-02-29 23:59:59 UTC");
    }
    #[test]
    fn entries_are_appended_and_a_large_log_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crash.log");
        append(&path, "t1", "first");
        append(&path, "t2", "second\n");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[t1]") && text.contains("first\n\n[t2]"));
        assert!(text.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))));
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&path, "t3", "third");
        assert!(dir.path().join("crash.log.old").exists());
        assert!(std::fs::read_to_string(&path).unwrap().contains("third"));
    }
    #[test]
    fn a_new_entry_is_pointed_out_once() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!take_unseen(dir.path()), "no log, nothing to point out");
        append(&dir.path().join("crash.log"), "t", "boom");
        assert!(take_unseen(dir.path()));
        assert!(!take_unseen(dir.path()));
        append(&dir.path().join("crash.log"), "t", "again");
        assert!(take_unseen(dir.path()));
    }
    /// Installs the real hook and watchdog, so it lives alone in its process: run it with
    /// `cargo test crashlog -- --ignored`. It takes about seven seconds.
    #[test]
    #[ignore]
    fn a_panic_and_a_frozen_frame_reach_the_log() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path().to_path_buf());
        note("importing big.json");
        {
            let _frame = Frame::begin();
            std::thread::sleep(Duration::from_secs(7));
        }
        let _ = std::thread::spawn(|| panic!("boom")).join();
        let log = std::fs::read_to_string(dir.path().join("crash.log")).unwrap();
        assert!(log.contains("NOT RESPONDING"), "{log}");
        assert!(log.contains("RESPONDING AGAIN"), "{log}");
        assert!(log.contains("PANIC at") && log.contains("boom"), "{log}");
        assert_eq!(
            log.matches("last step: importing big.json").count(),
            3,
            "{log}"
        );
        eprintln!("{log}");
    }
    #[test]
    fn the_last_step_is_kept_for_the_log() {
        note("importing a.json");
        assert_eq!(step(), "importing a.json");
        note(String::from("creating project"));
        assert_eq!(step(), "creating project");
    }
}
