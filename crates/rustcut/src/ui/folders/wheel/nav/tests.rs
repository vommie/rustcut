use std::path::Path;

use rustcut_core::folders::Spot;

use super::*;

const RADII: Radii = Radii {
    inner: 30.0,
    outer: 100.0,
    dead: 12.0,
};

/// Favorites on 4 places: the source 0, Clients 1 (group: North 2, South 3), Drone 4 on place 2.
fn tree() -> FolderTree {
    let mut t = FolderTree::default();
    let c = t.add_group("Clients", Spot::End).unwrap();
    t.add_folder(Path::new("/n"), Spot::Into(c)).unwrap();
    t.add_folder(Path::new("/s"), Spot::Into(c)).unwrap();
    let d = t.add_folder(Path::new("/d"), Spot::End).unwrap();
    t.put_on_place(FolderId::SOURCE, 0).unwrap();
    t.put_on_place(c, 1).unwrap();
    t.put_on_place(d, 2).unwrap();
    t
}

fn ctx(t: &FolderTree) -> Context<'_> {
    Context {
        tree: t,
        layout: Layout {
            places: 4,
            divided: false,
        },
        current: None,
    }
}

/// The middle of place `i` of 4 (0 top, 1 right, 2 bottom, 3 left).
fn spot(i: usize) -> (f32, f32) {
    [(0.0, -65.0), (65.0, 0.0), (0.0, 65.0), (-65.0, 0.0)][i]
}

fn held(t: &FolderTree) -> WheelNav {
    let mut nav = WheelNav::new(Timing::default(), RADII);
    assert_eq!(nav.press(0), Outcome::Changed);
    assert!(nav.is_arming());
    assert!((nav.hold_progress(125) - 0.5).abs() < 0.01);
    assert_eq!(nav.moved(0.0, 0.0, 100, ctx(t)), Outcome::Changed);
    assert_eq!(nav.tick(250, ctx(t)), Outcome::Opened);
    nav
}

#[test]
fn the_empty_wheels_middle_leads_to_the_settings() {
    let t = FolderTree::default();
    let mut nav = held(&t);
    assert!(nav.is_empty());
    nav.moved(5.0, 0.0, 300, ctx(&t));
    assert!(nav.middle(600).is_some_and(|p| p > 0.4));
    assert!(nav.needs_ticks());
    assert_eq!(nav.tick(900, ctx(&t)), Outcome::Settings);
    assert!(!nav.is_open());
    let mut nav = WheelNav::new(Timing::default(), RADII);
    nav.open_by_key(ctx(&t));
    assert_eq!(nav.highlight(), None, "nothing to point at");
    assert_eq!(nav.key(KeyAction::Enter, ctx(&t)), Outcome::Settings);
    nav.open_by_key(ctx(&t));
    nav.key(KeyAction::Right, ctx(&t));
    assert_eq!(nav.click(0.0, 0.0, ctx(&t)), Outcome::Settings);
    // Opened by key, the pointer takes over once it moves: resting in the middle leads there too.
    nav.open_by_key(ctx(&t));
    assert_eq!(nav.moved(90.0, 0.0, 1_000, ctx(&t)), Outcome::Nothing, "where it stood");
    assert_eq!(nav.moved(92.0, 0.0, 1_010, ctx(&t)), Outcome::Nothing, "a jitter");
    assert_eq!(nav.moved(5.0, 0.0, 1_100, ctx(&t)), Outcome::Changed);
    assert_eq!(nav.mode(), Some(Mode::Clicks));
    assert_eq!(nav.tick(1_700, ctx(&t)), Outcome::Settings);
}

#[test]
fn places_are_found_clockwise_from_the_top() {
    assert_eq!(place_at(0.0, -50.0, 4, 30.0), Some(0));
    assert_eq!(place_at(50.0, 0.0, 4, 30.0), Some(1));
    assert_eq!(place_at(0.0, 50.0, 4, 30.0), Some(2));
    assert_eq!(place_at(-50.0, 0.0, 4, 30.0), Some(3));
    assert_eq!(
        place_at(-50.0, -45.0, 4, 30.0),
        Some(3),
        "just left of the top-left diagonal"
    );
    assert_eq!(place_at(5.0, 5.0, 4, 30.0), None, "the middle");
    assert_eq!(place_at(50.0, 0.0, 0, 30.0), None);
}

