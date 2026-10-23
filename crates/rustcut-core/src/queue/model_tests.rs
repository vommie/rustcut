//! Tests of the pure queue model: the scheduler rules and property tests over random command sequences.

use std::path::PathBuf;

use proptest::prelude::*;

use super::model::{Model, moved};
use super::{MoveTarget, Tally};
use crate::job::{JobId, JobState, JobStateKind, sample_spec};
use crate::naming::same_path;
use crate::settings::WhenDone;
use crate::store::{JobRecord, SpecSlot};

fn job(id: i64, target: &str, state: JobState) -> JobRecord {
    let mut spec = sample_spec();
    spec.target.stem = target.into();
    JobRecord {
        id: JobId(id),
        position: id,
        state,
        attempt: 1,
        created_at: 0,
        started_at: None,
        finished_at: None,
        source_path: spec.source.clone(),
        target_path: spec.target_path(),
        note: None,
        spec: SpecSlot::Decoded(Box::new(spec)),
        sync: None,
    }
}

fn ids(raw: &[i64]) -> Vec<JobId> {
    raw.iter().copied().map(JobId).collect()
}

/// Starts what the scheduler picks, like the actor does.
fn start_picks(model: &mut Model) -> Vec<JobId> {
    let picks = model.schedule();
    for id in &picks {
        model
            .transition(*id, JobState::Rendering { pass: 0, passes: 0 }, None)
            .unwrap();
    }
    picks
}

#[test]
fn same_targets_never_run_together_and_never_stall_the_rest() {
    let jobs = vec![
        job(1, "a", JobState::Waiting),
        job(2, "a", JobState::Waiting),
        job(3, "b", JobState::Waiting),
        job(4, "c", JobState::Held),
        job(5, "d", JobState::Waiting),
    ];
    let mut model = Model::new(jobs, 3, false, WhenDone::Nothing);
    assert_eq!(
        start_picks(&mut model),
        ids(&[1, 3, 5]),
        "job 2 waits for job 1's target"
    );
    model.transition(JobId(1), JobState::Finished, None).unwrap();
    model.count_end(&JobState::Finished);
    assert_eq!(start_picks(&mut model), ids(&[2]));
    assert_eq!(model.take_done(), None, "jobs still run");
}

#[test]
fn unreadable_rows_paused_queues_and_worker_counts() {
    let mut unreadable = job(2, "b", JobState::Waiting);
    unreadable.spec = SpecSlot::Unreadable {
        version: 9,
        error: "newer".into(),
    };
    let jobs = vec![
        job(1, "a", JobState::Waiting),
        unreadable,
        job(3, "c", JobState::Waiting),
    ];
    let mut model = Model::new(jobs, 1, true, WhenDone::Nothing);
    assert!(model.schedule().is_empty(), "paused");
    model.set_paused(false);
    assert_eq!(start_picks(&mut model), ids(&[1]), "one worker");
    model.set_workers(4);
    assert_eq!(
        start_picks(&mut model),
        ids(&[3]),
        "the unreadable row is skipped, never started"
    );
    model.set_pause_after_current(true);
    assert!(model.schedule().is_empty());
    assert!(!model.settle_pause(), "jobs still run");
    for id in [1, 3] {
        model
            .transition(JobId(id), JobState::Failed { reason: "x".into() }, None)
            .unwrap();
        model.count_end(&JobState::Failed { reason: "x".into() });
    }
    assert!(model.settle_pause() && model.is_paused());
    assert_eq!(model.take_done(), Some(Tally { finished: 0, failed: 2 }));
    assert_eq!(model.take_done(), None, "reported once");
}

#[test]
fn moves_keep_the_relative_order() {
    let order = ids(&[1, 2, 3, 4, 5]);
    assert_eq!(moved(&order, &ids(&[2, 4]), MoveTarget::Up), ids(&[2, 1, 4, 3, 5]));
    assert_eq!(
        moved(&order, &ids(&[1, 2]), MoveTarget::Up),
        order,
        "a block at the top stays"
    );
    assert_eq!(moved(&order, &ids(&[4, 5]), MoveTarget::Down), order);
    assert_eq!(moved(&order, &ids(&[1, 3]), MoveTarget::Down), ids(&[2, 1, 4, 3, 5]));
    assert_eq!(moved(&order, &ids(&[3, 5]), MoveTarget::Top), ids(&[3, 5, 1, 2, 4]));
    assert_eq!(moved(&order, &ids(&[1, 3]), MoveTarget::Bottom), ids(&[2, 4, 5, 1, 3]));
    assert_eq!(
        moved(&order, &ids(&[5]), MoveTarget::Before(JobId(2))),
        ids(&[1, 5, 2, 3, 4])
    );
    assert_eq!(
        moved(&order, &ids(&[2]), MoveTarget::Before(JobId(2))),
        ids(&[1, 3, 4, 5, 2])
    );
    assert_eq!(
        moved(&order, &ids(&[9]), MoveTarget::Top),
        order,
        "unknown ids change nothing"
    );
}

