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

/// Makes `model` equal to `rows` where every row has an identity (`id`): rows of ids that are gone are removed, new ids
/// are inserted at their place and a row that changed is set in place, so Slint keeps the elements of the other rows and
/// runs the appear animation only for the new ones (the toasts). A row that moved is removed and inserted again.
pub fn sync_keyed<T: Clone + PartialEq + 'static, K: PartialEq>(
    model: &VecModel<T>,
    rows: Vec<T>,
    id: impl Fn(&T) -> K,
) {
    let ids: Vec<K> = rows.iter().map(&id).collect();
    for index in (0..model.row_count()).rev() {
        if model.row_data(index).is_some_and(|row| !ids.contains(&id(&row))) {
            model.remove(index);
        }
    }
    for (index, row) in rows.into_iter().enumerate() {
        let key = id(&row);
        match model.row_data(index) {
            Some(current) if id(&current) == key => {
                if current != row {
                    model.set_row_data(index, row);
                }
            }
            _ => {
                if let Some(old) =
                    (index..model.row_count()).find(|&i| model.row_data(i).is_some_and(|r| id(&r) == key))
                {
                    model.remove(old);
                }
                model.insert(index, row);
            }
        }
    }
    while model.row_count() > ids.len() {
        model.remove(model.row_count() - 1);
    }
}

#[cfg(test)]
mod tests {
    use slint::{Model as _, VecModel};

    use super::sync_keyed;

    #[test]
    fn keyed_rows_keep_their_place_and_new_ones_are_inserted() {
        let model = VecModel::from(vec![(1, "a"), (2, "b"), (3, "c")]);
        sync_keyed(&model, vec![(4, "d"), (1, "a"), (3, "C")], |r| r.0);
        assert_eq!(model.iter().collect::<Vec<_>>(), [(4, "d"), (1, "a"), (3, "C")]);
        sync_keyed(&model, vec![(3, "C"), (4, "d")], |r| r.0);
        assert_eq!(model.iter().collect::<Vec<_>>(), [(3, "C"), (4, "d")]);
        sync_keyed(&model, Vec::new(), |r| r.0);
        assert_eq!(model.row_count(), 0);
    }
}
