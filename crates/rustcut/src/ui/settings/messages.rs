//! The Messages page (M6.5b, user decisions Q-N8/Q-N10): three tabs. "Look & behavior" holds the generic cards
//! (`catalog/messages.rs`); "Message kinds" lists every kind of message with its place (only the places the kind
//! allows; critical kinds are locked), its desktop switch, a test that shows the real message at its real place, and
//! "Don't show again" for hints and confirmations; "Muted" lists the muted kinds with a way back. The search filters
//! the kinds and the muted list. Pure rows here; the bridge's handlers below.

use rustcut_core::settings::{NoticePlace, NoticeSettings, Settings};
use slint::{ModelRc, SharedString, VecModel};

use super::SettingsBridge;
use crate::ui::notices::{self, Kind, Route};
use crate::{MessageKindView, SettingsUi};

/// The page's tabs (kept in `ui-state.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Look = 0,
    Kinds = 1,
    Muted = 2,
}

impl Tab {
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Kinds,
            2 => Self::Muted,
            _ => Self::Look,
        }
    }
}

/// One kind as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindRow {
    pub kind: Kind,
    /// The group's title on the first row of a group.
    pub header: Option<&'static str>,
    pub places: &'static [NoticePlace],
    pub place: NoticePlace,
    pub desktop: bool,
    pub muted: bool,
    /// What the last test showed ("" = not tested just now).
    pub tested: String,
}

/// The word for a place in the list's choice.
pub const fn place_label(place: NoticePlace) -> &'static str {
    match place {
        NoticePlace::Toast => "Toast",
        NoticePlace::Banner => "Banner",
        NoticePlace::Bubble => "Bubble",
        NoticePlace::Dialog => "Dialog",
        NoticePlace::Status => "Status line",
        NoticePlace::Nothing => "Nothing",
    }
}

/// Whether `kind` matches the search (title, example, group or id; any case).
pub fn matches(kind: Kind, search: &str) -> bool {
    let needle = search.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    let info = kind.info();
    [info.title, info.sample.0, info.sample.1, info.group.title(), info.id]
        .iter()
        .any(|text| text.to_lowercase().contains(&needle))
}

