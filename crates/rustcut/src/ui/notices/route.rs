//! Where a message goes: the kind's default place unless the user chose another one it allows, never for a critical
//! kind (Q-N10); muted only for hints and confirmations (Q-N8); the desktop notification beside it.

use rustcut_core::settings::{NoticePlace, NoticeSettings};

use super::kind::{Kind, Level};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub place: NoticePlace,
    /// The kind may go to the desktop (whether it does also depends on the window, see [`desktop_now`]).
    pub desktop: bool,
    /// "Don't show again": only the history records it.
    pub muted: bool,
}

pub fn route(kind: Kind, level: Level, settings: &NoticeSettings) -> Route {
    let info = kind.info();
    if info.critical {
        return Route {
            place: info.place,
            desktop: info.desktop,
            muted: false,
        };
    }
    let user = settings.kinds.get(info.id);
    let place = user
        .and_then(|r| r.place)
        .filter(|p| info.places.contains(p))
        .unwrap_or(info.place);
    let muted = can_mute(kind) && level.is_quiet() && settings.muted.contains(info.id);
    Route {
        place: if muted { NoticePlace::Nothing } else { place },
        desktop: !muted && settings.desktop.enabled && user.and_then(|r| r.desktop).unwrap_or(info.desktop),
        muted,
    }
}

/// The place of `kind` as the user set it (its default unless changed; critical kinds keep theirs).
pub fn place_of(settings: &NoticeSettings, kind: Kind) -> NoticePlace {
    let info = kind.info();
    if info.critical {
        return info.place;
    }
    settings
        .kinds
        .get(info.id)
        .and_then(|r| r.place)
        .filter(|p| info.places.contains(p))
        .unwrap_or(info.place)
}

/// Sets the place of `kind`; its default is stored as "no change" (the file keeps only the user's choices).
pub fn set_place(settings: &mut NoticeSettings, kind: Kind, place: NoticePlace) -> Result<(), String> {
    let info = kind.info();
    if info.critical || !info.places.contains(&place) {
        return Err(format!("\"{}\" cannot appear there", info.title));
    }
    let route = settings.kinds.entry(info.id.to_owned()).or_default();
    route.place = (place != info.place).then_some(place);
    settings.kinds.retain(|_, r| !r.is_default());
    Ok(())
}

/// Whether `kind` also goes to the desktop as the user set it.
pub fn desktop_of(settings: &NoticeSettings, kind: Kind) -> bool {
    let info = kind.info();
    settings
        .kinds
        .get(info.id)
        .and_then(|r| r.desktop)
        .unwrap_or(info.desktop)
}

pub fn set_desktop(settings: &mut NoticeSettings, kind: Kind, on: bool) {
    let info = kind.info();
    let route = settings.kinds.entry(info.id.to_owned()).or_default();
    route.desktop = (on != info.desktop).then_some(on);
    settings.kinds.retain(|_, r| !r.is_default());
}

/// Whether "Don't show again" is offered for a kind: hints and confirmations only, never a critical kind.
pub fn can_mute(kind: Kind) -> bool {
    let info = kind.info();
    !info.critical && info.level.is_quiet()
}

/// Whether a message routed to the desktop is sent now: always when it has no place in the window (the countdown
/// after the queue), else only while RustCut is not in front, so it never says the same twice (Q-N8: "errors also on
/// the desktop while RustCut is in the background").
pub fn desktop_now(route: Route, window_in_front: bool) -> bool {
    route.desktop && (route.place == NoticePlace::Nothing || !window_in_front)
}

#[cfg(test)]
mod tests {
    use rustcut_core::settings::KindRoute;

    use super::*;

    #[test]
    fn user_choices_apply_where_the_kind_allows_them() {
        let mut settings = NoticeSettings::default();
        let default = route(Kind::JobFailed, Level::Error, &settings);
        assert_eq!((default.place, default.desktop), (NoticePlace::Toast, true));

        settings.kinds.insert(
            "job_failed".into(),
            KindRoute {
                place: Some(NoticePlace::Banner),
                desktop: Some(false),
            },
        );
        // A place the kind does not offer is ignored.
        settings.kinds.insert(
            "job_finished".into(),
            KindRoute {
                place: Some(NoticePlace::Banner),
                desktop: None,
            },
        );
        // A critical kind keeps its place.
        settings.kinds.insert(
            "save_failed".into(),
            KindRoute {
                place: Some(NoticePlace::Nothing),
                desktop: None,
            },
        );
        let failed = route(Kind::JobFailed, Level::Error, &settings);
        assert_eq!((failed.place, failed.desktop), (NoticePlace::Banner, false));
        assert_eq!(
            route(Kind::JobFinished, Level::Success, &settings).place,
            NoticePlace::Nothing
        );
        assert_eq!(
            route(Kind::SaveFailed, Level::Error, &settings).place,
            NoticePlace::Banner
        );

        settings.desktop.enabled = false;
        assert!(!route(Kind::QueueDone, Level::Success, &settings).desktop);
    }

    #[test]
    fn only_quiet_messages_can_be_muted() {
        let mut settings = NoticeSettings::default();
        for id in ["job_queued", "save_failed", "auto_crop", "job_failed"] {
            settings.muted.insert(id.into());
        }
        let queued = route(Kind::JobQueued, Level::Success, &settings);
        assert!(queued.muted && queued.place == NoticePlace::Nothing && !queued.desktop);
        assert!(!route(Kind::SaveFailed, Level::Error, &settings).muted, "critical");
        assert!(!route(Kind::JobFailed, Level::Error, &settings).muted, "an error kind");
        // A warning of a quiet kind ("only black frames") still shows.
        let warning = route(Kind::AutoCrop, Level::Warning, &settings);
        assert!(!warning.muted && warning.place == NoticePlace::Toast);
        assert!(can_mute(Kind::SectionAdded) && !can_mute(Kind::JobFailed) && !can_mute(Kind::OpenFailed));
    }

    #[test]
    fn only_changed_choices_are_stored() {
        let mut settings = NoticeSettings::default();
        set_desktop(&mut settings, Kind::JobFinished, true);
        set_place(&mut settings, Kind::JobFinished, NoticePlace::Toast).unwrap();
        assert!(desktop_of(&settings, Kind::JobFinished));
        assert_eq!(place_of(&settings, Kind::JobFinished), NoticePlace::Toast);
        set_desktop(&mut settings, Kind::JobFinished, false);
        set_place(&mut settings, Kind::JobFinished, NoticePlace::Nothing).unwrap();
        assert!(settings.kinds.is_empty(), "back to the defaults = nothing stored");
        assert!(set_place(&mut settings, Kind::JobFinished, NoticePlace::Banner).is_err());
        assert!(
            set_place(&mut settings, Kind::SaveFailed, NoticePlace::Banner).is_err(),
            "critical"
        );
    }

    #[test]
    fn the_desktop_gets_what_the_window_does_not_show() {
        let toast = Route {
            place: NoticePlace::Toast,
            desktop: true,
            muted: false,
        };
        assert!(!desktop_now(toast, true));
        assert!(desktop_now(toast, false));
        let nowhere = Route {
            place: NoticePlace::Nothing,
            ..toast
        };
        assert!(desktop_now(nowhere, true));
        assert!(!desktop_now(
            Route {
                desktop: false,
                ..nowhere
            },
            false
        ));
    }
}
