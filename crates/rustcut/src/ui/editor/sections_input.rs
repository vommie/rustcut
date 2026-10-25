//! The Sections card's callbacks: marks from the playhead or typed, seeking to a time, adding, selecting (Ctrl, Shift),
//! editing and deleting sections, and the context menus (P9.5) whose entries act on the selection. A refused typed time
//! says why in a bubble at its field (`section_bubbles.rs`); the rows' drag & drop is `sections_drag.rs`.

use std::rc::Rc;

use slint::ComponentHandle as _;

use super::Editor;
use super::section_bubbles::{mark_anchor, row_anchor};
use crate::ui::jump;
use crate::ui::sections::{Bound, EditError, SectionAction, SectionsState, list_menu, selection_menu};
use crate::{Jump, MainWindow, Sections};

pub(super) fn connect(editor: &Rc<Editor>, window: &MainWindow) {
    let sections = window.global::<Sections<'_>>();
    let this = Rc::downgrade(editor);
    sections.on_set_mark(move |which| {
        if let Some(editor) = this.upgrade() {
            editor.set_mark_here(bound(which));
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_seek_mark(move |which| {
        if let Some(editor) = this.upgrade() {
            editor.go_to_mark(bound(which));
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_edit_mark(move |which, text| {
        this.upgrade().is_some_and(|editor| {
            let result = editor
                .state
                .borrow_mut()
                .sections
                .set_mark_text(bound(which), text.as_str());
            editor.finish_typed(&mark_anchor(which), result)
        })
    });
    let this = Rc::downgrade(editor);
    sections.on_mark_typed(move |which, text| {
        if let Some(editor) = this.upgrade() {
            editor.time_typed(
                &mark_anchor(which),
                SectionsState::check_mark_text(bound(which), text.as_str()),
            );
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_mark_canceled(move |which| {
        if let Some(editor) = this.upgrade() {
            editor.time_done(&mark_anchor(which));
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_add(move || {
        if let Some(editor) = this.upgrade() {
            editor.add_section();
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_pick(move |row, control, shift| {
        if let (Some(editor), Ok(row)) = (this.upgrade(), usize::try_from(row)) {
            editor.state.borrow_mut().sections.pick(row, control, shift);
            editor.publish();
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_clear_all(move || {
        if let Some(editor) = this.upgrade() {
            editor.run_section_action(SectionAction::ClearAll);
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_clear_selection(move || {
        if let Some(editor) = this.upgrade() {
            editor.state.borrow_mut().sections.clear_selection();
            editor.publish();
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_delete_row(move |row| {
        if let Some(editor) = this.upgrade() {
            editor.delete_row(row);
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_context_menu(move |row, x, y| {
        if let Some(editor) = this.upgrade() {
            editor.open_menu(row, x, y);
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_seek_row(move |row, which| {
        if let Some(editor) = this.upgrade() {
            let t = {
                let st = editor.state.borrow();
                usize::try_from(row)
                    .ok()
                    .and_then(|i| st.sections.list().sections().nth(i))
                    .map(|s| match bound(which) {
                        Bound::Start => s.start(),
                        Bound::End => s.end(),
                    })
            };
            if let Some(t) = t {
                editor.with_player(|p| p.seek_to(t, true));
            }
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_edit_row(move |row, which, text| {
        this.upgrade().is_some_and(|editor| {
            let result = {
                let mut st = editor.state.borrow_mut();
                match usize::try_from(row).ok().and_then(|i| st.sections.id_at(i)) {
                    Some(id) => st.sections.edit(id, bound(which), text.as_str()),
                    None => Err(EditError::Gone),
                }
            };
            editor.finish_typed(&row_anchor(row, which), result)
        })
    });
    let this = Rc::downgrade(editor);
    sections.on_row_typed(move |row, which, text| {
        if let Some(editor) = this.upgrade() {
            let check = {
                let st = editor.state.borrow();
                match usize::try_from(row).ok().and_then(|i| st.sections.id_at(i)) {
                    Some(id) => st.sections.check_edit(id, bound(which), text.as_str()),
                    None => Err(EditError::Gone),
                }
            };
            editor.time_typed(&row_anchor(row, which), check);
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_row_canceled(move |row, which| {
        if let Some(editor) = this.upgrade() {
            editor.time_done(&row_anchor(row, which));
        }
    });
}

fn bound(which: i32) -> Bound {
    if which == 0 { Bound::Start } else { Bound::End }
}

impl Editor {
    /// Publishes an accepted typed time; a refused one says why in a bubble at its field (`anchor`). `true` =
    /// accepted (the field closes).
    fn finish_typed(&self, anchor: &str, result: Result<(), EditError>) -> bool {
        match result {
            Ok(()) => {
                self.time_done(anchor);
                self.edit_accepted();
                self.publish();
                true
            }
            Err(e) => {
                self.time_refused(anchor, &e);
                false
            }
        }
    }

    /// The row's own delete button (held).
    fn delete_row(&self, row: i32) {
        if self.probe_blocks() {
            return;
        }
        let deleted = {
            let mut st = self.state.borrow_mut();
            let id = usize::try_from(row).ok().and_then(|i| st.sections.id_at(i));
            id.is_some_and(|id| st.sections.delete(id))
        };
        if deleted {
            tracing::info!(row, "[Editor] Section deleted by its button");
            self.publish();
        }
    }

    /// The context menu of the row `row` (it is selected first unless it is; the entries act on the selection) or,
    /// with `row` < 0, of the empty space.
    fn open_menu(self: &Rc<Self>, row: i32, x: f32, y: f32) {
        let Some(menu) = self.menu.borrow().upgrade() else {
            return;
        };
        let items = {
            let mut st = self.state.borrow_mut();
            match usize::try_from(row).ok() {
                Some(index) => {
                    if st.sections.id_at(index).is_some_and(|id| !st.sections.is_selected(id)) {
                        st.sections.pick(index, false, false);
                    }
                    selection_menu(&st.sections, &self.key_label(crate::ui::keymap::Action::DeleteSection))
                }
                None => list_menu(&st.sections),
            }
        };
        self.publish();
        let this = Rc::downgrade(self);
        menu.show(&items, x, y, move |id| {
            if let (Some(editor), Some(action)) = (this.upgrade(), SectionAction::from_id(id)) {
                editor.run_section_action(action);
            }
        });
    }

    /// An entry of the context menus.
    fn run_section_action(&self, action: SectionAction) {
        tracing::info!(action = action.id(), "[Editor] Sections menu");
        let edits = !matches!(action, SectionAction::SelectAll | SectionAction::ClearSelection);
        if edits && self.probe_blocks() {
            return;
        }
        let mut flash = None;
        {
            let mut st = self.state.borrow_mut();
            let s = &mut st.sections;
            match action {
                SectionAction::Duplicate => flash = s.duplicate_selected().first().copied(),
                SectionAction::MoveUp => {
                    s.move_selected(true);
                }
                SectionAction::MoveDown => {
                    s.move_selected(false);
                }
                SectionAction::Delete => {
                    s.delete_selected();
                }
                SectionAction::SelectAll => s.select_all(),
                SectionAction::ClearSelection => s.clear_selection(),
                SectionAction::ClearAll => s.clear_list(),
            }
        }
        if edits {
            self.edit_accepted();
        }
        self.publish();
        // The copy flashes once it is laid out (and scrolls into view).
        if let (Some(row), Some(window)) = (flash, self.window.upgrade()) {
            jump::flash(&window.global::<Jump<'_>>(), &format!("sections:{row}"));
        }
    }
}
