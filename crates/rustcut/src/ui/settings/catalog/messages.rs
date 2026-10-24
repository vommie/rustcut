//! The Messages page's first tab, "Look & behavior" (M6.5b, user decisions Q-N8/Q-N10): every option of the
//! questionnaire, the user's answer as the default. The kinds and the muted list are the page's own editor
//! (`settings/messages.rs`).

use rustcut_core::settings::{
    BannerAnimation, BannerAttention, BellBadge, Settings, ToastAnimation, ToastOrder, ToastPosition, ToastStacking,
};

use super::Card;
use crate::ui::settings::field::{choice, number, switch};

fn desktop_on(s: &Settings) -> bool {
    s.notices.desktop.enabled
}

fn bell_on(s: &Settings) -> bool {
    s.notices.history.bell
}

fn stacking_limits(s: &Settings) -> bool {
    matches!(s.notices.toasts.stacking, ToastStacking::Some | ToastStacking::Deck)
}

pub static MESSAGES: &[Card] = &[
    Card {
        title: "Toasts",
        description: "Short cards that say what just happened and leave by themselves. The pointer on a card stops \
                      its time and shows the whole text.",
        fields: &[
            choice!(
                "notices.toasts.position",
                "Where",
                "Where the cards appear: on the video or in the window's corners.",
                notices.toasts.position,
                [
                    (ToastPosition::WindowTopRight, "Window, top right"),
                    (ToastPosition::WindowBottomRight, "Window, bottom right"),
                    (ToastPosition::VideoTopRight, "Video, top right"),
                    (ToastPosition::VideoTopCenter, "Video, top center"),
                    (ToastPosition::VideoBottomRight, "Video, bottom right"),
                    (ToastPosition::VideoBottomCenter, "Video, bottom center"),
                ]
            ),
            choice!(
                "notices.toasts.stacking",
                "When several come",
                "Some and \"+2 more\" for the rest, every card, one at a time with a pager, a deck that fans out \
                 under the pointer, or one card that counts them.",
                notices.toasts.stacking,
                [
                    (ToastStacking::Some, "Some, then \"+N more\""),
                    (ToastStacking::All, "All of them"),
                    (ToastStacking::Pager, "One at a time"),
                    (ToastStacking::Deck, "A deck"),
                    (ToastStacking::Summary, "One summary card"),
                ]
            ),
            number!(
                "notices.toasts.max_visible",
                "At most",
                "How many cards show at once (with \"Some\" and the deck).",
                notices.toasts.max_visible: u8,
                2..=5,
                1,
                "cards"
            )
            .under(stacking_limits),
            choice!(
                "notices.toasts.order",
                "Order",
                "Errors first, then warnings, then the rest; or simply by time.",
                notices.toasts.order,
                [
                    (ToastOrder::Severity, "Most important, then newest"),
                    (ToastOrder::NewestTop, "Newest on top"),
                    (ToastOrder::NewestBottom, "Newest at the bottom"),
                ]
            ),
            choice!(
                "notices.toasts.animation",
                "How they come",
                "Slide in from their side, fade in, fall in from above, or simply appear.",
                notices.toasts.animation,
                [
                    (ToastAnimation::Slide, "Slide in"),
                    (ToastAnimation::Fade, "Fade in"),
                    (ToastAnimation::Drop, "Drop in"),
                    (ToastAnimation::None, "No animation"),
                ]
            ),
            number!(
                "notices.toasts.info_secs",
                "Hints stay",
                "Seconds a hint (blue) stays; 0 = until it is closed.",
                notices.toasts.info_secs: u8,
                0..=60,
                1,
                "s"
            ),
            number!(
                "notices.toasts.success_secs",
                "Confirmations stay",
                "Seconds a confirmation (green) stays; 0 = until it is closed.",
                notices.toasts.success_secs: u8,
                0..=60,
                1,
                "s"
            ),
            number!(
                "notices.toasts.warning_secs",
                "Warnings stay",
                "Seconds a warning (yellow) stays; 0 = until it is closed.",
                notices.toasts.warning_secs: u8,
                0..=60,
                1,
                "s"
            ),
            number!(
                "notices.toasts.error_secs",
                "Errors stay",
                "Seconds an error (red) stays; 0 = until it is closed.",
                notices.toasts.error_secs: u8,
                0..=60,
                1,
                "s"
            ),
        ],
    },
    Card {
        title: "Banners",
        description: "States that hold lie over the top of the video, one at a time; × hides one until it changes.",
        fields: &[
            choice!(
                "notices.banners.animation",
                "How they come",
                "Slide down like a curtain, fade in, or simply appear.",
                notices.banners.animation,
                [
                    (BannerAnimation::Curtain, "Curtain"),
                    (BannerAnimation::Fade, "Fade in"),
                    (BannerAnimation::None, "No animation"),
                ]
            ),
            choice!(
                "notices.banners.new_error",
                "A new error",
                "A new error banner lights up once.",
                notices.banners.new_error,
                [
                    (BannerAttention::Pulse, "Lights up once"),
                    (BannerAttention::Nothing, "Nothing")
                ]
            ),
        ],
    },
    Card {
        title: "History",
        description: "The messages of this session: the bell in the menu bar lists the last three, \"Show all\" the \
                      rest in the Logs panel.",
        fields: &[
            switch!(
                "notices.history.bell",
                "Bell in the menu bar",
                "Off: no bell; the Logs panel's \"Messages\" log keeps them.",
                notices.history.bell
            ),
            choice!(
                "notices.history.badge",
                "Number on the bell",
                "What the number counts until the list is opened.",
                notices.history.badge,
                [
                    (BellBadge::ErrorsWarnings, "Unread errors and warnings"),
                    (BellBadge::All, "Every unread message"),
                    (BellBadge::None, "No number"),
                ]
            )
            .under(bell_on),
        ],
    },
    Card {
        title: "Desktop notifications",
        description: "Which kinds also go to the desktop is set per kind on the next tab; errors go there while \
                      RustCut is not in front.",
        fields: &[
            switch!(
                "notices.desktop.enabled",
                "Show notifications",
                "Master switch for every desktop notification.",
                notices.desktop.enabled
            ),
            number!(
                "notices.desktop.timeout_secs",
                "Shown for",
                "Seconds a notification stays; 0 = until it is dismissed.",
                notices.desktop.timeout_secs: u16,
                0..=600,
                1,
                "s"
            )
            .under(desktop_on),
        ],
    },
];
