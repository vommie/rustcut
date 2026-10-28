//! The folder pictures as Slint images (P11.2): built-in icons by name and stored pictures by hash, turned into pixels
//! on the runtime (ADR-4: never on the UI thread), cached, and handed out by [`Pictures::image`]. A picture that is not
//! ready yet is requested and shows as missing (the first letter); every listener is told when one arrives.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use rustcut_core::folders::Picture;
use rustcut_core::folders::pictures::{PictureCommand, PictureHandle, PictureKind, Rgba, decode_webp, render_svg};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

include!(concat!(env!("OUT_DIR"), "/folder_icons.rs"));

/// The side built-in icons are drawn at (pixels; they are scaled down where they show smaller).
const ICON_SIDE: u32 = 128;
/// How long the listeners wait for more pictures before they are told.
const NOTIFY_DELAY: std::time::Duration = std::time::Duration::from_millis(40);
/// The side stored SVGs are drawn at (the stored images are at most this large too).
const STORED_SIDE: u32 = 256;

/// The SVG of the built-in icon `name`.
pub fn icon_svg(name: &str) -> Option<&'static str> {
    FOLDER_ICONS
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .and_then(|i| FOLDER_ICONS.get(i))
        .map(|(_, svg)| *svg)
}

/// The names of the built-in icons, sorted.
pub fn icon_names() -> impl Iterator<Item = &'static str> {
    FOLDER_ICONS.iter().map(|(n, _)| *n)
}

/// A picture's pixels and whether it is drawn in one color (tinted by the UI).
#[derive(Clone)]
pub struct Shown {
    pub image: Image,
    pub mono: bool,
}

/// Pixels made on the runtime (`SharedPixelBuffer` is `Send`, `Image` is not).
struct Pixels {
    buffer: SharedPixelBuffer<Rgba8Pixel>,
    mono: bool,
}

fn to_pixels(rgba: Rgba, mono: bool) -> Pixels {
    Pixels {
        buffer: SharedPixelBuffer::clone_from_slice(&rgba.pixels, rgba.width, rgba.height),
        mono,
    }
}

pub struct Pictures {
    me: Weak<Self>,
    store: PictureHandle,
    runtime: tokio::runtime::Handle,
    cache: RefCell<HashMap<Picture, Option<Shown>>>,
    pending: RefCell<HashSet<Picture>>,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
    /// The listeners are told once for a burst of pictures (the icon grid asks for all of them at once).
    notify_pending: Cell<bool>,
}

impl Pictures {
    pub fn new(store: PictureHandle, runtime: tokio::runtime::Handle) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            store,
            runtime,
            cache: RefCell::default(),
            pending: RefCell::default(),
            listeners: RefCell::default(),
            notify_pending: Cell::new(false),
        })
    }

    pub fn store(&self) -> &PictureHandle {
        &self.store
    }

    /// `listener` runs on the UI thread whenever a requested picture is ready.
    pub fn listen(&self, listener: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Box::new(listener));
    }

    /// The picture when it is ready; else `None` and it is requested.
    pub fn image(&self, picture: &Picture) -> Option<Shown> {
        if let Some(found) = self.cache.borrow().get(picture) {
            return found.clone();
        }
        self.request(picture.clone());
        None
    }

    /// Tells the listeners after a short while (once for every picture that arrived meanwhile).
    fn notify_soon(&self) {
        if self.notify_pending.replace(true) {
            return;
        }
        let weak = Weak::clone(&self.me);
        slint::Timer::single_shot(NOTIFY_DELAY, move || {
            let Some(me) = weak.upgrade() else { return };
            me.notify_pending.set(false);
            for listener in me.listeners.borrow().iter() {
                listener();
            }
        });
    }

    fn request(&self, picture: Picture) {
        if !self.pending.borrow_mut().insert(picture.clone()) {
            return;
        }
        let task = match &picture {
            Picture::Icon(name) => {
                let svg = icon_svg(name);
                self.runtime.spawn_blocking(move || {
                    svg.and_then(|svg| render_svg(svg.as_bytes(), ICON_SIDE))
                        .map(|rgba| to_pixels(rgba, true))
                })
            }
            Picture::Stored(hash) => {
                let store = self.store.clone();
                let hash = hash.clone();
                self.runtime.spawn(async move {
                    let stored = store
                        .request(|reply| PictureCommand::Get { hash, reply })
                        .await
                        .ok()?
                        .ok()??;
                    tokio::task::spawn_blocking(move || match stored.kind {
                        PictureKind::Svg { mono } => render_svg(&stored.data, STORED_SIDE).map(|p| to_pixels(p, mono)),
                        PictureKind::WebP => decode_webp(&stored.data).map(|p| to_pixels(p, false)),
                    })
                    .await
                    .ok()?
                })
            }
        };
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let pixels = task.await.ok().flatten();
            let Some(me) = weak.upgrade() else { return };
            me.pending.borrow_mut().remove(&picture);
            if pixels.is_none() {
                tracing::warn!(?picture, "[Folders] A folder picture could not be drawn");
            }
            let shown = pixels.map(|p| Shown {
                image: Image::from_rgba8(p.buffer),
                mono: p.mono,
            });
            me.cache.borrow_mut().insert(picture, shown);
            me.notify_soon();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Folders] The event loop is gone; no folder pictures");
        }
    }
}
