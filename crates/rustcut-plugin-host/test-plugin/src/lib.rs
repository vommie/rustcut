//! RustCut's test plugin. Its `mode` setting decides what `handle` does:
//! `ok` (a notice and, with a video, a target), `fail` (an error), `panic`, `slow` (an endless loop),
//! `memory` (allocates until it is stopped), `quiet` (API 1.1: hides its marks, sets 3 stars and tells the target
//! folder id it reads from the host).

use rustcut_plugin_api::{
    Action, Context, Event, Issue, Level, Notice, Place, Plugin, SetRating, SetTarget, Target, export_plugin,
};
use std::sync::Mutex;

#[derive(serde::Deserialize, Default, Clone)]
struct Settings {
    #[serde(default)]
    mode: String,
}

static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);

struct TestPlugin;

impl Plugin for TestPlugin {
    fn configure(settings: String) -> Result<(), String> {
        let parsed: Settings = rustcut_plugin_api::parse_settings(&settings)?;
        if parsed.mode == "refuse" {
            return Err("mode refuse refuses every configuration".to_owned());
        }
        rustcut_plugin_api::log(Level::Info, &format!("configured, mode {}", parsed.mode));
        *SETTINGS.lock().map_err(|_| "poisoned".to_owned())? = Some(parsed);
        Ok(())
    }

    fn handle(event: Event, context: Context) -> Result<Vec<Action>, String> {
        let mode = SETTINGS.lock().map_err(|_| "poisoned".to_owned())?.clone().unwrap_or_default().mode;
        match mode.as_str() {
            "fail" => Err("asked to fail".to_owned()),
            "panic" => panic!("asked to panic"),
            "slow" => {
                let mut n: u64 = 0;
                loop {
                    n = std::hint::black_box(n.wrapping_add(1));
                }
            }
            "quiet" => {
                rustcut_plugin_api::hide_marks();
                let folder = rustcut_plugin_api::target_folder();
                Ok(vec![
                    Action::SetRating(SetRating { stars: 3, reason: "quiet".into() }),
                    Action::Notify(Notice::new(Place::None, Level::Info, "Test plugin", format!("folder {folder:?}"))),
                ])
            }
            "memory" => {
                let mut blocks = Vec::new();
                loop {
                    blocks.push(std::hint::black_box(vec![1_u8; 1 << 20]));
                }
            }
            _ => {
                let name = match event {
                    Event::VideoOpened(_) => "video-opened",
                    Event::TagAssigned(_) => "tag-assigned",
                    Event::TagRemoved(_) => "tag-removed",
                };
                let mut actions = vec![Action::Notify(Notice::new(Place::Toast, Level::Info, "Test plugin", name))];
                if let Some(video) = context.video {
                    let folder = video.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_owned());
                    actions.push(Action::SetTarget(SetTarget { target: Target::Path(folder), reason: "test".into() }));
                }
                Ok(actions)
            }
        }
    }

    fn check(settings: String) -> Vec<Issue> {
        match rustcut_plugin_api::parse_settings::<Settings>(&settings) {
            Ok(s) if !["", "ok", "fail", "panic", "slow", "memory", "refuse", "quiet"].contains(&s.mode.as_str()) => vec![Issue {
                field: "mode".into(),
                row: None,
                level: Level::Warning,
                message: format!("unknown mode {}", s.mode),
            }],
            Ok(_) => Vec::new(),
            Err(err) => vec![Issue { field: "mode".into(), row: None, level: Level::Error, message: err }],
        }
    }
}

export_plugin!(TestPlugin);
