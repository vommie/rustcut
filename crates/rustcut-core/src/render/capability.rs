//! Which encoders really work here (ADR-14): ffmpeg's encoder list, then a one-frame test encode per known encoder in
//! the background. Hardware encoders (VAAPI, NVENC, QSV) are offered only when their test passes; a VAAPI encoder is
//! tried on every DRM render node until one works.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;

use super::runner::without_context;
use super::{EncoderBackend, HardwareContext, VideoEncoder};
use crate::media::ProbeError;
use crate::media::run_tool as run;

/// Time limit of one listing or test encode (a broken driver can hang).
pub const TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Test encodes running at the same time.
const PARALLEL_TESTS: usize = 3;
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

/// What became of one encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Works,
    /// This ffmpeg build does not list it.
    NotBuilt,
    /// Listed, but the test encode failed (no device, no driver, unsupported hardware): ffmpeg's reason.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderStatus {
    pub encoder: VideoEncoder,
    pub availability: Availability,
}

/// The result of the check, kept for the session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// ffmpeg's version line (`ffmpeg version n9.0.1 ...`).
    pub version: Option<String>,
    /// Every known encoder in catalog order.
    pub encoders: Vec<EncoderStatus>,
    /// vid.stab's filters are built in (deshake).
    pub vidstab: bool,
    pub hardware: HardwareContext,
}

impl Capabilities {
    pub fn works(&self, encoder: VideoEncoder) -> bool {
        self.encoders
            .iter()
            .any(|s| s.encoder == encoder && s.availability == Availability::Works)
    }

    /// The encoders that can be offered.
    pub fn working(&self) -> impl Iterator<Item = VideoEncoder> + '_ {
        self.encoders
            .iter()
            .filter(|s| s.availability == Availability::Works)
            .map(|s| s.encoder)
    }

    /// One line for the log: `libsvtav1 libx264 ... (not working: h264_vaapi: ...)`.
    pub fn summary(&self) -> String {
        let working: Vec<&str> = self.working().map(|e| e.spec().ffmpeg_name).collect();
        let failed: Vec<String> = self
            .encoders
            .iter()
            .filter_map(|s| match &s.availability {
                Availability::Failed(reason) => Some(format!("{}: {reason}", s.encoder.spec().ffmpeg_name)),
                Availability::Works | Availability::NotBuilt => None,
            })
            .collect();
        let mut text = format!("working: {}", working.join(" "));
        if !failed.is_empty() {
            text.push_str("; not working: ");
            text.push_str(&failed.join("; "));
        }
        text.push_str(if self.vidstab {
            "; vid.stab present"
        } else {
            "; vid.stab missing"
        });
        text
    }
}

/// Starts [`detect_capabilities`] in the background on `runtime` and logs the result; the receiver holds it once known
/// (`None` until then, and when ffmpeg could not run).
pub fn spawn_capability_check(
    runtime: &tokio::runtime::Handle,
    ffmpeg: Option<PathBuf>,
) -> watch::Receiver<Option<Arc<Capabilities>>> {
    let (sender, receiver) = watch::channel(None);
    runtime.spawn(async move {
        let started = std::time::Instant::now();
        match detect_capabilities(ffmpeg.as_deref()).await {
            Ok(caps) => {
                let millis = started.elapsed().as_millis();
                tracing::info!(millis, "[Render] Encoder check: {}", caps.summary());
                if !caps.vidstab {
                    tracing::warn!(
                        "[Render] ffmpeg has no vid.stab filters: deshake cannot render (sudo pacman -S vid.stab)"
                    );
                }
                sender.send_replace(Some(Arc::new(caps)));
            }
            Err(e) => tracing::warn!(error = %e, "[Render] Encoder check failed"),
        }
    });
    receiver
}

/// The encoder names of `ffmpeg -encoders` (lines like ` V....D libx264   libx264 H.264 ...`).
pub fn parse_encoder_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let flags = words.next()?;
            let name = words.next()?;
            let is_entry = flags.len() == 6 && flags.starts_with(['V', 'A', 'S']) && name != "=";
            is_entry.then(|| name.to_owned())
        })
        .collect()
}

