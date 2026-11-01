//! What fits into the transport bar at its width (P12 5.4, user decision Q-P12d; pure, tested). The bar gives up one thing
//! after the other: the percent, the slider (the speaker opens a popup), the total time, and then the section buttons,
//! the frame steps and the audio buttons move into a "..." menu. The widths are those of `player_transport.slint`, and
//! a step depends only on the bar's width and the file's time format, never on the time shown or a chosen kind, so
//! nothing jitters.

/// The gap between two items of the bar.
const SPACING: f32 = 4.0;
/// Left and right padding together.
const PADDING: f32 = 16.0;
const BUTTON: f32 = 30.0 + SPACING;
const CHEVRON_BUTTON: f32 = 42.0 + SPACING;
const SEPARATOR: f32 = 1.0 + SPACING;
const SLIDER: f32 = 90.0 + SPACING;
const PERCENT: f32 = 36.0 + SPACING;
/// The space between the volume and the times.
const VOLUME_GAP: f32 = 14.0 + SPACING;
/// The narrow form's space between the buttons and the time.
const TIME_GAP: f32 = 4.0 + SPACING;

/// What the bar shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub percent: bool,
    pub slider: bool,
    pub total: bool,
    pub frames: bool,
    pub sections: bool,
    pub audio: bool,
    /// The "..." button: the time stands beside the buttons, the button at the right end.
    pub more: bool,
    /// The width left over in the stretch of the bar.
    pub spare: f32,
}

impl Fit {
    /// Everything, the widest form.
    const FULL: Self = Self {
        percent: true,
        slider: true,
        total: true,
        frames: true,
        sections: true,
        audio: true,
        more: false,
        spare: 0.0,
    };

    /// The width this form needs for times of `time` and a slash of `slash` px (the stretch's own gap included).
    fn needs(self, time: f32, slash: f32) -> f32 {
        let part = |shown: bool, width: f32| if shown { width } else { 0.0 };
        // The play button, the stretch and the speaker are always there.
        BUTTON + SPACING + BUTTON + PADDING - SPACING
            + part(self.frames, 2.0 * BUTTON)
            + part(self.sections, SEPARATOR + 3.0 * BUTTON)
            + part(self.audio, CHEVRON_BUTTON + BUTTON + SEPARATOR)
            + part(self.slider, SLIDER)
            + part(self.percent, PERCENT)
            + part(!self.more, VOLUME_GAP)
            + part(self.more, TIME_GAP + BUTTON)
            + time
            + SPACING
            + part(self.total, slash + SPACING + time + SPACING)
    }
}

/// The widest form that fits into `width`, for times of `time` and a slash of `slash` px.
pub fn fit(width: f32, time: f32, slash: f32) -> Fit {
    let no_percent = Fit {
        percent: false,
        ..Fit::FULL
    };
    let no_slider = Fit {
        slider: false,
        ..no_percent
    };
    let no_total = Fit {
        total: false,
        ..no_slider
    };
    let no_sections = Fit {
        sections: false,
        more: true,
        ..no_total
    };
    let no_frames = Fit {
        frames: false,
        ..no_sections
    };
    let narrowest = Fit {
        audio: false,
        ..no_frames
    };
    [Fit::FULL, no_percent, no_slider, no_total, no_sections, no_frames]
        .into_iter()
        .find(|form| form.needs(time, slash) <= width)
        .map_or(
            Fit {
                spare: (width - narrowest.needs(time, slash)).max(0.0),
                ..narrowest
            },
            |form| Fit {
                spare: width - form.needs(time, slash),
                ..form
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the bar measures here.
    const TIME: f32 = 64.0;
    const SLASH: f32 = 7.0;

    #[test]
    fn a_wide_bar_shows_everything() {
        let f = fit(1000.0, TIME, SLASH);
        assert!(f.percent && f.slider && f.total && f.frames && f.sections && f.audio && !f.more);
    }

    #[test]
    fn the_parts_go_in_their_order() {
        let forms: Vec<_> = (240..=800)
            .map(|w| {
                let f = fit(w as f32, TIME, SLASH);
                (f.percent, f.slider, f.total, f.sections, f.frames, f.audio)
            })
            .collect();
        let mut seen = forms;
        seen.dedup();
        assert_eq!(
            seen,
            [
                (false, false, false, false, false, false),
                (false, false, false, false, true, true),
                (false, false, false, true, true, true),
                (false, false, true, true, true, true),
                (false, true, true, true, true, true),
                (true, true, true, true, true, true),
            ]
        );
    }

    #[test]
    fn the_steps_are_where_the_user_was_told() {
        // Questionnaire 2026-11-01: about 635, 595, 500 and 425 px.
        let edge = |want: fn(&Fit) -> bool| (240..=800).find(|w| want(&fit(*w as f32, TIME, SLASH))).unwrap();
        assert!((630..=650).contains(&edge(|f| f.percent)));
        assert!((590..=610).contains(&edge(|f| f.slider)));
        assert!((495..=515).contains(&edge(|f| f.total)));
        assert!((410..=430).contains(&edge(|f| f.sections)));
    }

    #[test]
    fn the_narrowest_column_keeps_play_time_volume_and_more() {
        let f = fit(240.0, TIME, SLASH);
        assert!(f.more);
        assert!(!f.total);
        assert!(f.spare >= 0.0);
    }

    #[test]
    fn spare_is_never_negative() {
        for w in 0..1000 {
            assert!(fit(w as f32, TIME, SLASH).spare >= 0.0, "{w}");
        }
    }
}