#[test]
fn removing_spares_running_jobs() {
    let jobs = vec![job(1, "a", JobState::Waiting), job(2, "b", JobState::Finished)];
    let mut model = Model::new(jobs, 1, false, WhenDone::Nothing);
    start_picks(&mut model);
    assert_eq!(model.remove(&ids(&[1, 2])), ids(&[2]));
    assert_eq!(model.order(), ids(&[1]));
}

/// One step of a random session.
#[derive(Debug, Clone)]
enum Op {
    Schedule,
    End(usize, bool),
    Hold(usize),
    Release(usize),
    Cancel(usize),
    Restart(usize),
    Pause(bool),
    PauseAfter(bool),
    Workers(u8),
    Move(Vec<usize>, u8),
    Remove(usize),
    Pass(usize),
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => Just(Op::Schedule),
        3 => (0usize..12, any::<bool>()).prop_map(|(i, ok)| Op::End(i, ok)),
        1 => (0usize..12).prop_map(Op::Hold),
        1 => (0usize..12).prop_map(Op::Release),
        1 => (0usize..12).prop_map(Op::Cancel),
        1 => (0usize..12).prop_map(Op::Restart),
        1 => any::<bool>().prop_map(Op::Pause),
        1 => any::<bool>().prop_map(Op::PauseAfter),
        1 => (0u8..6).prop_map(Op::Workers),
        1 => (prop::collection::vec(0usize..12, 0..4), 0u8..5).prop_map(|(s, t)| Op::Move(s, t)),
        1 => (0usize..12).prop_map(Op::Remove),
        1 => (0usize..12).prop_map(Op::Pass),
    ]
}

fn pick(model: &Model, i: usize) -> Option<JobId> {
    let order = model.order();
    order.get(i % order.len().max(1)).copied()
}

fn apply(model: &mut Model, op: &Op) {
    match op {
        Op::Schedule => {
            start_picks(model);
        }
        Op::End(i, ok) => {
            let running = model.running();
            if let Some(id) = running.get(i % running.len().max(1)) {
                let next = if *ok {
                    JobState::Finished
                } else {
                    JobState::Failed { reason: "x".into() }
                };
                if let Some(JobState::Paused { pass, passes }) = model.get(*id).map(|r| r.state.clone()) {
                    model
                        .transition(*id, JobState::Rendering { pass, passes }, None)
                        .unwrap();
                }
                model.transition(*id, next.clone(), None).unwrap();
                model.count_end(&next);
            }
        }
        Op::Hold(i) | Op::Release(i) | Op::Cancel(i) | Op::Restart(i) => {
            if let Some(id) = pick(model, *i) {
                let next = match op {
                    Op::Hold(_) => JobState::Held,
                    Op::Cancel(_) => JobState::Canceled,
                    _ => JobState::Waiting,
                };
                // Refusals are fine: the model must only never break its rules.
                let _ = model.transition(id, next, None);
            }
        }
        Op::Pause(on) => {
            model.set_paused(*on);
            for id in model.running() {
                let next = match model.get(id).map(|r| r.state.clone()) {
                    Some(JobState::Rendering { pass, passes }) if *on => JobState::Paused { pass, passes },
                    Some(JobState::Paused { pass, passes }) if !on => JobState::Rendering { pass, passes },
                    _ => continue,
                };
                model.transition(id, next, None).unwrap();
            }
        }
        Op::PauseAfter(on) => {
            model.set_pause_after_current(*on);
        }
        Op::Workers(n) => model.set_workers(*n),
        Op::Move(sel, to) => {
            let chosen: Vec<JobId> = sel.iter().filter_map(|i| pick(model, *i)).collect();
            let target = match to {
                0 => MoveTarget::Up,
                1 => MoveTarget::Down,
                2 => MoveTarget::Top,
                3 => MoveTarget::Bottom,
                _ => MoveTarget::Before(pick(model, 0).unwrap_or(JobId(0))),
            };
            model.reorder(&chosen, target);
        }
        Op::Remove(i) => {
            if let Some(id) = pick(model, *i) {
                model.remove(&[id]);
            }
        }
        Op::Pass(i) => {
            let running = model.running();
            if let Some(id) = running.get(i % running.len().max(1)) {
                let next = match model.get(*id).map(|r| r.state.clone()) {
                    Some(JobState::Rendering { pass, .. }) => JobState::Rendering {
                        pass: pass + 1,
                        passes: 3,
                    },
                    Some(JobState::Paused { pass, .. }) => JobState::Paused {
                        pass: pass + 1,
                        passes: 3,
                    },
                    _ => return,
                };
                model.transition(*id, next, None).unwrap();
            }
        }
    }
    model.settle_pause();
}

