//! Settings > Plugins (P12 block 8, Q-P12g): the options of all plugins; the list itself is the page's own editor.

use super::Card;
use crate::ui::settings::field::{path_field, switch};

pub const PLUGINS: &[Card] = &[Card {
    title: "Plugins",
    description: "Plugins react to what happens in RustCut, for example by setting the target folder of a video. Each \
                  runs in a sandbox: no files, no network, a time and a memory limit.",
    fields: &[
        switch!(
            "plugins.enabled",
            "Run plugins",
            "Off: no plugin runs; each keeps its own switch and settings. To start once without plugins: rustcut --no-plugins.",
            plugins.enabled
        ),
        path_field!(
            "plugins.update_folder",
            "Update folder",
            "Where \"Check for updates\" looks for newer plugin files (.rcplugin), for example the dist folder of your \
             plugin repository. Empty: nowhere.",
            plugins.update_folder,
            "No folder",
            folder: true
        ),
    ],
}];
