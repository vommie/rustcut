//! The move dialog without Slint (P12 block 7, user decision Q-P12f): from the chosen folder (and, for one job, the name)
//! and what the runtime found out about the drives, it decides what each job does, every check the dialog shows, the
//! conflicts the user decides, and the moves the queue gets. Nothing runs before the user decided everything.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::naming::{FolderScan, QueuedTarget, next_free_variant, same_path, sanitize_stem};
use rustcut_core::queue::TargetMove;

/// One selected job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: JobId,
    /// The target's name without extension, and the extension.
    pub stem: String,
    pub extension: String,
    pub state: JobStateKind,
    pub target: PathBuf,
    pub source: PathBuf,
    /// Its file is being moved already (or waits behind other moves).
    pub moving: bool,
    /// Its spec cannot be read by this RustCut.
    pub unreadable: bool,
}

impl Job {
    fn file(&self, stem: &str) -> String {
        format!("{stem}.{}", self.extension)
    }
}

/// Where the jobs go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Folder {
    /// Nothing chosen yet.
    None,
    /// Next to each job's source video.
    Source,
    Path(PathBuf),
}

/// What the runtime found out (filled in after every change; missing entries count as not checked yet).
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub scans: HashMap<PathBuf, FolderScan>,
    /// A folder that cannot hold the files: why (it is a file, it cannot be created, no permission).
    pub unusable: HashMap<PathBuf, String>,
    /// Free bytes on the drive of a folder.
    pub free: HashMap<PathBuf, u64>,
    /// A finished job's file: its size and whether it goes to another drive (a copy); `None` = not at its target.
    pub files: HashMap<JobId, Option<(u64, bool)>>,
}

/// How a conflict is solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Resolution {
    #[default]
    AddNumber,
    Skip,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub level: Level,
    pub text: String,
}

/// A job whose new name is taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub id: JobId,
    pub file: String,
    /// The name "Add (n)" gives it.
    pub numbered: String,
    /// An existing file (can be overwritten), not a queued job's target or another selected job's name.
    pub file_exists: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub moves: Vec<TargetMove>,
    pub checks: Vec<Check>,
    pub conflicts: Vec<Conflict>,
    /// Files that move (finished jobs), of them copies to another drive, and their bytes.
    pub files: usize,
    pub copies: usize,
    pub copy_bytes: u64,
    /// The drives were not checked yet for the current choice.
    pub pending: bool,
}

impl Plan {
    /// Move can be pressed.
    pub fn ready(&self) -> bool {
        !self.pending && !self.moves.is_empty() && !self.checks.iter().any(|c| c.level == Level::Error)
    }
}

/// The folder a job goes to.
pub fn destination(job: &Job, folder: &Folder) -> Option<PathBuf> {
    match folder {
        Folder::None => None,
        Folder::Source => job.source.parent().map(Path::to_path_buf),
        Folder::Path(path) => Some(path.clone()),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn size_text(bytes: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a size shown with one decimal")]
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} kB", (b / 1e3).ceil())
    }
}

