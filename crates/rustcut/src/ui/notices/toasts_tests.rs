use std::time::{Duration, Instant};

use rustcut_core::settings::{ToastOrder, ToastSettings, ToastStacking};

use super::*;

fn toast(kind: Kind, level: Level, title: &str, secs: Option<u64>) -> NewToast {
    NewToast {
        kind,
        level,
        title: title.into(),
        detail: String::new(),
        key: None,
        progress: None,
        actions: Vec::new(),
        clickable: false,
        time: "12:00:00".into(),
        shown_for: secs.map(Duration::from_secs),
        job: None,
    }
}

fn ids(layout: &Layout) -> Vec<u64> {
    layout.shown.iter().map(|s| s.id).collect()
}

#[test]
fn cards_time_out_pause_under_the_pointer_and_fade_before_they_go() {
    let t0 = Instant::now();
    let mut stack = ToastStack::default();
    let settings = ToastSettings::default();
    let a = stack
        .add(toast(Kind::JobQueued, Level::Success, "Job 1 queued", Some(5)), t0)
        .id();
    let e = stack
        .add(toast(Kind::JobFailed, Level::Error, "Job failed", None), t0)
        .id();
    assert_eq!(ids(&stack.layout(&settings)), [e, a], "errors first");
    assert_eq!(
        ToastStack::remaining(stack.get(a).unwrap(), t0 + Duration::from_secs(1)),
        Some(0.8)
    );

    stack.hover(a, true, t0 + Duration::from_secs(4));
    assert!(!stack.tick(t0 + Duration::from_secs(30)), "paused");
    stack.hover(a, false, t0 + Duration::from_secs(30));
    assert!(!stack.tick(t0 + Duration::from_millis(30_900)));
    assert!(
        stack.tick(t0 + Duration::from_millis(31_100)),
        "the second it had left is over"
    );
    assert!(stack.get(a).unwrap().leaving.is_some());
    assert_eq!(ids(&stack.layout(&settings)), [e, a], "still drawn while it fades");
    assert!(stack.tick(t0 + Duration::from_secs(32)));
    assert_eq!(ids(&stack.layout(&settings)), [e]);
    assert!(!stack.tick(t0 + Duration::from_secs(3_600)), "an error stays");
    assert!(stack.close(e, t0));
    stack.tick(t0 + Duration::from_secs(3_601));
    assert!(stack.is_empty());
}

#[test]
fn repeats_count_up_and_keyed_cards_change_in_place() {
    let t0 = Instant::now();
    let mut stack = ToastStack::default();
    let first = stack.add(toast(Kind::LogCopied, Level::Success, "3 lines copied", Some(5)), t0);
    let again = stack.add(
        toast(Kind::LogCopied, Level::Success, "3 lines copied", Some(5)),
        t0 + Duration::from_secs(4),
    );
    assert_eq!(again, Added::Repeated(first.id()));
    let card = stack.get(first.id()).unwrap();
    assert_eq!(card.count, 2);
    assert!(!stack.tick(t0 + Duration::from_secs(8)), "its time started again");

    let mut progress = toast(Kind::ProbeRender, Level::Info, "Probe render", None);
    progress.key = Some("probe".into());
    progress.progress = Some(0.1);
    let p = stack.add(progress.clone(), t0).id();
    progress.progress = Some(0.5);
    assert_eq!(stack.add(progress.clone(), t0), Added::Updated(p));
    progress.title = "Probe render ready".into();
    progress.shown_for = Some(Duration::from_secs(5));
    assert_eq!(stack.add(progress, t0), Added::Updated(p));
    assert_eq!(stack.get(p).unwrap().title, "Probe render ready");
    assert_eq!(stack.close_key("probe", t0), Some(p));
    assert_eq!(stack.close_key("probe", t0), None);
}

#[test]
fn the_layout_follows_the_stacking_and_order() {
    let t0 = Instant::now();
    let mut stack = ToastStack::default();
    let mut settings = ToastSettings {
        max_visible: 2,
        ..ToastSettings::default()
    };
    let a = stack.add(toast(Kind::JobQueued, Level::Success, "a", Some(5)), t0).id();
    let b = stack
        .add(toast(Kind::SectionRefused, Level::Warning, "b", Some(5)), t0)
        .id();
    let c = stack.add(toast(Kind::JobQueued, Level::Success, "c", Some(5)), t0).id();
    let d = stack.add(toast(Kind::JobFailed, Level::Error, "d", None), t0).id();

    let some = stack.layout(&settings);
    assert_eq!((ids(&some), some.more), (vec![d, b], 2));
    stack.set_expanded(true);
    assert_eq!(ids(&stack.layout(&settings)), [d, b, c, a]);
    stack.set_expanded(false);

    settings.order = ToastOrder::NewestTop;
    assert_eq!(ids(&stack.layout(&settings)), [d, c]);
    settings.order = ToastOrder::NewestBottom;
    assert_eq!(ids(&stack.layout(&settings)), [a, b]);

    settings.order = ToastOrder::Severity;
    settings.stacking = ToastStacking::All;
    assert_eq!(stack.layout(&settings).shown.len(), 4);

    settings.stacking = ToastStacking::Deck;
    let deck = stack.layout(&settings);
    assert_eq!(deck.shown, [Shown { id: d, depth: 0 }, Shown { id: b, depth: 1 }]);
    assert_eq!(deck.more, 2);

    settings.stacking = ToastStacking::Pager;
    let pager = stack.layout(&settings);
    assert_eq!((ids(&pager), pager.pager), (vec![d], Some((0, 4))));
    stack.page(-1);
    assert_eq!(stack.layout(&settings).pager, Some((3, 4)));
    stack.page(2);
    assert_eq!(ids(&stack.layout(&settings)), [b]);

    settings.stacking = ToastStacking::Summary;
    let summary = stack.layout(&settings);
    assert!(summary.shown.is_empty());
    assert_eq!(
        summary.summary,
        Some(Summary {
            total: 4,
            errors: 1,
            warnings: 1,
            level: Level::Error
        })
    );
    stack.set_expanded(true);
    assert_eq!(stack.layout(&settings).shown.len(), 4);
}

#[test]
fn a_card_that_is_not_drawn_goes_at_once() {
    let t0 = Instant::now();
    let mut stack = ToastStack::default();
    let settings = ToastSettings {
        max_visible: 2,
        ..ToastSettings::default()
    };
    let ids_: Vec<u64> = (0..4)
        .map(|i| {
            stack
                .add(toast(Kind::JobQueued, Level::Success, &format!("{i}"), Some(5)), t0)
                .id()
        })
        .collect();
    stack.layout(&settings);
    stack.close_all(t0);
    let layout = stack.layout(&settings);
    assert_eq!(
        layout.shown.len(),
        2,
        "the two drawn ones fade, the hidden ones are gone"
    );
    assert_eq!(stack.toasts().len(), 2);
    assert!(ids_.iter().rev().take(2).all(|id| stack.get(*id).is_some()));
}