/// The kinds that match `search`, in the catalog's order, with their group's title on the first row of each group.
pub fn kind_rows(settings: &NoticeSettings, search: &str, tested: Option<(Kind, &str)>) -> Vec<KindRow> {
    let mut last_group = None;
    Kind::ALL
        .iter()
        .copied()
        .filter(|k| matches(*k, search))
        .map(|kind| {
            let info = kind.info();
            let header = (last_group != Some(info.group)).then(|| info.group.title());
            last_group = Some(info.group);
            KindRow {
                kind,
                header,
                places: info.places,
                place: notices::place_of(settings, kind),
                desktop: notices::desktop_of(settings, kind),
                muted: settings.muted.contains(info.id),
                tested: tested
                    .filter(|(k, _)| *k == kind)
                    .map(|(_, text)| text.to_owned())
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// The muted kinds that match `search` (ids of unknown kinds are left out).
pub fn muted_rows(settings: &NoticeSettings, search: &str) -> Vec<Kind> {
    Kind::ALL
        .iter()
        .copied()
        .filter(|k| settings.muted.contains(k.id()) && matches(*k, search))
        .collect()
}

/// What a test showed, in words (the row says it for a moment).
pub fn test_note(route: Route) -> String {
    let place = if route.muted {
        "Not shown: muted".to_owned()
    } else {
        match route.place {
            NoticePlace::Toast => "Shown as a toast".to_owned(),
            NoticePlace::Banner => "Shown as a banner over the video".to_owned(),
            NoticePlace::Status => "Shown in the status line".to_owned(),
            NoticePlace::Bubble => "Shown as a bubble (here at the test button; usually at its field)".to_owned(),
            NoticePlace::Dialog | NoticePlace::Nothing => {
                "Not shown in the window: its place is \"Nothing\"".to_owned()
            }
        }
    };
    if route.desktop {
        format!("{place}, and on the desktop")
    } else {
        place
    }
}

fn view(row: &KindRow) -> MessageKindView {
    let info = row.kind.info();
    MessageKindView {
        id: info.id.into(),
        header: row.header.unwrap_or_default().into(),
        title: info.title.into(),
        sample: if info.sample.1.is_empty() {
            info.sample.0.into()
        } else {
            format!("{} · {}", info.sample.0, info.sample.1).into()
        },
        level: info.level.index(),
        places: ModelRc::new(VecModel::from(
            row.places
                .iter()
                .map(|p| SharedString::from(place_label(*p)))
                .collect::<Vec<_>>(),
        )),
        place_index: i32::try_from(row.places.iter().position(|p| *p == row.place).unwrap_or(0)).unwrap_or(0),
        locked: info.critical,
        desktop: row.desktop,
        desktop_allowed: info.place != NoticePlace::Bubble,
        mutable: notices::can_mute(row.kind),
        muted: row.muted,
        tested: row.tested.as_str().into(),
    }
}

/// The kinds' rows as last shown, so only changed rows are made again (their choices keep their elements).
#[derive(Debug, Default)]
pub struct Shown {
    pub kinds: Vec<KindRow>,
    pub muted: Vec<KindRow>,
}

impl SettingsBridge {
    /// The page's tabs, the kinds and the muted list.
    pub(super) fn publish_messages(&self, ui: &SettingsUi<'_>, settings: &Settings) {
        let state = self.state.borrow();
        let tested = self.tested.borrow();
        let tested = tested.as_ref().map(|(k, text)| (*k, text.as_str()));
        let kinds = kind_rows(&settings.notices, &state.messages_search, tested);
        let muted: Vec<KindRow> = muted_rows(&settings.notices, &state.messages_search)
            .into_iter()
            .map(|kind| KindRow {
                kind,
                header: None,
                places: kind.info().places,
                place: notices::place_of(&settings.notices, kind),
                desktop: false,
                muted: true,
                tested: String::new(),
            })
            .collect();
        ui.set_messages_tab(i32::from(state.messages_tab));
        ui.set_messages_search(state.messages_search.as_str().into());
        ui.set_kinds_count(i32::try_from(Kind::ALL.len()).unwrap_or(0));
        ui.set_muted_count(i32::try_from(settings.notices.muted.len()).unwrap_or(0));
        let mut shown = self.messages_shown.borrow_mut();
        crate::util::model::sync_with(&self.models.kinds, &mut shown.kinds, &kinds, view);
        crate::util::model::sync_with(&self.models.muted, &mut shown.muted, &muted, view);
    }

    pub(super) fn select_messages_tab(&self, index: i32) {
        let tab = Tab::from_index(u8::try_from(index).unwrap_or(0));
        self.state.borrow_mut().messages_tab = tab as u8;
        self.publish();
    }

    pub(super) fn messages_search_edited(&self, text: &str) {
        text.clone_into(&mut self.state.borrow_mut().messages_search);
        self.publish();
    }

    pub(super) fn kind_place(&self, id: &str, index: i32) {
        let Some(kind) = Kind::from_id(id) else { return };
        let Some(place) = usize::try_from(index)
            .ok()
            .and_then(|i| kind.info().places.get(i))
            .copied()
        else {
            return;
        };
        tracing::info!(kind = id, ?place, "[Settings] Message place");
        self.update(|s| {
            if let Err(why) = notices::set_place(&mut s.notices, kind, place) {
                tracing::warn!(kind = id, why, "[Settings] Message place refused");
            }
        });
    }

    pub(super) fn kind_desktop(&self, id: &str) {
        let Some(kind) = Kind::from_id(id).filter(|k| !k.info().critical) else {
            return;
        };
        self.update(|s| {
            let on = notices::desktop_of(&s.notices, kind);
            notices::set_desktop(&mut s.notices, kind, !on);
        });
    }

    /// "Don't show again" and back (hints and confirmations only).
    pub(super) fn kind_mute(&self, id: &str) {
        let Some(kind) = Kind::from_id(id).filter(|k| notices::can_mute(*k)) else {
            return;
        };
        self.update(|s| {
            if !s.notices.muted.remove(kind.id()) {
                s.notices.muted.insert(kind.id().to_owned());
            }
        });
    }

    /// The test: the real message at its real place; the row says where it went for a few seconds (Q-N10).
    pub(super) fn kind_test(&self, id: &str) {
        let Some(kind) = Kind::from_id(id) else { return };
        let route = notices::test(kind);
        tracing::info!(kind = id, place = ?route.place, desktop = route.desktop, "[Settings] Message tested");
        self.tested.replace(Some((kind, test_note(route))));
        let me = std::rc::Weak::clone(&self.me);
        self.tested_timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_secs(4),
            move || {
                if let Some(b) = me.upgrade() {
                    b.tested.replace(None);
                    b.publish();
                }
            },
        );
        self.publish();
    }

    /// Resets the kinds and the muted list (the page's held reset button; the cards reset by their fields).
    pub(super) fn reset_kinds(&self) {
        self.update(|s| {
            s.notices.kinds.clear();
            s.notices.muted.clear();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_filters_and_groups_have_one_header() {
        let settings = NoticeSettings::default();
        let all = kind_rows(&settings, "", None);
        assert_eq!(all.len(), Kind::ALL.len());
        assert_eq!(all.first().and_then(|r| r.header), Some("Saving"));
        let headers = all.iter().filter(|r| r.header.is_some()).count();
        let groups: std::collections::HashSet<_> = Kind::ALL.iter().map(|k| k.info().group).collect();
        assert_eq!(headers, groups.len());

        let failed = kind_rows(&settings, "FAILED", Some((Kind::JobFailed, "Shown as a toast")));
        assert!(failed.iter().all(|r| matches(r.kind, "failed")));
        let job = failed.iter().find(|r| r.kind == Kind::JobFailed).unwrap();
        assert_eq!(
            (job.place, job.desktop, job.tested.as_str()),
            (NoticePlace::Toast, true, "Shown as a toast")
        );
        // A group's header appears on its first matching row.
        assert!(failed.first().and_then(|r| r.header).is_some());
        assert!(kind_rows(&settings, "no such message", None).is_empty());
    }

    #[test]
    fn muted_kinds_are_listed_and_tests_say_where() {
        let mut settings = NoticeSettings::default();
        settings.muted.insert("section_added".into());
        settings.muted.insert("from_a_newer_version".into());
        assert_eq!(muted_rows(&settings, ""), [Kind::SectionAdded]);
        assert!(muted_rows(&settings, "queue").is_empty());
        let route = |place, desktop, muted| Route { place, desktop, muted };
        assert_eq!(
            test_note(route(NoticePlace::Toast, true, false)),
            "Shown as a toast, and on the desktop"
        );
        assert_eq!(test_note(route(NoticePlace::Nothing, false, true)), "Not shown: muted");
        assert_eq!(
            test_note(route(NoticePlace::Nothing, true, false)),
            "Not shown in the window: its place is \"Nothing\", and on the desktop"
        );
    }
}