#[test]
fn a_short_press_is_a_normal_click() {
    let t = tree();
    let mut nav = WheelNav::new(Timing::default(), RADII);
    nav.press(0);
    nav.moved(3.0, 0.0, 100, ctx(&t));
    assert_eq!(nav.release(ctx(&t)), Outcome::Closed);
    assert!(!nav.is_open() && !nav.is_arming());
}

#[test]
fn releasing_over_a_folder_picks_it() {
    let t = tree();
    let mut nav = held(&t);
    let (x, y) = spot(2);
    nav.moved(x, y, 300, ctx(&t));
    assert_eq!(nav.highlight(), Some(2));
    assert_eq!(nav.release(ctx(&t)), Outcome::Picked(FolderId(4)));
    assert!(!nav.is_open());
}

#[test]
fn dwelling_opens_a_group_and_the_middle_goes_back_once_per_visit() {
    let t = tree();
    let mut nav = held(&t);
    let (x, y) = spot(1);
    nav.moved(x, y, 300, ctx(&t));
    assert_eq!(nav.fill(600).map(|(i, _)| i), Some(1));
    assert!(nav.needs_ticks());
    nav.tick(899, ctx(&t));
    assert_eq!(nav.depth(), 0, "not yet");
    nav.tick(900, ctx(&t));
    assert_eq!(nav.depth(), 1, "Clients opened after the dwell");
    assert_eq!(nav.level().unwrap().slots.first(), Some(&Slot::Entry(FolderId(2))));
    // Frozen: resting on the same spot does not open anything again.
    nav.tick(2_000, ctx(&t));
    assert_eq!(nav.fill(2_000), None);
    // The middle: back after the dwell, once per visit.
    nav.moved(0.0, 0.0, 2_100, ctx(&t));
    assert_eq!(nav.middle(2_400).map(|p| p > 0.4), Some(true));
    nav.tick(2_700, ctx(&t));
    assert_eq!(nav.depth(), 0);
    nav.tick(4_000, ctx(&t));
    assert_eq!(nav.depth(), 0, "still at the start");
    // Releasing in the middle cancels.
    assert_eq!(nav.release(ctx(&t)), Outcome::Closed);
}

#[test]
fn pushing_past_the_edge_opens_at_once_and_a_release_over_a_group_keeps_the_wheel() {
    let t = tree();
    let mut nav = held(&t);
    nav.moved(110.0, 0.0, 300, ctx(&t));
    assert_eq!(nav.depth(), 0, "inside the push edge");
    nav.moved(130.0, 0.0, 310, ctx(&t));
    assert_eq!(nav.depth(), 1, "pushed through");
    let mut nav = held(&t);
    let (x, y) = spot(1);
    nav.moved(x, y, 300, ctx(&t));
    assert_eq!(nav.release(ctx(&t)), Outcome::Changed);
    assert_eq!((nav.mode(), nav.depth()), (Some(Mode::Clicks), 1));
    // Clicks go on: the middle goes up, a folder picks.
    assert_eq!(nav.click(0.0, 0.0, ctx(&t)), Outcome::Changed);
    assert_eq!(nav.depth(), 0);
    let (x, y) = spot(2);
    assert_eq!(nav.click(x, y, ctx(&t)), Outcome::Picked(FolderId(4)));
}

