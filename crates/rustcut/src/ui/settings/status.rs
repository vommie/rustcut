//! What a row shows beside its control (user review 2026-10-27): whether a program of the settings is found and runs
//! (ffmpeg, ffprobe, rustview: where it was found and its version), and whether RustView answers (the connection test,
//! a turning symbol while it runs). Pure words here; the bridge runs the checks on the runtime (`program_checks`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Weak;
use std::time::{Duration, Instant};

use rustcut_core::diagnostics::{Located, locate};
use rustcut_core::rustview::ConnectionState;
use rustcut_core::settings::Settings;

use super::SettingsBridge;
use super::page::Page;

/// The turning symbol stays at least this long, so a quick answer does not just flicker.
const MIN_BUSY: Duration = Duration::from_millis(500);

/// The state beside a row: its symbol (`index`) and the words under the row's description.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RowStatus {
    #[default]
    None,
    Ok(String),
    Busy(String),
    Problem(String),
}

impl RowStatus {
    /// The number the Slint side uses (0 none, 1 ok, 2 busy, 3 problem).
    pub const fn index(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Ok(_) => 1,
            Self::Busy(_) => 2,
            Self::Problem(_) => 3,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::None => "",
            Self::Ok(t) | Self::Busy(t) | Self::Problem(t) => t,
        }
    }
}

/// The key of the connection test's row.
pub const TEST_KEY: &str = "rustview.test";

/// A program row: its key, the name used when the field is empty, the flag that prints its version.
struct Program {
    key: &'static str,
    name: &'static str,
    flag: &'static str,
}

const PROGRAMS: [Program; 4] = [
    Program {
        key: "advanced.ffmpeg",
        name: "ffmpeg",
        flag: "-version",
    },
    Program {
        key: "advanced.ffprobe",
        name: "ffprobe",
        flag: "-version",
    },
    Program {
        key: "rustview.binary",
        name: "rustview",
        flag: "--version",
    },
    Program {
        key: "player.mpv",
        name: "mpv",
        flag: "--version",
    },
];

/// The program a row's value means: the typed path or command, else its name (found in PATH).
fn program(settings: &Settings, key: &str) -> Option<PathBuf> {
    let typed = match key {
        "advanced.ffmpeg" => settings.advanced.ffmpeg.clone(),
        "advanced.ffprobe" => settings.advanced.ffprobe.clone(),
        "player.mpv" => settings.player.mpv.clone(),
        // A switched-off RustView's row shows nothing.
        "rustview.binary" if settings.rustview.enabled => settings.rustview.binary.clone(),
        _ => return None,
    };
    let name = PROGRAMS.iter().find(|p| p.key == key)?.name;
    Some(typed.unwrap_or_else(|| PathBuf::from(name)))
}

/// A program to look at: the row, the program and the flag of its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramCheck {
    pub key: &'static str,
    pub program: PathBuf,
    pub flag: &'static str,
}

/// The rows' states and what was looked at last.
#[derive(Debug, Default)]
pub struct Board {
    states: BTreeMap<&'static str, RowStatus>,
    /// The program each row was checked with (a changed value is checked again).
    checked: BTreeMap<&'static str, PathBuf>,
    /// The connection test runs or just answered (its result stays until the window shows again).
    test: Option<RowStatus>,
}

impl Board {
    pub fn get(&self, key: &str) -> RowStatus {
        if key == TEST_KEY {
            return self.test.clone().unwrap_or_else(|| self.connection());
        }
        self.states.get(key).cloned().unwrap_or_default()
    }

    /// The programs whose rows show a value not checked yet (marked busy); a switched-off RustView's row shows nothing.
    pub fn due(&mut self, settings: &Settings) -> Vec<ProgramCheck> {
        let mut due = Vec::new();
        for p in &PROGRAMS {
            let Some(program) = program(settings, p.key) else {
                self.states.remove(p.key);
                self.checked.remove(p.key);
                continue;
            };
            if self.checked.get(p.key) == Some(&program) {
                continue;
            }
            self.checked.insert(p.key, program.clone());
            self.states
                .insert(p.key, RowStatus::Busy("Looking for the program...".into()));
            due.push(ProgramCheck {
                key: p.key,
                program,
                flag: p.flag,
            });
        }
        due
    }

