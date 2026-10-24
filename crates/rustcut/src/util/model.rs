//! Updating Slint list models in place.

use slint::{Model as _, VecModel};

/// Makes `model` equal to `rows` by changing only the rows that differ (Slint keeps the elements of unchanged rows, their
/// state and their animations), then appending or removing at the end.
pub fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    let count = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        match model.row_data(index) {
            Some(current) if current == row => {}
            Some(_) => model.set_row_data(index, row),
            None => model.push(row),
        }
    }
    while model.row_count() > count {
        model.remove(model.row_count() - 1);
    }
}

/// Like [`sync`] for rows whose Slint form holds models (compared by pointer, so never equal): `shown` keeps the rows as
/// last shown, and only the rows that differ from it are made again with `make`.
pub fn sync_with<P: Clone + PartialEq, T: Clone + 'static>(
    model: &VecModel<T>,
    shown: &mut Vec<P>,
    rows: &[P],
    make: impl Fn(&P) -> T,
) {
    for (index, row) in rows.iter().enumerate() {
        match shown.get(index) {
            Some(current) if current == row && index < model.row_count() => {}
            _ if index < model.row_count() => model.set_row_data(index, make(row)),
            _ => model.push(make(row)),
        }
    }
    while model.row_count() > rows.len() {
        model.remove(model.row_count() - 1);
    }
    rows.clone_into(shown);
}
