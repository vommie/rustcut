//! The actor kit of the services (ADR-4): each service owns its state and takes commands over a channel; replies come back
//! through a oneshot, changes go out on a broadcast bus. No lock is held across `.await` or a UI call.
//!
//! Commands use unbounded channels: they come from user actions and services in small numbers, and the UI thread must never
//! wait for a full channel. Events use a bounded broadcast; a subscriber that falls behind gets `Lagged` and resyncs from a
//! snapshot command.

use tokio::sync::{broadcast, mpsc, oneshot};

/// The actor behind a handle has stopped (shutdown or a bug); the command was not delivered or not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the service has stopped")]
pub struct ActorGone;

/// The reply slot of a request; the actor answers with [`Reply::send`] (ignoring a requester that went away).
pub type Reply<T> = oneshot::Sender<T>;

/// The sending side of an actor's mailbox. Cheap to clone; usable from any thread, including the UI thread.
#[derive(Debug)]
pub struct Handle<C> {
    tx: mpsc::UnboundedSender<C>,
}

impl<C> Clone for Handle<C> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

/// The receiving side, owned by the actor's task or thread.
pub type Mailbox<C> = mpsc::UnboundedReceiver<C>;

/// A new mailbox and its handle.
pub fn mailbox<C>() -> (Handle<C>, Mailbox<C>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Handle { tx }, rx)
}

impl<C> Handle<C> {
    /// Sends a command without waiting (never blocks, also on the UI thread).
    pub fn send(&self, command: C) -> Result<(), ActorGone> {
        self.tx.send(command).map_err(|_closed| ActorGone)
    }

    /// Sends a command built around a reply slot and waits for the answer.
    pub async fn request<T>(&self, make: impl FnOnce(Reply<T>) -> C) -> Result<T, ActorGone> {
        let (reply, answer) = oneshot::channel();
        self.send(make(reply))?;
        answer.await.map_err(|_dropped| ActorGone)
    }

    /// [`Handle::request`] for plain threads that are not on the runtime (the mpv event thread, the store thread, tests).
    /// Must not be called on the UI thread or inside the runtime.
    pub fn blocking_request<T>(&self, make: impl FnOnce(Reply<T>) -> C) -> Result<T, ActorGone> {
        let (reply, answer) = oneshot::channel();
        self.send(make(reply))?;
        answer.blocking_recv().map_err(|_dropped| ActorGone)
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// Broadcast bus of one service's events.
#[derive(Debug)]
pub struct EventBus<E> {
    tx: broadcast::Sender<E>,
}

impl<E> Clone for EventBus<E> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

impl<E: Clone> EventBus<E> {
    /// Default capacity: enough for a burst of row updates between two UI flushes.
    pub const CAPACITY: usize = 1024;

    pub fn new() -> Self {
        Self::with_capacity(Self::CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity.max(1));
        Self { tx }
    }

    /// Publishes an event; having no subscriber is fine.
    pub fn publish(&self, event: E) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<E> {
        self.tx.subscribe()
    }
}

impl<E: Clone> Default for EventBus<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Cmd {
        Add(u32, Reply<u32>),
        Stop,
    }

    async fn adder(mut rx: Mailbox<Cmd>, bus: EventBus<u32>) {
        let mut total = 0;
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Cmd::Add(n, reply) => {
                    total += n;
                    bus.publish(total);
                    let _ = reply.send(total);
                }
                Cmd::Stop => break,
            }
        }
    }

    #[tokio::test]
    async fn request_reply_and_events() {
        let (handle, rx) = mailbox();
        let bus = EventBus::new();
        let mut events = bus.subscribe();
        let task = tokio::spawn(adder(rx, bus.clone()));
        assert_eq!(handle.request(|r| Cmd::Add(2, r)).await, Ok(2));
        assert_eq!(handle.request(|r| Cmd::Add(3, r)).await, Ok(5));
        assert_eq!(events.recv().await.unwrap(), 2);
        assert_eq!(events.recv().await.unwrap(), 5);
        handle.send(Cmd::Stop).unwrap();
        task.await.unwrap();
        assert_eq!(handle.request(|r| Cmd::Add(1, r)).await, Err(ActorGone));
        assert!(handle.is_closed());
    }

    #[test]
    fn blocking_request_from_a_plain_thread() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (handle, rx) = mailbox();
        let bus = EventBus::with_capacity(4);
        let worker = std::thread::spawn(move || rt.block_on(adder(rx, bus)));
        assert_eq!(handle.blocking_request(|r| Cmd::Add(7, r)), Ok(7));
        handle.send(Cmd::Stop).unwrap();
        worker.join().unwrap();
    }
}