/// Whether `ffmpeg -filters` lists both vid.stab filters.
pub fn has_vidstab(filters: &str) -> bool {
    let listed = |name: &str| filters.lines().any(|l| l.split_whitespace().nth(1) == Some(name));
    listed("vidstabdetect") && listed("vidstabtransform")
}

/// The arguments of the one-frame test of `encoder` (on `vaapi_device` for VAAPI).
pub fn test_args(encoder: VideoEncoder, vaapi_device: Option<&Path>) -> Vec<OsString> {
    let spec = encoder.spec();
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error"]
        .map(OsString::from)
        .to_vec();
    if let Some(device) = vaapi_device {
        args.push("-vaapi_device".into());
        args.push(device.as_os_str().to_owned());
    }
    args.extend(
        [
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=256x256:r=25:d=0.2",
            "-frames:v",
            "1",
        ]
        .map(OsString::from),
    );
    if spec.backend == EncoderBackend::Vaapi {
        args.extend(["-vf", "format=nv12,hwupload"].map(OsString::from));
    } else {
        args.extend(["-pix_fmt", spec.pix_fmt_8].map(OsString::from));
    }
    args.extend(["-c:v", spec.ffmpeg_name, "-f", "null", "-"].map(OsString::from));
    args
}

/// Runs the check with `ffmpeg` (`None` = from `PATH`). Fails only when ffmpeg itself cannot run.
pub async fn detect_capabilities(ffmpeg: Option<&Path>) -> Result<Capabilities, ProbeError> {
    let program = ffmpeg.unwrap_or_else(|| Path::new("ffmpeg")).to_path_buf();
    let listing = |what: &str| {
        run(
            &program,
            vec!["-hide_banner".into(), what.into()],
            TEST_TIMEOUT,
            MAX_OUTPUT,
        )
    };
    let encoders = listing("-encoders").await?;
    let built = parse_encoder_list(&String::from_utf8_lossy(&encoders.stdout));
    let filters = listing("-filters").await?;
    let vidstab = has_vidstab(&String::from_utf8_lossy(&filters.stdout));
    let version = listing("-version")
        .await
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_owned));
    let nodes = render_nodes().await;

    let limit = Arc::new(Semaphore::new(PARALLEL_TESTS));
    let mut tests = JoinSet::new();
    let mut statuses = Vec::new();
    for encoder in VideoEncoder::ALL {
        if !built.iter().any(|n| n == encoder.spec().ffmpeg_name) {
            statuses.push(EncoderStatus {
                encoder,
                availability: Availability::NotBuilt,
            });
            continue;
        }
        let (program, nodes, limit) = (program.clone(), nodes.clone(), Arc::clone(&limit));
        tests.spawn(async move {
            let _permit = limit.acquire_owned().await;
            let (availability, device) = test_encoder(&program, encoder, &nodes).await;
            (encoder, availability, device)
        });
    }
    let mut vaapi_device = None;
    while let Some(joined) = tests.join_next().await {
        match joined {
            Ok((encoder, availability, device)) => {
                if availability == Availability::Works && vaapi_device.is_none() {
                    vaapi_device = device;
                }
                statuses.push(EncoderStatus { encoder, availability });
            }
            Err(e) => tracing::warn!(error = %e, "[Render] An encoder test did not finish"),
        }
    }
    statuses.sort_by_key(|s| VideoEncoder::ALL.iter().position(|e| *e == s.encoder));
    Ok(Capabilities {
        version,
        encoders: statuses,
        vidstab,
        hardware: HardwareContext { vaapi_device },
    })
}

