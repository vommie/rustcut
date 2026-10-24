//! `[notices]` of `config.toml`: how RustCut's messages appear (M6.5, user decisions Q-N8 and Q-N10). Toasts, banners,
//! the history bell and the desktop notifications have their options here; which message kind goes where is stored
//! only where the user changed it (`kinds`), so the app's catalog of kinds keeps the defaults. Every value a newer
//! version may add reads as its default (`DefaultOnError`), so a typo never resets the other options.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

/// Where a message appears in the app (the desktop notification is an extra switch beside it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticePlace {
    /// A card that disappears by itself (errors stay until closed).
    Toast,
    /// A strip over the top of the video that stays while its state holds.
    Banner,
    /// A closable bubble at the element it is about.
    Bubble,
    /// A window over RustCut that needs an answer.
    Dialog,
    /// The slim status line at the bottom.
    Status,
    /// Not shown (the history still records it).
    Nothing,
}

/// Where toasts appear (Q-N8: every choice of the questionnaire is a setting, the user's answer the default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastPosition {
    VideoTopRight,
    VideoTopCenter,
    VideoBottomRight,
    VideoBottomCenter,
    /// Over the right sidebar, below the menu bar.
    #[default]
    WindowTopRight,
    WindowBottomRight,
}

/// What happens when more toasts come than fit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastStacking {
    /// The first `max_visible`, then "+N more" (a click shows all).
    #[default]
    Some,
    /// Every toast, one below the other.
    All,
    /// One at a time with "‹ 1 / 5 ›".
    Pager,
    /// A deck: the others lie offset behind the first and fan out under the pointer.
    Deck,
    /// One card "5 messages: 1 error, 2 warnings" that opens into the list.
    Summary,
}

/// In which order the visible toasts stand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastOrder {
    NewestTop,
    NewestBottom,
    /// Errors first, then warnings, then the rest; newest first within each.
    #[default]
    Severity,
}

/// How toasts come and go; the direction follows the position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastAnimation {
    /// Slide in from the nearest side and fade in.
    #[default]
    Slide,
    Fade,
    /// Fall in from above.
    Drop,
    None,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToastSettings {
    #[serde_as(as = "DefaultOnError")]
    pub position: ToastPosition,
    #[serde_as(as = "DefaultOnError")]
    pub stacking: ToastStacking,
    /// How many toasts show at most with `Some` and `Deck` (2-5).
    pub max_visible: u8,
    #[serde_as(as = "DefaultOnError")]
    pub order: ToastOrder,
    #[serde_as(as = "DefaultOnError")]
    pub animation: ToastAnimation,
    /// Seconds a toast of each level stays; 0 = until it is closed.
    pub info_secs: u8,
    pub success_secs: u8,
    pub warning_secs: u8,
    pub error_secs: u8,
}

impl ToastSettings {
    pub const MAX_VISIBLE: RangeInclusive<u8> = 2..=5;
    /// The longest time a toast can be set to stay.
    pub const MAX_SECS: u8 = 60;
}

impl Default for ToastSettings {
    fn default() -> Self {
        Self {
            position: ToastPosition::default(),
            stacking: ToastStacking::default(),
            max_visible: 3,
            order: ToastOrder::default(),
            animation: ToastAnimation::default(),
            info_secs: 5,
            success_secs: 5,
            warning_secs: 8,
            error_secs: 0,
        }
    }
}

/// How a banner appears.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BannerAnimation {
    /// It slides down like a curtain.
    #[default]
    Curtain,
    Fade,
    None,
}

/// What a new error banner does to be seen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BannerAttention {
    /// It lights up once.
    #[default]
    Pulse,
    Nothing,
}

#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BannerSettings {
    #[serde_as(as = "DefaultOnError")]
    pub animation: BannerAnimation,
    #[serde_as(as = "DefaultOnError")]
    pub new_error: BannerAttention,
}

/// What the number on the bell counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BellBadge {
    /// Unread errors and warnings.
    #[default]
    ErrorsWarnings,
    /// Every unread message.
    All,
    /// No number.
    None,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// The bell in the menu bar with the last messages.
    pub bell: bool,
    #[serde_as(as = "DefaultOnError")]
    pub badge: BellBadge,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            bell: true,
            badge: BellBadge::default(),
        }
    }
}

