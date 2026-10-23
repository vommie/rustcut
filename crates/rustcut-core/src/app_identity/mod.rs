//! Names and ids of the application, in one place.

/// Name shown to the user (window titles, About, notifications).
pub const APP_NAME: &str = "RustCut";

/// Reverse-DNS application id: XDG app id of the windows, desktop file and AppStream id.
pub const APP_ID: &str = "io.github.vommie.rustcut";

/// Name of the binary and of the XDG config, data and state folders.
pub const BINARY_NAME: &str = "rustcut";

/// Version of the workspace (`[workspace.package] version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_ends_with_the_binary_name() {
        assert!(APP_ID.ends_with(&format!(".{BINARY_NAME}")));
        assert_eq!(APP_NAME.to_lowercase(), BINARY_NAME);
    }

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{VERSION}");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{VERSION}");
    }
}
