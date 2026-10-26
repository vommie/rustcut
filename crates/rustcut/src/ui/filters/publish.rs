//! Shows the filters: the card (its rows, its header and the open options menu, all through the `Filters` global) and
//! the crop overlay on the video.

use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::state::CropMode;
use super::{FiltersBridge, index, input, kind_id, kind_tooltip, strings, view};
use crate::ui::settings::option_help;
use crate::util::model::sync;
use crate::{CropOverlay, FilterRowView, FilterTips, Filters, TipRow};

/// The menus' tooltips: the help texts of Settings > Filters (one place for both, Q-P9.13: short, plain, with a
/// recommendation).
pub(super) fn tips() -> FilterTips {
    let t = |key: &str| option_help(key).into();
    FilterTips {
        deinterlacer: t("filters.deinterlacer"),
        output: t("filters.deinterlace_rate"),
        field_order: t("filters.field_order"),
        only_interlaced: t("filters.only_interlaced"),
        limit: t("filters.crop_search.limit"),
        sides: t("filters.crop_search.sides"),
        samples: t("filters.crop_search.samples"),
        sections_only: t("filters.crop_search.sections_only"),
        shakiness: t("filters.deshake.shakiness"),
        accuracy: t("filters.deshake.accuracy"),
        stepsize: t("filters.deshake.stepsize"),
        smoothing: t("filters.deshake.smoothing"),
        tripod: t("filters.deshake.tripod"),
        max_shift: t("filters.deshake.max_shift"),
        max_angle: t("filters.deshake.max_angle"),
        zoom: t("filters.deshake.zoom"),
        optzoom: t("filters.deshake.optzoom"),
        zoom_speed: t("filters.deshake.zoom_speed"),
        borders: t("filters.deshake.crop"),
        interpolation: t("filters.deshake.interpolation"),
        scaler: t("filters.resize_algorithm"),
    }
}

impl FiltersBridge {
    pub(super) fn publish_card(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let card = view::card(&self.state.borrow());
        let f = window.global::<Filters<'_>>();
        f.set_has_media(card.has_media);
        f.set_preview(card.preview);
        f.set_keep(card.keep);
        f.set_output(card.output.as_str().into());
        f.set_output_tooltip(card.output_tooltip.as_str().into());
        f.set_output_warning(card.output_warning);
        sync(
            &self.rows,
            card.rows
                .iter()
                .map(|r| FilterRowView {
                    kind: kind_id(r.kind).into(),
                    title: r.kind.label().into(),
                    tooltip: kind_tooltip(r.kind).into(),
                    enabled: r.enabled,
                    lines: r.lines(),
                    has_menu: r.has_menu(),
                })
                .collect(),
        );
        f.set_deinterlacer(index(card.deinterlacer));
        f.set_field_rate(card.field_rate);
        f.set_field_options(card.field_options);
        f.set_field_order(index(card.field_order));
        f.set_only_interlaced(card.only_interlaced);
        f.set_interlace_hint(card.interlace_hint.as_str().into());
        f.set_interlace_level(card.interlace_level as i32);
        f.set_interlace_detail(card.interlace_detail.as_str().into());
        f.set_interlace_rows(ModelRc::new(VecModel::from(
            card.interlace_rows
                .iter()
                .map(|(label, value)| TipRow {
                    label: label.as_str().into(),
                    value: value.as_str().into(),
                    heading: false,
                })
                .collect::<Vec<_>>(),
        )));
        f.set_interlace_counts(card.interlace_counts.as_str().into());
        f.set_interlace_checking(card.interlace_checking);
        let c = card.crop;
        f.set_crop_edges(strings([c.top, c.right, c.bottom, c.left].iter().map(u32::to_string)));
        f.set_crop_size(card.crop_size.as_str().into());
        f.set_crop_editing(card.crop_editing);
        f.set_can_edit_crop(card.can_edit_crop);
        f.set_auto_crop_running(card.auto_crop_running);
        f.set_search_limit(i32::from(card.search_limit));
        f.set_search_sides(index(card.search_sides));
        f.set_search_samples(card.search_samples.to_string().into());
        f.set_search_sections_only(card.search_sections_only);
        // The glyph shows the result (turns, then a mirror left to right); the buttons show the mirrors as set.
        let (turns, mirrored) = card.orientation.orientation().turns_and_mirror();
        f.set_orientation_turns(i32::from(turns));
        f.set_orientation_mirrored(mirrored);
        f.set_orientation_turn(i32::from(card.orientation.normalized().turns));
        f.set_orientation_mirror_horizontal(card.orientation.mirror_horizontal);
        f.set_orientation_mirror_vertical(card.orientation.mirror_vertical);
        f.set_orientation_label(card.orientation.label().into());
        f.set_resize_width(card.resize_width.as_str().into());
        f.set_resize_height(card.resize_height.as_str().into());
        f.set_resize_auto_width(card.resize_auto_width.as_str().into());
        f.set_resize_auto_height(card.resize_auto_height.as_str().into());
        f.set_algorithm(index(card.resize_algorithm));
        f.set_deshake_values(strings(card.deshake_values.iter().map(String::as_str)));
        let d = card.deshake;
        f.set_deshake_optzoom(i32::from(d.optzoom));
        f.set_deshake_borders(i32::from(d.crop == rustcut_core::edit::DeshakeCrop::Keep));
        f.set_deshake_interpolation(input::interpolation_index(d.interpolation));
        f.set_deshake_tripod(d.tripod);
        f.set_deshake_summary(card.deshake_summary.as_str().into());
        self.publish_menu(&f);
    }

    /// The open options menu's title and whether its options equal the defaults.
    fn publish_menu(&self, f: &Filters<'_>) {
        let kind = self.menu_kind.get();
        f.set_menu_kind(kind.map_or("", kind_id).into());
        f.set_menu_title(kind.map_or("", |k| k.label()).into());
        f.set_menu_pinned(self.menu_pinned.get());
        f.set_menu_default(kind.is_none_or(|k| self.state.borrow().options_are_default(k)));
    }

    pub(super) fn publish_overlay(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let o = window.global::<CropOverlay<'_>>();
        let (Some(session), Some(view)) = (st.crop_session(), st.crop_view()) else {
            o.set_active(false);
            return;
        };
        let p = view.picture();
        let crop = st.crop();
        let k = view.kept_rect(crop);
        o.set_active(true);
        o.set_quick(session.mode == CropMode::Quick);
        o.set_picture_x(p.x);
        o.set_picture_y(p.y);
        o.set_picture_width(p.width);
        o.set_picture_height(p.height);
        o.set_kept_x(k.x);
        o.set_kept_y(k.y);
        o.set_kept_width(k.width);
        o.set_kept_height(k.height);
        o.set_handle(session.active_handle().map_or(-1, input::handle_index));
        o.set_dragging(session.dragging());
        let kept = view.shown_crop(crop).kept(view.shown_size());
        o.set_size_text(
            kept.map(|s| format!("{}×{}", s.width, s.height))
                .unwrap_or_default()
                .into(),
        );
        o.set_ratio_label(st.ratio_label().into());
        o.set_hint(
            match session.mode {
                CropMode::Edit => "Drag a corner, an edge or the area (Shift keeps the ratio) · Enter or a double click keeps · Esc restores",
                CropMode::Quick => "Ctrl held: drag the nearest edge or corner · Esc restores",
            }
            .into(),
        );
    }
}
