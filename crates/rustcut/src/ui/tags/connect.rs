//! The panel's callbacks and the task that follows the sync actor's events.

use std::rc::Rc;

use rustcut_core::rustview::{SyncEvent, SyncService};
use rustcut_core::settings::TagPanelState;
use slint::ComponentHandle as _;
use tokio::sync::broadcast::error::RecvError;

use super::{TagPanel, TagsBridge, tag_id, with_bridge};
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::jump::JumpTarget;
use crate::{MainWindow, Playback, TagRate};

pub(super) fn connect(bridge: &Rc<TagsBridge>, window: &MainWindow) {
    let t = window.global::<TagRate<'_>>();
    macro_rules! on {
        ($setter:ident, |$b:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = Rc::downgrade(bridge);
            t.$setter(move |$($arg),*| {
                if let Some($b) = weak.upgrade() {
                    $body;
                }
            });
        }};
    }
    on!(on_retry, |b| b.retry());
    on!(on_connection_jump_clicked, |b| crate::ui::jump::go(
        b.connection_jump.get().unwrap_or(JumpTarget::Settings {
            page: crate::ui::settings::page::Page::RustView,
            key: "rustview.enabled",
        })
    ));
    on!(on_apply_edit, |b| b.apply_edit());
    on!(on_cancel_edit, |b| b.cancel_edit());
    on!(on_rating_clicked, |b, rating| b.change(|s| s.click_rating(rating)));
    // Ctrl+0..5 (RustView's keys): the rating, and its digit in the middle of the video (the buttons show it themselves).
    on!(on_rate_key, |b, rating| {
        let Some(window) = b.window.upgrade() else { return };
        let playback = window.global::<Playback<'_>>();
        let Ok(rating) = u8::try_from(rating) else { return };
        let mut rated = false;
        b.change(|s| rated = s.rate_key(rating, playback.get_has_video()));
        if rated {
            tracing::info!(rating, "[Tags] Rating set by key");
            crate::ui::player::flash(&playback, crate::ui::player::FlashKind::Rating(rating));
        }
    });
    on!(on_tag_clicked, |b, id| if let Some(id) = tag_id(id) {
        b.change(|s| s.click_tag(id));
    });
    on!(on_tag_right_clicked, |b, id| if let Some(id) = tag_id(id) {
        let locked = b.state.borrow_mut().toggle_lock(id);
        if locked {
            b.publish();
        }
    });
    on!(on_tag_toggle_expand, |b, id| if let Some(id) = tag_id(id) {
        b.change(|s| s.toggle_expanded(id));
    });
    on!(on_filter_changed, |b, text| b.change(|s| s.filter = text.to_string()));
    on!(on_filter_regex_toggled, |b, on| b.change(|s| s.regex = on));
    on!(on_filter_done, |b| b.publish());
    on!(on_tree_height_changed, |b, height| b.tree_height_changed(height));
    on!(on_tree_menu, |b, x, y| b.show_tree_menu(x, y));
    on!(on_refresh_tree, |b| b.refresh_tree());
    on!(on_history_tag_clicked, |b, id| if let Some(id) = tag_id(id) {
        b.change(|s| s.click_tag(id));
    });
    on!(on_history_width_changed, |b, width| b.history_width_changed(width));
    on!(
        on_history_pill_measured,
        |b, id, check, width| if let Some(id) = tag_id(id) {
            b.history_pill_measured(id, check, width);
        }
    );
    on!(on_history_collapsed_changed, |b, collapsed| b
        .state
        .borrow_mut()
        .history_collapsed =
        collapsed);
    on!(on_history_resized, |b, height| {
        let range = TagPanelState::HISTORY_HEIGHT;
        let height = az::checked_cast::<f32, u32>(height.round()).unwrap_or(*range.start());
        b.state.borrow_mut().history_height = height.clamp(*range.start(), *range.end());
    });
    on!(on_apply_last_tags, |b| b.change(TagPanel::apply_last_tags));
    on!(on_apply_last_rating, |b| b.change(TagPanel::apply_last_rating));
    on!(on_clear, |b| b.change(TagPanel::clear));
    on!(on_toggle_show_hidden, |b| b.change(TagPanel::toggle_show_hidden));
    on!(on_toggle_search, |b| b.change(|s| {
        s.search_shown = !s.search_shown;
        tracing::info!(shown = s.search_shown, "[Tags] Tag filter field toggled");
    }));
    on!(on_toggle_active, |b| b.toggle_active());
    on!(on_open_filter, |b| b.open_filter());
    on!(on_toggle_warn, |b| b.toggle_warn());
}

/// Follows the sync actor's events; a subscriber that fell behind asks for the tree again.
pub(super) async fn follow(sync: SyncService, ui: UiDispatcher) {
    let mut events = sync.events.subscribe();
    loop {
        match events.recv().await {
            Ok(SyncEvent::JobSynced { .. }) => {}
            Ok(event) => with_bridge(&ui, move |b| b.event(event)),
            Err(RecvError::Lagged(missed)) => {
                tracing::debug!(missed, "[Tags] Fell behind the RustView sync; loading the tree again");
                with_bridge(&ui, |b| b.load_tree());
            }
            Err(RecvError::Closed) => return,
        }
    }
}
