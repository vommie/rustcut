//! systemd-logind over the system bus: the sleep inhibitor while a job renders, suspend and power off when the queue is
//! done (ADR-15). Every call runs on the runtime; the UI never waits for D-Bus.

use zbus::{Connection, OwnedFd, Proxy};

const SERVICE: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

/// What logind should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Suspend,
    PowerOff,
}

impl PowerAction {
    const fn method(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::PowerOff => "PowerOff",
        }
    }
}

async fn manager(connection: &Connection) -> zbus::Result<Proxy<'static>> {
    Proxy::new(connection, SERVICE, PATH, MANAGER).await
}

/// Takes a `sleep:idle` lock ("block"): the system neither suspends nor goes idle while the returned descriptor lives.
pub async fn inhibit(connection: &Connection, why: &str) -> zbus::Result<OwnedFd> {
    manager(connection)
        .await?
        .call("Inhibit", &("sleep:idle", "RustCut", why, "block"))
        .await
}

/// Suspends or powers off without asking for a password (`interactive = false`): when the system wants one (another
/// user is logged in, a policy), logind refuses and the reason comes back.
pub async fn act(connection: &Connection, action: PowerAction) -> zbus::Result<()> {
    manager(connection).await?.call(action.method(), &(false,)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_sleep_lock_can_be_taken_and_dropped() {
        // Needs systemd-logind on the system bus, like the desktop RustCut runs on.
        let connection = Connection::system().await.unwrap();
        let fd = inhibit(&connection, "test").await.unwrap();
        drop(fd);
        assert_eq!(PowerAction::PowerOff.method(), "PowerOff");
    }
}
