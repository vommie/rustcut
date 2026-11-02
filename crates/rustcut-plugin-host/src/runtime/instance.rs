//! One running plugin: its store with the limits, the captured output and the three calls.

use std::time::Duration;

use wasmtime::{ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder, Trap};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};

use super::bindings::{self, rustcut::plugin::host};
use super::{Loaded, TICK, convert};
use crate::types::{Action, Context, Event, Issue, Level, LogLine};

/// The most a plugin may write on stdout and stderr together before its instance is made again.
const OUTPUT_CAPACITY: usize = 1 << 20;

/// The most log lines one call keeps.
const MAX_LOG_LINES: usize = 200;

/// The longest log line kept, in characters.
const MAX_LOG_LINE: usize = 2000;

/// A call's time limit and an instance's memory limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub time: Duration,
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(1),
            memory_bytes: 64 << 20,
        }
    }
}

/// Why a call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("needed more than {0} MB of memory")]
    OutOfMemory(usize),
    #[error("crashed: {0}")]
    Trap(String),
    #[error("{0}")]
    Failed(String),
    #[error("could not start: {0}")]
    Start(String),
}

impl CallError {
    /// Whether the instance must be made again before the next call (it stopped in the middle of something).
    #[must_use]
    pub fn poisons(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }
}

/// What a plugin's store holds.
pub(super) struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limiter: Limiter,
    log: Vec<LogLine>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl bindings::rustcut::plugin::types::Host for State {}

impl host::Host for State {
    fn log(&mut self, level: host::Level, message: String) {
        push_line(&mut self.log, convert::level(level), &message);
    }
}

fn push_line(log: &mut Vec<LogLine>, level: Level, text: &str) {
    if log.len() < MAX_LOG_LINES {
        log.push(LogLine {
            level,
            text: text.chars().take(MAX_LOG_LINE).collect(),
        });
    }
}

/// The store limits plus a note that memory was refused.
struct Limiter {
    inner: StoreLimits,
    memory_refused: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(&mut self, current: usize, desired: usize, maximum: Option<usize>) -> wasmtime::Result<bool> {
        let allowed = self.inner.memory_growing(current, desired, maximum)?;
        self.memory_refused |= !allowed;
        Ok(allowed)
    }

    fn table_growing(&mut self, current: usize, desired: usize, maximum: Option<usize>) -> wasmtime::Result<bool> {
        self.inner.table_growing(current, desired, maximum)
    }
}

/// A running plugin. Not thread-safe by itself: a [`super::Worker`] owns one on its own thread.
pub struct Instance {
    store: Store<State>,
    plugin: bindings::Plugin,
    output: MemoryOutputPipe,
    output_seen: usize,
    limits: Limits,
}

impl Instance {
    /// Makes an instance of a loaded plugin (about 0.2 ms).
    ///
    /// # Errors
    /// [`CallError::Start`] when the component cannot be instantiated within the limits.
    pub fn new(loaded: &Loaded, limits: Limits) -> Result<Self, CallError> {
        let output = MemoryOutputPipe::new(OUTPUT_CAPACITY);
        // No preopened folders, no environment, no arguments, no network: only clocks and random numbers.
        let wasi = WasiCtx::builder().stdout(output.clone()).stderr(output.clone()).build();
        let limiter = Limiter {
            inner: StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .instances(64)
                .tables(64)
                .build(),
            memory_refused: false,
        };
        let state = State {
            wasi,
            table: ResourceTable::new(),
            limiter,
            log: Vec::new(),
        };
        let mut store = Store::new(&loaded.inner.engine, state);
        store.limiter(|state| &mut state.limiter);
        store.set_epoch_deadline(deadline_ticks(limits.time));
        let plugin = loaded
            .inner
            .pre
            .instantiate(&mut store)
            .map_err(|err| CallError::Start(format!("{err:#}")))?;
        Ok(Self {
            store,
            plugin,
            output,
            output_seen: 0,
            limits,
        })
    }

