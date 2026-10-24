//! The pages of the Settings window in the order of its sidebar (HANDOFF section 8.S).

/// One page of the Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    General,
    Player,
    Editor,
    Filters,
    Export,
    Profiles,
    Queue,
    RustView,
    Warnings,
    Messages,
    Dialogs,
    Hotkeys,
    Desktop,
    Appearance,
    Advanced,
    About,
}

/// The editor a page shows below its generic cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    None,
    Folders,
    Profiles,
    Hotkeys,
    About,
    Messages,
}

impl Page {
    pub const ALL: [Self; 16] = [
        Self::General,
        Self::Player,
        Self::Editor,
        Self::Filters,
        Self::Export,
        Self::Profiles,
        Self::Queue,
        Self::RustView,
        Self::Warnings,
        Self::Messages,
        Self::Dialogs,
        Self::Hotkeys,
        Self::Desktop,
        Self::Appearance,
        Self::Advanced,
        Self::About,
    ];

    /// The id kept in `ui-state.toml`.
    pub const fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Player => "player",
            Self::Editor => "editor",
            Self::Filters => "filters",
            Self::Export => "export",
            Self::Profiles => "profiles",
            Self::Queue => "queue",
            Self::RustView => "rustview",
            Self::Warnings => "warnings",
            Self::Messages => "messages",
            Self::Dialogs => "dialogs",
            Self::Hotkeys => "hotkeys",
            Self::Desktop => "desktop",
            Self::Appearance => "appearance",
            Self::Advanced => "advanced",
            Self::About => "about",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Player => "Player",
            Self::Editor => "Editor & Sections",
            Self::Filters => "Filters",
            Self::Export => "Export & Naming",
            Self::Profiles => "Render Profiles",
            Self::Queue => "Queue",
            Self::RustView => "RustView",
            Self::Warnings => "Warnings & Confirmations",
            Self::Messages => "Messages",
            Self::Dialogs => "Dialogs",
            Self::Hotkeys => "Hotkeys",
            Self::Desktop => "Desktop Integration",
            Self::Appearance => "Appearance",
            Self::Advanced => "Advanced & Logs",
            Self::About => "About",
        }
    }

    /// The page with `id`; an unknown id (a later version's page) is `None`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub const fn special(self) -> Special {
        match self {
            Self::Export => Special::Folders,
            Self::Profiles => Special::Profiles,
            Self::Hotkeys => Special::Hotkeys,
            Self::About => Special::About,
            Self::Messages => Special::Messages,
            _ => Special::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_ids_are_none() {
        for page in Page::ALL {
            assert_eq!(Page::from_id(page.id()), Some(page));
            assert_eq!(Page::from_index(page.index()), Some(page));
        }
        assert_eq!(Page::from_id("later"), None);
    }
}
