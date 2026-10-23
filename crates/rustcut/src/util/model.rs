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
