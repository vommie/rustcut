//! One thread per plugin: it owns the plugin's instance, runs its calls one after the other and makes the instance
//! again after a crash, a timeout or too much output. A slow plugin therefore never holds up another one, and the
//! caller never waits: every answer arrives through a [`Reply`].

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CallError, Instance, Limits, Loaded};
use crate::types::{Action, Context, Event, Issue, LogLine};

/// The answer to one call: the result, what the plugin wrote meanwhile and how long it took.
#[derive(Debug, Clone, PartialEq)]
pub struct CallReport<T> {
    pub result: Result<T, CallError>,
    pub log: Vec<LogLine>,
    pub took: Duration,
}

/// Receives a [`CallReport`] on the worker's thread.
pub type Reply<T> = Box<dyn FnOnce(CallReport<T>) + Send>;

enum Job {
    Configure {
        settings: Value,
        reply: Reply<()>,
    },
    Handle {
        event: Event,
        context: Context,
        reply: Reply<Vec<Action>>,
    },
    Check {
        settings: Value,
        reply: Reply<Vec<Issue>>,
    },
    Limits(Limits),
}

/// A plugin's thread; cheap to clone. Dropping the last clone lets the thread end after the call it is running.
#[derive(Clone)]
pub struct Worker {
    jobs: mpsc::Sender<Job>,
}

impl Worker {
    /// Starts the thread; the instance is made on the first call.
    ///
    /// # Errors
    /// The thread cannot be started.
    pub fn spawn(loaded: Loaded, limits: Limits) -> std::io::Result<Self> {
        let (jobs, inbox) = mpsc::channel();
        let name = format!("rustcut-plugin-{}", loaded.manifest().id);
        std::thread::Builder::new()
            .name(name)
            .spawn(move || run(&loaded, limits, &inbox))?;
        Ok(Self { jobs })
    }

    pub fn configure(&self, settings: Value, reply: Reply<()>) {
        self.send(Job::Configure { settings, reply });
    }

    pub fn handle(&self, event: Event, context: Context, reply: Reply<Vec<Action>>) {
        self.send(Job::Handle { event, context, reply });
    }

    pub fn check(&self, settings: Value, reply: Reply<Vec<Issue>>) {
        self.send(Job::Check { settings, reply });
    }

    /// New limits; they apply from the next instance (made at once).
    pub fn set_limits(&self, limits: Limits) {
        self.send(Job::Limits(limits));
    }

    fn send(&self, job: Job) {
        if self.jobs.send(job).is_err() {
            tracing::error!("[Plugins] a plugin thread ended unexpectedly; the call is dropped");
        }
    }
}

struct Slot<'a> {
    loaded: &'a Loaded,
    limits: Limits,
    instance: Option<Instance>,
    /// The last settings handed over, for a fresh instance.
    settings: Option<Value>,
}

fn run(loaded: &Loaded, limits: Limits, inbox: &mpsc::Receiver<Job>) {
    let mut slot = Slot {
        loaded,
        limits,
        instance: None,
        settings: None,
    };
    for job in inbox {
        match job {
            Job::Configure { settings, reply } => {
                slot.settings = Some(settings.clone());
                reply(slot.call(false, |instance| instance.configure(&settings)));
            }
            Job::Handle { event, context, reply } => {
                reply(slot.call(true, |instance| instance.handle(&event, &context)));
            }
            Job::Check { settings, reply } => reply(slot.call(true, |instance| instance.check(&settings))),
            Job::Limits(limits) => {
                slot.limits = limits;
                slot.instance = None;
            }
        }
    }
}

impl Slot<'_> {
    fn call<T>(&mut self, configured: bool, run: impl FnOnce(&mut Instance) -> Result<T, CallError>) -> CallReport<T> {
        let start = Instant::now();
        let mut log = Vec::new();
        let result = self.ready(configured, &mut log).and_then(|instance| {
            let result = run(instance);
            log.extend(instance.take_log());
            result
        });
        if result.as_ref().is_err_and(CallError::poisons) || self.instance.as_ref().is_some_and(Instance::output_full) {
            self.instance = None;
        }
        CallReport {
            result,
            log,
            took: start.elapsed(),
        }
    }

    /// The instance, made (and configured, when `configured`) if there is none.
    fn ready(&mut self, configured: bool, log: &mut Vec<LogLine>) -> Result<&mut Instance, CallError> {
        if self.instance.is_none() {
            let mut instance = Instance::new(self.loaded, self.limits)?;
            if configured && let Some(settings) = &self.settings {
                let result = instance.configure(settings);
                log.extend(instance.take_log());
                if let Err(err) = result {
                    return Err(match err {
                        CallError::Failed(text) => CallError::Failed(format!("the settings were refused: {text}")),
                        other => other,
                    });
                }
            }
            self.instance = Some(instance);
        }
        self.instance
            .as_mut()
            .ok_or_else(|| CallError::Start("no instance".to_owned()))
    }
}
