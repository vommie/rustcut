//! The words and numbers of the folder progress bar (Q-NF2, pure): the number candidates the bar tries from the longest
//! to the shortest (it shows the first that fits, else none), the percent, the order in words and the neighbors window.

use rustcut_core::folder_walk::order::{SortRole, SortSpec};
use rustcut_core::folder_walk::{NamesBecause, OrderSource};

/// `999`, `1.5k`, `37.5k`, `100k`, `1.2M`: one decimal below a hundred of the unit, cut (never rounded up past the
/// real number).
pub fn abbreviate(n: usize) -> String {
    let unit = |n: usize, size: usize, sign: char| {
        let whole = n / size;
        let tenth = (n % size) * 10 / size;
        if whole >= 100 || tenth == 0 {
            format!("{whole}{sign}")
        } else {
            format!("{whole}.{tenth}{sign}")
        }
    };
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => unit(n, 1_000, 'k'),
        _ => unit(n, 1_000_000, 'M'),
    }
}

/// What the bar tries to show, longest first (the user's order, Q-NF2): `37512/100000`, `37512/100k`, `37512`,
/// `37.5k`. Duplicates (small numbers) go.
pub fn candidates(position: usize, total: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(4);
    for text in [
        format!("{position}/{total}"),
        format!("{position}/{}", abbreviate(total)),
        position.to_string(),
        abbreviate(position),
    ] {
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// Position ÷ total as a share (0..=1).
pub fn fraction(position: usize, total: usize) -> f32 {
    if total == 0 {
        return 0.0;
    }
    let share =
        f64::from(u32::try_from(position).unwrap_or(u32::MAX)) / f64::from(u32::try_from(total).unwrap_or(u32::MAX));
    #[expect(clippy::cast_possible_truncation, reason = "a share between 0 and 1 fits an f32")]
    let share = share.clamp(0.0, 1.0) as f32;
    share
}

/// `10%`, `37.5%`, `0.1%`: a whole percent without decimals, else one (cut, so 99.99 % never reads 100 %); a first
/// file of a huge folder reads `<0.1%`, never `0%`.
pub fn percent(position: usize, total: usize) -> String {
    if total == 0 {
        return "0%".to_owned();
    }
    let tenths = position.saturating_mul(1_000) / total;
    if tenths == 0 && position > 0 {
        "<0.1%".to_owned()
    } else if tenths.is_multiple_of(10) {
        format!("{}%", tenths / 10)
    } else {
        format!("{}.{}%", tenths / 10, tenths % 10)
    }
}

/// "10 of 100 (10%)".
pub fn title(position: usize, total: usize) -> String {
    format!("{position} of {total} ({})", percent(position, total))
}

/// "Dolphin: Modified, newest first", or names A to Z with the reason.
pub fn order_words(sort: &SortSpec, source: &OrderSource) -> String {
    let role = match &sort.role {
        SortRole::Name | SortRole::Unsupported(_) => "Name",
        SortRole::Size => "Size",
        SortRole::Modified => "Modified",
        SortRole::Created => "Created",
        SortRole::Accessed => "Accessed",
        SortRole::Type => "Type",
    };
    let direction = match (&sort.role, sort.descending) {
        (SortRole::Modified | SortRole::Created | SortRole::Accessed, false) => "oldest first",
        (SortRole::Modified | SortRole::Created | SortRole::Accessed, true) => "newest first",
        (SortRole::Size, false) => "smallest first",
        (SortRole::Size, true) => "largest first",
        (_, false) => "A to Z",
        (_, true) => "Z to A",
    };
    match source {
        OrderSource::FileManager => format!("Dolphin: {role}, {direction}"),
        OrderSource::Names(because) => {
            let why = match because {
                NamesBecause::Off => "Dolphin's order is off",
                NamesBecause::NotRunning => "Dolphin does not run",
                NamesBecause::NotOpen => "no Dolphin tab shows the folder",
                NamesBecause::Unsupported(_) => "Dolphin's order cannot be rebuilt",
                NamesBecause::Failed(_) => "Dolphin did not answer",
            };
            format!("Names A to Z ({why})")
        }
    }
}

/// The places of the neighbors list: up to `around` before and after `current`, within `0..total`.
pub fn neighbors(current: usize, total: usize, around: usize) -> std::ops::Range<usize> {
    let start = current.saturating_sub(around);
    let end = current.saturating_add(around).saturating_add(1).min(total);
    start.min(end)..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_get_shorter_in_the_users_order() {
        assert_eq!(candidates(10, 100), ["10/100", "10"]);
        assert_eq!(
            candidates(37_512, 100_000),
            ["37512/100000", "37512/100k", "37512", "37.5k"]
        );
        assert_eq!(candidates(1, 1_500), ["1/1500", "1/1.5k", "1"]);
        assert_eq!(abbreviate(999), "999");
        assert_eq!(abbreviate(1_000), "1k");
        assert_eq!(abbreviate(9_999), "9.9k");
        assert_eq!(abbreviate(100_000), "100k");
        assert_eq!(abbreviate(123_456), "123k");
        assert_eq!(abbreviate(1_250_000), "1.2M");
    }

    #[test]
    fn percent_is_position_over_total() {
        assert_eq!(title(10, 100), "10 of 100 (10%)");
        assert_eq!(percent(1, 1_000), "0.1%");
        assert_eq!(percent(37_512, 100_000), "37.5%");
        assert_eq!(percent(9_999, 10_000), "99.9%");
        assert_eq!(percent(1, 1), "100%");
        assert_eq!(percent(0, 0), "0%");
        assert_eq!(percent(1, 12_001), "<0.1%");
        assert_eq!(title(1, 12_001), "1 of 12001 (<0.1%)");
        assert!((fraction(10, 100) - 0.1).abs() < 1e-6);
        assert!((fraction(1, 1) - 1.0).abs() < 1e-6);
        assert!(fraction(5, 0).abs() < 1e-6);
    }

    #[test]
    fn orders_in_words() {
        let date = SortSpec {
            role: SortRole::Modified,
            descending: true,
            hidden_last: false,
        };
        assert_eq!(
            order_words(&date, &OrderSource::FileManager),
            "Dolphin: Modified, newest first"
        );
        let names = SortSpec::default();
        assert_eq!(
            order_words(&names, &OrderSource::Names(NamesBecause::NotOpen)),
            "Names A to Z (no Dolphin tab shows the folder)"
        );
    }

    #[test]
    fn neighbors_stay_inside_the_folder() {
        assert_eq!(neighbors(10, 100, 5), 5..16);
        assert_eq!(neighbors(1, 100, 5), 0..7);
        assert_eq!(neighbors(99, 100, 5), 94..100);
        assert_eq!(neighbors(0, 1, 5), 0..1);
    }
}
