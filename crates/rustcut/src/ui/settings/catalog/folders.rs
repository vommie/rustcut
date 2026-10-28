//! Target Folders (P11.3, user decision Q-FP2): the folder wheel's options and how both folder pickers show pictures
//! and colors. The tree itself is the page's own editor (`folder_tree/`).

use rustcut_core::settings::{ColorMode, PictureShape, Settings, WheelButton, WheelLabels, WheelMiddle, WheelSize};

use super::Card;
use crate::ui::settings::field::{choice, number, switch};

fn mouse_trigger(s: &Settings) -> bool {
    s.folder_wheel.button != WheelButton::Off
}

pub static FOLDERS: &[Card] = &[Card {
    title: "Folder wheel",
    description: "A wheel of target folders over the video: hold the middle mouse button over the video, or press \
                      its key anywhere (Hotkeys). Pointing at a folder and letting go chooses it; a group opens after \
                      a short rest on it. Choosing never saves the job.",
    fields: &[
        choice!(
            "folder_wheel.button",
            "Mouse button",
            "Holding this button over the video opens the wheel; Off leaves only the key.",
            folder_wheel.button,
            [(WheelButton::Middle, "Middle"), (WheelButton::Off, "Off")]
        ),
        number!(
            "folder_wheel.hold_ms",
            "Hold time",
            "How long the button is held before the wheel opens; a ring fills meanwhile. A shorter press stays a \
             normal middle click.",
            folder_wheel.hold_ms: u16,
            50..=2000,
            50,
            "ms"
        )
        .under(mouse_trigger),
        number!(
            "folder_wheel.dwell_ms",
            "Rest on a group",
            "How long the pointer rests on a group before it opens (its field fills), and in the middle before \
             the wheel goes one level up.",
            folder_wheel.dwell_ms: u16,
            100..=3000,
            50,
            "ms"
        ),
        switch!(
            "folder_wheel.push",
            "Push through",
            "Moving the pointer past a group's outer edge opens it at once.",
            folder_wheel.push
        ),
        number!(
            "folder_wheel.places",
            "Places per level",
            "How many fields a level has. When a level has more entries, its last place becomes \"More ...\".",
            folder_wheel.places: u8,
            4..=12,
            1,
            ""
        ),
        choice!(
            "folder_wheel.divided",
            "Places",
            "Fixed: every level has the same places (a favorite keeps its direction). Divided: a level with fewer \
                 entries has bigger fields.",
            folder_wheel.divided,
            [(false, "Fixed"), (true, "Divided")]
        ),
        choice!(
            "folder_wheel.size",
            "Size",
            "Automatic: 40 % of the window's shorter side, never too small to hit, almost the whole window when it \
                 is small.",
            folder_wheel.size,
            [
                (WheelSize::Automatic, "Automatic"),
                (WheelSize::Small, "Small"),
                (WheelSize::Medium, "Medium"),
                (WheelSize::Large, "Large")
            ]
        ),
        choice!(
            "folder_wheel.labels",
            "Fields show",
            "What a field of the wheel shows; the middle always shows the pointed entry's whole name.",
            folder_wheel.labels,
            [
                (WheelLabels::PictureAndName, "Picture and name"),
                (WheelLabels::Picture, "Picture"),
                (WheelLabels::Name, "Name")
            ]
        ),
        choice!(
            "folder_wheel.middle",
            "Middle shows",
            "What the middle shows for the folder or group under the pointer: its picture (its name when it has \
                 none), or always its name.",
            folder_wheel.middle,
            [(WheelMiddle::Picture, "Picture"), (WheelMiddle::Name, "Name")]
        ),
        number!(
            "folder_wheel.opacity",
            "Opacity",
            "How much the wheel's fields cover the video and the panels behind them.",
            folder_wheel.opacity: u8,
            20..=100,
            5,
            "%"
        ),
        switch!(
            "folder_wheel.breadcrumb",
            "Path above the wheel",
            "Shows the groups opened so far above the wheel.",
            folder_wheel.breadcrumb
        ),
        switch!(
            "folder_wheel.dim",
            "Dim the video",
            "The video is darker behind the wheel.",
            folder_wheel.dim
        ),
        switch!(
            "folder_wheel.animations",
            "Animations",
            "The wheel and its levels fade and grow in.",
            folder_wheel.animations
        ),
        choice!(
            "folder_wheel.wheel_shape",
            "Shape in the wheel",
            "The shape own pictures and letters have in the wheel.",
            folder_wheel.wheel_shape,
            [
                (PictureShape::Round, "Round"),
                (PictureShape::Rounded, "Rounded"),
                (PictureShape::Square, "Square")
            ]
        ),
        choice!(
            "folder_wheel.list_shape",
            "Shape in lists",
            "The shape own pictures and letters have in the lists.",
            folder_wheel.list_shape,
            [
                (PictureShape::Round, "Round"),
                (PictureShape::Rounded, "Rounded"),
                (PictureShape::Square, "Square")
            ]
        ),
        choice!(
            "folder_wheel.color_mode",
            "Color shows",
            "Background + colored icons: the wheel's fields are tinted (stronger when pointed), lists show a \
                 stripe, icons take the color; own pictures stay as they are. Background: icons in the text color. \
                 When pointed: only the pointed field is tinted.",
            folder_wheel.color_mode,
            [
                (ColorMode::BackgroundAndIcons, "Background + icons"),
                (ColorMode::Background, "Background"),
                (ColorMode::Pointed, "When pointed")
            ]
        ),
    ],
}];
