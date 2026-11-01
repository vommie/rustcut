//! Settings > Player > Timeline thumbnails (P12 block 6, user decision Q-P12e).

use rustcut_core::settings::{
    ThumbDensity, ThumbLook, ThumbMissing, ThumbPlace, ThumbPortrait, ThumbProgress, ThumbZone,
};

use super::Card;
use crate::ui::settings::field::{Field, choice, number, switch};

fn thumbnails_on(settings: &rustcut_core::settings::Settings) -> bool {
    settings.thumbnails.enabled
}

pub const CARD: Card = Card {
    title: "Timeline thumbnails",
    description: "A picture of the place under the pointer. They are made in the background, coarse first, then finer.",
    fields: &[
        switch!(
            "thumbnails.enabled",
            "Show thumbnails",
            "Off: no picture, and no thumbnails are made.",
            thumbnails.enabled
        ),
        choice!(
            "thumbnails.look",
            "Time",
            "Where the time stands.",
            thumbnails.look,
            [
                (ThumbLook::TimeBelow, "Below the picture"),
                (ThumbLook::TimeInside, "In the picture"),
                (ThumbLook::PictureOnly, "Only the picture")
            ]
        )
        .under(thumbnails_on),
        number!(
            "thumbnails.width",
            "Width",
            "The picture's width. 0 fits itself: 240 px, at most 60 % of the video column.",
            thumbnails.width: u16,
            0..=640,
            20,
            "px"
        )
        .under(thumbnails_on),
        choice!(
            "thumbnails.portrait",
            "Portrait videos",
            "How large the picture of a portrait video is, compared to a wide one.",
            thumbnails.portrait,
            [
                (ThumbPortrait::SameHeight, "Same height"),
                (ThumbPortrait::SameArea, "Same area"),
                (ThumbPortrait::SameWidth, "Same width")
            ]
        )
        .under(thumbnails_on),
        number!(
            "thumbnails.portrait_percent",
            "Portrait size",
            "Fine adjustment of the portrait picture: +50 makes it half as large again.",
            thumbnails.portrait_percent: i16,
            -50..=100,
            5,
            "%"
        )
        .under(thumbnails_on),
        choice!(
            "thumbnails.place",
            "Place",
            "Where the picture stands.",
            thumbnails.place,
            [
                (ThumbPlace::FollowPointer, "Follows the pointer"),
                (ThumbPlace::Center, "In the middle")
            ]
        )
        .under(thumbnails_on),
        choice!(
            "thumbnails.zone",
            "Shows on",
            "Which part of the timeline shows it.",
            thumbnails.zone,
            [
                (ThumbZone::RulerAndTrack, "Ruler and track"),
                (ThumbZone::Ruler, "Ruler"),
                (ThumbZone::Track, "Track")
            ]
        )
        .under(thumbnails_on),
        number!(
            "thumbnails.delay_ms",
            "Delay",
            "Waits this long when the pointer enters the timeline, so it does not flash on the way to the buttons.",
            thumbnails.delay_ms: u32,
            0..=2000,
            50,
            "ms"
        )
        .under(thumbnails_on),
        switch!(
            "thumbnails.while_dragging",
            "While dragging",
            "The big picture already follows a drag, so the thumbnail would show the same.",
            thumbnails.while_dragging
        )
        .under(thumbnails_on),
        choice!(
            "thumbnails.missing",
            "No picture near",
            "What shows while the nearest picture is far from the place.",
            thumbnails.missing,
            [
                (ThumbMissing::NearestWithDot, "Nearest picture with a dot"),
                (ThumbMissing::Nearest, "Nearest picture"),
                (ThumbMissing::EmptyField, "Empty field"),
                (ThumbMissing::Nothing, "Nothing")
            ]
        )
        .under(thumbnails_on),
        choice!(
            "thumbnails.progress",
            "Progress",
            "The strip over the ruler has a fine line per thumbnail; the status bar counts them when it takes a while.",
            thumbnails.progress,
            [
                (ThumbProgress::Strip, "Strip over the ruler"),
                (ThumbProgress::StatusBar, "Status bar"),
                (ThumbProgress::Nothing, "Nothing")
            ]
        )
        .under(thumbnails_on),
        switch!(
            "thumbnails.show_filters",
            "Show crop and rotation",
            "The picture shows the crop and the orientation like the big picture. The stored thumbnails stay as they are.",
            thumbnails.show_filters
        )
        .under(thumbnails_on),
        choice!(
            "thumbnails.density",
            "Density",
            "How close they are: an hour of video gets 600, 900 or 1500.",
            thumbnails.density,
            [
                (ThumbDensity::Coarse, "Coarse"),
                (ThumbDensity::Normal, "Normal"),
                (ThumbDensity::Fine, "Fine")
            ]
        )
        .under(thumbnails_on),
        number!(
            "thumbnails.workers",
            "Workers",
            "How many are made at the same time. 0 = automatic: half the cores, at most 4.",
            thumbnails.workers: u8,
            0..=16,
            1,
            ""
        )
        .under(thumbnails_on),
        number!(
            "thumbnails.nice",
            "Priority",
            "19 is the lowest. While a job renders, thumbnails go just ahead of it.",
            thumbnails.nice: u8,
            0..=19,
            1,
            ""
        )
        .under(thumbnails_on),
        number!(
            "thumbnails.quality",
            "Quality",
            "WebP quality of the stored pictures. It counts for pictures made from now on.",
            thumbnails.quality: u8,
            0..=100,
            5,
            ""
        )
        .under(thumbnails_on),
        Field::button(
            "thumbnails.clear",
            "Thumbnail cache",
            "Empties the cache of job sources. It empties by itself with the jobs.",
            "Empty"
        ),
    ],
};
