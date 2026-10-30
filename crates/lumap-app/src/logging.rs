//! Where lumap's log lines go: stderr (as a terminal shows them), a file per run under the
//! app's local data folder (`%LOCALAPPDATA%\lumap\logs` on Windows), and a ring of recent lines
//! that the lab harness serves at `GET /lab/logs`. A panic is logged with its backtrace before
//! the app goes down, so a crash leaves its reason in the file.
//!
//! The level per module comes from `LUMAP_LOG` in the usual `env_logger` form,
//! for example `info,lumap::editor::scan=trace`. Without either, lumap's own modules log at
//! debug and everything else at info, with the chatty GPU crates at warn.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Log files kept: the newest ones, one per run.
pub const KEEP_FILES: usize = 20;
/// Recent lines kept in memory for the lab.
const RING: usize = 5000;
/// What a run logs when nothing says otherwise.
const DEFAULT_FILTER: &str = "info,lumap=debug,lumap_app=debug,lumap_core=debug,\
    lumap_sources=debug,lumap_vision=debug,lumap_render=debug,lumap_control=debug,\
    wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn";

static RECENT: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static FILE: OnceLock<PathBuf> = OnceLock::new();

/// The folder log files go in.
pub fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("lumap").join("logs"))
}

/// This run's log file, once logging started and the file could be made.
pub fn current_file() -> Option<&'static Path> {
    FILE.get().map(PathBuf::as_path)
}

/// Starts logging for this run: the filter, the file (old ones pruned) and the panic hook.
/// Returns the file's path. Safe to call once; later calls change nothing.
pub fn init() -> Option<PathBuf> {
    // Use a lumap-specific switch. A machine-wide RUST_LOG (often `warn` for development
    // tools) must not silently turn off the diagnostic history users rely on after a failure.
    let spec = std::env::var("LUMAP_LOG").unwrap_or_else(|_| DEFAULT_FILTER.to_string());
    let filter = Filter::parse(&spec);
    let max = filter.max();
    let (file, path) = match dir().and_then(|d| open_file(&d)) {
        Some((f, p)) => (Some(f), Some(p)),
        None => (None, None),
    };
    let logger = Logger {
        filter,
        file: Mutex::new(file.map(BufWriter::new)),
    };
    if log::set_boxed_logger(Box::new(logger)).is_err() {
        return None;
    }
    log::set_max_level(max);
    if let Some(p) = &path {
        let _ = FILE.set(p.clone());
    }
    install_panic_hook();
    path
}

/// The most recent `lines` lines at `level` or more severe, containing `contains` when given
/// (case-insensitive), oldest first.
pub fn recent(lines: usize, level: Level, contains: Option<&str>) -> Vec<String> {
    let needle = contains.map(str::to_lowercase);
    let Some(ring) = RECENT.get() else {
        return Vec::new();
    };
    let ring = ring
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out: Vec<String> = ring
        .iter()
        .rev()
        .filter(|l| line_level(l).is_some_and(|lv| lv <= level))
        .filter(|l| needle.as_ref().is_none_or(|n| l.to_lowercase().contains(n)))
        .take(lines)
        .cloned()
        .collect();
    out.reverse();
    out
}

/// A line's level, read back from its text.
fn line_level(line: &str) -> Option<Level> {
    let word = line.split_whitespace().nth(1)?;
    word.parse().ok()
}

fn open_file(dir: &Path) -> Option<(File, PathBuf)> {
    std::fs::create_dir_all(dir).ok()?;
    prune(dir, KEEP_FILES.saturating_sub(1));
    let stamp = timestamp(SystemTime::now())
        .replace([':', '-'], "")
        .replace('T', "-");
    let stamp = stamp.split('.').next().unwrap_or_default().to_string();
    let path = dir.join(format!("lumap-{stamp}-{}.log", std::process::id()));
    File::create(&path).ok().map(|f| (f, path))
}

/// Deletes the oldest `lumap-*.log` files so at most `keep` remain.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("lumap-"))
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("log"))
        })
        .collect();
    // Names start with the time, so name order is age order.
    logs.sort();
    let extra = logs.len().saturating_sub(keep);
    for old in logs.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        log::error!(
            target: "lumap::panic",
            "thread '{}' panicked: {info}\n{backtrace}",
            thread.name().unwrap_or("unnamed")
        );
        log::logger().flush();
        previous(info);
    }));
}

struct Logger {
    filter: Filter,
    file: Mutex<Option<BufWriter<File>>>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    #[allow(clippy::print_stderr)] // A terminal mirror is one of this logger's deliberate sinks.
    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let thread = std::thread::current();
        let line = format!(
            "{} {:<5} [{}] {}: {}",
            timestamp(SystemTime::now()),
            record.level(),
            thread.name().unwrap_or("-"),
            record.target(),
            record.args()
        );
        eprintln!("{line}");
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = writeln!(f, "{line}");
                // Every line reaches the disk: the line before a crash is the one that matters.
                let _ = f.flush();
            }
        }
        let ring = RECENT.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING)));
        let mut ring = ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ring.len() == RING {
            ring.pop_front();
        }
        ring.push_back(line);
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

