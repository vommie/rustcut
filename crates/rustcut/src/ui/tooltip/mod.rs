//! The tooltips' Rust side (M5.5): `TooltipLogic` in `ui/tooltip.slint` asks for the key caps of a shortcut text
//! ([`keys`]) and for the bubble's place ([`placement`]); both are pure and answered on the UI thread inside Slint's
//! bindings. Every window with a `TooltipLayer` installs them (each window has its own globals).

pub mod keys;
pub mod placement;
#[cfg(test)]
mod placement_tests;

use slint::{ComponentHandle, ModelRc, VecModel};

use self::placement::{Rect, Side};
use crate::{IconId, TipKey, TipKeys, TipPlacement, TipRect, TipSide, TooltipLogic};

/// Answers `TooltipLogic`'s callbacks for `window`.
pub fn install<C>(window: &C)
where
    C: ComponentHandle,
    for<'a> TooltipLogic<'a>: slint::Global<'a, C>,
{
    let logic = window.global::<TooltipLogic<'_>>();
    logic.on_key_caps(|text| key_caps(&text));
    logic.on_place(|anchor, width, height, area_width, area_height, beside| {
        place(&anchor, width, height, area_width, area_height, beside)
    });
}

fn key_caps(text: &str) -> TipKeys {
    let caps = keys::caps(text);
    let rows: Vec<TipKey> = caps
        .caps
        .iter()
        .map(|cap| TipKey {
            text: cap.text.as_str().into(),
            icon: cap.icon.map_or(IconId::None, crate::icons::Icon::to_slint),
            alternative: cap.alternative,
        })
        .collect();
    TipKeys {
        caps: ModelRc::new(VecModel::from(rows)),
        keys: caps.keys,
        alternatives: caps.alternatives,
    }
}

fn rect(r: &TipRect) -> Rect {
    Rect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    }
}

fn place(anchor: &TipRect, width: f32, height: f32, area_width: f32, area_height: f32, beside: bool) -> TipPlacement {
    to_slint(placement::place(
        rect(anchor),
        width,
        height,
        area_width,
        area_height,
        beside,
    ))
}

/// A bubble beside a list's row (the job card): the row is cut to the list first, then the bubble goes beside it.
pub fn place_beside_row(anchor: &TipRect, list: &TipRect, size: (f32, f32), area: (f32, f32)) -> TipPlacement {
    let anchor = placement::clip_rows(rect(anchor), rect(list));
    to_slint(placement::place(anchor, size.0, size.1, area.0, area.1, true))
}

fn to_slint(p: placement::Placement) -> TipPlacement {
    TipPlacement {
        x: p.x,
        y: p.y,
        side: match p.side {
            Side::Below => TipSide::Below,
            Side::Above => TipSide::Above,
            Side::Right => TipSide::Right,
            Side::Left => TipSide::Left,
            Side::Over => TipSide::Over,
        },
        arrow: p.arrow,
    }
}
