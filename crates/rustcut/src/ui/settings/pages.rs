//! The sidebar's pages with sub-pages (P12 block 8, user request: a general mechanism, not only for plugins): a page
//! may have sub-pages below it; such a page has a chevron at its right end and folds them in and out. The folded pages
//! are kept in `ui-state.toml`. Today the Plugins page has one sub-page per plugin.

use super::page::Page;

/// The id prefix of a plugin's page.
const PLUGIN_PREFIX: &str = "plugin:";

/// A page of the sidebar: one of the fixed pages or a sub-page that exists while RustCut runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PageRef {
    Fixed(Page),
    /// The page of the plugin with this id (below Plugins).
    Plugin(String),
}

impl PageRef {
    /// The id kept in `ui-state.toml`: the page's id, or `plugin:<id>`.
    pub fn id(&self) -> String {
        match self {
            Self::Fixed(page) => page.id().to_owned(),
            Self::Plugin(id) => format!("{PLUGIN_PREFIX}{id}"),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.strip_prefix(PLUGIN_PREFIX) {
            Some(plugin) if !plugin.is_empty() => Some(Self::Plugin(plugin.to_owned())),
            Some(_) => None,
            None => Page::from_id(id).map(Self::Fixed),
        }
    }

    /// The fixed page it belongs to (a sub-page's parent).
    pub fn page(&self) -> Page {
        match self {
            Self::Fixed(page) => *page,
            Self::Plugin(_) => Page::Plugins,
        }
    }
}

/// A sub-page and its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubPage {
    pub page: PageRef,
    pub title: String,
}

/// One row of the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRow {
    pub page: PageRef,
    pub title: String,
    pub depth: u8,
    pub expandable: bool,
    pub expanded: bool,
}

/// The rows: every fixed page, and below each its sub-pages unless it is folded.
pub fn rows(children: &dyn Fn(Page) -> Vec<SubPage>, folded: &[String]) -> Vec<PageRow> {
    let mut rows = Vec::new();
    for page in Page::ALL {
        let subs = children(page);
        let expanded = !folded.iter().any(|id| id == page.id());
        rows.push(PageRow {
            page: PageRef::Fixed(page),
            title: page.title().to_owned(),
            depth: 0,
            expandable: !subs.is_empty(),
            expanded,
        });
        if expanded {
            rows.extend(subs.into_iter().map(|sub| PageRow {
                page: sub.page,
                title: sub.title,
                depth: 1,
                expandable: false,
                expanded: false,
            }));
        }
    }
    rows
}

/// What the sidebar remembers: the folded pages and the scroll slots of the sub-pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageState {
    /// Ids of the pages whose sub-pages are folded in.
    pub folded: Vec<String>,
    /// The sidebar's rows as shown last.
    pub rows: Vec<PageRow>,
    /// Sub-pages in the order they were first shown: their scroll slots follow the fixed pages'.
    extra_slots: Vec<PageRef>,
}

impl PageState {
    pub fn new(folded: &[String]) -> Self {
        Self {
            folded: folded.to_vec(),
            ..Self::default()
        }
    }

    /// Folds a page in or out; `false` when it has no sub-pages.
    pub fn toggle(&mut self, page: Page) -> bool {
        if !self
            .rows
            .iter()
            .any(|row| row.page == PageRef::Fixed(page) && row.expandable)
        {
            return false;
        }
        if let Some(at) = self.folded.iter().position(|id| id == page.id()) {
            self.folded.remove(at);
        } else {
            self.folded.push(page.id().to_owned());
        }
        true
    }

    /// The row of `page` (a folded sub-page shows as its parent's row).
    pub fn row_of(&self, page: &PageRef) -> usize {
        self.rows
            .iter()
            .position(|row| row.page == *page)
            .or_else(|| self.rows.iter().position(|row| row.page == PageRef::Fixed(page.page())))
            .unwrap_or(0)
    }

    /// The page of row `index`.
    pub fn at(&self, index: usize) -> Option<PageRef> {
        self.rows.get(index).map(|row| row.page.clone())
    }

    /// The scroll position slot of `page`: the fixed pages' indexes, then one per sub-page.
    pub fn slot(&mut self, page: &PageRef) -> usize {
        match page {
            PageRef::Fixed(page) => page.index(),
            sub @ PageRef::Plugin(_) => {
                let at = self.extra_slots.iter().position(|p| p == sub).unwrap_or_else(|| {
                    self.extra_slots.push(sub.clone());
                    self.extra_slots.len() - 1
                });
                Page::ALL.len() + at
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugins(page: Page) -> Vec<SubPage> {
        if page == Page::Plugins {
            ["auto-folder", "auto-rate"]
                .into_iter()
                .map(|id| SubPage {
                    page: PageRef::Plugin(id.into()),
                    title: id.into(),
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    #[test]
    fn ids_round_trip() {
        for page in [PageRef::Fixed(Page::Queue), PageRef::Plugin("auto-folder".into())] {
            assert_eq!(PageRef::from_id(&page.id()), Some(page));
        }
        assert_eq!(PageRef::from_id("plugin:"), None);
        assert_eq!(PageRef::from_id("later"), None);
    }

    #[test]
    fn sub_pages_follow_their_page_until_it_is_folded() {
        let mut state = PageState::new(&[]);
        state.rows = rows(&plugins, &state.folded);
        let at = state.row_of(&PageRef::Fixed(Page::Plugins));
        assert!(state.rows[at].expandable && state.rows[at].expanded);
        assert_eq!(state.at(at + 1), Some(PageRef::Plugin("auto-folder".into())));
        assert_eq!(state.rows[at + 1].depth, 1);

        assert!(state.toggle(Page::Plugins));
        state.rows = rows(&plugins, &state.folded);
        assert_eq!(state.rows.len(), Page::ALL.len());
        assert_eq!(
            state.row_of(&PageRef::Plugin("auto-rate".into())),
            at,
            "a folded sub-page shows as its parent"
        );
        assert!(!state.toggle(Page::Queue), "a page without sub-pages does not fold");
        assert!(state.toggle(Page::Plugins));
        assert!(state.folded.is_empty());
    }

    #[test]
    fn sub_pages_get_their_own_scroll_slots() {
        let mut state = PageState::new(&[]);
        assert_eq!(state.slot(&PageRef::Fixed(Page::General)), 0);
        let a = state.slot(&PageRef::Plugin("a".into()));
        let b = state.slot(&PageRef::Plugin("b".into()));
        assert_eq!((a, b), (Page::ALL.len(), Page::ALL.len() + 1));
        assert_eq!(state.slot(&PageRef::Plugin("a".into())), a);
    }
}