    /// A program's check answered; `false` when the row's value changed meanwhile (its newer check will answer).
    pub fn located(&mut self, check: &ProgramCheck, located: &Located) -> bool {
        if self.checked.get(check.key) != Some(&check.program) {
            return false;
        }
        self.states.insert(check.key, program_status(check, located));
        true
    }

    /// Every program is looked at again (the window shows again: something may have been installed meanwhile).
    pub fn forget_programs(&mut self) {
        self.checked.clear();
    }

    pub fn testing(&self) -> bool {
        matches!(self.test, Some(RowStatus::Busy(_)))
    }

    pub fn test_started(&mut self) {
        self.test = Some(RowStatus::Busy("Asking RustView...".into()));
    }

    pub fn test_answered(&mut self, answer: Result<usize, String>) {
        self.test = Some(match answer {
            Ok(tags) => RowStatus::Ok(format!(
                "RustView answered just now ({tags} tags). Tags and ratings reach the library."
            )),
            Err(reason) => RowStatus::Problem(format!("RustView did not answer: {reason}")),
        });
    }

    /// The window shows again: the test row follows the link's state again.
    pub fn forget_test(&mut self) {
        if !self.testing() {
            self.test = None;
        }
    }

    /// The link's own state (shown while no test ran since the window showed).
    pub fn follow_connection(&mut self, state: &ConnectionState) {
        self.states.insert(TEST_KEY, connection_status(state));
    }

    fn connection(&self) -> RowStatus {
        self.states.get(TEST_KEY).cloned().unwrap_or_default()
    }
}

/// The words for a program that was looked at.
fn program_status(check: &ProgramCheck, located: &Located) -> RowStatus {
    let typed = check.program.components().count() > 1 || check.program.is_absolute();
    match (&located.path, &located.version) {
        (Some(_), Ok(version)) if typed => RowStatus::Ok(format!("Works: {version}")),
        (Some(path), Ok(version)) => RowStatus::Ok(format!("Found in PATH: {} ({version})", path.display())),
        (Some(path), Err(why)) => RowStatus::Problem(format!("{} does not run: {why}", path.display())),
        (None, Err(why) | Ok(why)) => RowStatus::Problem(format!("{why}: install it, or choose its file here.")),
    }
}

/// The link's state in words (the test row before a test).
pub fn connection_status(state: &ConnectionState) -> RowStatus {
    match state {
        ConnectionState::Connected => RowStatus::Ok("Connected: RustView answers.".into()),
        ConnectionState::Unknown | ConnectionState::Disabled => RowStatus::None,
        ConnectionState::Unavailable { reason, retry_in_secs } => RowStatus::Problem(if *retry_in_secs > 0 {
            format!("Not reachable: {reason}. Tried again in {retry_in_secs} s.")
        } else {
            format!("Not reachable: {reason}.")
        }),
    }
}

impl SettingsBridge {
    /// Looks at the programs of the page shown (RustView, Advanced & Logs) whose value was not checked yet, on the
    /// runtime; the rows say "busy" until the answers come.
    pub(super) fn check_programs(&self, page: Page, settings: &Settings) {
        if !matches!(page, Page::Player | Page::RustView | Page::Advanced) {
            return;
        }
        if let Some(connection) = self.hooks.borrow().rustview_connection.as_ref() {
            self.status.borrow_mut().follow_connection(&connection());
        }
        let due = self.status.borrow_mut().due(settings);
        for check in due {
            let task = {
                let (program, flag) = (check.program.clone(), check.flag);
                self.runtime.spawn(async move { locate(&program, flag).await })
            };
            let me = Weak::clone(&self.me);
            let spawned = slint::spawn_local(async move {
                let Ok(located) = task.await else { return };
                let Some(b) = me.upgrade() else { return };
                tracing::debug!(option = check.key, found = ?located.path, version = ?located.version,
                    "[Settings] Program looked at");
                if b.status.borrow_mut().located(&check, &located) {
                    b.publish();
                }
            });
            if let Err(e) = spawned {
                tracing::error!(error = %e, "[Settings] The event loop is gone");
            }
        }
    }

