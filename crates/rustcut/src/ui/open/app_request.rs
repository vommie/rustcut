//! Requests of other programs over the single-instance socket (protocol 2, 0.0.120): RustSearch, which stays open next to
//! RustCut, asks for RustCut's state (`status`, every 2 s while its window shows) and opens videos (`app_open`).
//!
//! An `app_open` opens the file at once, like File > Open, and RustCut stays behind RustSearch. Only when the open video
//! has unsaved sections and `dialogs.ask_before_rustsearch_replaces` is on (the default) does RustCut ask first (keep,
//! discard and open, save and open) and come to the front for it. File > Open, drops and a second start never ask: opening
//! a file at once is RustCut's intended behavior; the question belongs to RustSearch's requests only.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use slint::ComponentHandle as _;
use slint::winit_030::WinitWindowAccessor as _;

use super::{FileOpener, file_name};
use crate::app::SettingsWriter;
use crate::desktop::instance::{ORIGINS, Outcome, PROTOCOL, Reply, Request, Responder, Status};
use crate::ui::editor::Editor;
use crate::ui::export::ExportBridge;
use crate::ui::player::PlayerBridge;
use crate::{AppOpen, MainWindow};

/// How often a "Save and open" looks whether its save ended.
const SAVE_POLL: Duration = Duration::from_millis(100);
/// A save that waits longer than this (its conflict question left open) no longer opens the file afterwards.
const SAVE_PATIENCE: Duration = Duration::from_mins(30);

thread_local! {
    static CURRENT: RefCell<Weak<AppRequests>> = const { RefCell::new(Weak::new()) };
}

/// What an `app_open` does, from RustCut's state when it arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Open it now (no unsaved sections, or the question is switched off).
    OpenNow,
    /// The file is open already: nothing to do (opening it again would only restart it).
    AlreadyOpen,
    /// Ask first.
    Ask,
    /// The question is open already: the newer file takes the older one's place in it.
    AskAgain,
    /// A dialog of RustCut's own is open or a job is being saved: refuse, nothing happens.
    Busy,
}

/// RustCut's state at the moment of a request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    /// `dialogs.ask_before_rustsearch_replaces`.
    pub ask_setting: bool,
    pub unsaved: bool,
    pub busy: bool,
    pub asking: bool,
    /// The file asked for is the open one.
    pub same_file: bool,
}

pub fn decide(facts: Facts) -> Decision {
    if facts.asking {
        Decision::AskAgain
    } else if facts.busy {
        Decision::Busy
    } else if facts.same_file {
        Decision::AlreadyOpen
    } else if facts.ask_setting && facts.unsaved {
        Decision::Ask
    } else {
        Decision::OpenNow
    }
}

/// The first file of an `app_open`, or why the request is refused.
pub fn requested_file(origin: &str, files: &[PathBuf]) -> Result<PathBuf, String> {
    if !ORIGINS.contains(&origin) {
        return Err(format!("unknown origin \"{origin}\" (known: {})", ORIGINS.join(", ")));
    }
    let first = files.first().ok_or_else(|| "no file".to_owned())?;
    if !first.is_absolute() {
        return Err(format!("not an absolute path: {}", first.display()));
    }
    Ok(first.clone())
}

/// The question's words.
pub fn question(new: &Path, current: Option<&Path>) -> (String, String) {
    let current = current.map_or_else(|| "the open video".to_owned(), |c| format!("\"{}\"", file_name(c)));
    (
        "Open the file from RustSearch?".to_owned(),
        format!(
            "RustSearch sends \"{}\". The sections of {current} are not saved as a job yet: keep them, discard them, or \
             save them first.",
            file_name(new)
        ),
    )
}

/// The user's answer to the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Keep,
    Discard,
    Save,
}

pub struct AppRequests {
    window: slint::Weak<MainWindow>,
    opener: Weak<FileOpener>,
    editor: Weak<Editor>,
    export: Weak<ExportBridge>,
    player: Weak<PlayerBridge>,
    writer: SettingsWriter,
    /// The file the open question is about.
    asking: RefCell<Option<PathBuf>>,
    /// The file that opens once the running save ended with the sections saved ("Save and open").
    after_save: RefCell<Option<(PathBuf, Instant)>>,
    save_timer: slint::Timer,
}

/// The bridges an `app_open` reaches.
pub struct Parts<'a> {
    pub window: &'a MainWindow,
    pub opener: &'a Rc<FileOpener>,
    pub editor: &'a Rc<Editor>,
    pub export: &'a Rc<ExportBridge>,
    pub player: &'a Rc<PlayerBridge>,
    pub writer: &'a SettingsWriter,
}