    #[must_use]
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Hands the plugin its settings (a JSON object).
    ///
    /// # Errors
    /// The plugin refused the settings, crashed or ran out of time or memory.
    pub fn configure(&mut self, settings: &serde_json::Value) -> Result<(), CallError> {
        let text = settings.to_string();
        self.call(|plugin, store| plugin.call_configure(store, &text))?
            .map_err(CallError::Failed)
    }

    /// Hands the plugin an event and returns its actions.
    ///
    /// # Errors
    /// See [`Instance::configure`].
    pub fn handle(&mut self, event: &Event, context: &Context) -> Result<Vec<Action>, CallError> {
        let event = convert::event_to_wit(event);
        let context = convert::context_to_wit(context);
        let actions = self
            .call(|plugin, store| plugin.call_handle(store, &event, &context))?
            .map_err(CallError::Failed)?;
        Ok(actions.into_iter().map(convert::action_from_wit).collect())
    }

    /// Lets the plugin check settings the user is editing.
    ///
    /// # Errors
    /// See [`Instance::configure`].
    pub fn check(&mut self, settings: &serde_json::Value) -> Result<Vec<Issue>, CallError> {
        let text = settings.to_string();
        let issues = self.call(|plugin, store| plugin.call_check(store, &text))?;
        Ok(issues.into_iter().map(convert::issue_from_wit).collect())
    }

    /// The lines the plugin wrote since the last call of this method.
    pub fn take_log(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut self.store.data_mut().log)
    }

    /// Whether the plugin wrote so much output that the instance should be made again.
    #[must_use]
    pub fn output_full(&self) -> bool {
        self.output_seen > OUTPUT_CAPACITY / 2
    }

    fn call<T>(
        &mut self,
        run: impl FnOnce(&bindings::Plugin, &mut Store<State>) -> wasmtime::Result<T>,
    ) -> Result<T, CallError> {
        self.store.data_mut().limiter.memory_refused = false;
        self.store.set_epoch_deadline(deadline_ticks(self.limits.time));
        let result = run(&self.plugin, &mut self.store);
        self.collect_output();
        result.map_err(|err| {
            if err.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
                CallError::Timeout(self.limits.time)
            } else if self.store.data().limiter.memory_refused {
                CallError::OutOfMemory(self.limits.memory_bytes >> 20)
            } else {
                CallError::Trap(trap_text(&err))
            }
        })
    }

    fn collect_output(&mut self) {
        let bytes = self.output.contents();
        let Some(new) = bytes.get(self.output_seen..) else {
            return;
        };
        self.output_seen = bytes.len();
        let text = String::from_utf8_lossy(new);
        let log = &mut self.store.data_mut().log;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            push_line(log, Level::Warning, line);
        }
    }
}

/// The epoch ticks a call may use: the time limit rounded up, plus one for the tick already running.
fn deadline_ticks(time: Duration) -> u64 {
    let ticks = time.as_millis().div_ceil(TICK.as_millis().max(1));
    u64::try_from(ticks).unwrap_or(u64::MAX / 2).saturating_add(1)
}

/// The trap's own words, without wasmtime's backtrace.
fn trap_text(err: &wasmtime::Error) -> String {
    match err.downcast_ref::<Trap>() {
        Some(trap) => trap.to_string(),
        None => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_round_up_and_add_the_running_tick() {
        assert_eq!(deadline_ticks(Duration::from_millis(1000)), 101);
        assert_eq!(deadline_ticks(Duration::from_millis(105)), 12);
        assert_eq!(deadline_ticks(Duration::ZERO), 1);
    }

    #[test]
    fn only_a_refusal_keeps_the_instance() {
        assert!(!CallError::Failed("no".into()).poisons());
        assert!(CallError::Timeout(Duration::from_secs(1)).poisons());
        assert!(CallError::Trap("x".into()).poisons());
    }
}
