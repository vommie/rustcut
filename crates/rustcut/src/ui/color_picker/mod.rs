//! Rust side of `ui/color_picker*.slint` (taken over from RustView's `src/ui/color_picker.rs` on the user's request, P8):
//! the hex codec the Slint fields ask (Slint cannot take a string apart) and the picker in a window of its own that
//! belongs to the window it was opened from (X11: `WM_TRANSIENT_FOR`, so it stays above that window; centered on it).
//!
//! Call [`install`] once for every window that contains a `ColorPicker` or a `ColorSwatchButton`; [`open`] shows the
//! picker. It is modal for its parent (D-P9h, review 69): the caller dims the parent until the picker reports its end,
//! and a click on the dimmed parent calls [`raise`]. UI thread only.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use slint::winit_030::WinitWindowAccessor as _;
use slint::{Color, ComponentHandle, Global, SharedString};

use crate::desktop::x11_dialog;
use crate::util::hex_color;
use crate::{ColorCodec, ColorPickerWindow, TooltipSettings};

pub fn install<C: ComponentHandle>(component: &C)
where
    for<'a> ColorCodec<'a>: Global<'a, C>,
{
    let codec = component.global::<ColorCodec<'_>>();
    codec.on_to_hex(|color, with_alpha| to_hex(color, with_alpha).into());
    codec.on_is_valid(|text, with_alpha| parse(&text, with_alpha).is_some());
    codec.on_from_hex(|text| parse(&text, true).unwrap_or_else(|| Color::from_argb_u8(255, 0, 0, 0)));
}

fn to_hex(color: Color, with_alpha: bool) -> String {
    let (r, g, b) = (color.red(), color.green(), color.blue());
    if with_alpha {
        hex_color::format_rgba([r, g, b, color.alpha()])
    } else {
        hex_color::format_rgb([r, g, b])
    }
}

/// `None` unless `text` is a color in the accepted forms (`#rgb`, `#rrggbb`, and with alpha `#rgba`, `#rrggbbaa`).
fn parse(text: &SharedString, with_alpha: bool) -> Option<Color> {
    let [r, g, b, a] = hex_color::parse_rgba(text)?;
    Some(Color::from_argb_u8(if with_alpha { a } else { 255 }, r, g, b))
}

/// The opaque color of a `#rrggbb` text (`None` for anything else).
pub fn color_of(text: &str) -> Option<Color> {
    hex_color::parse_rgb(text).map(|[r, g, b]| Color::from_rgb_u8(r, g, b))
}

/// The `#rrggbb` text of a color (its alpha is dropped).
pub fn hex_of(color: Color) -> String {
    to_hex(color, false)
}

thread_local! {
    static WINDOW: RefCell<Option<ColorPickerWindow>> = const { RefCell::new(None) };
    /// The parent the window manager is told about when the picker's window is set up the first time.
    static PENDING_PARENT: Cell<Option<u32>> = const { Cell::new(None) };
    /// What the opener does when the picker ends (its parent stops being dimmed).
    static ON_END: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// The picker ended (OK, Cancel, closed, replaced): tells the opener once.
fn ended() {
    let on_end = ON_END.with(|slot| slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()));
    if let Some(on_end) = on_end {
        on_end();
    }
}

/// Brings the open picker to the front and gives it the keyboard (a click on its dimmed parent).
pub fn raise() {
    WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref()
            && window.window().is_visible()
        {
            window
                .window()
                .with_winit_window(slint::winit_030::winit::window::Window::focus_window);
        }
    });
}

/// What the picker window starts with.
pub struct PickerRequest {
    pub title: String,
    pub initial: Color,
    /// Offers a "Default" button that restores this color.
    pub default: Option<Color>,
    pub show_alpha: bool,
    /// The tooltips of the window it was opened from (enabled, delay in ms).
    pub tooltips: (bool, i64),
}