/// The bubbles at an input (M6.5c, user decision Q-N10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BubbleSettings {
    /// How opaque a bubble starts, in percent (the mouse wheel over it changes it while it shows).
    pub opacity: u8,
}

impl BubbleSettings {
    pub const OPACITY: RangeInclusive<u8> = 20..=100;
}

impl Default for BubbleSettings {
    fn default() -> Self {
        Self { opacity: 100 }
    }
}

/// Desktop notifications (moved here from `desktop.notifications` in settings version 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopNoticeSettings {
    pub enabled: bool,
    /// Seconds a notification stays (0 = until dismissed).
    pub timeout_secs: u16,
}

impl Default for DesktopNoticeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: 8,
        }
    }
}

/// The user's choice for one message kind; a missing value is the kind's default.
#[serde_as]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KindRoute {
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<NoticePlace>,
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop: Option<bool>,
}

impl KindRoute {
    pub const fn is_default(&self) -> bool {
        self.place.is_none() && self.desktop.is_none()
    }
}

#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoticeSettings {
    pub toasts: ToastSettings,
    pub banners: BannerSettings,
    pub history: HistorySettings,
    pub bubbles: BubbleSettings,
    pub desktop: DesktopNoticeSettings,
    /// Changed places by kind id (`job_failed = { place = "banner", desktop = false }`); the app's catalog owns the ids,
    /// the defaults and which places a kind allows. Entries of unknown kinds are kept (a newer version's).
    #[serde_as(as = "DefaultOnError<BTreeMap<_, DefaultOnError>>")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub kinds: BTreeMap<String, KindRoute>,
    /// Kinds the user chose "Don't show again" for (only hints and confirmations can be muted).
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub muted: BTreeSet<String>,
}

impl NoticeSettings {
    /// Brings the numbers into range; a note per change.
    pub fn sanitize(&mut self, notes: &mut Vec<String>) {
        let toasts = &mut self.toasts;
        if !ToastSettings::MAX_VISIBLE.contains(&toasts.max_visible) {
            toasts.max_visible = toasts
                .max_visible
                .clamp(*ToastSettings::MAX_VISIBLE.start(), *ToastSettings::MAX_VISIBLE.end());
            notes.push("notices.toasts.max_visible brought into range".to_owned());
        }
        for (name, secs) in [
            ("info_secs", &mut toasts.info_secs),
            ("success_secs", &mut toasts.success_secs),
            ("warning_secs", &mut toasts.warning_secs),
            ("error_secs", &mut toasts.error_secs),
        ] {
            if *secs > ToastSettings::MAX_SECS {
                *secs = ToastSettings::MAX_SECS;
                notes.push(format!("notices.toasts.{name} limited to {}", ToastSettings::MAX_SECS));
            }
        }
        let opacity = &mut self.bubbles.opacity;
        if !BubbleSettings::OPACITY.contains(opacity) {
            *opacity = (*opacity).clamp(*BubbleSettings::OPACITY.start(), *BubbleSettings::OPACITY.end());
            notes.push("notices.bubbles.opacity brought into range".to_owned());
        }
        let before = self.kinds.len();
        self.kinds.retain(|_, route| !route.is_default());
        if self.kinds.len() != before {
            notes.push("notices.kinds: empty entries dropped".to_owned());
        }
    }
}

/// Settings version 1 -> 2: the desktop notifications and the known-source warning moved to `[notices]` (user decision
/// Q-N10: options that have a better place move there). The old defaults equal the kinds' defaults, so only changed
/// values are written.
pub fn migrate_notice_options(table: &mut toml::Table) -> Result<(), String> {
    let mut kinds = toml::Table::new();
    let mut desktop = toml::Table::new();
    if let Some(toml::Value::Table(old)) = table
        .get_mut("desktop")
        .and_then(toml::Value::as_table_mut)
        .and_then(|d| d.remove("notifications"))
    {
        for (key, value) in old {
            match (key.as_str(), value) {
                ("enabled" | "timeout_secs", value) => {
                    desktop.insert(key, value);
                }
                ("job_finished", toml::Value::Boolean(on)) if on => {
                    kinds.insert(key, route_table("desktop", toml::Value::Boolean(on)));
                }
                ("job_failed" | "queue_done", toml::Value::Boolean(on)) if !on => {
                    kinds.insert(key, route_table("desktop", toml::Value::Boolean(on)));
                }
                _ => {}
            }
        }
    }
    if let Some(toml::Value::Boolean(false)) = table
        .get_mut("warnings")
        .and_then(toml::Value::as_table_mut)
        .and_then(|w| w.remove("known_source"))
    {
        kinds.insert(
            "known_source".to_owned(),
            route_table("place", toml::Value::String("nothing".to_owned())),
        );
    }
    if desktop.is_empty() && kinds.is_empty() {
        return Ok(());
    }
    let notices = table
        .entry("notices")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| "notices is not a table".to_owned())?;
    if !desktop.is_empty() {
        notices.insert("desktop".to_owned(), toml::Value::Table(desktop));
    }
    if !kinds.is_empty() {
        notices.insert("kinds".to_owned(), toml::Value::Table(kinds));
    }
    Ok(())
}

