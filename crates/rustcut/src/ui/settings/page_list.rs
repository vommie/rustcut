//! The sidebar with sub-pages (`pages.rs` decides; this fills the window and answers its clicks).

use slint::{Model as _, SharedString};

use super::SettingsBridge;
use super::page::Page;
use super::pages::{self, PageRef, SubPage};
use crate::util::model::sync;
use crate::{SettingsPageItem, SettingsUi};

impl SettingsBridge {
    /// The page or sub-page shown.
    pub(super) fn shown_page(&self) -> PageRef {
        self.sub.borrow().clone().unwrap_or(PageRef::Fixed(self.page.get()))
    }

    /// The sub-pages of `page`: one per plugin below Plugins.
    fn sub_pages(&self, page: Page) -> Vec<SubPage> {
        if page != Page::Plugins {
            return Vec::new();
        }
        let Some(plugins) = self.hooks.borrow().plugins.clone() else {
            return Vec::new();
        };
        // One page per id: an installed plugin and a development folder with the same id share their settings.
        let mut seen = std::collections::HashSet::new();
        plugins
            .list()
            .iter()
            .filter(|plugin| seen.insert(plugin.id.clone()))
            .map(|plugin| SubPage {
                page: PageRef::Plugin(plugin.id.clone()),
                title: plugin.name().to_owned(),
            })
            .collect()
    }

    /// Fills the page list, the shown row, its scroll slot and the title; the plugin's id when a plugin page shows.
    pub(super) fn publish_page_list(&self, ui: &SettingsUi<'_>) -> Option<String> {
        let children = |page| self.sub_pages(page);
        // A plugin page whose plugin went (removed) gives way to the Plugins page.
        if let Some(sub) = self.sub.borrow().clone()
            && !children(sub.page()).iter().any(|s| s.page == sub)
        {
            self.sub.replace(None);
        }
        let shown = self.shown_page();
        let mut list = self.page_list.borrow_mut();
        list.rows = pages::rows(&children, &list.folded);
        let items: Vec<SettingsPageItem> = list
            .rows
            .iter()
            .map(|row| SettingsPageItem {
                title: SharedString::from(row.title.as_str()),
                depth: i32::from(row.depth),
                expandable: row.expandable,
                expanded: row.expanded,
            })
            .collect();
        sync(&self.models.pages, items);
        ui.set_page(i32::try_from(list.row_of(&shown)).unwrap_or(0));
        let slot = list.slot(&shown);
        while self.models.page_scroll.row_count() <= slot {
            self.models.page_scroll.push(0.0);
        }
        ui.set_page_slot(i32::try_from(slot).unwrap_or(0));
        let title = match &shown {
            PageRef::Fixed(page) => page.title().to_owned(),
            PageRef::Plugin(id) => children(Page::Plugins)
                .into_iter()
                .find(|s| s.page == shown)
                .map_or_else(|| id.clone(), |s| s.title),
        };
        ui.set_page_title(title.into());
        match shown {
            PageRef::Plugin(id) => Some(id),
            PageRef::Fixed(_) => None,
        }
    }

    /// A click on row `index` of the page list.
    pub(super) fn select_row(&self, index: usize) {
        let Some(page) = self.page_list.borrow().at(index) else {
            return;
        };
        self.show_page(page);
    }

    /// Shows a page or sub-page.
    pub(super) fn show_page(&self, page: PageRef) {
        self.hotkeys.borrow_mut().cancel();
        self.page.set(page.page());
        self.sub.replace(matches!(page, PageRef::Plugin(_)).then_some(page));
        self.publish();
        crate::ui::notices::layout_changed();
    }

    /// The chevron of row `index`.
    pub(super) fn toggle_row(&self, index: usize) {
        let page = self.page_list.borrow().at(index);
        let Some(PageRef::Fixed(page)) = page else { return };
        if self.page_list.borrow_mut().toggle(page) {
            self.publish();
        }
    }
}
