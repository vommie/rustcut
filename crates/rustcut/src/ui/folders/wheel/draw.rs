//! What a folder wheel shows, without its window (P12 3.4-3.7): the main window's wheel and the small wheel in Settings
//! draw the same views. Every length comes from the settings' percents of the diameter (`geometry::share`), so the
//! wheel looks the same at every size.

use std::f32::consts::TAU;
use std::path::Path;

use rustcut_core::folders::{EntryRef, FolderId, FolderTree, Level, Slot};
use rustcut_core::settings::{ColorMode, FolderWheelSettings, PictureShape, SpokePictures, WheelLabels, WheelMiddle};

use super::anim::Pose;
use super::{geometry, label};
use crate::ui::folders::pictures::Pictures;
use crate::ui::folders::view;
use crate::{FolderPictureView, WheelLookView, WheelMiddleView, WheelSlotView};

/// Tenths of a percent of the diameter: the ring, the pointed line, the thin line and the marks (P12: they grow too).
const RING: u16 = 4;
const THIN: u16 = 2;
const MARK: u16 = 6;
/// Where the mark of the current folder stands, inside the outer edge.
const ARC_INSET: u16 = 12;
/// How much of a place's angle that mark takes.
const ARC_SHARE: f32 = 0.6;
/// How strong a color lies over a picture when the pointer is elsewhere and on it (the user's note).
const TINT: f32 = 0.22;
const TINT_POINTED: f32 = 0.1;
/// How dark the pointed picture stays: it turns bright.
const DIM_POINTED: f32 = 0.15;

const fn shape(shape: PictureShape) -> i32 {
    match shape {
        PictureShape::Round => 0,
        PictureShape::Rounded => 1,
        PictureShape::Square => 2,
    }
}

/// The spokes' shapes in a square of `diameter`.
pub fn face(w: &FolderWheelSettings, diameter: f32) -> geometry::Face {
    let (inner, outer) = geometry::radii(diameter, w.middle_size);
    let c = diameter / 2.0;
    geometry::Face {
        cx: c,
        cy: c,
        inner,
        outer,
        gap: geometry::share(diameter, w.gap),
        corner: geometry::share(diameter, w.corners),
    }
}

/// How the wheel looks, for Slint.
pub fn look(w: &FolderWheelSettings, diameter: f32, font: f32) -> WheelLookView {
    let (inner, outer) = geometry::radii(diameter, w.middle_size);
    WheelLookView {
        size: diameter,
        inner,
        outer,
        middle: (inner - geometry::share(diameter, w.middle_gap)).max(1.0),
        font_size: font,
        labels: match w.labels {
            WheelLabels::PictureAndName => 0,
            WheelLabels::Picture => 1,
            WheelLabels::Name => 2,
        },
        shape: shape(w.wheel_shape),
        tint: w.color_mode == ColorMode::BackgroundAndIcons,
        opacity: f32::from(w.opacity) / 100.0,
        ring: geometry::line(diameter, RING),
        // Without a gap there is no line between the spokes (the user's choice).
        thin: if w.gap == 0 {
            0.0
        } else {
            geometry::line(diameter, THIN)
        },
        mark: geometry::line(diameter, MARK),
    }
}

/// The font of a wheel of `diameter`.
pub fn font(diameter: f32) -> f32 {
    (diameter * 0.034).clamp(8.0, 22.0)
}

/// One wheel's drawing input.
pub struct Scene<'a> {
    pub wheel: &'a FolderWheelSettings,
    pub tree: &'a FolderTree,
    pub level: &'a Level,
    pub diameter: f32,
    /// Physical pixels per logical pixel (the masks of pictures).
    pub scale: f32,
    pub highlight: Option<usize>,
    pub current: Option<FolderId>,
    pub fill: Option<(usize, f32)>,
    pub pictures: &'a Pictures,
    pub missing: &'a dyn Fn(&Path) -> bool,
    /// Where a spoke stands in a running animation.
    pub pose: &'a dyn Fn(usize) -> Pose,
    /// How far the current folder's spoke lit up (0..1).
    pub flash: f32,
}

