use std::cmp::Ordering;

use super::{NameKey, NameOrder, SortingChoice, extension_start, parse_locale};

/// The order Dolphin 26.08 showed for these names (name ascending, natural sorting, en_US; 2026-11-17).
const DOLPHIN_ORDER: [&str; 21] = [
    "_x.mov",
    "#hash.mp4",
    "9.mp4",
    "10.mp4",
    "apfel.mp4",
    "Ärger.mkv",
    "File a1.10.mp4",
    "File a1.9.mp4",
    "File a2.mp4",
    "file A2.mp4",
    "File a2 b.mp4",
    "File a10.mp4",
    "file B.mp4",
    "file b.mp4",
    "File D.mp4",
    "file_b.mp4",
    "file-b.mp4",
    "notes.txt",
    "Ol.mp4",
    "Öl.mp4",
    "Zebra.mp4",
];

fn sorted(order: &NameOrder, names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(ToString::to_string).collect();
    names.sort_by(|a, b| order.compare(a, b).then_with(|| a.cmp(b)));
    names
}

#[test]
fn matches_the_order_dolphin_showed() {
    let order = NameOrder::new(SortingChoice::Natural, "en_US.UTF-8");
    let mut shuffled = DOLPHIN_ORDER;
    shuffled.reverse();
    shuffled.swap(3, 15);
    assert_eq!(sorted(&order, &shuffled), DOLPHIN_ORDER);
}

#[test]
fn the_german_locale_keeps_that_order() {
    let order = NameOrder::new(SortingChoice::Natural, "de_DE.UTF-8");
    let mut shuffled = DOLPHIN_ORDER;
    shuffled.rotate_left(7);
    assert_eq!(sorted(&order, &shuffled), DOLPHIN_ORDER);
}

#[test]
fn numbers_compare_by_value_and_as_decimals() {
    let order = NameOrder::new(SortingChoice::Natural, "C");
    assert_eq!(order.compare("a2.mp4", "a10.mp4"), Ordering::Less);
    assert_eq!(order.compare("a1.10.mp4", "a1.9.mp4"), Ordering::Less);
    assert_eq!(order.compare("v1.2.10", "v1.2.9"), Ordering::Greater);
    assert_eq!(order.compare("v1", "v1.5"), Ordering::Less);
    assert_eq!(order.compare("clip 0042", "clip 41"), Ordering::Greater);
    assert_eq!(
        order.compare("x99999999999999999999999", "x100000000000000000000000"),
        Ordering::Less
    );
    // Equal numbers: the plain string decides ("007" < "7").
    assert_eq!(order.compare("file007", "file7"), Ordering::Less);
}

#[test]
fn the_extension_counts_after_the_base_name() {
    let order = NameOrder::new(SortingChoice::Natural, "C");
    assert_eq!(order.compare("a.webm", "a.mp4"), Ordering::Greater);
    assert_eq!(order.compare("a.mp4", "a b.mp4"), Ordering::Less);
    assert_eq!(order.compare("a", "a.mp4"), Ordering::Less);
}

#[test]
fn extension_split_follows_dolphin() {
    for (name, base) in [
        ("a.tar.gz", "a.tar"),
        ("v1.5", "v1.5"),
        ("clip.final cut", "clip.final cut"),
        (".hidden", ".hidden"),
        ("noext", "noext"),
        ("Ärger.mkv", "Ärger"),
    ] {
        let split = extension_start(name).unwrap_or(name.len());
        assert_eq!(name.get(..split), Some(base), "{name}");
    }
}

#[test]
fn keys_compare_like_names() {
    let order = NameOrder::new(SortingChoice::Natural, "en_US.UTF-8");
    for pair in DOLPHIN_ORDER.windows(2) {
        let [a, b] = pair else { continue };
        assert_eq!(
            order.compare_keys(&NameKey::new(a), &NameKey::new(b)),
            order.compare(a, b)
        );
        assert_eq!(NameKey::new(a).name(), *a);
    }
}

#[test]
fn code_point_modes() {
    let sensitive = NameOrder::new(SortingChoice::CaseSensitive, "C");
    assert_eq!(sensitive.compare("Zebra", "apfel"), Ordering::Less);
    let insensitive = NameOrder::new(SortingChoice::CaseInsensitive, "C");
    assert_eq!(insensitive.compare("Zebra", "apfel"), Ordering::Greater);
    assert_eq!(insensitive.compare("a10", "a2"), Ordering::Less);
}

#[test]
fn config_values_and_locales() {
    assert_eq!(SortingChoice::from_config("2"), SortingChoice::CaseSensitive);
    assert_eq!(
        SortingChoice::from_config("CaseInsensitiveSorting"),
        SortingChoice::CaseInsensitive
    );
    assert_eq!(SortingChoice::from_config(""), SortingChoice::Natural);
    assert_eq!(parse_locale("de_DE.UTF-8@euro").to_string(), "de-DE");
    assert_eq!(parse_locale("POSIX").to_string(), "und");
    assert_eq!(parse_locale("not a locale!").to_string(), "und");
}

#[test]
fn sort_keys_sort_like_the_collator() {
    let order = NameOrder::new(SortingChoice::Natural, "de_DE.UTF-8");
    let names = [
        "a",
        "A",
        "ä",
        "Ä",
        "a1",
        "a01",
        "a 1",
        "a.b",
        "a-b",
        "a_b",
        "File 9.mp4",
        "file 10.mp4",
        "Öl",
        "ol",
        "",
        "1.10",
        "1.9",
        "x1.2.3",
        "x1.2.10",
        "äb",
        "ab",
        "Ab",
        "b",
    ];
    for a in names {
        for b in names {
            assert_eq!(
                order.compare_keys(&order.key(a), &order.key(b)),
                order.compare(a, b),
                "{a:?} / {b:?}"
            );
        }
    }
}