fn route_table(key: &str, value: toml::Value) -> toml::Value {
    let mut route = toml::Table::new();
    route.insert(key.to_owned(), value);
    toml::Value::Table(route)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changed_kinds_are_written_and_unknown_values_read_as_defaults() {
        let mut settings = NoticeSettings::default();
        settings.kinds.insert(
            "job_failed".into(),
            KindRoute {
                place: Some(NoticePlace::Banner),
                desktop: None,
            },
        );
        settings.kinds.insert("job_queued".into(), KindRoute::default());
        let mut notes = Vec::new();
        settings.sanitize(&mut notes);
        assert_eq!(notes, ["notices.kinds: empty entries dropped"]);
        let text = toml::to_string(&settings).unwrap();
        assert!(text.contains("[kinds.job_failed]\nplace = \"banner\""), "{text}");
        assert!(!text.contains("muted"), "{text}");

        let read: NoticeSettings = toml::from_str(
            "[toasts]\nposition = \"teleport\"\nmax_visible = 9\n[kinds.x]\nplace = \"somewhere\"\ndesktop = true\n",
        )
        .unwrap();
        assert_eq!(read.toasts.position, ToastPosition::WindowTopRight);
        assert_eq!(
            read.kinds["x"],
            KindRoute {
                place: None,
                desktop: Some(true)
            }
        );
        let mut read = read;
        let mut notes = Vec::new();
        read.sanitize(&mut notes);
        assert_eq!(read.toasts.max_visible, 5);
    }

    #[test]
    fn bubbles_default_to_opaque_and_stay_in_range() {
        // A 0.0.26 file has no [bubbles].
        let read: NoticeSettings = toml::from_str("[toasts]\nmax_visible = 4\n").unwrap();
        assert_eq!(read.bubbles.opacity, 100);
        let mut read: NoticeSettings = toml::from_str("[bubbles]\nopacity = 5\n").unwrap();
        let mut notes = Vec::new();
        read.sanitize(&mut notes);
        assert_eq!(read.bubbles.opacity, 20);
        assert_eq!(notes, ["notices.bubbles.opacity brought into range"]);
    }

    #[test]
    fn version_1_options_move_to_notices() {
        let mut table: toml::Table = toml::from_str(
            "version = 1\n[desktop]\ntaskbar_progress = false\n[desktop.notifications]\nenabled = false\njob_finished = true\n\
             job_failed = true\nqueue_done = false\ntimeout_secs = 3\n[warnings]\nno_tags = false\nknown_source = false\n",
        )
        .unwrap();
        migrate_notice_options(&mut table).unwrap();
        let text = toml::to_string(&table).unwrap();
        let expected: toml::Table = toml::from_str(
            "version = 1\n[desktop]\ntaskbar_progress = false\n[warnings]\nno_tags = false\n[notices.desktop]\nenabled = false\n\
             timeout_secs = 3\n[notices.kinds.job_finished]\ndesktop = true\n[notices.kinds.queue_done]\ndesktop = false\n\
             [notices.kinds.known_source]\nplace = \"nothing\"\n",
        )
        .unwrap();
        assert_eq!(table, expected, "{text}");

        // Defaults stay defaults: nothing is written.
        let mut table: toml::Table =
            toml::from_str("[desktop.notifications]\njob_failed = true\n[warnings]\nknown_source = true\n").unwrap();
        migrate_notice_options(&mut table).unwrap();
        assert!(!table.contains_key("notices"), "{table:?}");
        assert!(table["warnings"].as_table().unwrap().is_empty());
    }
}