/// One test encode; VAAPI encoders try each render node. Returns the node that worked.
async fn test_encoder(program: &Path, encoder: VideoEncoder, nodes: &[PathBuf]) -> (Availability, Option<PathBuf>) {
    if encoder.spec().backend != EncoderBackend::Vaapi {
        return (try_once(program, test_args(encoder, None)).await, None);
    }
    if nodes.is_empty() {
        return (
            Availability::Failed("no DRM render node (/dev/dri/renderD*)".to_owned()),
            None,
        );
    }
    let mut last = Availability::Failed(String::new());
    for node in nodes {
        last = try_once(program, test_args(encoder, Some(node))).await;
        if last == Availability::Works {
            return (last, Some(node.clone()));
        }
    }
    (last, None)
}

async fn try_once(program: &Path, args: Vec<OsString>) -> Availability {
    match run(program, args, TEST_TIMEOUT, MAX_OUTPUT).await {
        Ok(out) if out.status.success() => Availability::Works,
        Ok(out) => {
            // With `-loglevel error` the first line is the cause; the lines after it are ffmpeg's follow-ups.
            let reason = out.stderr.lines().map(without_context).find(|l| !l.is_empty());
            Availability::Failed(reason.unwrap_or("no message").to_owned())
        }
        Err(e) => Availability::Failed(e.to_string()),
    }
}

/// `/dev/dri/renderD*`, sorted.
async fn render_nodes() -> Vec<PathBuf> {
    let mut nodes = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir("/dev/dri").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().starts_with("renderD") {
                nodes.push(entry.path());
            }
        }
    }
    nodes.sort();
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODERS: &str = "\
Encoders:
 V..... = Video
 A..... = Audio
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D h264_vaapi           H.264/AVC (VAAPI) (codec h264)
 VF.... libsvtav1            SVT-AV1(Scalable Video Technology for AV1) encoder (codec av1)
 A....D libopus              libopus Opus (codec opus)
";

    #[test]
    fn the_encoder_list() {
        assert_eq!(
            parse_encoder_list(ENCODERS),
            ["libx264", "h264_vaapi", "libsvtav1", "libopus"]
        );
    }

    #[test]
    fn vidstab_needs_both_filters() {
        let both = " ... vidstabdetect     V->V       Extract relative transformations\n ... vidstabtransform  V->V       Transform\n";
        assert!(has_vidstab(both));
        assert!(!has_vidstab(" ... vidstabdetect     V->V       Extract\n"));
    }

    #[test]
    fn test_encodes() {
        let text = |args: Vec<OsString>| {
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            text(test_args(VideoEncoder::X264, None)),
            "-hide_banner -nostdin -loglevel error -f lavfi -i color=c=black:s=256x256:r=25:d=0.2 -frames:v 1 \
             -pix_fmt yuv420p -c:v libx264 -f null -"
        );
        assert_eq!(
            text(test_args(
                VideoEncoder::HevcVaapi,
                Some(Path::new("/dev/dri/renderD128"))
            )),
            "-hide_banner -nostdin -loglevel error -vaapi_device /dev/dri/renderD128 -f lavfi -i \
             color=c=black:s=256x256:r=25:d=0.2 -frames:v 1 -vf format=nv12,hwupload -c:v hevc_vaapi -f null -"
        );
    }

    #[test]
    fn summary_and_queries() {
        let caps = Capabilities {
            version: None,
            encoders: vec![
                EncoderStatus {
                    encoder: VideoEncoder::X264,
                    availability: Availability::Works,
                },
                EncoderStatus {
                    encoder: VideoEncoder::H264Nvenc,
                    availability: Availability::Failed("Cannot load libcuda.so.1".into()),
                },
                EncoderStatus {
                    encoder: VideoEncoder::Rav1e,
                    availability: Availability::NotBuilt,
                },
            ],
            vidstab: true,
            hardware: HardwareContext::default(),
        };
        assert!(caps.works(VideoEncoder::X264) && !caps.works(VideoEncoder::H264Nvenc));
        assert_eq!(caps.working().collect::<Vec<_>>(), [VideoEncoder::X264]);
        assert_eq!(
            caps.summary(),
            "working: libx264; not working: h264_nvenc: Cannot load libcuda.so.1; vid.stab present"
        );
    }
}
