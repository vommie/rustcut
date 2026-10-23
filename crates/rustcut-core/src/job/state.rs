//! The job state machine (HANDOFF section 5): Waiting -> (Held) -> Rendering <-> Paused -> Finished | Failed | Canceled;
//! a finished, failed or canceled job goes back to Waiting only as a new attempt (retry/requeue).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Where a job stands. Stored as its [`JobStateKind`] (indexed column) plus this value as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Waiting,
    /// Postponed by the user; skipped by the scheduler until released (PyCutRv "Pending").
    Held,
    /// `pass` of `passes` is running (deshake analysis and two-pass encoding add passes); pass 0 = starting (the source
    /// is probed again and the passes are planned, so their number is not known yet).
    Rendering {
        pass: u8,
        passes: u8,
    },
    Paused {
        pass: u8,
        passes: u8,
    },
    /// Rendered, verified and moved onto the target.
    Finished,
    Failed {
        reason: String,
    },
    Canceled,
}

/// The state without its details: the indexed column of the store and the queue filter chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStateKind {
    Waiting,
    Held,
    Rendering,
    Paused,
    Finished,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a job state")]
pub struct ParseStateError(pub String);

impl JobStateKind {
    pub const ALL: [Self; 7] = [
        Self::Waiting,
        Self::Held,
        Self::Rendering,
        Self::Paused,
        Self::Finished,
        Self::Failed,
        Self::Canceled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Held => "held",
            Self::Rendering => "rendering",
            Self::Paused => "paused",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    /// The process of a job in this state may be running (it must be stopped or recovered).
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Rendering | Self::Paused)
    }

    /// Nothing happens to the job any more unless the user starts a new attempt.
    pub const fn is_done(self) -> bool {
        matches!(self, Self::Finished | Self::Failed | Self::Canceled)
    }
}

impl fmt::Display for JobStateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for JobStateKind {
    type Err = ParseStateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| ParseStateError(s.to_owned()))
    }
}

impl JobState {
    pub const fn kind(&self) -> JobStateKind {
        match self {
            Self::Waiting => JobStateKind::Waiting,
            Self::Held => JobStateKind::Held,
            Self::Rendering { .. } => JobStateKind::Rendering,
            Self::Paused { .. } => JobStateKind::Paused,
            Self::Finished => JobStateKind::Finished,
            Self::Failed { .. } => JobStateKind::Failed,
            Self::Canceled => JobStateKind::Canceled,
        }
    }

    /// Whether the queue engine may move a job from `self` to `next`. Going back to Waiting from a done state starts a new
    /// attempt; from Rendering/Paused it is the crash/quit recovery (ADR-9).
    pub fn can_become(&self, next: &Self) -> bool {
        use JobStateKind as K;
        match (self.kind(), next.kind()) {
            (K::Waiting, K::Held | K::Rendering | K::Canceled)
            | (K::Held, K::Waiting | K::Canceled)
            | (K::Rendering, K::Paused | K::Finished | K::Failed | K::Canceled | K::Waiting)
            | (K::Paused, K::Rendering | K::Canceled | K::Waiting)
            | (K::Finished | K::Failed | K::Canceled, K::Waiting) => true,
            // The next pass of the same render (pass 0 = starting: the source is probed and the passes planned); a
            // paused render learns its next pass when it was paused between two passes.
            (K::Rendering, K::Rendering) | (K::Paused, K::Paused) => match (self, next) {
                (Self::Rendering { pass: a, .. }, Self::Rendering { pass: b, .. })
                | (Self::Paused { pass: a, .. }, Self::Paused { pass: b, .. }) => b > a,
                _ => false,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn arb_state() -> impl Strategy<Value = JobState> {
        prop_oneof![
            Just(JobState::Waiting),
            Just(JobState::Held),
            (1u8..4, 1u8..4).prop_map(|(pass, passes)| JobState::Rendering { pass, passes }),
            (1u8..4, 1u8..4).prop_map(|(pass, passes)| JobState::Paused { pass, passes }),
            Just(JobState::Finished),
            "[a-z ]{0,10}".prop_map(|reason| JobState::Failed { reason }),
            Just(JobState::Canceled),
        ]
    }

    #[test]
    fn typical_lifecycle() {
        let path = [
            JobState::Waiting,
            JobState::Rendering { pass: 1, passes: 2 },
            JobState::Paused { pass: 1, passes: 2 },
            JobState::Rendering { pass: 1, passes: 2 },
            JobState::Rendering { pass: 2, passes: 2 },
            JobState::Finished,
            JobState::Waiting,
        ];
        for pair in path.windows(2) {
            assert!(pair[0].can_become(&pair[1]), "{:?} -> {:?}", pair[0], pair[1]);
        }
        assert!(!JobState::Held.can_become(&JobState::Rendering { pass: 1, passes: 1 }));
        assert!(!JobState::Finished.can_become(&JobState::Failed { reason: String::new() }));
        assert!(!JobState::Rendering { pass: 2, passes: 2 }.can_become(&JobState::Rendering { pass: 1, passes: 2 }));
    }

    #[test]
    fn kinds_parse_and_serialize() {
        for kind in JobStateKind::ALL {
            assert_eq!(kind.as_str().parse::<JobStateKind>(), Ok(kind));
        }
        assert!("pending".parse::<JobStateKind>().is_err());
        let json = serde_json::to_string(&JobState::Failed {
            reason: "ffmpeg exited with 1".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"failed","reason":"ffmpeg exited with 1"}"#);
    }

    proptest! {
        #[test]
        fn done_states_only_restart(from in arb_state(), to in arb_state()) {
            if from.kind().is_done() && from.can_become(&to) {
                prop_assert_eq!(to.kind(), JobStateKind::Waiting);
            }
        }

        #[test]
        fn nothing_skips_waiting_into_finished(from in arb_state()) {
            if from.can_become(&JobState::Finished) {
                prop_assert_eq!(from.kind(), JobStateKind::Rendering);
            }
        }
    }
}
