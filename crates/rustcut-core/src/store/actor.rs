//! The store thread: the only owner of the SQLite connection (ADR-4). Commands come through a [`StoreHandle`]; every
//! command answers with the result of the [`Store`] method of the same name.

use std::thread::JoinHandle;

use super::{JobRecord, LogLine, LogStream, Store, StoreError};
use crate::actor::{Handle, Reply, mailbox};
use crate::job::{JobId, JobSpec, JobState};

type Answer<T> = Reply<Result<T, StoreError>>;

/// Commands of the store thread (frozen interface; new commands are added, existing ones keep their shape).
#[derive(Debug)]
pub enum StoreCommand {
    InsertJob {
        spec: Box<JobSpec>,
        state: JobState,
        reply: Answer<JobRecord>,
    },
    Job {
        id: JobId,
        reply: Answer<Option<JobRecord>>,
    },
    Jobs {
        reply: Answer<Vec<JobRecord>>,
    },
    UpdateSpec {
        id: JobId,
        spec: Box<JobSpec>,
        reply: Answer<()>,
    },
    SetState {
        id: JobId,
        state: JobState,
        note: Option<String>,
        reply: Answer<()>,
    },
    SetOrder {
        ids: Vec<JobId>,
        reply: Answer<()>,
    },
    DeleteJobs {
        ids: Vec<JobId>,
        reply: Answer<usize>,
    },
    SetSync {
        id: JobId,
        sync: Option<String>,
        reply: Answer<()>,
    },
    RecoverInterrupted {
        note: String,
        reply: Answer<Vec<JobId>>,
    },
    /// Fire and forget: log lines are written in order, a failure is logged by the store thread.
    AppendLog {
        id: JobId,
        stream: LogStream,
        lines: Vec<String>,
    },
    Log {
        id: JobId,
        limit: usize,
        reply: Answer<Vec<LogLine>>,
    },
    /// Ends the thread after the commands before it; the reply comes when the connection is closed.
    Shutdown {
        reply: Reply<()>,
    },
}

pub type StoreHandle = Handle<StoreCommand>;

/// Starts the store thread with an open store.
pub fn spawn_store(mut store: Store) -> std::io::Result<(StoreHandle, JoinHandle<()>)> {
    let (handle, mut rx) = mailbox::<StoreCommand>();
    let thread = std::thread::Builder::new()
        .name("rustcut-store".into())
        .spawn(move || {
            tracing::debug!("[Store] Thread started");
            while let Some(command) = rx.blocking_recv() {
                match command {
                    StoreCommand::InsertJob { spec, state, reply } => {
                        let _ = reply.send(store.insert_job(&spec, &state));
                    }
                    StoreCommand::Job { id, reply } => {
                        let _ = reply.send(store.job(id));
                    }
                    StoreCommand::Jobs { reply } => {
                        let _ = reply.send(store.jobs());
                    }
                    StoreCommand::UpdateSpec { id, spec, reply } => {
                        let _ = reply.send(store.update_spec(id, &spec));
                    }
                    StoreCommand::SetState { id, state, note, reply } => {
                        let _ = reply.send(store.set_state(id, &state, note.as_deref()));
                    }
                    StoreCommand::SetOrder { ids, reply } => {
                        let _ = reply.send(store.set_order(&ids));
                    }
                    StoreCommand::DeleteJobs { ids, reply } => {
                        let _ = reply.send(store.delete_jobs(&ids));
                    }
                    StoreCommand::SetSync { id, sync, reply } => {
                        let _ = reply.send(store.set_sync(id, sync.as_deref()));
                    }
                    StoreCommand::RecoverInterrupted { note, reply } => {
                        let _ = reply.send(store.recover_interrupted(&note));
                    }
                    StoreCommand::AppendLog { id, stream, lines } => {
                        if let Err(e) = store.append_log(id, stream, &lines) {
                            tracing::warn!(job = id.0, error = %e, "[Store] Log lines could not be written");
                        }
                    }
                    StoreCommand::Log { id, limit, reply } => {
                        let _ = reply.send(store.log(id, limit));
                    }
                    StoreCommand::Shutdown { reply } => {
                        drop(store);
                        let _ = reply.send(());
                        tracing::debug!("[Store] Thread stopped");
                        return;
                    }
                }
            }
            tracing::debug!("[Store] Thread stopped (all handles dropped)");
        })?;
    Ok((handle, thread))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::sample_spec;

    #[tokio::test]
    async fn commands_reach_the_store_in_order() {
        let (store, thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
        let spec = Box::new(sample_spec());
        let a = store
            .request(|reply| StoreCommand::InsertJob {
                spec: spec.clone(),
                state: JobState::Waiting,
                reply,
            })
            .await
            .unwrap()
            .unwrap();
        store
            .send(StoreCommand::AppendLog {
                id: a.id,
                stream: LogStream::App,
                lines: vec!["queued".into()],
            })
            .unwrap();
        let log = store
            .request(|reply| StoreCommand::Log {
                id: a.id,
                limit: 10,
                reply,
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            log.len(),
            1,
            "fire-and-forget lines are written before later requests are answered"
        );
        let jobs = store
            .request(|reply| StoreCommand::Jobs { reply })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(jobs.len(), 1);
        store.request(|reply| StoreCommand::Shutdown { reply }).await.unwrap();
        thread.join().unwrap();
        assert!(store.request(|reply| StoreCommand::Jobs { reply }).await.is_err());
    }
}
