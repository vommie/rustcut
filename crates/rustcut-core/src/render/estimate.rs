//! The size estimate of a render (ADR-14): after the first seconds of the pass that writes the file, the bytes so far
//! scaled to the whole output. Early numbers are dominated by the container header and the encoder's lookahead, so
//! nothing is estimated before [`ESTIMATE_AFTER`].

use crate::queue::JobProgress;
use crate::time::Timestamp;

/// Output time a pass must have reached before its size says anything about the whole.
pub const ESTIMATE_AFTER: Timestamp = Timestamp::from_secs(3);

/// The expected size of the finished file in bytes, or `None` while it is too early (or a pass that writes no file
/// runs: the deshake analysis and the first of two passes go to `-f null`).
pub fn estimate_size(progress: &JobProgress) -> Option<u64> {
    if progress.pass < progress.passes || progress.size_bytes == 0 {
        return None;
    }
    let enough = ESTIMATE_AFTER.min(Timestamp::from_micros(progress.total.as_micros() / 2));
    if progress.done < enough || progress.done.as_micros() <= 0 {
        return None;
    }
    scale_size(progress.size_bytes, progress.done, progress.total)
}

/// `bytes` written for `rendered` of output, scaled to `total`.
pub fn scale_size(bytes: u64, rendered: Timestamp, total: Timestamp) -> Option<u64> {
    let rendered = u128::try_from(rendered.as_micros()).ok().filter(|r| *r > 0)?;
    let total = u128::try_from(total.as_micros()).ok()?;
    u64::try_from(u128::from(bytes) * total / rendered).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobId;

    fn progress(pass: u8, passes: u8, done: i64, total: i64, size_bytes: u64) -> JobProgress {
        JobProgress {
            id: JobId(1),
            pass,
            passes,
            done: Timestamp::from_secs(done),
            total: Timestamp::from_secs(total),
            fps: 25.0,
            speed: 1.0,
            size_bytes,
            eta_secs: None,
        }
    }

    #[test]
    fn estimates_only_from_the_writing_pass_after_a_few_seconds() {
        assert_eq!(estimate_size(&progress(1, 1, 10, 100, 1_000_000)), Some(10_000_000));
        assert_eq!(estimate_size(&progress(1, 1, 2, 100, 1_000_000)), None, "too early");
        assert_eq!(
            estimate_size(&progress(1, 2, 50, 100, 1_000_000)),
            None,
            "first of two passes"
        );
        assert_eq!(estimate_size(&progress(2, 2, 50, 100, 1_000_000)), Some(2_000_000));
        assert_eq!(estimate_size(&progress(1, 1, 10, 100, 0)), None, "nothing written yet");
        // A 4 s clip is estimated after 2 s, half of it.
        assert_eq!(estimate_size(&progress(1, 1, 2, 4, 500)), Some(1_000));
    }

    #[test]
    fn scaling() {
        let t = Timestamp::from_secs;
        assert_eq!(scale_size(2_000_000, t(10), t(200)), Some(40_000_000));
        assert_eq!(scale_size(1, t(0), t(10)), None);
        assert_eq!(
            scale_size(u64::MAX, Timestamp::from_micros(1), t(1_000_000)),
            None,
            "overflow"
        );
    }
}
