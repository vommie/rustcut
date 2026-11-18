use super::*;

fn nav(count: usize, current: usize) -> Navigator {
    let mut nav = Navigator::new(15);
    let files = (1..=count)
        .map(|i| PathBuf::from(format!("/v/clip {i:03}.mp4")))
        .collect();
    nav.set_files(Arc::new(files), Some(current));
    nav
}

#[test]
fn it_opens_on_the_page_of_the_open_video() {
    let mut nav = nav(120, 46);
    nav.show_current();
    let view = nav.view(None);
    assert_eq!(view.page, 3);
    assert_eq!(view.range, "46–60 of 120");
    assert_eq!(view.rows.len(), 15);
    assert!(
        view.rows
            .get(1)
            .is_some_and(|r| r.current && r.highlighted && r.index == 46)
    );
    assert_eq!(view.current_page, Some(3));
    assert_eq!(nav.highlighted(), Some(&PathBuf::from("/v/clip 047.mp4")));
}

#[test]
fn keys_move_the_highlight_and_the_page_follows() {
    let mut nav = nav(120, 46);
    nav.show_current();
    nav.step(-2);
    assert_eq!(nav.view(None).page, 2, "row 45 is on page 3 (counted from 1)");
    nav.step(15);
    assert_eq!(nav.view(None).page, 3);
    nav.end();
    assert_eq!(nav.view(None).range, "106–120 of 120");
    nav.step(1);
    assert_eq!(
        nav.highlighted(),
        Some(&PathBuf::from("/v/clip 120.mp4")),
        "stops at the end"
    );
    nav.home();
    nav.step(-1);
    assert_eq!(nav.highlighted(), Some(&PathBuf::from("/v/clip 001.mp4")));
    nav.go_to_page(99);
    assert_eq!(nav.view(None).page, 7, "the last page");
    nav.hover(14);
    assert_eq!(nav.highlighted(), Some(&PathBuf::from("/v/clip 120.mp4")));
    nav.hover(3);
    assert_eq!(nav.highlighted(), Some(&PathBuf::from("/v/clip 109.mp4")));
}

#[test]
fn a_search_pages_its_hits_and_keeps_the_real_numbers() {
    let mut nav = nav(120, 46);
    let pattern = Pattern::parse("clip 00?.mp4", false).unwrap().unwrap();
    nav.set_hits(Some(filter(nav.names(), &pattern)));
    nav.show_current();
    let view = nav.view(Some(&pattern));
    assert_eq!(view.range, "1–9 of 9 found");
    assert_eq!(view.current_page, None, "the open video is no hit");
    assert!(!nav.current_is_hit());
    assert_eq!(view.rows.first().map(|r| r.index), Some(0));
    assert!(view.rows.iter().all(|r| !r.current));
    let nothing = Pattern::parse("xyz", false).unwrap().unwrap();
    nav.set_hits(Some(filter(nav.names(), &nothing)));
    let view = nav.view(Some(&nothing));
    assert_eq!(view.range, "0 found");
    assert!(view.rows.is_empty());
    assert_eq!(nav.highlighted(), None);
    nav.set_hits(None);
    assert_eq!(nav.view(None).range, "1–15 of 120");
}

#[test]
fn the_pager_keeps_seven_slots() {
    use Slot::{Gap, Page};
    assert_eq!(slots(1, 0), vec![Page(0)]);
    assert_eq!(slots(7, 6).len(), 7);
    assert_eq!(
        slots(8, 0),
        vec![Page(0), Page(1), Page(2), Page(3), Page(4), Gap, Page(7)]
    );
    assert_eq!(
        slots(8, 3),
        vec![Page(0), Page(1), Page(2), Page(3), Page(4), Gap, Page(7)]
    );
    assert_eq!(
        slots(8, 4),
        vec![Page(0), Gap, Page(3), Page(4), Page(5), Page(6), Page(7)]
    );
    assert_eq!(
        slots(20, 9),
        vec![Page(0), Gap, Page(8), Page(9), Page(10), Gap, Page(19)]
    );
    for at in 0..20 {
        assert_eq!(slots(20, at).len(), 7, "at {at}");
        assert!(slots(20, at).contains(&Page(at)), "at {at}");
    }
}

#[test]
fn a_name_keeps_its_hits_and_its_end_when_cut() {
    let seg = |name: &str, spans: &[Range<usize>]| {
        segments(name, spans)
            .into_iter()
            .map(|s| (s.text, s.hit, s.shrink))
            .collect::<Vec<_>>()
    };
    assert_eq!(seg("a.mp4", &[]), vec![("a.mp4".to_owned(), false, true)]);
    // Long names: the end (counter and extension) is a piece of its own that never gives way.
    let long = "2026-07-12 the harbor at dawn with boats 03.mp4";
    let pieces = seg(long, std::slice::from_ref(&(11..14)));
    assert_eq!(pieces.first(), Some(&("2026-07-12 ".to_owned(), false, false)));
    assert_eq!(pieces.get(1), Some(&("the".to_owned(), true, false)));
    assert!(
        pieces.get(2).is_some_and(|p| !p.1 && p.2),
        "the head of the rest gives way: {pieces:?}"
    );
    assert!(pieces.last().is_some_and(|p| p.0.ends_with("03.mp4") && !p.2));
    let joined: String = pieces.iter().map(|p| p.0.as_str()).collect();
    assert_eq!(joined, long);
    // A hit at the end: the longest plain piece gives way.
    assert_eq!(
        seg("clip one.mp4", std::slice::from_ref(&(8..12))),
        vec![("clip one".to_owned(), false, true), (".mp4".to_owned(), true, false)]
    );
}

#[test]
fn fewer_rows_and_a_new_listing_keep_the_place() {
    let mut nav = nav(120, 46);
    nav.show_current();
    nav.set_rows(20);
    assert_eq!(nav.view(None).range, "41–60 of 120");
    let files: Vec<PathBuf> = (1..=10).map(|i| PathBuf::from(format!("/v/{i}.mp4"))).collect();
    nav.set_files(Arc::new(files), Some(2));
    let view = nav.view(None);
    assert_eq!(view.page, 0, "clamped to the only page");
    assert_eq!(view.range, "1–10 of 10");
}
