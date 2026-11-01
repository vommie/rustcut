//! The thumbnails' two messages (user decision Q-P12e): the fallback's banner, which shows once and again only after the
//! helper worked in between, and the banner at the start about a large cache.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::ui::dispatcher::UiDispatcher;
use crate::ui::notices::{self, Kind, Notice};

/// The cache's size for the banner at the start.
const LARGE_CACHE: u64 = 4 * 1024 * 1024 * 1024;

/// `Some(value)` sets the memory; the answer is what it holds.
type FallbackMemory = Rc<dyn Fn(Option<bool>) -> bool>;

thread_local! {
    static FALLBACK_MEMORY: RefCell<Option<FallbackMemory>> = const { RefCell::new(None) };
}

/// Connects the persistent memory of the fallback's banner: `Some(value)` sets it, the answer is the value.
pub fn set_fallback_memory(memory: impl Fn(Option<bool>) -> bool + 'static) {
    FALLBACK_MEMORY.set(Some(Rc::new(memory)));
}

fn fallback_memory(value: Option<bool>) -> bool {
    FALLBACK_MEMORY.with_borrow(|memory| memory.as_ref().is_some_and(|memory| memory(value)))
}

/// The helper is back: the next failure tells again.
pub(super) fn helper_worked() {
    if fallback_memory(None) {
        fallback_memory(Some(false));
    }
}

/// The banner shows once, and again only after the helper worked in between (user decision Q-P12e).
pub(super) fn fallback_used() {
    if !fallback_memory(None) {
        fallback_memory(Some(true));
        notices::post(Notice::new(Kind::ThumbnailsFallback, "Thumbnails take the slow way").detail(
            "The thumbnail helper did not start, usually because FFmpeg was updated. RustCut makes them through ffmpeg \
             now. Update RustCut to get the fast way back.",
        ));
    }
}

/// The banner at the start about a large cache.
pub fn check_cache_size(path: PathBuf, ui: &UiDispatcher, runtime: &tokio::runtime::Handle) {
    let ui = ui.clone();
    runtime.spawn(async move {
        let size = tokio::fs::metadata(&path).await.map_or(0, |meta| meta.len());
        if size > LARGE_CACHE {
            ui.post(move || {
                notices::post(
                    Notice::new(Kind::ThumbnailCacheLarge, "The thumbnail cache is large").detail(format!(
                        "It takes {}. Settings > Player can empty it, and it empties by itself with the jobs.",
                        humansize::format_size(size, humansize::BINARY)
                    )),
                );
            });
        }
    });
}
