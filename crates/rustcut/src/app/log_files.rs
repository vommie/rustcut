//! The log folder's housekeeping (0.0.67, user review): the daily files of earlier days are packed (`.log.gz`,
//! `flate2`), files older than `advanced.log_keep_days` are deleted, and while the folder holds more than
//! `advanced.log_max_mb` the oldest files go first. The log and its shareable copy are one budget. Today's files and
//! files written in the last minutes (the writer may still hold them) are never touched.
//!
//! RustCut does this itself instead of `tracing-appender`'s `max_log_files`: that one matched `rustcut.*` and so also
//! counted (and deleted) the shareable copy's files, and it neither packs nor knows a size. Runs on its own thread
//! (`rustcut-logs`) at start, after a settings change and every hour.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use chrono::NaiveDate;

/// The file families: `rustcut.DATE.log`, `rustcut-shareable.DATE.log` (and `.gz` once packed).
const PREFIXES: [&str; 2] = ["rustcut", "rustcut-shareable"];
/// A file written this recently is still in use.
const IN_USE: Duration = Duration::from_mins(10);
const EVERY: Duration = Duration::from_hours(1);

/// How much of the log folder stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub keep_days: u16,
    pub max_bytes: u64,
}

impl Retention {
    pub fn new(keep_days: u16, max_mb: u32) -> Self {
        Self {
            keep_days: keep_days.max(1),
            max_bytes: u64::from(max_mb.max(1)).saturating_mul(1024 * 1024),
        }
    }
}

/// One file of the log folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFile {
    pub name: String,
    pub date: NaiveDate,
    pub packed: bool,
    pub size: u64,
    /// Written in the last minutes (or today): never packed or deleted.
    pub in_use: bool,
}

/// What housekeeping does with a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Pack(String),
    Delete(String),
}

