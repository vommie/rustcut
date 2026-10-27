//! `[filters]` of `config.toml`: what a fresh filter chain starts with (Settings > Filters; each filter's menu saves its
//! values here with "Save as default"). Every field has a default, so files of older versions load.

use serde::{Deserialize, Serialize};

use crate::edit::{
    DEFAULT_RATIOS, DeinterlaceRate, Deinterlacer, DeshakeParams, FieldParity, Filter, FilterChain, RatioSet,
    ResizeAlgorithm, even_multiple, final_multiple,
};
use crate::media::CropSearch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterDefaults {
    /// Deinterlacer selected when the filter is switched on.
    pub deinterlacer: Deinterlacer,
    /// One frame per frame or per field (0.0.48).
    pub deinterlace_rate: DeinterlaceRate,
    /// Which field comes first (0.0.48).
    pub field_order: FieldParity,
    /// Leave frames alone that are not marked as interlaced (0.0.48).
    pub only_interlaced: bool,
    /// How auto-crop searches (0.0.48).
    pub crop_search: CropSearch,
    /// Defaults of the deshake filter.
    pub deshake: DeshakeParams,
    /// The scaler of Resize (0.0.48; lanczos, the sharpest of ffmpeg's common scalers for shrinking, D-P9k).
    pub resize_algorithm: ResizeAlgorithm,
    /// The aspect ratios the crop's ratio button goes through and Resize offers ("16:9", "2.39:1"; 0.0.50, P9.14).
    pub crop_ratios: Vec<String>,
    /// The final size rule (0.0.60, Q-P9s): every render's width and height are cut down to multiples of this after
    /// all filters (`edit::FINAL_MULTIPLES`: 0 = off, 2 = even, the default, 4, 8, 16). 0.0.51-0.0.59 had it as the
    /// crop's `crop_multiple` (moved by the migration `final-size-rule`).
    pub final_multiple: u8,
}

impl Default for FilterDefaults {
    fn default() -> Self {
        Self {
            deinterlacer: Deinterlacer::Bwdif,
            deinterlace_rate: DeinterlaceRate::Frame,
            field_order: FieldParity::Auto,
            only_interlaced: false,
            crop_search: CropSearch::default(),
            deshake: DeshakeParams::default(),
            resize_algorithm: ResizeAlgorithm::Lanczos,
            crop_ratios: DEFAULT_RATIOS.iter().map(ToString::to_string).collect(),
            final_multiple: even_multiple(),
        }
    }
}

impl FilterDefaults {
    /// The values limited to what the filters accept.
    pub fn sanitized(&self) -> Self {
        Self {
            // A picked border color belongs to one video, never to the defaults.
            crop_search: CropSearch {
                color: None,
                ..self.crop_search.sanitized()
            },
            deshake: self.deshake.sanitized(),
            crop_ratios: self.ratios().entries(),
            final_multiple: final_multiple(self.final_multiple),
            ..self.clone()
        }
    }

    /// The crop's aspect ratios (entries that are no ratio or repeat one are left out).
    pub fn ratios(&self) -> RatioSet {
        RatioSet::parse(&self.crop_ratios).0
    }

    /// The deinterlace filter with these defaults.
    pub const fn deinterlace(&self) -> Filter {
        Filter::Deinterlace {
            deinterlacer: self.deinterlacer,
            rate: self.deinterlace_rate,
            field_order: self.field_order,
            only_interlaced: self.only_interlaced,
        }
    }

    /// The resize filter with these defaults (no size: it keeps the picture).
    pub const fn resize(&self) -> Filter {
        Filter::Resize {
            width: None,
            height: None,
            algorithm: self.resize_algorithm,
        }
    }

    /// A fresh chain: every filter off, in the default order, with these defaults.
    pub fn chain(&self) -> FilterChain {
        let mut chain = FilterChain::default();
        chain.set(self.deinterlace());
        chain.set(Filter::Deshake(self.deshake.sanitized()));
        chain.set(self.resize());
        chain
    }
}