impl AppRequests {
    /// Answers RustSearch's requests from now on (the app owns the only strong reference).
    pub fn install(parts: &Parts<'_>) -> Rc<Self> {
        let this = Rc::new(Self {
            window: parts.window.as_weak(),
            opener: Rc::downgrade(parts.opener),
            editor: Rc::downgrade(parts.editor),
            export: Rc::downgrade(parts.export),
            player: Rc::downgrade(parts.player),
            writer: parts.writer.clone(),
            asking: RefCell::new(None),
            after_save: RefCell::new(None),
            save_timer: slint::Timer::default(),
        });
        let g = parts.window.global::<AppOpen<'_>>();
        let on = |answer: Answer| {
            let weak = Rc::downgrade(&this);
            move || {
                if let Some(this) = weak.upgrade() {
                    this.answered(answer);
                }
            }
        };
        g.on_keep(on(Answer::Keep));
        g.on_discard(on(Answer::Discard));
        g.on_save(on(Answer::Save));
        CURRENT.with(|c| *c.borrow_mut() = Rc::downgrade(&this));
        this
    }

    fn status(&self) -> Status {
        let asking = self.asking.borrow().is_some();
        Status {
            protocol: PROTOCOL,
            version: rustcut_core::app_identity::VERSION.to_owned(),
            file: self.player.upgrade().and_then(|p| p.current_file()),
            unsaved: self.editor.upgrade().is_some_and(|e| e.has_unsaved_sections()),
            busy: self.busy(asking),
            asking,
        }
    }

    /// A dialog of RustCut's own is open (not the question) or a job is being saved.
    fn busy(&self, asking: bool) -> bool {
        let modal = self.editor.upgrade().is_some_and(|e| e.modal_open()) && !asking;
        let saving = self.export.upgrade().is_some_and(|e| e.is_saving());
        modal || saving || self.after_save.borrow().is_some()
    }

    fn app_open(&self, origin: &str, files: &[PathBuf]) -> Reply {
        let file = match requested_file(origin, files) {
            Ok(file) => file,
            Err(message) => return Reply::err(message),
        };
        let current = self.player.upgrade().and_then(|p| p.current_file());
        let asking = self.asking.borrow().is_some();
        let facts = Facts {
            ask_setting: self.writer.current().dialogs.ask_before_rustsearch_replaces,
            unsaved: self.editor.upgrade().is_some_and(|e| e.has_unsaved_sections()),
            busy: self.busy(asking),
            asking,
            same_file: current.as_deref() == Some(file.as_path()),
        };
        let decision = decide(facts);
        tracing::info!(?decision, file = %file.display(), "[Open] A file from RustSearch");
        match decision {
            Decision::OpenNow => {
                // As File > Open; RustCut stays behind RustSearch (it comes to the front only to ask).
                if let Some(opener) = self.opener.upgrade() {
                    opener.open(file);
                }
                Reply::outcome(Outcome::Opened, "opened")
            }
            Decision::AlreadyOpen => Reply::outcome(Outcome::Opened, "already open"),
            Decision::Ask | Decision::AskAgain => {
                self.ask(file, current.as_deref());
                Reply::outcome(Outcome::Asking, "asking")
            }
            Decision::Busy => Reply::outcome(Outcome::Busy, "RustCut is busy with a dialog or a save"),
        }
    }

    fn ask(&self, file: PathBuf, current: Option<&Path>) {
        let Some(window) = self.window.upgrade() else { return };
        let (title, text) = question(&file, current);
        let g = window.global::<AppOpen<'_>>();
        g.set_title(title.into());
        g.set_text(text.into());
        *self.asking.borrow_mut() = Some(file);
        g.set_open(true);
        window.window().set_minimized(false);
        window
            .window()
            .with_winit_window(slint::winit_030::winit::window::Window::focus_window);
    }

    fn answered(self: &Rc<Self>, answer: Answer) {
        if let Some(window) = self.window.upgrade() {
            window.global::<AppOpen<'_>>().set_open(false);
            window.invoke_focus_keys();
        }
        let Some(file) = self.asking.borrow_mut().take() else {
            return;
        };
        tracing::info!(?answer, file = %file.display(), "[Open] The question about RustSearch's file was answered");
        match answer {
            Answer::Keep => {}
            Answer::Discard => {
                if let Some(opener) = self.opener.upgrade() {
                    opener.open(file);
                }
            }
            Answer::Save => self.save_then_open(file),
        }
    }