    /// The window shows again: programs are looked at again, the test row follows the link again.
    pub(super) fn status_refresh(&self) {
        let mut board = self.status.borrow_mut();
        board.forget_programs();
        board.forget_test();
    }

    /// "Test the connection": RustView is asked for its tags now; the row turns until it answers.
    pub(super) fn test_rustview(&self) {
        if self.status.borrow().testing() {
            return;
        }
        let Some(answer) = self.hooks.borrow().test_rustview.as_ref().and_then(|test| test()) else {
            return;
        };
        tracing::info!("[Settings] RustView connection test");
        self.status.borrow_mut().test_started();
        self.publish();
        let started = Instant::now();
        let me = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let answer = match answer.await {
                Ok(Ok(tree)) => Ok(tree.tags.len()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_gone) => Err("the RustView link has stopped".to_owned()),
            };
            tracing::info!(?answer, "[Settings] RustView connection test answered");
            slint::Timer::single_shot(MIN_BUSY.saturating_sub(started.elapsed()), move || {
                if let Some(b) = me.upgrade() {
                    b.status.borrow_mut().test_answered(answer);
                    b.publish();
                }
            });
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(path: Option<&str>, version: Result<&str, &str>) -> Located {
        Located {
            path: path.map(PathBuf::from),
            version: version.map(str::to_owned).map_err(str::to_owned),
        }
    }

    #[test]
    fn programs_are_checked_once_per_value_and_say_where_they_are() {
        let mut board = Board::default();
        let mut s = Settings::default();
        let due = board.due(&s);
        assert_eq!(due.len(), 4, "{due:?}");
        assert!(board.due(&s).is_empty(), "checked once per value");
        assert_eq!(board.get("advanced.ffmpeg").index(), 2);
        let ffmpeg = due.first().unwrap().clone();
        assert!(board.located(&ffmpeg, &located(Some("/usr/bin/ffmpeg"), Ok("ffmpeg version 9.0.1"))));
        assert_eq!(
            board.get("advanced.ffmpeg"),
            RowStatus::Ok("Found in PATH: /usr/bin/ffmpeg (ffmpeg version 9.0.1)".into())
        );

        // A typed path is checked again; the old answer is dropped.
        s.advanced.ffmpeg = Some("/opt/ffmpeg".into());
        let again = board.due(&s);
        assert_eq!(again.len(), 1);
        assert!(!board.located(&ffmpeg, &located(Some("/usr/bin/ffmpeg"), Ok("x"))));
        let typed = again.first().unwrap();
        assert!(board.located(typed, &located(None, Err("/opt/ffmpeg does not exist"))));
        assert_eq!(board.get("advanced.ffmpeg").index(), 3);
        assert!(board.get("advanced.ffmpeg").text().contains("choose its file"));

        // A switched-off RustView's program row shows nothing.
        s.rustview.enabled = false;
        assert!(board.due(&s).is_empty());
        assert_eq!(board.get("rustview.binary"), RowStatus::None);
    }

    #[test]
    fn the_test_row_follows_the_link_until_a_test_answers() {
        let mut board = Board::default();
        assert_eq!(board.get(TEST_KEY), RowStatus::None);
        board.follow_connection(&ConnectionState::Connected);
        assert_eq!(
            board.get(TEST_KEY).index(),
            1,
            "connected: the check mark before any test"
        );
        board.test_started();
        assert!(board.testing());
        assert_eq!(board.get(TEST_KEY).index(), 2);
        board.forget_test();
        assert!(board.testing(), "a running test stays");
        board.test_answered(Err("not installed".into()));
        assert_eq!(board.get(TEST_KEY).index(), 3);
        board.forget_test();
        assert_eq!(board.get(TEST_KEY).index(), 1, "the link's state again");
        let late = connection_status(&ConnectionState::Unavailable {
            reason: "not installed".into(),
            retry_in_secs: 8,
        });
        assert_eq!(late.text(), "Not reachable: not installed. Tried again in 8 s.");
    }
}
