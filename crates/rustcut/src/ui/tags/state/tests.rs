use super::*;

fn tree() -> Arc<TagTree> {
    let tags = [
        (1, None, "Places"),
        (2, Some(1), "Beach"),
        (3, Some(1), "Alps"),
        (4, None, "Anna"),
    ]
    .into_iter()
    .map(|(id, parent_id, name)| TagItem {
        id,
        parent_id,
        name: name.into(),
    })
    .collect();
    Arc::new(TagTree {
        success: true,
        total_tags: 4,
        tags,
    })
}

fn panel() -> TagPanel {
    let mut p = TagPanel::new(&TagPanelState::default());
    p.set_tree(tree());
    p
}

#[test]
fn clicks_assign_and_rating_toggles() {
    let mut p = panel();
    p.click_tag(2);
    p.click_tag(4);
    p.click_tag(99);
    assert_eq!(p.draft.tags, [2, 4]);
    assert!(p.history.is_empty(), "only a saved job changes the history");
    p.click_tag(2);
    assert_eq!(p.draft.tags, [4]);
    p.click_rating(3);
    assert_eq!(p.draft.rating, Some(3));
    p.click_rating(3);
    assert_eq!(p.draft.rating, None, "the chosen rating again clears it");
    p.click_rating(7);
    assert_eq!(p.draft.rating, None);
    p.click_tag(2);
    assert_eq!(p.draft.tags, [4, 2]);
}

#[test]
fn a_saved_job_moves_its_tags_to_the_front_and_ages_the_others() {
    let mut p = panel();
    p.history = vec![1, 2, 3];
    p.last_tags = vec![1];
    assert_eq!((p.age(1), p.age(2)), (0, 1), "a file before 0.0.76 has no ages");
    p.click_tag(4);
    p.click_tag(3);
    p.click_tag(2);
    p.click_tag(2);
    assert_eq!(p.history, [1, 2, 3], "clicks change nothing");
    p.saved(&[3, 4], None);
    assert_eq!(p.history, [3, 4, 1, 2], "the last clicked first");
    assert_eq!([3, 4, 1, 2].map(|id| p.age(id)), [0, 0, 1, 2]);
    p.click_tag(1);
    p.saved(&[1], None);
    p.saved(&[1], None);
    assert_eq!(p.history, [1, 3, 4, 2]);
    assert_eq!([1, 3, 4, 2].map(|id| p.age(id)), [0, 2, 2, 4]);
    p.saved(&[], None);
    assert_eq!(p.age(1), 0, "a job without tags changes nothing");
    let mut saved = TagPanelState::default();
    p.save_into(&mut saved);
    assert_eq!(
        (saved.history.clone(), saved.history_age.clone()),
        (vec![1, 3, 4, 2], vec![0, 2, 2, 4])
    );
    assert_eq!(TagPanel::new(&saved).age(2), 4, "a restart keeps the ages");
}

#[test]
fn locked_tags_survive_a_save_and_clear() {
    let mut p = panel();
    p.click_tag(2);
    assert!(p.toggle_lock(4));
    assert_eq!(p.draft.tags, [2, 4], "locking assigns");
    p.click_rating(5);
    assert_eq!(p.draft_metadata(), (vec![2, 4], Some(5)));
    p.saved(&[2, 4], Some(5));
    assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4], None));
    assert_eq!((p.last_tags.clone(), p.last_rating), (vec![2, 4], Some(5)));
    p.apply_last_tags();
    p.apply_last_rating();
    assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4, 2], Some(5)));
    assert!(p.can_clear());
    p.clear();
    assert_eq!(p.draft.tags, [4]);
    assert!(!p.can_clear(), "only the locked tag is left");
    // A save without tags or rating keeps the last ones.
    p.saved(&[], None);
    assert_eq!((p.last_tags.clone(), p.last_rating), (vec![2, 4], Some(5)));
    // Unlocking keeps the tag for this job; unassigning a locked tag unlocks it.
    p.toggle_lock(4);
    assert_eq!((p.locked.is_empty(), p.draft.tags.clone()), (true, vec![4]));
    p.toggle_lock(4);
    p.click_tag(4);
    assert!(p.locked.is_empty() && p.draft.tags.is_empty());
}

