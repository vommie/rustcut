//! OpenGL on Slint's context: the framebuffer texture mpv draws into, and the state guard around mpv's drawing.
//!
//! Everything here runs inside Slint's rendering notifier, where Slint has made its OpenGL context current on the UI thread.
#![allow(unsafe_code, reason = "OpenGL calls through glow are unsafe FFI")]
#![allow(
    clippy::multiple_unsafe_ops_per_block,
    reason = "each block is a sequence of GL calls on the current context; one SAFETY comment states what all of them need"
)]

use std::ffi::{CStr, c_void};
use std::num::NonZeroU32;

use glow::HasContext as _;

/// OpenGL function pointers of Slint's context.
pub(super) fn load(get_proc_address: &dyn Fn(&CStr) -> *const c_void) -> glow::Context {
    // SAFETY: Slint passes the loader of its own context, which is current while the notifier runs; glow only resolves the
    // function pointers here and calls them later while the same context is current (every use is inside the notifier).
    unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) }
}

/// Vendor, renderer and version of the context, for the log ("llvmpipe" means software rendering).
pub(super) fn describe(gl: &glow::Context) -> String {
    // SAFETY: the context is current; string queries with valid enums.
    let (vendor, renderer, version) = unsafe {
        (
            gl.get_parameter_string(glow::VENDOR),
            gl.get_parameter_string(glow::RENDERER),
            gl.get_parameter_string(glow::VERSION),
        )
    };
    format!("{vendor} / {renderer} / {version}")
}

/// An RGBA texture with a framebuffer object around it: mpv renders a frame into it, Slint draws it as an image.
pub(super) struct VideoTarget {
    texture: glow::NativeTexture,
    framebuffer: glow::NativeFramebuffer,
    width: u32,
    height: u32,
}

impl VideoTarget {
    /// Allocates a `width` x `height` texture and its framebuffer. Call inside a [`StateGuard`] (bindings change).
    pub(super) fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Self, String> {
        let w = i32::try_from(width).map_err(|e| format!("width {width}: {e}"))?;
        let h = i32::try_from(height).map_err(|e| format!("height {height}: {e}"))?;
        // SAFETY: the context is current (see the module docs); the texture and framebuffer are created by this context and
        // stay bound only while this block runs (the caller's StateGuard restores the previous bindings).
        unsafe {
            let texture = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, enum_param(glow::LINEAR));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, enum_param(glow::LINEAR));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, enum_param(glow::CLAMP_TO_EDGE));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, enum_param(glow::CLAMP_TO_EDGE));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                enum_param(glow::RGBA8),
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);

            let framebuffer = match gl.create_framebuffer() {
                Ok(fb) => fb,
                Err(e) => {
                    gl.delete_texture(texture);
                    return Err(e);
                }
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_framebuffer(framebuffer);
                gl.delete_texture(texture);
                return Err(format!("the framebuffer is incomplete (status {status:#x})"));
            }
            Ok(Self {
                texture,
                framebuffer,
                width,
                height,
            })
        }
    }

    /// Size in physical pixels.
    pub(super) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The framebuffer id as mpv's render call wants it.
    pub(super) fn framebuffer_id(&self) -> Option<i32> {
        i32::try_from(self.framebuffer.0.get()).ok()
    }

    /// The texture as a Slint image. Without `flip_y` mpv writes the top row first (MPV_RENDER_PARAM_FLIP_Y is "usually
    /// needed for OpenGL default framebuffer"), which is Slint's default top-left origin.
    pub(super) fn image(&self) -> slint::Image {
        // SAFETY: the texture is a GL_TEXTURE_2D with RGBA8 storage created by the context Slint renders with (the one current
        // in the notifier), and it is deleted only in `delete`, which the surface calls after it stopped presenting this image.
        let builder = unsafe {
            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                self.texture.0,
                (self.width, self.height).into(),
            )
        };
        builder.origin(slint::BorrowedOpenGLTextureOrigin::TopLeft).build()
    }

    /// Frees the GL objects (the context must be current).
    pub(super) fn delete(self, gl: &glow::Context) {
        // SAFETY: both objects were created by this context, which is current; nothing uses them after this call.
        unsafe {
            gl.delete_framebuffer(self.framebuffer);
            gl.delete_texture(self.texture);
        }
    }
}

