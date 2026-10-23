//! Jobs: what to render (the versioned [`JobSpec`], stored as JSON) and where a job stands ([`JobState`]).

mod spec;
mod state;

use serde::{Deserialize, Serialize};

pub use spec::{JOB_SPEC_VERSION, JobMetadata, JobSpec, SourceSummary, TargetNaming, TargetSpec};
pub use state::{JobState, JobStateKind, ParseStateError};

#[cfg(test)]
pub(crate) use spec::tests::sample as sample_spec;

/// Id of a job, assigned by the store (`AUTOINCREMENT`): never reused, also after deletions (PyCutRv bug 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub i64);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}
