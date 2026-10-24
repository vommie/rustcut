use proptest::prelude::*;

use super::*;

fn t(secs: i64) -> Timestamp {
    Timestamp::from_secs(secs)
}

fn with_marks(start: i64, end: i64) -> SectionsState {
    let mut s = SectionsState::default();
    s.set_duration(Some(t(100)));
    s.set_mark(Bound::Start, t(start));
    s.set_mark(Bound::End, t(end));
    s
}

fn starts(s: &SectionsState) -> Vec<i64> {
    s.rows().iter().map(|r| r.start.as_micros() / 1_000_000).collect()
}

#[test]
fn a_mark_on_the_wrong_side_clears_the_other() {
    let mut s = with_marks(10, 20);
    assert_eq!(s.current(), Ok(Section::new(t(10), t(20)).unwrap()));
    // A new start after the end begins a new section: the end is cleared.
    s.set_mark(Bound::Start, t(30));
    assert_eq!(s.marks(), (Some(t(30)), None));
    assert_eq!(s.current(), Err(EditError::NoRange));
    // An end before the start clears the start.
    s.set_mark(Bound::End, t(5));
    assert_eq!(s.marks(), (None, Some(t(5))));
    // Equal marks are not a range.
    s.set_mark(Bound::Start, t(2));
    s.set_mark(Bound::End, t(2));
    assert_eq!(s.marks(), (None, Some(t(2))));
}

#[test]
fn marks_are_limited_to_the_video() {
    let mut s = with_marks(10, 20);
    s.set_mark(Bound::End, t(500));
    assert_eq!(s.mark(Bound::End), Some(t(100)));
    assert_eq!(s.set_mark_text(Bound::Start, "0:00:12.5"), Ok(()));
    assert_eq!(s.mark(Bound::Start), Some(Timestamp::from_millis(12_500)));
    assert_eq!(
        s.set_mark_text(Bound::Start, "12:xx"),
        Err(EditError::NotATime("12:xx".into()))
    );
    assert_eq!(s.set_mark_text(Bound::Start, "  "), Ok(()));
    assert_eq!(s.mark(Bound::Start), None);
}

#[test]
fn adding_appends_selects_and_refuses_duplicates() {
    let mut s = with_marks(10, 20);
    let a = s.add_current().unwrap();
    assert_eq!(s.selected(), Some(a));
    assert_eq!(s.marks(), (Some(t(10)), Some(t(20))), "the marks stay");
    assert_eq!(s.add_current(), Err(EditError::Duplicate));
    s.set_mark(Bound::Start, t(30));
    assert_eq!(s.add_current(), Err(EditError::NoRange));
    s.set_mark(Bound::End, t(40));
    let b = s.add_current().unwrap();
    assert_eq!(s.selected(), Some(b));
    assert_eq!(starts(&s), [10, 30]);
}

#[test]
fn deleting_selects_the_row_above_then_the_one_below() {
    let mut s = SectionsState::default();
    for (a, b) in [(0, 1), (2, 3), (4, 5)] {
        s.set_mark(Bound::Start, t(a));
        s.set_mark(Bound::End, t(b));
        s.add_current().unwrap();
    }
    s.select(s.id_at(1));
    assert!(s.delete_selected());
    assert_eq!(s.selected(), s.id_at(0));
    assert!(s.delete_selected());
    assert_eq!(starts(&s), [4]);
    assert_eq!(s.selected(), s.id_at(0), "the row below took the place");
    assert!(s.delete_selected());
    assert_eq!(s.selected(), None);
    assert!(!s.delete_selected());
}

#[test]
fn moving_and_editing_rows() {
    let mut s = SectionsState::default();
    s.set_duration(Some(t(60)));
    for (a, b) in [(0, 10), (20, 30)] {
        s.set_mark(Bound::Start, t(a));
        s.set_mark(Bound::End, t(b));
        s.add_current().unwrap();
    }
    let second = s.id_at(1).unwrap();
    assert!(s.move_selected(true));
    assert_eq!(starts(&s), [20, 0]);
    assert!(!s.move_selected(true));
    // Typed times: reversed bounds are swapped, equal bounds refused, times past the end limited.
    assert_eq!(s.edit(second, Bound::End, "0:00:05"), Ok(()));
    assert_eq!(s.list().get(second), Section::new(t(5), t(20)).ok());
    assert_eq!(s.edit(second, Bound::Start, "20"), Err(EditError::Empty));
    assert_eq!(s.edit(second, Bound::End, "9:00"), Ok(()));
    assert_eq!(s.list().get(second).map(Section::end), Some(t(60)));
    assert_eq!(
        s.edit(second, Bound::End, "soon"),
        Err(EditError::NotATime("soon".into()))
    );
    s.list.remove(second);
    assert_eq!(s.edit(second, Bound::End, "1"), Err(EditError::Gone));
}

#[test]
fn dragged_edges_stop_before_their_opposite() {
    let mut s = with_marks(10, 20);
    let id = s.add_current().unwrap();
    let gap = Timestamp::from_millis(40);
    let start = Edge {
        owner: EdgeOwner::Section(id),
        bound: Bound::Start,
    };
    s.drag_edge(start, t(50), gap);
    assert_eq!(s.edge_time(start), Some(t(20) - gap));
    s.drag_edge(start, t(-5), gap);
    assert_eq!(s.edge_time(start), Some(Timestamp::ZERO));
    let end = Edge {
        owner: EdgeOwner::Marks,
        bound: Bound::End,
    };
    s.drag_edge(end, t(1), gap);
    assert_eq!(s.mark(Bound::End), Some(t(10) + gap));
    s.drag_edge(end, t(1_000), gap);
    assert_eq!(s.mark(Bound::End), Some(t(100)));
}