    /// Saves the sections as a job (with every question of a save), then opens `file` when they are saved.
    fn save_then_open(self: &Rc<Self>, file: PathBuf) {
        let Some(export) = self.export.upgrade() else { return };
        *self.after_save.borrow_mut() = Some((file, Instant::now()));
        export.save();
        let weak = Rc::downgrade(self);
        self.save_timer.start(slint::TimerMode::Repeated, SAVE_POLL, move || {
            if let Some(this) = weak.upgrade() {
                this.save_tick();
            }
        });
    }

    fn save_tick(&self) {
        let saving = self.export.upgrade().is_some_and(|e| e.is_saving());
        let waited_too_long = self
            .after_save
            .borrow()
            .as_ref()
            .is_none_or(|(_, since)| since.elapsed() > SAVE_PATIENCE);
        if saving && !waited_too_long {
            return;
        }
        self.save_timer.stop();
        let Some((file, _)) = self.after_save.borrow_mut().take() else {
            return;
        };
        let unsaved = self.editor.upgrade().is_some_and(|e| e.has_unsaved_sections());
        if saving || unsaved {
            // The save was refused or canceled (its own message says why): the sections stay, the file is not opened.
            tracing::info!(file = %file.display(), "[Open] The sections were not saved; RustSearch's file was not opened");
        } else if let Some(opener) = self.opener.upgrade() {
            opener.open(file);
        }
    }
}

/// Answers a `status` or `app_open` (UI thread; see the module documentation).
pub fn handle(request: Request, answer: Responder) {
    let Some(this) = CURRENT.with(|c| c.borrow().upgrade()) else {
        // Not installed yet (or quitting): dropping `answer` replies "not handled".
        return;
    };
    let reply = match request {
        Request::Status => Reply::status(this.status()),
        Request::AppOpen { origin, files } => this.app_open(&origin, &files),
        Request::Ping | Request::Open { .. } => Reply::err("not a request of another program"),
    };
    answer.reply(reply);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            ask_setting: true,
            ..Facts::default()
        }
    }

    #[test]
    fn it_asks_only_with_unsaved_sections_and_the_switch_on() {
        assert_eq!(decide(facts()), Decision::OpenNow);
        let unsaved = Facts {
            unsaved: true,
            ..facts()
        };
        assert_eq!(decide(unsaved), Decision::Ask);
        // Off: the file opens at once, as with File > Open.
        assert_eq!(
            decide(Facts {
                ask_setting: false,
                ..unsaved
            }),
            Decision::OpenNow
        );
    }

    #[test]
    fn a_busy_rustcut_refuses_and_an_open_question_takes_the_newer_file() {
        let busy = Facts {
            busy: true,
            unsaved: true,
            ..facts()
        };
        assert_eq!(decide(busy), Decision::Busy);
        assert_eq!(decide(Facts { asking: true, ..busy }), Decision::AskAgain);
        assert_eq!(
            decide(Facts {
                asking: true,
                ..facts()
            }),
            Decision::AskAgain
        );
    }

    #[test]
    fn the_open_file_is_not_opened_again() {
        let same = Facts {
            same_file: true,
            unsaved: true,
            ..facts()
        };
        assert_eq!(decide(same), Decision::AlreadyOpen);
        assert_eq!(decide(Facts { busy: true, ..same }), Decision::Busy);
    }

    #[test]
    fn only_known_programs_and_absolute_paths_are_taken() {
        let files = vec![PathBuf::from("/v/a.mkv"), PathBuf::from("/v/b.mkv")];
        assert_eq!(requested_file("rustsearch", &files), Ok(PathBuf::from("/v/a.mkv")));
        assert!(requested_file("other", &files).unwrap_err().contains("unknown origin"));
        assert_eq!(requested_file("rustsearch", &[]), Err("no file".to_owned()));
        assert!(
            requested_file("rustsearch", &[PathBuf::from("a.mkv")])
                .unwrap_err()
                .contains("absolute")
        );
    }

    #[test]
    fn the_question_names_both_files() {
        let (title, text) = question(Path::new("/v/new clip.mkv"), Some(Path::new("/w/old.mp4")));
        assert_eq!(title, "Open the file from RustSearch?");
        assert!(
            text.contains("\"new clip.mkv\"") && text.contains("\"old.mp4\""),
            "{text}"
        );
        let (_, text) = question(Path::new("/v/n.mkv"), None);
        assert!(text.contains("the open video"), "{text}");
    }
}