#[test]
fn editing_a_job_leaves_the_draft_alone() {
    let mut p = panel();
    p.toggle_lock(4);
    p.click_rating(2);
    let original = JobMetadata {
        tags: vec![3],
        rating: Some(1),
        source_hash: Some("abc".into()),
        without_rustview: false,
    };
    p.begin_edit(JobId(7), "Clip - 01".into(), original);
    assert_eq!(p.choice().tags, [3]);
    assert!(p.edited().is_none(), "nothing changed yet");
    assert!(!p.toggle_lock(2), "no locks while a job is edited");
    p.click_tag(2);
    p.click_rating(4);
    let (id, metadata) = p.edited().unwrap();
    assert_eq!(id, JobId(7));
    assert_eq!(
        metadata,
        JobMetadata {
            tags: vec![2, 3],
            rating: Some(4),
            source_hash: Some("abc".into()),
            without_rustview: false,
        }
    );
    p.clear();
    assert_eq!(
        p.choice(),
        &Choice::default(),
        "clearing an edited job clears everything"
    );
    p.end_edit();
    assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4], Some(2)));
}

#[test]
fn hidden_branches_the_eye_and_a_new_tree_prune_unknown_ids() {
    let mut p = panel();
    p.click_tag(2);
    p.toggle_branch(1);
    assert!(p.marks.hidden.contains(&1));
    assert_eq!(p.draft.tags, [2], "hiding does not assign");
    assert_eq!(p.assigned_in_hidden(), 1);
    assert_eq!(p.rows().iter().map(|r| r.id).collect::<Vec<_>>(), [4]);
    p.expanded = [1, 2, 3, 4].into_iter().collect();
    p.toggle_show_hidden();
    let rows: Vec<(i32, bool)> = p.rows().iter().map(|r| (r.id, r.is_dimmed)).collect();
    assert_eq!(
        rows,
        [(4, false), (1, true), (3, true), (2, true)],
        "the eye shows them dimmed"
    );
    p.click_tag(3);
    assert_eq!(p.draft.tags, [2, 3], "a click on a shown hidden tag assigns it");
    p.toggle_show_hidden();

    let mut smaller = (*tree()).clone();
    smaller.tags.retain(|t| t.id == 4);
    p.set_tree(Arc::new(smaller));
    assert!(p.marks.hidden.is_empty() && p.draft.tags.is_empty() && p.history.is_empty());
    assert_eq!(p.expanded, [4].into_iter().collect());
}

#[test]
fn regex_filter_falls_back_and_state_round_trips() {
    let mut p = panel();
    p.expanded = [1, 2, 3, 4].into_iter().collect();
    p.filter = "be".into();
    assert_eq!(p.rows().iter().map(|r| r.id).collect::<Vec<_>>(), [1, 2]);
    p.regex = true;
    p.filter = "^(al|an)".into();
    assert_eq!(p.rows().iter().map(|r| r.id).collect::<Vec<_>>(), [4, 1, 3]);
    p.filter = "(".into();
    assert!(p.filter_regex().is_err());
    assert!(p.rows().is_empty(), "matched as text: no tag holds a bracket");
    p.search_shown = false;
    assert!(
        !p.rows().is_empty() && p.filter_regex().unwrap().is_none(),
        "a hidden field does not filter"
    );

    p.toggle_lock(2);
    p.history_height = 120;
    p.toggle_show_hidden();
    let mut saved = TagPanelState::default();
    p.save_into(&mut saved);
    let restored = TagPanel::new(&saved);
    assert_eq!(restored.draft.tags, [2], "a locked tag is assigned after a restart");
    let mut again = TagPanelState::default();
    restored.save_into(&mut again);
    assert_eq!(again, saved);
}

#[test]
fn loaded_labels_replace_the_choice_and_keep_locked_tags() {
    let mut p = panel();
    p.toggle_lock(4);
    p.click_tag(3);
    p.click_rating(1);
    p.take_labels(&[2, 99, 4, 2], Some(5));
    assert_eq!(
        (p.draft.tags.clone(), p.draft.rating),
        (vec![4, 2], Some(5)),
        "the locked tag first, unknown ids dropped, each once; the old choice is gone"
    );
    p.take_labels(&[], Some(0));
    assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4], Some(0)));
    let original = JobMetadata {
        tags: vec![3],
        rating: Some(1),
        source_hash: None,
        without_rustview: false,
    };
    p.begin_edit(JobId(7), "Clip".into(), original);
    p.take_labels(&[2], Some(3));
    assert_eq!(
        p.choice(),
        &Choice {
            tags: vec![2],
            rating: Some(3)
        },
        "an edited job takes them, without locks"
    );
    assert_eq!(
        (p.draft.tags.clone(), p.draft.rating),
        (vec![4], Some(0)),
        "the draft waits"
    );
    let (_, edited) = p.edited().unwrap();
    assert_eq!((edited.tags, edited.rating), (vec![2], Some(3)));
}