/// Levels per module, `env_logger` style: `info,lumap=debug,wgpu=warn`. The longest matching
/// module prefix wins; a bare level is the default.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    default: LevelFilter,
    rules: Vec<(String, LevelFilter)>,
}

impl Filter {
    /// Reads a filter; parts that make no sense are skipped.
    pub fn parse(spec: &str) -> Self {
        let mut default = LevelFilter::Info;
        let mut rules = Vec::new();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((module, level)) => {
                    if let Ok(level) = level.trim().parse() {
                        rules.push((module.trim().to_string(), level));
                    }
                }
                None => {
                    if let Ok(level) = part.parse() {
                        default = level;
                    } else {
                        // A bare module name means everything from it.
                        rules.push((part.to_string(), LevelFilter::Trace));
                    }
                }
            }
        }
        // Longest first, so the most specific rule is found first.
        rules.sort_by_key(|(m, _)| std::cmp::Reverse(m.len()));
        Self { default, rules }
    }

    /// The level allowed for a log target.
    pub fn level_for(&self, target: &str) -> LevelFilter {
        self.rules
            .iter()
            .find(|(m, _)| {
                target == m
                    || target
                        .strip_prefix(m.as_str())
                        .is_some_and(|rest| rest.starts_with("::"))
            })
            .map_or(self.default, |(_, l)| *l)
    }

    /// The most verbose level any target may log at.
    pub fn max(&self) -> LevelFilter {
        self.rules
            .iter()
            .map(|(_, l)| *l)
            .fold(self.default, std::cmp::max)
    }
}

/// UTC time as `2026-10-30T22:10:41.123Z`.
pub fn timestamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, day) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        d.subsec_millis()
    )
}

/// Year, month and day of a count of days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn filters_pick_the_most_specific_module() {
        let f = Filter::parse("warn, lumap=debug, lumap::editor::scan=trace, wgpu=error, junk=x");
        assert_eq!(f.level_for("other"), LevelFilter::Warn);
        assert_eq!(f.level_for("lumap"), LevelFilter::Debug);
        assert_eq!(f.level_for("lumap::editor"), LevelFilter::Debug);
        assert_eq!(
            f.level_for("lumap::editor::scan::session"),
            LevelFilter::Trace
        );
        assert_eq!(
            f.level_for("lumap_vision"),
            LevelFilter::Warn,
            "a prefix, not a module"
        );
        assert_eq!(f.level_for("wgpu"), LevelFilter::Error);
        assert_eq!(f.max(), LevelFilter::Trace);
        let bare = Filter::parse("lumap");
        assert_eq!(bare.level_for("lumap::x"), LevelFilter::Trace);
        assert_eq!(bare.level_for("x"), LevelFilter::Info);
        let default = Filter::parse(DEFAULT_FILTER);
        assert_eq!(default.level_for("lumap::editor::scan"), LevelFilter::Debug);
        assert_eq!(default.level_for("wgpu_core::device"), LevelFilter::Warn);
        assert_eq!(default.level_for("eframe"), LevelFilter::Info);
    }

    #[test]
    fn timestamps_are_utc_iso() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let t = UNIX_EPOCH + Duration::from_millis(1_793_398_970_123);
        assert_eq!(timestamp(t), "2026-10-30T22:22:50.123Z");
        let leap = UNIX_EPOCH + Duration::from_hours(264_384); // 2000-02-29
        assert!(timestamp(leap).starts_with("2000-02-29T"));
    }

    #[test]
    fn levels_are_read_back_from_lines() {
        assert_eq!(
            line_level("2026-10-30T22:22:50.123Z DEBUG [main] lumap: x"),
            Some(Level::Debug)
        );
        assert_eq!(
            line_level("2026-10-30T22:22:50.123Z WARN  [main] x: y"),
            Some(Level::Warn)
        );
        assert_eq!(line_level("garbage"), None);
    }

    #[test]
    fn pruning_keeps_the_newest_logs_only() {
        let dir = std::env::temp_dir().join(format!("lumap-log-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_default();
        for name in [
            "lumap-20261001-100000-1.log",
            "lumap-20261002-100000-1.log",
            "lumap-20261003-100000-1.log",
            "notes.txt",
        ] {
            std::fs::write(dir.join(name), "x").unwrap_or_default();
        }
        prune(&dir, 2);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        left.sort();
        assert_eq!(
            left,
            [
                "lumap-20261002-100000-1.log",
                "lumap-20261003-100000-1.log",
                "notes.txt"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn initialized_logger_writes_and_keeps_recent_lines() {
        let Some(path) = init() else {
            panic!("a run log should be available");
        };
        log::error!(target: "lumap::logging::test", "logger smoke test");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        assert!(text.contains("logger smoke test"), "{text:?}");
        assert!(recent(10, Level::Trace, Some("smoke test"))
            .iter()
            .any(|line| line.contains("logger smoke test")));
    }
}
