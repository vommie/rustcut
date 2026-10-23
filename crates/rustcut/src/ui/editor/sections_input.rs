//! The Sections card's callbacks: marks from the playhead or typed, seeking to a time, adding, selecting, editing, moving
//! and deleting sections. Refused edits say why in the status line.

use std::rc::Rc;

use slint::ComponentHandle as _;

use super::Editor;
use crate::ui::sections::{Bound, EditError};
use crate::ui::status::Level;
use crate::{MainWindow, Sections};

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
            editor.finish_edit(result)
        })
    });
    let this = Rc::downgrade(editor);
    sections.on_add(move || {
        if let Some(editor) = this.upgrade() {
            editor.add_section();
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_select(move |row| {
        if let Some(editor) = this.upgrade() {
            {
                let mut st = editor.state.borrow_mut();
                let id = usize::try_from(row).ok().and_then(|i| st.sections.id_at(i));
                st.sections.select(id);
            }
            editor.publish();
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
            editor.finish_edit(result)
        })
    });
    let this = Rc::downgrade(editor);
    sections.on_move_up(move || {
        if let Some(editor) = this.upgrade() {
            editor.move_selected(true);
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_move_down(move || {
        if let Some(editor) = this.upgrade() {
            editor.move_selected(false);
        }
    });
    let this = Rc::downgrade(editor);
    sections.on_delete(move || {
        if let Some(editor) = this.upgrade() {
            editor.delete_section();
        }
    });
}

fn bound(which: i32) -> Bound {
    if which == 0 { Bound::Start } else { Bound::End }
}

impl Editor {
    /// Publishes an accepted edit; a refused one says why. `true` = accepted (the field closes).
    fn finish_edit(&self, result: Result<(), EditError>) -> bool {
        match result {
            Ok(()) => {
                self.edit_accepted();
                self.publish();
                true
            }
            Err(e) => {
                self.status(Level::Warning, &e.to_string());
                false
            }
        }
    }

    fn move_selected(&self, up: bool) {
        if self.state.borrow_mut().sections.move_selected(up) {
            self.publish();
        }
    }
}
