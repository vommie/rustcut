//! The numbered series of a job's name (M5.4, user request): every file in the folder that the name template makes with
//! some counter value, whatever its extension ("Holiday - 01.mkv", "Holiday - 02.mp4", "Holiday - 17.webm"). The
//! conflict dialog lists all of them, because these are the files a user mixes up most easily.
//!
//! The template is rendered once with a marker number to learn what stands before and after the counter; a file name
//! counts only when its stem renders back exactly with its own number (so padding, other text and date variables must
//! match too).

/// A number no template turns into anything but its digits (all ten different, nine of them).
const MARKER: u32 = 918_273_645;

/// A file of the series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesFile {
    pub counter: u32,
    pub name: String,
}

/// The files of `names` that `stem_for(n)` makes for some counter `n`, sorted by counter, then name. Hidden files
/// (RustCut's temp files start with a dot) are left out. Empty when the template does not show the counter.
pub fn series<'a>(
    names: impl IntoIterator<Item = &'a String>,
    stem_for: impl Fn(u32) -> Option<String>,
) -> Vec<SeriesFile> {
    let Some(probe) = stem_for(MARKER) else {
        return Vec::new();
    };
    let marker = MARKER.to_string();
    let Some((prefix, suffix)) = probe.split_once(&marker) else {
        return Vec::new();
    };
    let mut found: Vec<SeriesFile> = names
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .filter_map(|name| {
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _ext)| stem);
            let digits = stem.strip_prefix(prefix)?.strip_suffix(suffix)?;
            if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let counter: u32 = digits.parse().ok()?;
            (stem_for(counter).as_deref() == Some(stem)).then(|| SeriesFile {
                counter,
                name: name.clone(),
            })
        })
        .collect();
    found.sort_by(|a, b| a.counter.cmp(&b.counter).then_with(|| a.name.cmp(&b.name)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn padded(n: u32) -> String {
        format!("Holiday - {n:02}")
    }

    #[test]
    fn every_number_and_extension_of_the_scheme_and_nothing_else() {
        let folder = names(&[
            "Holiday - 02.mp4",
            "Holiday - 01.mkv",
            "Holiday - 10.webm",
            "Holiday - 01.mp4",
            "Holiday - 1.mkv",
            "Holiday - 03 (1).mkv",
            "Holiday - 04 extra.mkv",
            "Holiday.mkv",
            "holiday - 05.mkv",
            ".Holiday - 06.rustcut-part.mkv",
            "Holiday - 07",
            "Holiday - x8.mkv",
            "Holiday - 9999999999.mkv",
        ]);
        let found: Vec<(u32, &str)> = series(&folder, |n| Some(padded(n)))
            .iter()
            .map(|f| (f.counter, folder.iter().find(|n| **n == f.name).unwrap().as_str()))
            .collect();
        assert_eq!(
            found,
            vec![
                (1, "Holiday - 01.mkv"),
                (1, "Holiday - 01.mp4"),
                (2, "Holiday - 02.mp4"),
                (7, "Holiday - 07"),
                (10, "Holiday - 10.webm"),
            ],
            "unpadded, other text, other case, hidden and non-numbers are not the series"
        );
    }

    #[test]
    fn text_after_the_counter_and_templates_without_one() {
        let folder = names(&["03 - Holiday.mkv", "3 - Holiday.mkv", "Holiday.mkv"]);
        let after = series(&folder, |n| Some(format!("{n:02} - Holiday")));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].counter, 3);
        assert!(
            series(&folder, |_| Some("Holiday".to_owned())).is_empty(),
            "no counter, no series"
        );
        assert!(series(&folder, |_| None).is_empty(), "a template that fails");
    }
}
