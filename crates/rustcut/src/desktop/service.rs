//! The desktop actor (P5, ADR-15) on the runtime: holds logind's sleep lock while a job renders (`desktop.inhibit_sleep`),
//! shows the queue's progress on the taskbar entry (`desktop.taskbar_progress`), and suspends or powers off when asked.
//! The D-Bus connections are made when first needed; a bus that is missing is logged once and the rest goes on.

use rustcut_core::actor::{Handle, Mailbox, Reply, mailbox};
use zbus::{Connection, OwnedFd};

use super::logind::{self, PowerAction};
use super::taskbar::{self, Badge};

pub enum DesktopCommand {
    /// A job renders now (`true`) or none does.
    Rendering(bool),
    /// What the taskbar entry shows.
    Badge(Badge),
    /// The settings changed.
    Settings { inhibit: bool, taskbar: bool },
    /// Suspend or power off; the reply is logind's refusal, if any.
    Power {
        action: PowerAction,
        reply: Reply<Result<(), String>>,
    },
}

pub type DesktopHandle = Handle<DesktopCommand>;

struct Desktop {
    system: Option<Connection>,
    session: Option<Connection>,
    /// A bus could not be reached (logged once).
    system_failed: bool,
    session_failed: bool,
    lock: Option<OwnedFd>,
    rendering: bool,
    inhibit: bool,
    taskbar: bool,
    badge: Badge,
    shown: Option<Badge>,
}

/// Starts the actor; it ends (dropping the lock and clearing the taskbar entry) when every handle is gone.
pub fn spawn_desktop(runtime: &tokio::runtime::Handle, inhibit: bool, taskbar: bool) -> DesktopHandle {
    let (handle, mailbox) = mailbox();
    let desktop = Desktop {
        system: None,
        session: None,
        system_failed: false,
        session_failed: false,
        lock: None,
        rendering: false,
        inhibit,
        taskbar,
        badge: Badge::NONE,
        shown: None,
    };
    runtime.spawn(desktop.run(mailbox));
    handle
}

impl Desktop {
    async fn run(mut self, mut mailbox: Mailbox<DesktopCommand>) {
        while let Some(command) = mailbox.recv().await {
            match command {
                DesktopCommand::Rendering(on) => self.rendering = on,
                DesktopCommand::Badge(badge) => self.badge = badge,
                DesktopCommand::Settings { inhibit, taskbar } => (self.inhibit, self.taskbar) = (inhibit, taskbar),
                DesktopCommand::Power { action, reply } => {
                    let _ = reply.send(self.power(action).await);
                    continue;
                }
            }
            self.reconcile().await;
        }
        self.rendering = false;
        self.badge = Badge::NONE;
        self.reconcile().await;
        tracing::debug!("[Desktop] Stopped");
    }

    /// Makes the sleep lock and the taskbar entry match what is wanted now.
    async fn reconcile(&mut self) {
        let want_lock = self.rendering && self.inhibit;
        if want_lock && self.lock.is_none() {
            if let Some(system) = self.system().await {
                match logind::inhibit(&system, "Rendering video").await {
                    Ok(fd) => {
                        self.lock = Some(fd);
                        tracing::info!("[Desktop] Sleep is blocked while a job renders");
                    }
                    Err(e) => tracing::warn!(error = %e, "[Desktop] Sleep could not be blocked"),
                }
            }
        } else if !want_lock && self.lock.take().is_some() {
            tracing::info!("[Desktop] Sleep is allowed again");
        }
        let wanted = if self.taskbar { self.badge } else { Badge::NONE };
        if self.shown != Some(wanted)
            && (self.shown.is_some() || wanted != Badge::NONE)
            && let Some(session) = self.session().await
        {
            match taskbar::show(&session, wanted).await {
                Ok(()) => self.shown = Some(wanted),
                Err(e) => tracing::debug!(error = %e, "[Desktop] The taskbar entry could not be updated"),
            }
        }
    }

    async fn power(&mut self, action: PowerAction) -> Result<(), String> {
        let Some(system) = self.system().await else {
            return Err("the system bus cannot be reached".into());
        };
        tracing::info!(?action, "[Desktop] Asking logind");
        logind::act(&system, action).await.map_err(|e| {
            tracing::warn!(?action, error = %e, "[Desktop] logind refused");
            e.to_string()
        })
    }

    async fn system(&mut self) -> Option<Connection> {
        if self.system.is_none() && !self.system_failed {
            match Connection::system().await {
                Ok(c) => self.system = Some(c),
                Err(e) => {
                    self.system_failed = true;
                    tracing::warn!(error = %e, "[Desktop] The system bus cannot be reached: no sleep lock, suspend or power off");
                }
            }
        }
        self.system.clone()
    }

    async fn session(&mut self) -> Option<Connection> {
        if self.session.is_none() && !self.session_failed {
            match Connection::session().await {
                Ok(c) => self.session = Some(c),
                Err(e) => {
                    self.session_failed = true;
                    tracing::warn!(error = %e, "[Desktop] The session bus cannot be reached: no taskbar progress");
                }
            }
        }
        self.session.clone()
    }
}
