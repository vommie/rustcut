//! Between the generated WIT types and RustCut's own (`crate::types`).

use std::path::PathBuf;

use super::bindings::rustcut::plugin::{host, types as wit};
use crate::types::{Action, Context, Event, Issue, Level, Notice, OpenedBy, Place, Tag, Target};

pub(super) fn level(level: host::Level) -> Level {
    match level {
        host::Level::Debug => Level::Debug,
        host::Level::Info => Level::Info,
        host::Level::Warning => Level::Warning,
        host::Level::Error => Level::Error,
    }
}

fn tag_to_wit(tag: &Tag) -> wit::Tag {
    wit::Tag {
        id: tag.id,
        name: tag.name.clone(),
        path: tag.path.clone(),
    }
}

fn path_text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(super) fn event_to_wit(event: &Event) -> wit::Event {
    match event {
        Event::VideoOpened(by) => wit::Event::VideoOpened(match by {
            OpenedBy::User => wit::OpenedBy::User,
            OpenedBy::Job => wit::OpenedBy::Job,
        }),
        Event::TagAssigned(tag) => wit::Event::TagAssigned(tag_to_wit(tag)),
        Event::TagRemoved(tag) => wit::Event::TagRemoved(tag_to_wit(tag)),
    }
}

pub(super) fn context_to_wit(context: &Context) -> wit::Context {
    wit::Context {
        video: context.video.as_deref().map(path_text),
        rating: context.rating,
        tags: context.tags.iter().map(tag_to_wit).collect(),
        target: context.target.as_deref().map(path_text),
    }
}

pub(super) fn action_from_wit(action: wit::Action) -> Action {
    match action {
        wit::Action::SetTarget(set) => Action::SetTarget {
            target: match set.target {
                wit::Target::Folder(id) => Target::Folder(id),
                wit::Target::Path(path) => Target::Path(PathBuf::from(path)),
            },
            reason: set.reason,
        },
        wit::Action::SetRating(set) => Action::SetRating {
            stars: set.stars,
            reason: set.reason,
        },
        wit::Action::Notify(notice) => Action::Notify(Notice {
            place: match notice.place {
                wit::Place::None => Place::None,
                wit::Place::Toast => Place::Toast,
                wit::Place::Banner => Place::Banner,
            },
            level: level(notice.level),
            title: notice.title,
            detail: notice.detail,
        }),
    }
}

pub(super) fn issue_from_wit(issue: wit::Issue) -> Issue {
    Issue {
        field: issue.field,
        row: issue.row,
        level: level(issue.level),
        message: issue.message,
    }
}