/// The date of a log file's name (`rustcut.2026-10-23.log`, `rustcut-shareable.2026-10-23.log.gz`) and whether it is
/// packed; other files are not the log's.
pub fn parse(name: &str) -> Option<(NaiveDate, bool)> {
    let (rest, packed) = name.strip_suffix(".gz").map_or((name, false), |r| (r, true));
    let rest = rest.strip_suffix(".log")?;
    let (prefix, date) = rest.rsplit_once('.')?;
    if !PREFIXES.contains(&prefix) {
        return None;
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok().map(|d| (d, packed))
}

/// Files too old are deleted, the other files of earlier days packed.
pub fn plan_age(files: &[LogFile], today: NaiveDate, retention: Retention) -> Vec<Step> {
    let first_kept = today
        .checked_sub_days(chrono::Days::new(u64::from(retention.keep_days.saturating_sub(1))))
        .unwrap_or(today);
    files
        .iter()
        .filter(|f| !f.in_use)
        .filter_map(|f| {
            if f.date < first_kept {
                Some(Step::Delete(f.name.clone()))
            } else if !f.packed && f.date < today {
                Some(Step::Pack(f.name.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// While the folder holds more than allowed, the oldest files that are not in use are deleted.
pub fn plan_size(files: &[LogFile], max_bytes: u64) -> Vec<Step> {
    let mut total: u64 = files.iter().map(|f| f.size).sum();
    let mut oldest: Vec<&LogFile> = files.iter().filter(|f| !f.in_use).collect();
    oldest.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.name.cmp(&b.name)));
    let mut steps = Vec::new();
    for file in oldest {
        if total <= max_bytes {
            break;
        }
        total = total.saturating_sub(file.size);
        steps.push(Step::Delete(file.name.clone()));
    }
    steps
}

/// The log files in `dir`; `today` is the date the writer uses (UTC, like `tracing-appender`).
fn scan(dir: &Path, today: NaiveDate) -> Vec<LogFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let now = SystemTime::now();
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (date, packed) = parse(&name)?;
            let meta = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let recent = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_none_or(|age| age < IN_USE);
            Some(LogFile {
                name,
                date,
                packed,
                size: meta.len(),
                in_use: date >= today || (recent && !packed),
            })
        })
        .collect()
}

/// Packs and deletes in `dir` as `retention` says; problems are logged, never fatal.
pub fn tidy(dir: &Path, retention: Retention) {
    let today = chrono::Utc::now().date_naive();
    run(dir, &plan_age(&scan(dir, today), today, retention));
    run(dir, &plan_size(&scan(dir, today), retention.max_bytes));
}

fn run(dir: &Path, steps: &[Step]) {
    for step in steps {
        let result = match step {
            Step::Pack(name) => pack(&dir.join(name)),
            Step::Delete(name) => std::fs::remove_file(dir.join(name)),
        };
        match result {
            Ok(()) => tracing::debug!(?step, "[App] Log folder tidied"),
            Err(e) => tracing::warn!(?step, error = %e, "[App] A log file could not be tidied"),
        }
    }
}

/// `name.log` -> `name.log.gz` (written beside it first, the original removed once the packed file is complete).
fn pack(path: &Path) -> std::io::Result<()> {
    let mut packed_name = path.as_os_str().to_owned();
    packed_name.push(".gz");
    let target = PathBuf::from(packed_name);
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let write = || -> std::io::Result<()> {
        let mut source = std::fs::File::open(path)?;
        let out = std::fs::File::create(&part)?;
        let mut encoder = flate2::write::GzEncoder::new(out, flate2::Compression::default());
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let n = source.read(&mut buffer)?;
            let Some(chunk) = buffer.get(..n).filter(|c| !c.is_empty()) else {
                break;
            };
            encoder.write_all(chunk)?;
        }
        encoder.finish()?.sync_all()?;
        std::fs::rename(&part, &target)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::remove_file(path)
}

/// The housekeeping thread; a newer `start` ends the older one's loop.
#[derive(Clone, Default)]
pub struct Keeper {
    generation: Arc<AtomicU64>,
}

impl Keeper {
    /// Tidies `dir` now and every hour with `retention`, on the `rustcut-logs` thread.
    pub fn start(&self, dir: &Path, retention: Retention) {
        let mine = self.generation.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        let generation = Arc::clone(&self.generation);
        let dir = dir.to_path_buf();
        let spawned = std::thread::Builder::new().name("rustcut-logs".into()).spawn(move || {
            while generation.load(Ordering::SeqCst) == mine {
                tidy(&dir, retention);
                std::thread::park_timeout(EVERY);
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[App] The log folder is not tidied (no thread)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn file(name: &str, size: u64, in_use: bool) -> LogFile {
        let (date, packed) = parse(name).unwrap();
        LogFile {
            name: name.into(),
            date,
            packed,
            size,
            in_use,
        }
    }

    #[test]
    fn names_of_both_families_are_parsed_and_others_ignored() {
        assert_eq!(parse("rustcut.2026-10-23.log"), Some((day(23), false)));
        assert_eq!(parse("rustcut-shareable.2026-10-23.log.gz"), Some((day(23), true)));
        assert_eq!(parse("rustcut.log"), None);
        assert_eq!(parse("rustcut-other.2026-10-23.log"), None);
        assert_eq!(parse("rustcut.2026-13-01.log"), None);
        assert_eq!(parse("notes.txt"), None);
    }

    #[test]
    fn earlier_days_are_packed_and_old_ones_deleted_in_both_families() {
        let files = [
            file("rustcut.2026-10-27.log", 10, true),
            file("rustcut.2026-10-26.log", 10, false),
            file("rustcut-shareable.2026-10-26.log", 10, false),
            file("rustcut.2026-10-25.log.gz", 1, false),
            file("rustcut.2026-10-20.log.gz", 1, false),
            file("rustcut-shareable.2026-10-13.log", 10, false),
        ];
        let steps = plan_age(&files, day(27), Retention::new(7, 100));
        assert_eq!(
            steps,
            [
                Step::Pack("rustcut.2026-10-26.log".into()),
                Step::Pack("rustcut-shareable.2026-10-26.log".into()),
                Step::Delete("rustcut.2026-10-20.log.gz".into()),
                Step::Delete("rustcut-shareable.2026-10-13.log".into()),
            ]
        );
        // One day kept: only today's files stay.
        let steps = plan_age(&files, day(27), Retention::new(1, 100));
        assert_eq!(steps.iter().filter(|s| matches!(s, Step::Delete(_))).count(), 5);
    }

    #[test]
    fn the_size_limit_deletes_the_oldest_and_never_files_in_use() {
        let files = [
            file("rustcut.2026-10-27.log", 60, true),
            file("rustcut.2026-10-26.log.gz", 20, false),
            file("rustcut-shareable.2026-10-25.log.gz", 20, false),
            file("rustcut.2026-10-24.log.gz", 20, false),
        ];
        assert!(plan_size(&files, 200).is_empty());
        assert_eq!(
            plan_size(&files, 90),
            [
                Step::Delete("rustcut.2026-10-24.log.gz".into()),
                Step::Delete("rustcut-shareable.2026-10-25.log.gz".into()),
            ]
        );
        assert_eq!(
            plan_size(&files, 10).len(),
            3,
            "today's file stays even above the limit"
        );
        assert_eq!(Retention::new(0, 0), Retention::new(1, 1));
    }

    #[test]
    fn packing_replaces_the_file_with_a_complete_gzip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rustcut.2026-10-20.log");
        std::fs::write(&path, "line one\nline two\n".repeat(1000)).unwrap();
        pack(&path).unwrap();
        assert!(!path.exists());
        let packed = std::fs::File::open(dir.path().join("rustcut.2026-10-20.log.gz")).unwrap();
        let mut text = String::new();
        flate2::read::GzDecoder::new(packed).read_to_string(&mut text).unwrap();
        assert_eq!(text, "line one\nline two\n".repeat(1000));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no part file left");

        // tidy: a file of an earlier day is packed unless it was written just now (the writer may hold it).
        let old = dir.path().join("rustcut-shareable.2000-01-01.log");
        std::fs::write(&old, "x").unwrap();
        tidy(dir.path(), Retention::new(36_500, 100));
        assert!(old.exists(), "written just now");
        let day_ago = SystemTime::now() - Duration::from_hours(24);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(day_ago)
            .unwrap();
        tidy(dir.path(), Retention::new(36_500, 100));
        assert!(!old.exists() && dir.path().join("rustcut-shareable.2000-01-01.log.gz").exists());
        tidy(dir.path(), Retention::new(7, 100));
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "older than a week: deleted"
        );
    }
}