#[test]
fn cut_points_and_overlaps() {
    let mut s = with_marks(10, 20);
    s.add_current().unwrap();
    s.set_mark(Bound::End, t(25));
    s.set_mark(Bound::Start, t(15));
    s.add_current().unwrap();
    let rows = s.rows();
    assert!(rows.iter().all(|r| r.overlapping));
    assert_eq!(s.cut_points(), [t(0), t(10), t(15), t(20), t(25), t(100)]);
    assert_eq!(rows.iter().map(|r| r.number).collect::<Vec<_>>(), [1, 2]);
    // The later section's bubble names the earlier one.
    assert_eq!(s.overlaps(), [(1, 1)]);
}

#[test]
fn typed_times_are_checked_without_a_change() {
    let mut s = with_marks(10, 20);
    let id = s.add_current().unwrap();
    assert!(SectionsState::check_mark_text(Bound::Start, "").is_ok());
    assert!(matches!(
        SectionsState::check_mark_text(Bound::Start, "1:2x"),
        Err(EditError::NotATime(_))
    ));
    assert_eq!(s.check_edit(id, Bound::End, "0:00:10"), Err(EditError::Empty));
    assert!(s.check_edit(id, Bound::End, "0:00:30").is_ok());
    assert_eq!(s.list().get(id).map(Section::end), Some(t(20)), "nothing changed");
}

#[test]
fn a_shorter_duration_cuts_the_list() {
    let mut s = with_marks(10, 20);
    let id = s.add_current().unwrap();
    s.set_duration(Some(t(5)));
    assert!(s.list().is_empty());
    assert_eq!(s.selected(), None);
    assert_eq!(s.marks(), (Some(t(5)), Some(t(5))));
    assert!(s.list().get(id).is_none());
    s.clear();
    assert!(s.list().is_empty() && s.marks() == (None, None) && s.duration().is_none());
}

#[derive(Debug, Clone)]
enum Op {
    Mark(bool, i64),
    Add,
    Select(usize),
    Delete,
    Move(bool),
    Drag(usize, bool, i64),
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<bool>(), -10i64..120).prop_map(|(s, v)| Op::Mark(s, v)),
        Just(Op::Add),
        (0usize..6).prop_map(Op::Select),
        Just(Op::Delete),
        any::<bool>().prop_map(Op::Move),
        (0usize..7, any::<bool>(), -10i64..120).prop_map(|(i, b, v)| Op::Drag(i, b, v)),
    ]
}

proptest! {
    /// Whatever the user does, the marks stay inside the video and in order, sections stay valid, and the selection
    /// always names a listed section.
    #[test]
    fn every_edit_keeps_the_state_valid(ops in prop::collection::vec(arb_op(), 0..40)) {
        let mut s = SectionsState::default();
        s.set_duration(Some(t(100)));
        let gap = Timestamp::from_millis(40);
        for op in ops {
            match op {
                Op::Mark(start, v) => s.set_mark(if start { Bound::Start } else { Bound::End }, t(v)),
                Op::Add => { let _ = s.add_current(); }
                Op::Select(i) => s.select(s.id_at(i)),
                Op::Delete => { s.delete_selected(); }
                Op::Move(up) => { s.move_selected(up); }
                Op::Drag(i, start, v) => {
                    let owner = s.id_at(i).map_or(EdgeOwner::Marks, EdgeOwner::Section);
                    let bound = if start { Bound::Start } else { Bound::End };
                    s.drag_edge(Edge { owner, bound }, t(v), gap);
                }
            }
            let (a, b) = s.marks();
            for m in [a, b].into_iter().flatten() {
                prop_assert!(m >= Timestamp::ZERO && m <= t(100));
            }
            if let (Some(a), Some(b)) = (a, b) {
                prop_assert!(a <= b);
            }
            for section in s.list().sections() {
                prop_assert!(section.start() < section.end() && section.end() <= t(100));
            }
            if let Some(id) = s.selected() {
                prop_assert!(s.list().get(id).is_some());
            }
        }
    }
}

#[test]
fn the_automatic_section_of_a_save_without_sections() {
    use rustcut_core::settings::AutoSection;
    let whole = Section::new(Timestamp::ZERO, t(100)).unwrap();
    let range = Section::new(t(4), t(9)).unwrap();
    let marked = with_marks(4, 9);
    let mut unmarked = SectionsState::default();
    unmarked.set_duration(Some(t(100)));
    let mut one = unmarked.clone();
    one.set_mark(Bound::Start, t(4));

    // PyCutRv's rule, the default since 0.0.31 (Q-P8): the marked range, without marks the whole video.
    assert_eq!(AutoSection::default(), AutoSection::RangeOrWhole);
    assert_eq!(marked.auto_section(AutoSection::RangeOrWhole), Ok(range));
    assert_eq!(unmarked.auto_section(AutoSection::RangeOrWhole), Ok(whole));
    assert!(
        one.auto_section(AutoSection::RangeOrWhole).is_err(),
        "one mark alone is refused"
    );
    // The other ways.
    assert!(marked.auto_section(AutoSection::Off).is_err());
    assert_eq!(marked.auto_section(AutoSection::WholeVideo), Ok(whole));
    assert_eq!(marked.auto_section(AutoSection::CurrentRange), Ok(range));
    assert!(unmarked.auto_section(AutoSection::CurrentRange).is_err());
    let mut unknown = SectionsState::default();
    unknown.set_duration(None);
    assert!(
        unknown.auto_section(AutoSection::RangeOrWhole).is_err(),
        "the length is not known yet"
    );
}