/// Settings version 4 -> 5 (0.0.59, night questionnaire Q-P9n "sharpen" A): Deshake sharpens after the transform, on
/// by default. A `[filters.deshake]` table written before has no `sharpen` key, which reads as off (so old jobs render
/// as they did); the saved defaults get it switched on, like every fresh default.
pub(super) fn migrate_deshake_sharpen(table: &mut toml::Table) -> Result<(), String> {
    let Some(filters) = table.get_mut("filters") else {
        return Ok(());
    };
    let filters = filters
        .as_table_mut()
        .ok_or_else(|| "filters is not a table".to_owned())?;
    let Some(deshake) = filters.get_mut("deshake") else {
        return Ok(());
    };
    let deshake = deshake
        .as_table_mut()
        .ok_or_else(|| "filters.deshake is not a table".to_owned())?;
    deshake
        .entry("sharpen")
        .or_insert(toml::Value::Boolean(DeshakeParams::default().sharpen));
    Ok(())
}

/// Settings version 5 -> 6 (0.0.60, Q-P9s): "Divisible by" left the Crop menu and became the final size rule;
/// `filters.crop_multiple` is now `filters.final_multiple`, with the value the user chose.
pub(super) fn migrate_final_size_rule(table: &mut toml::Table) -> Result<(), String> {
    let Some(filters) = table.get_mut("filters") else {
        return Ok(());
    };
    let filters = filters
        .as_table_mut()
        .ok_or_else(|| "filters is not a table".to_owned())?;
    if let Some(value) = filters.remove("crop_multiple") {
        filters.entry("final_multiple").or_insert(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::FilterKind;
    use crate::media::CropSides;
    use crate::settings::Settings;

    #[test]
    fn a_file_of_0_0_47_loads_with_the_new_defaults() {
        // 0.0.47 wrote the deinterlacer and the deshake values only.
        let s: Settings = toml::from_str(
            "version = 4\n[filters]\ndeinterlacer = \"yadif\"\n[filters.deshake]\nshakiness = 7\naccuracy = 12\n",
        )
        .unwrap();
        let f = &s.filters;
        assert_eq!(f.deinterlacer, Deinterlacer::Yadif);
        assert_eq!(
            (f.deinterlace_rate, f.field_order, f.only_interlaced),
            (DeinterlaceRate::Frame, FieldParity::Auto, false)
        );
        assert_eq!(f.crop_search, CropSearch::default());
        assert_eq!(
            (f.deshake.shakiness, f.deshake.max_shift, f.deshake.zoom_speed),
            (7, None, 25)
        );
        assert_eq!(f.resize_algorithm, ResizeAlgorithm::Lanczos);
        assert_eq!(
            f.crop_ratios.len(),
            DEFAULT_RATIOS.len(),
            "0.0.50's ratios start with RustView's list"
        );
        assert_eq!(f.final_multiple, 2, "the final size rule starts at even sizes");
    }

    #[test]
    fn the_crop_ratios_keep_the_users_order_without_broken_entries() {
        let defaults = FilterDefaults {
            crop_ratios: vec!["21:9".into(), "nonsense".into(), "42:18".into(), "4:5".into()],
            ..FilterDefaults::default()
        };
        assert_eq!(
            defaults.sanitized().crop_ratios,
            vec!["21:9".to_owned(), "4:5".to_owned()]
        );
        let text = toml::to_string(&defaults.sanitized()).unwrap();
        assert!(text.contains("crop_ratios = [\"21:9\", \"4:5\"]"), "{text}");
        assert_eq!(
            [0, 1, 3, 4, 9, 16, 200].map(final_multiple),
            [0, 2, 2, 4, 8, 16, 16],
            "0 = off"
        );
    }

    #[test]
    fn the_defaults_make_the_fresh_chain_and_round_trip() {
        let defaults = FilterDefaults {
            field_order: FieldParity::BottomFirst,
            crop_search: CropSearch {
                sides: CropSides::TopBottom,
                samples: 0,
                ..CropSearch::default()
            },
            ..FilterDefaults::default()
        };
        let clean = defaults.sanitized();
        assert_eq!(clean.crop_search.samples, 1);
        let chain = clean.chain();
        assert_eq!(chain.active().count(), 0, "every filter starts off");
        assert_eq!(
            chain.get(FilterKind::Deinterlace).map(|s| s.filter),
            Some(clean.deinterlace())
        );
        assert_eq!(chain.get(FilterKind::Resize).map(|s| s.filter), Some(clean.resize()));
        let text = toml::to_string(&clean).unwrap();
        assert!(text.contains("field_order = \"bottom_first\""), "{text}");
        assert!(text.contains("sides = \"top_bottom\""), "{text}");
        assert_eq!(toml::from_str::<FilterDefaults>(&text).unwrap(), clean);
    }
}