/// Opens the color picker in a window of its own over `parent`. Every change the user makes goes to `on_change` at
/// once; Cancel, Escape or closing the window reports the initial color again, OK keeps the last one. `on_end` runs
/// once when the picker ends in any way. A picker that is already open is replaced (its `on_end` runs first).
pub fn open<P: ComponentHandle>(
    parent: &P,
    request: &PickerRequest,
    on_change: impl Fn(Color) + 'static,
    on_end: impl Fn() + 'static,
) {
    ended();
    let created = WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            match ColorPickerWindow::new() {
                Ok(window) => {
                    install(&window);
                    crate::ui::tooltip::install(&window);
                    hint_when_set_up(&window);
                    *slot = Some(window);
                }
                Err(e) => tracing::error!(error = %e, "[ColorPicker] The picker window could not be made"),
            }
        }
        slot.as_ref().map(ComponentHandle::as_weak)
    });
    let Some(window) = created.and_then(|weak| weak.upgrade()) else {
        return;
    };

    window.set_dialog_title(request.title.as_str().into());
    window.set_show_alpha(request.show_alpha);
    window.set_default_color(request.default.unwrap_or_else(|| Color::from_argb_u8(0, 0, 0, 0)));
    window.set_original(request.initial);
    window.set_value(request.initial);
    let tooltips = window.global::<TooltipSettings<'_>>();
    tooltips.set_enabled(request.tooltips.0);
    tooltips.set_delay(request.tooltips.1);

    let on_change: Rc<dyn Fn(Color)> = Rc::new(on_change);
    let initial = request.initial;
    let changed = Rc::clone(&on_change);
    window.on_edited(move |color| changed(color));
    let weak = window.as_weak();
    window.on_cancelled(move || {
        on_change(initial);
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
        ended();
    });
    let weak = window.as_weak();
    window.on_accepted(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
        ended();
    });
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(window) = weak.upgrade() {
            window.invoke_cancelled();
        }
        slint::CloseRequestResponse::HideWindow
    });

    // The window manager learns the parent before the window is mapped: now when the window exists from an earlier
    // opening, else when Slint sets it up.
    let parent_xid = x11_dialog::xid(parent.window());
    match (x11_dialog::xid(window.window()), parent_xid) {
        (Some(dialog), Some(parent)) => {
            x11_dialog::set_parent(dialog, parent);
        }
        _ => PENDING_PARENT.with(|p| p.set(parent_xid)),
    }
    if let Err(e) = window.show() {
        tracing::error!(error = %e, "[ColorPicker] The picker window could not be shown");
        return;
    }
    ON_END.with(|slot| *slot.borrow_mut() = Some(Box::new(on_end)));
    x11_dialog::center_over(window.window(), parent.window());
    tracing::info!(title = %request.title, "[ColorPicker] Picker opened");
}

/// Writes the pending parent when Slint has made the picker's window (before it is mapped).
fn hint_when_set_up(window: &ColorPickerWindow) {
    let weak = window.as_weak();
    let installed = window.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::RenderingSetup)
            && let Some(window) = weak.upgrade()
            && let (Some(dialog), Some(parent)) = (x11_dialog::xid(window.window()), PENDING_PARENT.with(Cell::take))
        {
            x11_dialog::set_parent(dialog, parent);
        }
    });
    if let Err(e) = installed {
        tracing::warn!(error = ?e, "[ColorPicker] No rendering notifier: the picker has no parent window the first time");
    }
}

/// Hides the picker window (its parent closes). The color stays as the user last left it.
pub fn close() {
    WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref() {
            let _ = window.hide();
        }
    });
    ended();
}

/// RustCut quits: the picker window is hidden and dropped.
pub fn shutdown() {
    let window = WINDOW.with(|slot| slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()));
    if let Some(window) = window {
        let _ = window.hide();
        tracing::debug!("[ColorPicker] Picker window dropped");
    }
    ended();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_texts_and_colors() {
        assert_eq!(color_of("#444444"), Some(Color::from_rgb_u8(0x44, 0x44, 0x44)));
        assert_eq!(color_of("nope"), None);
        assert_eq!(hex_of(Color::from_argb_u8(10, 0x12, 0xab, 0xff)), "#12abff");
        assert_eq!(to_hex(Color::from_argb_u8(0x80, 1, 2, 3), true), "#01020380");
        assert_eq!(
            parse(&"#01020380".into(), false),
            Some(Color::from_argb_u8(255, 1, 2, 3))
        );
        assert_eq!(
            parse(&"#01020380".into(), true),
            Some(Color::from_argb_u8(0x80, 1, 2, 3))
        );
        assert_eq!(parse(&"red".into(), true), None);
    }
}
