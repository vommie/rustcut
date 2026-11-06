//! Dark or light in every window (questionnaire 2026-11-06-rustview-light-theme, the same rules as RustView's
//! `src/ui/theme.rs`): `Theme.dark` of `ui/styles.slint` and the std-widgets' `Palette.color-scheme`. Every window
//! passes through [`install`] (through `tooltip::install`), which
//! sets the current look and keeps the window to update it when Settings > Appearance > Theme or the desktop's color
//! scheme change. UI thread only.

use std::cell::{Cell, RefCell};

use rustcut_core::settings::{SystemScheme, ThemeMode, resolve_dark};
use slint::ComponentHandle;

use slint::language::ColorScheme;

use crate::{Palette, Theme};

type Setter = Box<dyn Fn(bool) -> bool>;

thread_local! {
    static MODE: Cell<ThemeMode> = const { Cell::new(ThemeMode::System) };
    static SYSTEM: Cell<SystemScheme> = const { Cell::new(SystemScheme::Unknown) };
    static WINDOWS: RefCell<Vec<Setter>> = const { RefCell::new(Vec::new()) };
}

/// Whether the windows are dark now.
pub fn is_dark() -> bool {
    resolve_dark(MODE.with(Cell::get), SYSTEM.with(Cell::get))
}

fn apply_to<C>(window: &C, dark: bool)
where
    C: ComponentHandle,
    for<'a> Theme<'a>: slint::Global<'a, C>,
    for<'a> Palette<'a>: slint::Global<'a, C>,
{
    window.global::<Theme<'_>>().set_dark(dark);
    window
        .global::<Palette<'_>>()
        .set_color_scheme(if dark { ColorScheme::Dark } else { ColorScheme::Light });
}

/// Gives `window` the current look and keeps it up to date (it is dropped from the list once the window is gone).
pub fn install<C>(window: &C)
where
    C: ComponentHandle + 'static,
    for<'a> Theme<'a>: slint::Global<'a, C>,
    for<'a> Palette<'a>: slint::Global<'a, C>,
{
    apply_to(window, is_dark());
    let weak = window.as_weak();
    WINDOWS.with(|w| {
        w.borrow_mut().push(Box::new(move |dark| {
            weak.upgrade().is_some_and(|win| {
                apply_to(&win, dark);
                true
            })
        }));
    });
}

fn apply_all() {
    let dark = is_dark();
    tracing::info!(dark, "[Theme] Windows follow the theme");
    WINDOWS.with(|w| w.borrow_mut().retain(|set| set(dark)));
}

/// Settings > Appearance > Theme (`appearance.theme`).
pub fn set_mode(mode: ThemeMode) {
    if MODE.with(|m| m.replace(mode)) != mode {
        apply_all();
    }
}

/// The desktop's color scheme (the portal watcher, `system::color_scheme`).
pub fn set_system(scheme: SystemScheme) {
    if SYSTEM.with(|s| s.replace(scheme)) != scheme {
        tracing::info!(?scheme, "[Theme] The desktop's color scheme changed");
        apply_all();
    }
}
