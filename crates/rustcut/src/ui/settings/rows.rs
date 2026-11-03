//! The Slint views of generic rows and hotkey rows.

use slint::{ModelRc, SharedString, VecModel};

use super::{hotkeys, view};
use crate::{HotkeyChipView, HotkeyRowView, SettingRowView};

pub(super) fn row_view(r: &view::RowView) -> SettingRowView {
    SettingRowView {
        key: r.key.into(),
        kind: r.kind as i32,
        label: r.label.into(),
        description: r.description.as_str().into(),
        checked: r.checked,
        items: ModelRc::new(VecModel::from(
            r.items.iter().map(|i| SharedString::from(*i)).collect::<Vec<_>>(),
        )),
        item_icons: ModelRc::new(VecModel::from(
            r.item_icons
                .iter()
                .copied()
                .map(crate::icons::Icon::to_slint)
                .collect::<Vec<_>>(),
        )),
        index: i32::try_from(r.index).unwrap_or(0),
        text: r.text.as_str().into(),
        placeholder: r.placeholder.into(),
        suffix: r.suffix.into(),
        hold: r.hold,
        segmented: r.kind == view::RowKind::Choice && crate::util::choice::segmented(&r.items),
        searchable: false,
        enabled: r.enabled,
        indent: r.indent,
        error: r.error.as_str().into(),
        icon: r.icon.map_or(crate::IconId::None, crate::icons::Icon::to_slint),
        status: r.status.index(),
        status_text: r.status.text().into(),
    }
}

pub(super) fn hotkey_view(row: &hotkeys::HotkeyRow) -> HotkeyRowView {
    HotkeyRowView {
        id: row.action.id().into(),
        title: row.action.title().into(),
        group: hotkeys::group(row.action).into(),
        keys: ModelRc::new(VecModel::from(
            row.keys
                .iter()
                .map(|(text, label)| HotkeyChipView {
                    text: text.as_str().into(),
                    label: label.as_str().into(),
                })
                .collect::<Vec<_>>(),
        )),
        changed: row.changed,
        defaults: row.defaults.as_str().into(),
    }
}
