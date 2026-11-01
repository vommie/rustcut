//! One picture of one file through the helper, for the conflict dialog: much faster than a process of ffmpeg per file (user
//! decision Q-P12e). `None` sends the caller to its ffmpeg path, so a missing helper only costs time.

use std::path::Path;
use std::time::Duration;

use image::RgbImage;
use rustcut_thumbs_protocol::{Job, Reply, Start};
use tokio::sync::mpsc;

use super::generator::{Arrived, Generator, Helper};
use super::view::{ThumbView, render};
use crate::edit::Orientation;

/// Longer than this and the helper is not the fast way for this file.
const LIMIT: Duration = Duration::from_secs(5);
/// Below the player and the desktop, like the other tools of the dialog.
const NICE: i32 = 10;

/// The keyframe at or before `ms`, turned like the player shows it.
pub async fn still(program: &Path, path: &Path, ms: u64, rotation: u16, long_side: u32) -> Option<RgbImage> {
    let start = Start {
        path: path.to_str()?.to_owned(),
        long_side,
        quality: 90,
        workers: 1,
        nice: NICE,
    };
    let (arrivals, mut incoming) = mpsc::unbounded_channel();
    let mut generator = Generator::Helper(Helper::start(program, &start, 0, arrivals).ok()?);
    generator.add(vec![Job::Key { ms }]);
    let answer = tokio::time::timeout(LIMIT, async {
        loop {
            match incoming.recv().await? {
                (_, Arrived::Reply(Reply::Thumb { .. }, picture)) => return Some(picture),
                (_, Arrived::Reply(Reply::Ready { .. }, _)) => {}
                (_, _other) => return None,
            }
        }
    })
    .await
    .ok()??;
    let view = ThumbView {
        before: Orientation::from_degrees_clockwise(rotation),
        ..ThumbView::default()
    };
    render(&answer, &view)
}