#[test]
fn the_keys_turn_open_go_back_and_close() {
    let t = tree();
    let mut nav = WheelNav::new(Timing::default(), RADII);
    assert_eq!(nav.open_by_key(ctx(&t)), Outcome::Opened);
    assert_eq!(
        (nav.mode(), nav.highlight()),
        (Some(Mode::Keys), None),
        "nothing chosen yet"
    );
    nav.key(KeyAction::Enter, ctx(&t));
    assert!(nav.is_open(), "Enter without a choice does nothing");
    nav.key(KeyAction::Left, ctx(&t));
    assert_eq!(nav.highlight(), Some(0), "the first turn starts at the first folder");
    nav.key(KeyAction::Left, ctx(&t));
    assert_eq!(nav.highlight(), Some(2), "empty places are skipped");
    nav.key(KeyAction::Right, ctx(&t));
    nav.key(KeyAction::Right, ctx(&t));
    assert_eq!(nav.highlight(), Some(1));
    nav.key(KeyAction::Open, ctx(&t));
    assert_eq!((nav.depth(), nav.highlight()), (1, None));
    nav.key(KeyAction::Back, ctx(&t));
    assert_eq!(nav.depth(), 0);
    nav.key(KeyAction::Right, ctx(&t));
    nav.key(KeyAction::Enter, ctx(&t));
    assert_eq!(nav.depth(), 1, "Enter opens a group");
    assert_eq!(
        nav.open_by_key(ctx(&t)),
        Outcome::Changed,
        "the key again: one level up"
    );
    assert_eq!(nav.depth(), 0);
    assert_eq!(
        nav.key(KeyAction::Again, ctx(&t)),
        Outcome::Closed,
        "at the start it closes"
    );
    nav.open_by_key(ctx(&t));
    nav.key(KeyAction::Right, ctx(&t));
    assert_eq!(nav.key(KeyAction::Enter, ctx(&t)), Outcome::Picked(FolderId::SOURCE));
    nav.open_by_key(ctx(&t));
    assert_eq!(nav.key(KeyAction::Escape, ctx(&t)), Outcome::Closed);
}

#[test]
fn the_keys_start_at_the_current_folder() {
    let t = tree();
    let c = Context {
        current: Some(FolderId(3)),
        ..ctx(&t)
    };
    let mut nav = WheelNav::new(Timing::default(), RADII);
    nav.open_by_key(c);
    nav.key(KeyAction::Right, c);
    assert_eq!(nav.highlight(), Some(1), "South lies in Clients, on place 1");
}

/// A wheel whose pointer starts in the middle, opened with the real pointer far from the center.
fn centered(t: &FolderTree) -> WheelNav {
    let mut nav = WheelNav::new(
        Timing {
            center: true,
            ..Timing::default()
        },
        RADII,
    );
    nav.press(0);
    nav.moved(300.0, 200.0, 10, ctx(t));
    assert_eq!(nav.tick(250, ctx(t)), Outcome::Opened);
    nav
}

#[test]
fn a_short_move_from_anywhere_chooses_a_spoke() {
    let t = tree();
    let mut nav = centered(&t);
    assert_eq!(nav.middle(300), None, "the middle is no cancel yet");
    assert_eq!(nav.virtual_pointer(), Some((0.0, 0.0)));
    nav.moved(305.0, 200.0, 300, ctx(&t));
    assert_eq!(nav.highlight(), None, "inside the dead zone");
    nav.moved(320.0, 200.0, 310, ctx(&t));
    assert_eq!(nav.highlight(), Some(1), "20 px to the right are enough");
    assert_eq!(nav.virtual_pointer(), Some((20.0, 0.0)));
    nav.moved(300.0, 500.0, 320, ctx(&t));
    let (x, y) = nav.virtual_pointer().unwrap();
    assert!(x.hypot(y) <= 100.5, "the dot stays on the wheel");
}

#[test]
fn every_level_starts_in_the_middle_again() {
    let t = tree();
    let mut nav = centered(&t);
    nav.moved(320.0, 200.0, 300, ctx(&t));
    nav.tick(900, ctx(&t));
    assert_eq!(nav.depth(), 1, "Clients opened after the dwell");
    assert_eq!(
        nav.take_change(),
        Some(LevelChange {
            back: false,
            place: Some(1)
        })
    );
    assert_eq!(nav.virtual_pointer(), Some((0.0, 0.0)));
    assert_eq!(nav.highlight(), None);
    // Back only after the pointer left the middle and came back.
    nav.tick(2_000, ctx(&t));
    assert_eq!(nav.depth(), 1);
    nav.moved(340.0, 200.0, 2_100, ctx(&t));
    nav.moved(320.0, 200.0, 2_200, ctx(&t));
    nav.tick(2_900, ctx(&t));
    assert_eq!(nav.depth(), 0);
    assert_eq!(nav.take_change().map(|c| c.back), Some(true));
}
