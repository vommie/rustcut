//! The app's own log in memory for the Logs panel (M4.3): a `tracing` layer keeps the newest lines that pass the log
//! filter (the same lines as the log file), with their time and level. The panel reads it on the UI thread when the
//! sequence number moved; the layer never calls into the UI (a log line from there would come back here).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use parking_lot::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// Lines kept; older ones go first.
pub const CAPACITY: usize = 5_000;

/// One line of the app's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookLine {
    /// Counts every line since the start (the first is 1).
    pub seq: u64,
    /// Local time, `14:05:09.123`.
    pub time: String,
    pub level: Level,
    /// The message followed by its fields (`[Queue] Started jobs=0 paused=false`).
    pub text: String,
}

#[derive(Debug, Default)]
struct Book {
    lines: VecDeque<BookLine>,
    seq: u64,
}

/// The shared log; cloning shares it.
#[derive(Debug, Clone, Default)]
pub struct LogBook(Arc<Mutex<Book>>);

impl LogBook {
    /// The number of the newest line (0 = none yet): a reader shows the log again when it moved.
    pub fn latest(&self) -> u64 {
        self.0.lock().seq
    }

    /// Every kept line, oldest first.
    pub fn lines(&self) -> Vec<BookLine> {
        self.0.lock().lines.iter().cloned().collect()
    }

    fn push(&self, level: Level, text: String) {
        let time = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        let mut book = self.0.lock();
        book.seq = book.seq.saturating_add(1);
        let seq = book.seq;
        if book.lines.len() >= CAPACITY {
            book.lines.pop_front();
        }
        book.lines.push_back(BookLine { seq, time, level, text });
    }

    /// The layer that writes into this log.
    pub fn layer(&self) -> BookLayer {
        BookLayer(self.clone())
    }
}

/// Writes every event into the [`LogBook`].
#[derive(Debug)]
pub struct BookLayer(LogBook);

impl<S: Subscriber> Layer<S> for BookLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut text = Text::default();
        event.record(&mut text);
        self.0.push(*event.metadata().level(), text.0);
    }
}

/// The message first, then `name=value` for every other field.
#[derive(Default)]
struct Text(String);

impl Visit for Text {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.insert_str(0, value);
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.insert_str(0, &format!("{value:?}"));
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    #[test]
    fn events_are_kept_with_message_fields_and_level() {
        let book = LogBook::default();
        let subscriber = tracing_subscriber::registry().with(book.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(jobs = 2, paused = false, "[Queue] Started");
            tracing::warn!(error = %"no answer", "[RustView] Not reachable");
        });
        let lines = book.lines();
        assert_eq!(book.latest(), 2);
        assert_eq!(lines.len(), 2);
        let first = lines.first().unwrap();
        assert_eq!((first.seq, first.level), (1, Level::INFO));
        assert_eq!(first.text, "[Queue] Started jobs=2 paused=false");
        assert_eq!(lines.get(1).unwrap().text, "[RustView] Not reachable error=no answer");
        assert_eq!(first.time.len(), "14:05:09.123".len());
    }

    #[test]
    fn only_the_newest_lines_are_kept() {
        let book = LogBook::default();
        for i in 0..CAPACITY + 10 {
            book.push(Level::DEBUG, format!("line {i}"));
        }
        let lines = book.lines();
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines.first().unwrap().text, "line 10");
        assert_eq!(book.latest(), u64::try_from(CAPACITY + 10).unwrap());
    }
}
