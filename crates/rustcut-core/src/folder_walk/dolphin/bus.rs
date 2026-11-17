//! Asking a running Dolphin over the session D-Bus: which windows exist, whether one shows a folder in any tab
//! (`org.kde.dolphin.MainWindow.isUrlOpen`), and how Dolphin sorts it (`org.freedesktop.FileManager1.SortOrderForUrl`,
//! which reads Dolphin's stored view properties of that folder).
//!
//! `org.freedesktop.FileManager1` is D-Bus activatable: calling it while no Dolphin runs would start one. So it is
//! only called while a Dolphin window exists.

use std::path::Path;
use std::time::Duration;

use zbus::Connection;
use zbus::fdo::{DBusProxy, IntrospectableProxy};
use zbus::names::BusName;

use super::super::order::SortRole;

/// What the running Dolphins said about a folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BusView {
    /// Dolphin windows over all Dolphin processes (a background daemon has none).
    pub windows: usize,
    /// One of them shows the folder in a tab (also a background tab).
    pub open: bool,
    /// Dolphin's sort role and "descending" for the folder, when asked and a window exists.
    pub sort: Option<(SortRole, bool)>,
}

const DOLPHIN_PREFIX: &str = "org.kde.dolphin-";
const FILE_MANAGER: &str = "org.freedesktop.FileManager1";
const MAIN_WINDOW: &str = "org.kde.dolphin.MainWindow";
/// The whole question may take this long; Dolphin answers in a few milliseconds.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// Asks every running Dolphin about `folder`.
///
/// # Errors
/// The session bus cannot be reached or a call fails in a way that is not "no such window".
pub async fn ask(folder: &Path, want_sort: bool) -> Result<BusView, String> {
    match tokio::time::timeout(TIMEOUT, ask_inner(folder, want_sort)).await {
        Ok(result) => result.map_err(|e| e.to_string()),
        Err(_) => Err("Dolphin did not answer in time".to_owned()),
    }
}

async fn ask_inner(folder: &Path, want_sort: bool) -> zbus::Result<BusView> {
    let url = url::Url::from_file_path(folder)
        .map_err(|()| zbus::Error::Failure(format!("not an absolute path: {}", folder.display())))?
        .to_string();
    let connection = Connection::session().await?;
    let bus = DBusProxy::new(&connection).await?;
    let mut view = BusView::default();
    for name in bus.list_names().await? {
        if !name.as_str().starts_with(DOLPHIN_PREFIX) {
            continue;
        }
        for window in windows(&connection, name.as_str()).await {
            view.windows += 1;
            if !view.open {
                view.open = is_open(&connection, name.as_str(), &window, &url)
                    .await
                    .unwrap_or(false);
            }
        }
    }
    if want_sort && view.windows > 0 {
        let owner = BusName::try_from(FILE_MANAGER)?;
        if bus.name_has_owner(owner).await? {
            let reply = connection
                .call_method(
                    Some(FILE_MANAGER),
                    "/org/freedesktop/FileManager1",
                    Some(FILE_MANAGER),
                    "SortOrderForUrl",
                    &(url.as_str(),),
                )
                .await?;
            let (role, order): (String, String) = reply.body().deserialize()?;
            view.sort = Some((SortRole::from_dolphin(&role), order == "descending"));
        }
    }
    Ok(view)
}

/// The object paths of a Dolphin process's main windows (`/dolphin/Dolphin_1`, ...).
async fn windows(connection: &Connection, name: &str) -> Vec<String> {
    introspect(connection, name)
        .await
        .map(|xml| window_paths(&xml))
        .unwrap_or_default()
}

async fn introspect(connection: &Connection, name: &str) -> zbus::Result<String> {
    let proxy = IntrospectableProxy::builder(connection)
        .destination(name.to_owned())
        .path("/dolphin")
        .build()
        .await?;
    Ok(proxy.introspect().await?)
}

/// The `Dolphin_<n>` child nodes of `/dolphin` in introspection XML.
#[must_use]
pub fn window_paths(xml: &str) -> Vec<String> {
    // D-Bus introspection starts with a DOCTYPE, which roxmltree refuses unless allowed.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let Ok(doc) = roxmltree::Document::parse_with_options(xml, options) else {
        return Vec::new();
    };
    doc.root_element()
        .children()
        .filter(|node| node.has_tag_name("node"))
        .filter_map(|node| node.attribute("name"))
        .filter(|name| {
            name.strip_prefix("Dolphin_")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|name| format!("/dolphin/{name}"))
        .collect()
}

async fn is_open(connection: &Connection, name: &str, window: &str, url: &str) -> zbus::Result<bool> {
    let reply = connection
        .call_method(Some(name), window, Some(MAIN_WINDOW), "isUrlOpen", &(url,))
        .await?;
    reply.body().deserialize()
}

#[cfg(test)]
mod tests {
    use super::window_paths;

    #[test]
    fn finds_main_windows_only() {
        let xml = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node><interface name="org.freedesktop.DBus.Introspectable"/><node name="Dolphin_1"/><node name="Dolphin_12"/><node name="Dolphin_x"/><node name="FileUndoManager"/></node>"#;
        assert_eq!(window_paths(xml), ["/dolphin/Dolphin_1", "/dolphin/Dolphin_12"]);
        assert!(window_paths("not xml").is_empty());
    }
}
