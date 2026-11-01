//! Target Folders (P11.3, user decision Q-FP2): the folder wheel's options and how both folder pickers show pictures
//! and colors. The tree itself is the page's own editor (`folder_tree/`).

use rustcut_core::settings::{
    ColorMode, PictureShape, Settings, SpokePictures, WheelButton, WheelLabels, WheelLevels, WheelMiddle, WheelOpening,
    WheelSize,
};

use super::Card;
use crate::ui::settings::field::{choice, number, percent, switch};

fn mouse_trigger(s: &Settings) -> bool {
    s.folder_wheel.button != WheelButton::Off
}

const fn pointer_center(s: &Settings) -> bool {
    s.folder_wheel.pointer_center
}

fn pictures_fill(s: &Settings) -> bool {
    s.folder_wheel.spoke_pictures == SpokePictures::Background
}

const fn video_dimmed(s: &Settings) -> bool {
    s.folder_wheel.dim
}

const fn animated(s: &Settings) -> bool {
    s.folder_wheel.animations
}

pub static FOLDERS: &[Card] = &[Card {
    title: "Opening and pointing",
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
        switch!(
            "folder_wheel.pointer_center",
            "Start the pointer in the middle",
            "Every level starts with the pointer in the wheel's middle, wherever it really is: a short move in a \
             direction chooses that spoke. A dot shows the pointer; the real one stays where it was.",
            folder_wheel.pointer_center
        ),
        percent!(
            "folder_wheel.pointer_distance",
            "Move to choose",
            "How far the pointer moves until it chooses a spoke, in percent of the wheel's size.",
            folder_wheel.pointer_distance,
            20..=150,
            5
        )
        .under(pointer_center),
    ],
},
Card {
    title: "Look",
    description: "How the wheel looks. Lengths are percents of the wheel's size, so it looks the same at every size. \
                  The small wheel beside these options shows every change at once.",
    fields: &[
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
            "What the middle shows for the folder or group under the pointer: its picture (its first letter when \
                 it has none), or its name.",
            folder_wheel.middle,
            [(WheelMiddle::Picture, "Picture"), (WheelMiddle::Name, "Name")]
        ),
        percent!(
            "folder_wheel.gap",
            "Gap between spokes",
            "How far the spokes stand apart. 0 = no gap and no line: they touch.",
            folder_wheel.gap,
            0..=60,
            1
        ),
        percent!(
            "folder_wheel.middle_gap",
            "Gap around the middle",
            "How far the spokes stand from the middle.",
            folder_wheel.middle_gap,
            0..=50,
            1
        ),
        percent!(
            "folder_wheel.corners",
            "Rounded corners",
            "How round the spokes' corners are. 0 = sharp.",
            folder_wheel.corners,
            0..=50,
            1
        ),
        number!(
            "folder_wheel.middle_size",
            "Size of the middle",
            "How large the middle is, in percent of the wheel. Larger: more room for its picture.",
            folder_wheel.middle_size: u8,
            24..=46,
            1,
            "%"
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
        choice!(
            "folder_wheel.spoke_pictures",
            "Own pictures",
            "Circle: a small circle above the name. Background: the picture fills the spoke, centered, and the name \
             stands on it. Icons and letters stay small.",
            folder_wheel.spoke_pictures,
            [(SpokePictures::Circle, "Circle"), (SpokePictures::Background, "Background")]
        ),
        number!(
            "folder_wheel.picture_dim",
            "Dim pictures",
            "How dark a picture is behind its name; the pointed one turns bright. A folder with a color takes a light \
             tint of it instead.",
            folder_wheel.picture_dim: u8,
            0..=80,
            5,
            "%"
        )
        .under(pictures_fill),
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
        number!(
            "folder_wheel.video_dim",
            "How dark",
            "How dark the video is behind the wheel.",
            folder_wheel.video_dim: u8,
            0..=80,
            5,
            "%"
        )
        .under(video_dimmed),
    ],
},
Card {
    title: "Colors",
    description: "Where an entry's color shows. A picture with a color keeps the color as a border.",
    fields: &[
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
},
Card {
    title: "Animations",
    description: "The wheel never waits for an animation: it works while it moves.",
    fields: &[
        switch!(
            "folder_wheel.animations",
            "Animations",
            "The main switch for everything below: without it the wheel simply appears.",
            folder_wheel.animations
        ),
        choice!(
            "folder_wheel.opening",
            "Opening",
            "How the wheel appears.",
            folder_wheel.opening,
            [
                (WheelOpening::Spokes, "One spoke at a time"),
                (WheelOpening::Fade, "Fade in"),
                (WheelOpening::Fan, "Fan out"),
                (WheelOpening::Spin, "Spin"),
                (WheelOpening::Off, "Off")
            ]
        )
        .under(animated),
        choice!(
            "folder_wheel.levels",
            "Changing levels",
            "How a group opens and how the wheel goes back.",
            folder_wheel.levels,
            [
                (WheelLevels::Zoom, "Zoom"),
                (WheelLevels::Spoke, "From the spoke"),
                (WheelLevels::Turn, "Turn"),
                (WheelLevels::Off, "Off")
            ]
        )
        .under(animated),
        number!(
            "folder_wheel.animation_speed",
            "Duration",
            "How long the animations take: 50 % is twice as fast, 200 % takes twice as long.",
            folder_wheel.animation_speed: u16,
            50..=200,
            10,
            "%"
        )
        .under(animated),
        switch!(
            "folder_wheel.pulse",
            "Current folder lights up",
            "Once, when the wheel opens.",
            folder_wheel.pulse
        )
        .under(animated),
        switch!(
            "folder_wheel.crossfade",
            "Middle fades over",
            "The middle's picture changes softly instead of at once.",
            folder_wheel.crossfade
        )
        .under(animated),
    ],
}];
