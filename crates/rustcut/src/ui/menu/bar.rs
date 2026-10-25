//! The menu bar's keys (P9.12, `ui/menu_bar.slint`): which entry Up and Down move to. The bar itself and what its
//! entries do live in Slint (main.slint builds the menus from the other globals); Slint has no loops, so the step over
//! separators, headings and disabled entries is this pure function behind `AppMenu.next-row`.

use slint::{ComponentHandle as _, Model as _};

use crate::{AppMenu, MainWindow, MenuItemView};

/// The next entry after `row` in direction `step` (1 down, -1 up) that can be chosen (an enabled action or submenu),
/// wrapping around; from no entry (`row` < 0) Down starts at the top and Up at the bottom. `None` when nothing can be
/// chosen.
pub fn next_row(usable: &[bool], row: i32, step: i32) -> Option<usize> {
    let count = i32::try_from(usable.len()).ok().filter(|c| *c > 0)?;
    let step = if step < 0 { -1 } else { 1 };
    let start = if row < 0 || row >= count {
        if step > 0 { -1 } else { count }
    } else {
        row
    };
    (1..=count)
        .map(|offset| (start + step * offset).rem_euclid(count))
        .find(|at| {
            usize::try_from(*at)
                .ok()
                .and_then(|i| usable.get(i))
                .copied()
                .unwrap_or(false)
        })
        .and_then(|at| usize::try_from(at).ok())
}

/// Answers `AppMenu.next-row` for the main window.
pub fn install(window: &MainWindow) {
    window.global::<AppMenu<'_>>().on_next_row(|items, row, step| {
        let usable: Vec<bool> = items
            .iter()
            .map(|item: MenuItemView| item.enabled && (item.kind == 0 || item.kind == 2))
            .collect();
        next_row(&usable, row, step)
            .and_then(|at| i32::try_from(at).ok())
            .unwrap_or(-1)
    });
}

#[cfg(test)]
mod tests {
    use super::next_row;

    #[test]
    fn up_and_down_skip_what_cannot_be_chosen_and_wrap() {
        // Open, separator, a disabled entry, Settings, separator, Quit.
        let usable = [true, false, false, true, false, true];
        assert_eq!(next_row(&usable, -1, 1), Some(0), "Down starts at the top");
        assert_eq!(next_row(&usable, -1, -1), Some(5), "Up starts at the bottom");
        assert_eq!(next_row(&usable, 0, 1), Some(3));
        assert_eq!(next_row(&usable, 3, 1), Some(5));
        assert_eq!(next_row(&usable, 5, 1), Some(0), "wraps");
        assert_eq!(next_row(&usable, 0, -1), Some(5), "wraps upwards");
        assert_eq!(
            next_row(&usable, 1, 1),
            Some(3),
            "from a separator the pointer stood on"
        );
        assert_eq!(next_row(&[false, false], 0, 1), None);
        assert_eq!(next_row(&[], -1, 1), None);
        assert_eq!(next_row(&[true], 0, 1), Some(0), "a single entry stays");
    }
}
