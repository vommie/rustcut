//! The Rust side of the shared controls (P9.1): `TextLogic` in `ui/controls_text.slint` asks where a path or file name
//! is cut ([`crate::util::text::split_middle`]); it is pure and answered on the UI thread inside Slint's bindings. Every
//! window that shows a `MiddleText` installs it (each window has its own globals). It also answers which entries a long
//! `ChoiceButton` list keeps while the user searches it ([`crate::util::choice::search`]).

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
    window.global::<TextLogic<'_>>().on_search(|items, query| {
        let items: Vec<slint::SharedString> = slint::Model::iter(&items).collect();
        let kept: Vec<i32> = crate::util::choice::search(&items, &query)
            .into_iter()
            .filter_map(|index| i32::try_from(index).ok())
            .collect();
        slint::ModelRc::new(slint::VecModel::from(kept))
    });
}

#[cfg(test)]
mod tests {
    /// Every text field is a `TextLine` (`ui/controls_text_line.slint`): a bare `TextInput` never scrolls to its cursor
    /// (user report 2026-11-02: typing past the end, the arrows and End left the text standing at its start).
    #[test]
    fn text_fields_follow_their_cursor() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
        let mut bare = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "slint") || path.ends_with("controls_text_line.slint") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or_default();
                if code.contains("TextInput {") || code.contains("TextInput{") {
                    bare.push(format!("{}:{}", path.display(), n + 1));
                }
            }
        }
        assert!(bare.is_empty(), "use TextLine instead of a bare TextInput: {bare:?}");
    }
}