proptest! {
    #[test]
    fn random_sessions_keep_the_rules(
        targets in prop::collection::vec(0u8..4, 1..10),
        workers in 1u8..4,
        ops in prop::collection::vec(arb_op(), 0..60),
    ) {
        let jobs = targets
            .iter()
            .zip(1..)
            .map(|(t, id)| job(id, &format!("t{t}"), JobState::Waiting))
            .collect();
        let mut model = Model::new(jobs, workers, false, WhenDone::Nothing);
        let mut known: Vec<JobId> = model.order();
        for op in &ops {
            let before: Vec<(JobId, JobState)> = model.jobs().iter().map(|j| (j.id, j.state.clone())).collect();
            let running_before = model.running().len();
            apply(&mut model, op);
            // Every row's state change is one the state machine allows.
            for job in model.jobs() {
                if let Some((_, old)) = before.iter().find(|(id, _)| *id == job.id)
                    && *old != job.state
                {
                    // An ending render that was paused counts as rendering again first (the actor does the same).
                    let via_rendering = match old {
                        JobState::Paused { pass, passes } => {
                            JobState::Rendering { pass: *pass, passes: *passes }.can_become(&job.state)
                        }
                        _ => false,
                    };
                    prop_assert!(old.can_become(&job.state) || via_rendering, "{old:?} -> {:?}", job.state);
                }
            }
            let running: Vec<PathBuf> = model
                .running()
                .iter()
                .filter_map(|id| model.get(*id).map(|r| r.target_path.clone()))
                .collect();
            if running.len() > running_before {
                // Fewer workers than running jobs (lowered while they run) only means nothing new starts.
                prop_assert!(running.len() <= usize::from(model.processing().workers));
            }
            for (i, a) in running.iter().enumerate() {
                for b in running.iter().skip(i + 1) {
                    prop_assert!(!same_path(a, b), "two running jobs write {}", a.display());
                }
            }
            // Removing only ever drops ids; nothing appears from nowhere and nothing running goes.
            let order = model.order();
            prop_assert!(order.iter().all(|id| known.contains(id)));
            known.clone_from(&order);
            if model.is_paused() {
                prop_assert!(model.schedule().is_empty());
            }
            prop_assert!(model.jobs().iter().all(|j| j.state.kind() != JobStateKind::Held || !model.schedule().contains(&j.id)));
        }
    }

    #[test]
    fn moves_are_permutations(n in 1usize..12, sel in prop::collection::vec(0usize..12, 0..6), to in 0u8..5, anchor in 0usize..12) {
        let order: Vec<JobId> = (1..=n).map(|i| JobId(i64::try_from(i).unwrap())).collect();
        let chosen: Vec<JobId> = sel.iter().filter_map(|i| order.get(*i).copied()).collect();
        let target = match to {
            0 => MoveTarget::Up,
            1 => MoveTarget::Down,
            2 => MoveTarget::Top,
            3 => MoveTarget::Bottom,
            _ => MoveTarget::Before(order.get(anchor).copied().unwrap_or(JobId(99))),
        };
        let after = moved(&order, &chosen, target);
        let mut sorted = after.clone();
        sorted.sort();
        prop_assert_eq!(&sorted, &order);
        let rel = |list: &[JobId]| list.iter().copied().filter(|id| chosen.contains(id)).collect::<Vec<_>>();
        prop_assert_eq!(rel(&after), rel(&order), "the selection keeps its relative order");
        let others = |list: &[JobId]| list.iter().copied().filter(|id| !chosen.contains(id)).collect::<Vec<_>>();
        prop_assert_eq!(others(&after), others(&order), "the other jobs keep theirs");
    }
}
