//! Text onto the clipboard without blocking the UI thread: one thread owns the clipboard (`arboard`; on X11 the owner
//! serves the content until another program takes it, and hands it to the clipboard manager when RustCut quits).

use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

static CLIPBOARD: OnceLock<Option<Sender<String>>> = OnceLock::new();

/// Puts `text` on the clipboard; `false` when there is no clipboard (logged).
pub fn copy(text: String) -> bool {
    let sender = CLIPBOARD.get_or_init(|| {
        let (tx, rx) = channel::<String>();
        let spawned = std::thread::Builder::new()
            .name("rustcut-clipboard".into())
            .spawn(move || {
                let mut clipboard = match arboard::Clipboard::new() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(error = %e, "[App] No clipboard");
                        return;
                    }
                };
                while let Ok(text) = rx.recv() {
                    if let Err(e) = clipboard.set_text(text) {
                        tracing::warn!(error = %e, "[App] The clipboard refused the text");
                    }
                }
            });
        match spawned {
            Ok(_thread) => Some(tx),
            Err(e) => {
                tracing::warn!(error = %e, "[App] The clipboard thread could not be started");
                None
            }
        }
    });
    sender.as_ref().is_some_and(|tx| tx.send(text).is_ok())
}