/// Saves the GL state Slint's renderer relies on, resets the context to OpenGL defaults for mpv (mpv's `render_gl.h`: "mpv
/// functions expect that the OpenGL state is reasonably set to OpenGL standard defaults"; femtovg leaves blending, face
/// culling, its program and vertex array active), and restores everything when dropped.
pub(super) struct StateGuard<'a> {
    gl: &'a glow::Context,
    draw_framebuffer: i32,
    read_framebuffer: i32,
    viewport: [i32; 4],
    program: i32,
    vertex_array: i32,
    array_buffer: i32,
    active_texture: i32,
    texture_2d: i32,
    unpack_buffer: i32,
    unpack: [(u32, i32); 4],
    capabilities: [(u32, bool); 5],
}

/// Capabilities that are switched off for mpv and switched back afterwards.
const CAPABILITIES: [u32; 5] = [
    glow::BLEND,
    glow::CULL_FACE,
    glow::DEPTH_TEST,
    glow::SCISSOR_TEST,
    glow::STENCIL_TEST,
];

/// Pixel upload parameters and their OpenGL defaults. femtovg leaves `UNPACK_ROW_LENGTH` at the width of its last upload,
/// which made mpv upload only the first row of each video plane.
const UNPACK_DEFAULTS: [(u32, i32); 4] = [
    (glow::UNPACK_ALIGNMENT, 4),
    (glow::UNPACK_ROW_LENGTH, 0),
    (glow::UNPACK_SKIP_PIXELS, 0),
    (glow::UNPACK_SKIP_ROWS, 0),
];

impl<'a> StateGuard<'a> {
    pub(super) fn enter(gl: &'a glow::Context) -> Self {
        // SAFETY: the context is current; these are plain state queries and state changes with valid enums and no objects
        // other than the default ones (0/None).
        unsafe {
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let capabilities = CAPABILITIES.map(|cap| (cap, gl.is_enabled(cap)));
            let mut guard = Self {
                gl,
                draw_framebuffer: gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING),
                read_framebuffer: gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING),
                viewport,
                program: gl.get_parameter_i32(glow::CURRENT_PROGRAM),
                vertex_array: gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING),
                array_buffer: gl.get_parameter_i32(glow::ARRAY_BUFFER_BINDING),
                active_texture: gl.get_parameter_i32(glow::ACTIVE_TEXTURE),
                texture_2d: 0,
                unpack_buffer: gl.get_parameter_i32(glow::PIXEL_UNPACK_BUFFER_BINDING),
                unpack: UNPACK_DEFAULTS.map(|(name, _)| (name, gl.get_parameter_i32(name))),
                capabilities,
            };
            gl.active_texture(glow::TEXTURE0);
            guard.texture_2d = gl.get_parameter_i32(glow::TEXTURE_BINDING_2D);

            for cap in CAPABILITIES {
                gl.disable(cap);
            }
            gl.use_program(None);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            for (name, value) in UNPACK_DEFAULTS {
                gl.pixel_store_i32(name, value);
            }
            guard
        }
    }
}

impl Drop for StateGuard<'_> {
    fn drop(&mut self) {
        let gl = self.gl;
        // SAFETY: the context is current; every id restored here was read from this context in `enter` and still names the
        // object Slint bound (Slint does not run between `enter` and this drop), or is 0/None.
        unsafe {
            gl.bind_framebuffer(
                glow::DRAW_FRAMEBUFFER,
                id(self.draw_framebuffer).map(glow::NativeFramebuffer),
            );
            gl.bind_framebuffer(
                glow::READ_FRAMEBUFFER,
                id(self.read_framebuffer).map(glow::NativeFramebuffer),
            );
            let [x, y, w, h] = self.viewport;
            gl.viewport(x, y, w, h);
            gl.use_program(id(self.program).map(glow::NativeProgram));
            gl.bind_vertex_array(id(self.vertex_array).map(glow::NativeVertexArray));
            gl.bind_buffer(glow::ARRAY_BUFFER, id(self.array_buffer).map(glow::NativeBuffer));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, id(self.texture_2d).map(glow::NativeTexture));
            gl.active_texture(u32::try_from(self.active_texture).unwrap_or(glow::TEXTURE0));
            gl.bind_buffer(
                glow::PIXEL_UNPACK_BUFFER,
                id(self.unpack_buffer).map(glow::NativeBuffer),
            );
            for (name, value) in self.unpack {
                gl.pixel_store_i32(name, value);
            }
            for (cap, enabled) in self.capabilities {
                if enabled {
                    gl.enable(cap);
                } else {
                    gl.disable(cap);
                }
            }
        }
    }
}

/// A GL enum passed where OpenGL takes a `GLint` (every GL enum is far below `i32::MAX`).
fn enum_param(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// A GL object name read with `glGetIntegerv`; 0 (and nonsense) means "none".
fn id(raw: i32) -> Option<NonZeroU32> {
    u32::try_from(raw).ok().and_then(NonZeroU32::new)
}