/// Decides everything for the jobs. `name` is the typed name of a single job (`None` for several).
pub fn plan(
    jobs: &[Job],
    folder: &Folder,
    name: Option<&str>,
    queue: &[QueuedTarget],
    facts: &Facts,
    resolution: Resolution,
) -> Plan {
    let mut plan = Plan::default();
    let error = |plan: &mut Plan, text: String| {
        plan.checks.push(Check {
            level: Level::Error,
            text,
        });
    };
    if let Folder::Path(path) = folder
        && !path.is_absolute()
    {
        error(&mut plan, "Type a whole path; it starts with /".into());
        return plan;
    }
    if *folder == Folder::None {
        plan.pending = true;
        return plan;
    }
    let typed = match name.map(sanitize_stem) {
        None => None,
        Some(Ok(stem)) => {
            if name.is_some_and(|n| n.trim() != stem) {
                plan.checks.push(Check {
                    level: Level::Info,
                    text: format!("The name becomes \"{stem}\" (a file name cannot hold / or control characters)"),
                });
            }
            Some(stem)
        }
        Some(Err(e)) => {
            error(&mut plan, format!("The name cannot be used: {e}"));
            return plan;
        }
    };
    let moved: HashSet<JobId> = jobs.iter().map(|j| j.id).collect();
    // Names the batch gives out, per folder.
    let mut given: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut unchanged = 0;
    let (mut targets, mut after_render) = (0, 0);
    let mut copy_per_folder: HashMap<PathBuf, u64> = HashMap::new();
    let mut checked_folders: HashSet<PathBuf> = HashSet::new();
    for job in jobs {
        let label = job.file(&job.stem);
        if job.unreadable {
            plan.checks.push(Check {
                level: Level::Warning,
                text: format!("{label} is left out: another RustCut version wrote it"),
            });
            continue;
        }
        if job.moving {
            plan.checks.push(Check {
                level: Level::Warning,
                text: format!("{label} is left out: its file is being moved already"),
            });
            continue;
        }
        let Some(dest) = destination(job, folder) else { continue };
        let stem = typed.clone().unwrap_or_else(|| job.stem.clone());
        let mut file = job.file(&stem);
        let mut to = dest.join(&file);
        if same_path(&to, &job.target) {
            unchanged += 1;
            continue;
        }
        if same_path(&to, &job.source) {
            error(&mut plan, format!("{file} would replace the job's own source video"));
            continue;
        }
        if checked_folders.insert(dest.clone())
            && let Some(why) = facts.unusable.get(&dest)
        {
            error(&mut plan, format!("{} cannot hold the files: {why}", dest.display()));
        }
        let finished = job.state == JobStateKind::Finished;
        let mut copy = None;
        if finished {
            match facts.files.get(&job.id) {
                None => plan.pending = true,
                Some(None) => {
                    plan.checks.push(Check {
                        level: Level::Warning,
                        text: format!(
                            "{label} is left out: the file is not at {} any more",
                            job.target.display()
                        ),
                    });
                    continue;
                }
                Some(Some((size, across))) => copy = Some((*size, *across)),
            }
        }
        let Some(scan) = facts.scans.get(&dest) else {
            plan.pending = true;
            continue;
        };
        let names = given.entry(dest.clone()).or_default();
        let queued = |file: &str| {
            let path = dest.join(file);
            queue
                .iter()
                .any(|q| !moved.contains(&q.id) && q.pending() && same_path(&q.target, &path))
        };
        // The job's own file at its old place does not stand in its way (a new name in the same folder).
        let exists = |file: &str| scan.names.contains(file) && !same_path(&dest.join(file), &job.target);
        let taken = |file: &str, names: &HashSet<String>| exists(file) || queued(file) || names.contains(file);
        let mut overwrite = false;
        if taken(&file, names) {
            let file_exists = exists(&file) && !queued(&file) && !names.contains(&file);
            let numbered = next_free_variant(&stem, |s| taken(&job.file(s), names)).map(|s| job.file(&s));
            plan.conflicts.push(Conflict {
                id: job.id,
                file: file.clone(),
                numbered: numbered.clone().unwrap_or_default(),
                file_exists,
            });
            match (resolution, numbered) {
                (Resolution::Skip, _) => continue,
                (Resolution::Overwrite, _) if file_exists => overwrite = true,
                (_, Some(numbered)) => {
                    file = numbered;
                    to = dest.join(&file);
                }
                (_, None) => {
                    error(&mut plan, format!("No free name for {file} in {}", dest.display()));
                    continue;
                }
            }
        }
        names.insert(file.clone());
        let new_stem = file
            .strip_suffix(&format!(".{}", job.extension))
            .unwrap_or(&file)
            .to_owned();
        if let Some((size, across)) = copy {
            plan.files += 1;
            if across {
                plan.copies += 1;
                plan.copy_bytes = plan.copy_bytes.saturating_add(size);
                *copy_per_folder.entry(dest.clone()).or_default() += size;
            }
        } else if job.state.is_active() {
            after_render += 1;
        } else {
            targets += 1;
        }
        plan.moves.push(TargetMove {
            id: job.id,
            folder: to.parent().map(Path::to_path_buf).unwrap_or(dest),
            stem: new_stem,
            overwrite,
        });
    }
    for (folder, bytes) in &copy_per_folder {
        if let Some(free) = facts.free.get(folder)
            && *free < bytes.saturating_add(bytes / 100)
        {
            error(
                &mut plan,
                format!(
                    "Not enough space on the drive of {}: {} needed, {} free",
                    folder.display(),
                    size_text(*bytes),
                    size_text(*free)
                ),
            );
        }
    }
    let mut info = Vec::new();
    if plan.files > 0 {
        let mut text = format!("Moves {}", plural(plan.files, "file", "files"));
        if plan.copies > 0 {
            let _ = write!(
                text,
                "; {} to another drive ({}): copied, read back and compared before the original is deleted",
                plan.copies,
                size_text(plan.copy_bytes)
            );
        }
        info.push(text);
    }
    if targets > 0 {
        info.push(format!(
            "Changes the target of {} still to render",
            plural(targets, "job", "jobs")
        ));
    }
    if after_render > 0 {
        info.push(format!(
            "{} now: the file moves once the render is done",
            plural(
                after_render,
                "rendering job changes its target",
                "rendering jobs change their targets"
            )
        ));
    }
    if unchanged > 0 {
        info.push(format!("{} there already", plural(unchanged, "job is", "jobs are")));
    }
    plan.checks.extend(info.into_iter().map(|text| Check {
        level: Level::Info,
        text,
    }));
    plan.checks.sort_by_key(|c| std::cmp::Reverse(c.level));
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: i64, stem: &str, state: JobStateKind) -> Job {
        Job {
            id: JobId(id),
            stem: stem.into(),
            extension: "mkv".into(),
            state,
            target: PathBuf::from(format!("/out/{stem}.mkv")),
            source: PathBuf::from(format!("/in/{stem}.mp4")),
            moving: false,
            unreadable: false,
        }
    }

    fn facts(folder: &str, names: &[&str]) -> Facts {
        let mut facts = Facts::default();
        facts.scans.insert(
            folder.into(),
            FolderScan {
                readable: true,
                missing: false,
                names: names.iter().map(|n| (*n).to_owned()).collect(),
            },
        );
        facts
    }

    fn to(path: &str) -> Folder {
        Folder::Path(path.into())
    }

    #[test]
    fn nothing_runs_before_the_drives_were_checked() {
        let jobs = [job(1, "a", JobStateKind::Finished)];
        let p = plan(&jobs, &to("/v"), None, &[], &Facts::default(), Resolution::AddNumber);
        assert!(p.pending && !p.ready());
        assert!(
            !plan(
                &jobs,
                &Folder::None,
                None,
                &[],
                &Facts::default(),
                Resolution::AddNumber
            )
            .ready()
        );
    }

    #[test]
    fn a_finished_file_moves_and_a_copy_is_counted_and_checked_for_space() {
        let jobs = [job(1, "a", JobStateKind::Finished), job(2, "b", JobStateKind::Waiting)];
        let mut f = facts("/v", &[]);
        f.files.insert(JobId(1), Some((5_000_000_000, true)));
        let p = plan(&jobs, &to("/v"), None, &[], &f, Resolution::AddNumber);
        assert!(p.ready(), "{p:?}");
        assert_eq!(
            (p.files, p.copies, p.copy_bytes, p.moves.len()),
            (1, 1, 5_000_000_000, 2)
        );
        assert_eq!(p.moves[0].folder, PathBuf::from("/v"));
        f.free.insert("/v".into(), 4_000_000_000);
        let full = plan(&jobs, &to("/v"), None, &[], &f, Resolution::AddNumber);
        assert!(!full.ready());
        assert!(full.checks[0].text.starts_with("Not enough space"), "{:?}", full.checks);
    }

    #[test]
    fn conflicts_get_a_number_are_skipped_or_overwritten() {
        let jobs = [job(1, "a", JobStateKind::Finished)];
        let mut f = facts("/v", &["a.mkv", "a (1).mkv"]);
        f.files.insert(JobId(1), Some((10, false)));
        let numbered = plan(&jobs, &to("/v"), None, &[], &f, Resolution::AddNumber);
        assert_eq!(numbered.conflicts.len(), 1);
        assert!(numbered.conflicts[0].file_exists);
        assert_eq!(numbered.moves[0].stem, "a (2)");
        assert!(!numbered.moves[0].overwrite);
        let skipped = plan(&jobs, &to("/v"), None, &[], &f, Resolution::Skip);
        assert!(skipped.moves.is_empty() && !skipped.ready());
        let over = plan(&jobs, &to("/v"), None, &[], &f, Resolution::Overwrite);
        assert_eq!((over.moves[0].stem.as_str(), over.moves[0].overwrite), ("a", true));
    }

    #[test]
    fn two_selected_jobs_and_a_queued_target_never_share_a_name() {
        let mut b = job(2, "a", JobStateKind::Waiting);
        b.target = "/other/a.mkv".into();
        let jobs = [job(1, "a", JobStateKind::Waiting), b];
        let queue = [QueuedTarget {
            id: JobId(9),
            target: "/v/a (1).mkv".into(),
            state: JobStateKind::Waiting,
        }];
        let p = plan(&jobs, &to("/v"), None, &queue, &facts("/v", &[]), Resolution::Overwrite);
        let stems: Vec<&str> = p.moves.iter().map(|m| m.stem.as_str()).collect();
        assert_eq!(stems, ["a", "a (2)"], "a queued target is never overwritten");
        assert!(!p.conflicts[0].file_exists);
    }

    #[test]
    fn one_job_can_get_a_new_name_in_its_own_folder() {
        let jobs = [job(1, "a", JobStateKind::Finished)];
        let mut f = facts("/out", &["a.mkv"]);
        f.files.insert(JobId(1), Some((10, false)));
        let p = plan(&jobs, &to("/out"), Some(" Beach "), &[], &f, Resolution::AddNumber);
        assert_eq!(p.moves[0].stem, "Beach");
        assert!(p.conflicts.is_empty(), "its own file does not stand in its way");
        let same = plan(&jobs, &to("/out"), Some("a"), &[], &f, Resolution::AddNumber);
        assert!(same.moves.is_empty() && same.checks.iter().any(|c| c.text.contains("there already")));
        let bad = plan(&jobs, &to("/out"), Some("  "), &[], &f, Resolution::AddNumber);
        assert_eq!(bad.checks[0].level, Level::Error);
        let slash = plan(&jobs, &to("/out"), Some("x/y"), &[], &f, Resolution::AddNumber);
        assert_eq!(slash.moves[0].stem, "x_y");
    }

    #[test]
    fn missing_files_moving_jobs_and_bad_folders_are_reported() {
        let mut moving = job(2, "b", JobStateKind::Finished);
        moving.moving = true;
        let mut own = job(3, "c", JobStateKind::Waiting);
        own.source = "/v/c.mkv".into();
        let jobs = [job(1, "a", JobStateKind::Finished), moving, own];
        let mut f = facts("/v", &[]);
        f.files.insert(JobId(1), None);
        f.unusable.insert("/v".into(), "it is a file".into());
        let p = plan(&jobs, &to("/v"), None, &[], &f, Resolution::AddNumber);
        let texts: Vec<&str> = p.checks.iter().map(|c| c.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("not at /out/a.mkv any more")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|t| t.contains("being moved already")));
        assert!(texts.iter().any(|t| t.contains("own source video")));
        assert!(texts.iter().any(|t| t.contains("cannot hold the files")));
        assert!(!p.ready());
        assert!(!plan(&jobs, &to("relative"), None, &[], &f, Resolution::AddNumber).ready());
    }

    #[test]
    fn next_to_the_source_uses_each_jobs_own_folder() {
        let jobs = [job(1, "a", JobStateKind::Held)];
        let p = plan(
            &jobs,
            &Folder::Source,
            None,
            &[],
            &facts("/in", &[]),
            Resolution::AddNumber,
        );
        assert_eq!(p.moves[0].folder, PathBuf::from("/in"));
    }
}
