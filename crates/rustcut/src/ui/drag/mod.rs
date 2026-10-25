//! Drag & drop of list rows (P9.4): the pure model (`model.rs`) and the window glue of `ui/drag.slint`: the edge
//! scrolling's step as a pure Slint callback, and Escape, which cancels the drag that runs (`DragState`; main.slint
//! asks it before the keymap). A list starts a drag with [`begin`] (its cancel and, for a list inside a card stack,
//! the stack's area: the stack scrolls along near its edges), and ends it with [`end`].

mod model;

use std::cell::RefCell;

use slint::{ComponentHandle, Global};

pub use model::{DragFrame, ListDrag, auto_scroll};

use crate::{DragLogic, DragState};

thread_local! {
    /// How the drag that runs is canceled (one at a time per app: the pointer drags one list).
    static CANCEL: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Connects a window's drag globals (every window with a draggable list).
pub fn install<C: ComponentHandle>(component: &C)
where
    for<'a> DragLogic<'a>: Global<'a, C>,
    for<'a> DragState<'a>: Global<'a, C>,
{
    component.global::<DragLogic<'_>>().on_scroll_step(auto_scroll);
    component.global::<DragState<'_>>().on_cancel(|| {
        let cancel = CANCEL.with(|slot| slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()));
        if let Some(cancel) = cancel {
            tracing::debug!("[Drag] Canceled with Escape");
            cancel();
        }
    });
}

/// A drag started in `component`; Escape runs `cancel`. `area` names the card stack that scrolls for it (its panel
/// key, `dock_panels.slint`), "" for a list that scrolls itself.
pub fn begin<C: ComponentHandle>(component: &C, area: &str, cancel: impl Fn() + 'static)
where
    for<'a> DragState<'a>: Global<'a, C>,
{
    CANCEL.with(|slot| *slot.borrow_mut() = Some(Box::new(cancel)));
    let state = component.global::<DragState<'_>>();
    state.set_area(area.into());
    state.set_active(true);
}

/// The drag ended (dropped or canceled).
pub fn end<C: ComponentHandle>(component: &C)
where
    for<'a> DragState<'a>: Global<'a, C>,
{
    CANCEL.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            slot.take();
        }
    });
    let state = component.global::<DragState<'_>>();
    state.set_active(false);
    state.set_area("".into());
}
