//! The Rust side of the shared controls (P9.1): `TextLogic` in `ui/controls_text.slint` asks where a path or file name
//! is cut ([`crate::util::text::split_middle`]); it is pure and answered on the UI thread inside Slint's bindings. Every
//! window that shows a `MiddleText` installs it (each window has its own globals).

use slint::ComponentHandle;

use crate::{TextLogic, TextSplit};

/// Answers `TextLogic`'s callback for `window`.
pub fn install<C>(window: &C)
where
    C: ComponentHandle,
    for<'a> TextLogic<'a>: slint::Global<'a, C>,
{
    window.global::<TextLogic<'_>>().on_split_middle(|text| {
        let (head, tail) = crate::util::text::split_middle(&text);
        TextSplit {
            head: head.into(),
            tail: tail.into(),
        }
    });
}
