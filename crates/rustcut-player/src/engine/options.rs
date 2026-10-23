//! The options the mpv core starts with. Settings (P4) map onto this struct; the defaults are PyCutRv's player behavior.

use libmpv2::{MpvInitializer, Result};

/// mpv's built-in profile for cheap scaling (bilinear, no dithering, no linear-light downscaling).
pub(crate) const FAST_PROFILE: &str = "fast";

/// How mpv scales the preview (the rendered output is not affected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewQuality {
    /// `Fast` on a software OpenGL renderer (llvmpipe), mpv's defaults on a GPU. Measured in M1.1 on llvmpipe with a 1080p
    /// video in a 1280x720 area: mpv's defaults 27 ms per frame (22 of 25 fps), `fast` 1.4 ms (25 fps).
    #[default]
    Auto,
    /// mpv's `fast` profile.
    Fast,
    /// mpv's default scalers.
    High,
}

/// Startup options of the mpv core.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerOptions {
    /// mpv `hwdec` (PyCutRv: `auto-copy`; copy-back decoding works with every render path).
    pub hwdec: String,
    /// Stay on the last frame at the end instead of unloading the file.
    pub keep_open: bool,
    /// Start playing after loading (PyCutRv setting "auto play").
    pub autoplay: bool,
    /// Scaling quality of the preview.
    pub preview_quality: PreviewQuality,
    /// Lowest mpv log level forwarded to RustCut's log (`fatal`, `error`, `warn`, `info`, `v`, `debug`).
    pub log_level: &'static str,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            hwdec: "auto-copy".into(),
            keep_open: true,
            autoplay: true,
            preview_quality: PreviewQuality::Auto,
            log_level: "warn",
        }
    }
}

impl PlayerOptions {
    /// Sets the options before mpv initializes.
    pub(crate) fn apply(&self, init: &MpvInitializer) -> Result<()> {
        // Video is drawn through the render API into Slint's OpenGL context (ADR-2).
        init.set_option("vo", "libmpv")?;
        // mpv_render_context_render() would otherwise wait for the frame's display time and block Slint's UI thread
        // (mpv manual, --video-timing-offset: "you can stop it from limiting your FPS").
        init.set_option("video-timing-offset", 0.0)?;
        init.set_option("hwdec", self.hwdec.as_str())?;
        init.set_option("keep-open", if self.keep_open { "yes" } else { "no" })?;
        init.set_option("pause", !self.autoplay)?;
        // An embedded player: no own key bindings, no on-screen controller, no window title of its own.
        init.set_option("input-default-bindings", false)?;
        init.set_option("input-vo-keyboard", false)?;
        init.set_option("osc", false)?;
        if self.preview_quality == PreviewQuality::Fast {
            init.set_option("profile", FAST_PROFILE)?;
        }
        Ok(())
    }
}

/// Whether an OpenGL renderer string (`GL_RENDERER`) names a software rasterizer (Mesa llvmpipe, softpipe, swrast).
pub(crate) fn is_software_renderer(renderer: &str) -> bool {
    let renderer = renderer.to_ascii_lowercase();
    ["llvmpipe", "softpipe", "swrast", "software rasterizer"]
        .iter()
        .any(|name| renderer.contains(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_renderers_are_recognized() {
        assert!(is_software_renderer("llvmpipe (LLVM 22.1.8, 256 bits)"));
        assert!(is_software_renderer("Mesa / softpipe / 4.5"));
        assert!(is_software_renderer("Software Rasterizer"));
        assert!(!is_software_renderer(
            "AMD Radeon RX 7800 XT (radeonsi, navi32, LLVM 22.1.8, DRM 3.64)"
        ));
        assert!(!is_software_renderer("NVIDIA GeForce RTX 4070/PCIe/SSE2"));
        assert!(!is_software_renderer("Mesa Intel(R) UHD Graphics 770 (ADL-S GT1)"));
    }
}
