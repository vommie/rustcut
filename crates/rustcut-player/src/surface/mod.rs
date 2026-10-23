//! The video surface: mpv draws each new frame into an OpenGL texture while Slint renders, and the app shows that texture.
//!
//! Flow (all on the UI thread, inside Slint's rendering notifier, with Slint's GL context current):
//! - `RenderingSetup`: load GL, create mpv's render context, tell the player the surface is ready (a waiting file loads).
//! - mpv's update callback (any thread) asks Slint for a redraw, at most one request in flight.
//! - `BeforeRendering`: (re)create the texture when the target size changed and present it; if mpv has a new frame, draw it
//!   into the texture. Slint then draws the texture as part of the same frame.
//! - `AfterRendering`: report the swap to mpv, record the frame time.
//! - `RenderingTeardown`: free mpv's render context and the texture (the context is still current).

mod gl;
mod mpv_render;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, GraphicsAPI, RenderingState};

use crate::engine::PlayerHandle;
use crate::stats::{FrameSample, FrameStats};
use gl::{StateGuard, VideoTarget};
use mpv_render::MpvRender;

/// How often the frame statistics are written to the log (debug level) while video frames arrive.
const STATS_PERIOD: Duration = Duration::from_secs(5);

/// What the surface needs from the app's window.
pub struct SurfaceHooks {
    /// The size in physical pixels the video is rendered at (the size of the element that shows it).
    pub target_size: Box<dyn Fn() -> slint::PhysicalSize>,
    /// Shows a new video image (called when the texture was created or resized).
    pub present: Box<dyn Fn(slint::Image)>,
}

/// Errors when attaching the surface to a window.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    #[error("the window cannot host the video surface: {0}")]
    Notifier(#[from] slint::SetRenderingNotifierError),
}

/// Installs the video surface as `component`'s rendering notifier (a window has only one).
pub fn attach_surface<C: ComponentHandle + 'static>(
    component: &C,
    player: PlayerHandle,
    hooks: SurfaceHooks,
) -> Result<(), SurfaceError> {
    let mut surface = Surface {
        player,
        hooks,
        redraw: Arc::new(RedrawRequest {
            pending: AtomicBool::new(false),
            window: component.as_weak(),
        }),
        gpu: None,
        frame: FrameTiming::new(),
    };
    component
        .window()
        .set_rendering_notifier(move |state, api| surface.notify(state, api))?;
    tracing::debug!("[Player] Video surface attached");
    Ok(())
}

/// Asks the event loop for a redraw from mpv's threads; one request at a time.
struct RedrawRequest<C: ComponentHandle + 'static> {
    pending: AtomicBool,
    window: slint::Weak<C>,
}

impl<C: ComponentHandle + 'static> RedrawRequest<C> {
    fn request(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            // Fails only while the event loop shuts down; the frame is not needed then.
            self.window.upgrade_in_event_loop(|c| c.window().request_redraw()).ok();
        }
    }

    fn clear(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

/// GL state of the surface; exists between `RenderingSetup` and `RenderingTeardown`.
struct Gpu {
    gl: glow::Context,
    mpv: MpvRender,
    target: Option<VideoTarget>,
}

struct Surface<C: ComponentHandle + 'static> {
    player: PlayerHandle,
    hooks: SurfaceHooks,
    redraw: Arc<RedrawRequest<C>>,
    gpu: Option<Gpu>,
    frame: FrameTiming,
}