impl Scene<'_> {
    pub fn slots(&self) -> Vec<WheelSlotView> {
        let w = self.wheel;
        let n = self.level.slots.len();
        let face = face(w, self.diameter);
        let (inner, outer) = (face.inner, face.outer);
        let c = self.diameter / 2.0;
        let font = font(self.diameter);
        let step = TAU / f32::from(u16::try_from(n.max(1)).unwrap_or(u16::MAX));
        let mid = f32::midpoint(inner, outer);
        let chord = 2.0 * mid * (step / 2.0).sin();
        let box_w = (chord - 6.0).min((outer - inner) * 1.5).max(24.0);
        let box_h = (outer - inner) * 0.86;
        self.level
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let (lx, ly) = geometry::at(c, c, mid, geometry::angle(i, n));
                let highlighted = self.highlight == Some(i);
                let entry = match slot {
                    Slot::Entry(id) => self.tree.entry(*id),
                    _ => None,
                };
                let color = entry.and_then(|e| view::color(e.color()));
                let tinted = color.is_some() && (w.color_mode != ColorMode::Pointed || highlighted);
                let name = match slot {
                    Slot::More(ids) => format!("More ... ({})", ids.len()),
                    _ => entry.map(EntryRef::wheel_name).unwrap_or_default().to_owned(),
                };
                let backdrop = entry.and_then(|e| self.backdrop(e, &face, i, n));
                let has_backdrop = backdrop.is_some();
                let label = label::fit(&name, if has_backdrop { chord.max(box_w) } else { box_w }, font);
                let holds = self.current.is_some_and(|id| slot.holds(self.tree, id));
                let (bx, by, bw, bh) = face.spoke_box(i, n);
                let warned = entry.is_some_and(|e| self.is_missing(e));
                let (warn_x, warn_y) =
                    geometry::at(c, c, inner + geometry::share(self.diameter, 40), geometry::angle(i, n));
                let a = geometry::angle(i, n);
                let spread = step * ARC_SHARE / 2.0;
                let pose = (self.pose)(i);
                WheelSlotView {
                    segment: face.spoke(i, n).into(),
                    fill: self
                        .fill
                        .filter(|(f, _)| *f == i)
                        .map(|(_, p)| face.fill(i, n, p))
                        .unwrap_or_default()
                        .into(),
                    mark: if slot.opens(self.tree).is_some() {
                        geometry::group_mark(c, outer, i, n).into()
                    } else {
                        "".into()
                    },
                    current: if holds {
                        let radius = outer - geometry::share(self.diameter, ARC_INSET);
                        geometry::arc(c, radius, a - spread, a + spread).into()
                    } else {
                        "".into()
                    },
                    x: lx - box_w / 2.0,
                    y: ly - box_h / 2.0,
                    w: box_w,
                    h: box_h,
                    name: label.text().into(),
                    scale: label.scale,
                    picture: entry.map_or_else(FolderPictureView::default, |e| view::picture(e, self.pictures)),
                    filled: !matches!(slot, Slot::Empty),
                    highlighted,
                    tint: color.unwrap_or_default(),
                    tinted,
                    more: matches!(slot, Slot::More(_)),
                    has_backdrop,
                    backdrop: backdrop.unwrap_or_default(),
                    bx,
                    by,
                    bw,
                    bh,
                    shade: self.shade(highlighted, color.is_some()),
                    warn: warned,
                    warn_x,
                    warn_y,
                    flash: if holds { self.flash } else { 0.0 },
                    hint: "".into(),
                    alpha: pose.alpha,
                    zoom: pose.zoom,
                    turn: pose.turn,
                }
            })
            .collect()
    }

    /// A picture in the spoke's shape while pictures are backgrounds; only own pictures fill a spoke.
    fn backdrop(&self, entry: EntryRef<'_>, face: &geometry::Face, index: usize, n: usize) -> Option<slint::Image> {
        if self.wheel.spoke_pictures != SpokePictures::Background {
            return None;
        }
        let picture = entry.picture()?;
        let (x, y, w, h) = face.spoke_box(index, n);
        let scale = self.scale.max(1.0);
        let shape = geometry::Face {
            cx: face.cx - x,
            cy: face.cy - y,
            ..*face
        }
        .scaled(scale)
        .spoke(index, n);
        self.pictures.spoke(&picture, w * scale, h * scale, shape)
    }

    /// How strong the layer over a picture is: dark without a color, the color's own light tint with one.
    fn shade(&self, pointed: bool, colored: bool) -> f32 {
        match (colored, pointed) {
            (true, false) => TINT,
            (true, true) => TINT_POINTED,
            (false, false) => f32::from(self.wheel.picture_dim) / 100.0,
            (false, true) => DIM_POINTED.min(f32::from(self.wheel.picture_dim) / 100.0),
        }
    }

    fn is_missing(&self, entry: EntryRef<'_>) -> bool {
        match entry {
            EntryRef::Node(node) => node.path.as_deref().is_some_and(|p| (self.missing)(p)),
            EntryRef::Source(_) => false,
        }
    }
}

/// The middle for the pointed place, or (nothing pointed) the current folder, faded.
pub struct MiddleInput<'a> {
    pub wheel: &'a FolderWheelSettings,
    pub tree: &'a FolderTree,
    pub level: &'a Level,
    pub diameter: f32,
    pub highlight: Option<usize>,
    pub current: Option<FolderId>,
    pub pictures: &'a Pictures,
}

impl MiddleInput<'_> {
    pub fn view(&self) -> WheelMiddleView {
        let pointed = self.highlight.and_then(|i| self.level.slots.get(i));
        let entry = match pointed {
            Some(Slot::Entry(id)) => self.tree.entry(*id),
            _ => None,
        };
        let resting = pointed.is_none();
        let entry = entry.or_else(|| self.current.filter(|_| resting).and_then(|id| self.tree.entry(id)));
        let name = match pointed {
            Some(Slot::More(ids)) => format!("More ... ({})", ids.len()),
            _ => entry.map(|e| e.wheel_name().to_owned()).unwrap_or_default(),
        };
        let (inner, _) = geometry::radii(self.diameter, self.wheel.middle_size);
        let text = label::fit(&name, (inner - 4.0) * 2.0 * 0.76, font(self.diameter));
        WheelMiddleView {
            // The picture (the default); an entry without one shows its first letter, like everywhere else. "More ..."
            // has neither: its name.
            shows_picture: self.wheel.middle == WheelMiddle::Picture && entry.is_some(),
            picture: entry.map(|e| view::picture(e, self.pictures)).unwrap_or_default(),
            name: text.text().into(),
            scale: text.scale,
            resting: resting && entry.is_some(),
            ..WheelMiddleView::default()
        }
    }
}