impl<C: ComponentHandle + 'static> Surface<C> {
    fn notify(&mut self, state: RenderingState, api: &GraphicsAPI<'_>) {
        match state {
            RenderingState::RenderingSetup => self.setup(api),
            RenderingState::BeforeRendering => self.before_rendering(),
            RenderingState::AfterRendering => self.after_rendering(),
            RenderingState::RenderingTeardown => self.teardown(),
            _ => {}
        }
    }

    fn setup(&mut self, api: &GraphicsAPI<'_>) {
        let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
            tracing::error!("[Player] The renderer does not use OpenGL; video cannot be shown");
            return;
        };
        let gl = gl::load(*get_proc_address);
        let description = gl::describe(&gl);
        tracing::info!(gl = %description, "[Player] Video surface on OpenGL");
        let mut mpv = match MpvRender::create(Arc::clone(self.player.mpv()), *get_proc_address) {
            Ok(mpv) => mpv,
            Err(e) => {
                tracing::error!(error = %e, "[Player] mpv's OpenGL renderer could not be created; video cannot be shown");
                return;
            }
        };
        let redraw = Arc::clone(&self.redraw);
        mpv.set_update_callback(move || redraw.request());
        self.gpu = Some(Gpu { gl, mpv, target: None });
        self.player.surface_ready(&description);
    }

    fn before_rendering(&mut self) {
        self.redraw.clear();
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        self.frame.begin();
        let size = (self.hooks.target_size)();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let new_frame = gpu.mpv.has_new_frame();
        let resized = gpu
            .target
            .as_ref()
            .is_none_or(|t| t.size() != (size.width, size.height));
        if !new_frame && !resized {
            return;
        }

        let guard = StateGuard::enter(&gpu.gl);
        if resized {
            if let Some(old) = gpu.target.take() {
                old.delete(&gpu.gl);
            }
            match VideoTarget::new(&gpu.gl, size.width, size.height) {
                Ok(target) => {
                    tracing::debug!(
                        width = size.width,
                        height = size.height,
                        "[Player] Video texture created"
                    );
                    (self.hooks.present)(target.image());
                    gpu.target = Some(target);
                }
                Err(e) => {
                    tracing::error!(error = %e, "[Player] The video texture could not be created");
                    return;
                }
            }
        }
        let Some(target) = gpu.target.as_ref() else {
            return;
        };
        let (Some(fbo), Ok(w), Ok(h)) = (
            target.framebuffer_id(),
            i32::try_from(size.width),
            i32::try_from(size.height),
        ) else {
            return;
        };
        let start = Instant::now();
        let result = gpu.mpv.render(fbo, w, h);
        drop(guard);
        match result {
            Ok(()) => self.frame.video_drawn(start.elapsed()),
            Err(e) => tracing::warn!(error = %e, "[Player] mpv could not draw the frame"),
        }
    }

    fn after_rendering(&mut self) {
        if let Some(gpu) = self.gpu.as_ref() {
            gpu.mpv.report_swap();
        }
        self.frame.end();
    }

    fn teardown(&mut self) {
        self.player.surface_lost();
        if let Some(gpu) = self.gpu.take() {
            if let Some(target) = gpu.target {
                target.delete(&gpu.gl);
            }
            // Dropping the render context frees mpv's GL objects; the context is still current here.
            drop(gpu.mpv);
        }
        self.frame.log_total();
        tracing::debug!("[Player] Video surface torn down");
    }
}

/// Times the Slint frames that drew a video frame and keeps the statistics.
struct FrameTiming {
    started: Option<Instant>,
    video_render: Option<Duration>,
    period: Option<FrameStats>,
    total: Option<FrameStats>,
    period_start: Instant,
}

impl FrameTiming {
    fn new() -> Self {
        let period = FrameStats::new();
        if period.is_none() {
            tracing::warn!("[Player] Frame statistics are not available");
        }
        Self {
            started: None,
            video_render: None,
            period,
            total: FrameStats::new(),
            period_start: Instant::now(),
        }
    }

    fn begin(&mut self) {
        self.started = Some(Instant::now());
        self.video_render = None;
    }

    fn video_drawn(&mut self, render: Duration) {
        self.video_render = Some(render);
    }

    fn end(&mut self) {
        let (Some(started), Some(render)) = (self.started.take(), self.video_render.take()) else {
            return;
        };
        let sample = FrameSample {
            render,
            frame: started.elapsed(),
            at: started,
        };
        for stats in [self.period.as_mut(), self.total.as_mut()].into_iter().flatten() {
            stats.record(sample);
        }
        if self.period_start.elapsed() >= STATS_PERIOD
            && let Some(period) = self.period.as_mut()
        {
            if let Some(r) = period.report() {
                tracing::debug!(
                    frames = r.frames,
                    fps = format_args!("{:.1}", r.fps),
                    render = %r.render_ms,
                    frame = %r.frame_ms,
                    interval = %r.interval_ms,
                    "[Player] Frame times (mean/p95/max)"
                );
            }
            period.reset();
            self.period_start = Instant::now();
        }
    }

    fn log_total(&self) {
        if let Some(r) = self.total.as_ref().and_then(FrameStats::report) {
            tracing::info!(
                frames = r.frames,
                fps = format_args!("{:.1}", r.fps),
                render = %r.render_ms,
                frame = %r.frame_ms,
                interval = %r.interval_ms,
                "[Player] Frame times of this session (mean/p95/max)"
            );
        }
    }
}
