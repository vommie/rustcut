//! One step through an ordered folder (plugin API 1.3 `step-folder`; RustCut's navigator and every test navigator use
//! it): the next or previous file from the open one,
//! files with a job passed over, the end of the folder or a wrap to the other end. Only the result crosses into the
//! plugin, however many files the folder has.

/// Where the step landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// The file's place in the list; `None` at the end of the folder.
    pub target: Option<usize>,
    /// Files with a job that were passed over.
    pub skipped: usize,
    /// The step went past the end and started over.
    pub wrapped: bool,
}

/// The file after `current` (`forward`) or before it among `len` files; `has_job(i)` files are passed over when
/// `skip_with_job`, and `wrap` goes on from the other end (never back to `current`). Without `current` the step starts
/// before the first (forward) or after the last file.
pub fn step(
    len: usize,
    current: Option<usize>,
    forward: bool,
    skip_with_job: bool,
    wrap: bool,
    has_job: impl Fn(usize) -> bool,
) -> Step {
    let mut result = Step {
        target: None,
        skipped: 0,
        wrapped: false,
    };
    let Ok(lap) = isize::try_from(len) else { return result };
    if lap == 0 {
        return result;
    }
    let current = current
        .and_then(|c| isize::try_from(c).ok())
        .filter(|c| (0..lap).contains(c));
    let mut position = current.unwrap_or(if forward { -1 } else { lap });
    for _ in 0..len {
        position += if forward { 1 } else { -1 };
        if !(0..lap).contains(&position) {
            if !wrap {
                return result;
            }
            result.wrapped = true;
            position = position.rem_euclid(lap);
        }
        if Some(position) == current {
            return result;
        }
        let Ok(index) = usize::try_from(position) else {
            return result;
        };
        if skip_with_job && has_job(index) {
            result.skipped += 1;
            continue;
        }
        result.target = Some(index);
        return result;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(jobs: &[bool], current: Option<usize>, forward: bool, skip: bool, wrap: bool) -> Step {
        step(jobs.len(), current, forward, skip, wrap, |i| {
            jobs.get(i).copied().unwrap_or(false)
        })
    }

    #[test]
    fn next_and_previous_stop_at_the_ends() {
        let list = [false, false, false];
        assert_eq!(walk(&list, Some(1), true, true, false).target, Some(2));
        assert_eq!(walk(&list, Some(1), false, true, false).target, Some(0));
        assert_eq!(
            walk(&list, Some(2), true, true, false),
            Step {
                target: None,
                skipped: 0,
                wrapped: false
            }
        );
        assert_eq!(walk(&list, Some(0), false, true, false).target, None);
    }

    #[test]
    fn files_with_a_job_are_passed_over_and_counted() {
        let list = [false, true, true, false];
        assert_eq!(
            walk(&list, Some(0), true, true, false),
            Step {
                target: Some(3),
                skipped: 2,
                wrapped: false
            }
        );
        assert_eq!(walk(&list, Some(0), true, false, false).target, Some(1));
        assert_eq!(
            walk(&[false, true, true], Some(0), true, true, false),
            Step {
                target: None,
                skipped: 2,
                wrapped: false
            }
        );
    }

    #[test]
    fn wrapping_starts_over_but_never_returns_to_the_open_file() {
        let list = [true, false, false];
        assert_eq!(
            walk(&list, Some(2), true, true, true),
            Step {
                target: Some(1),
                skipped: 1,
                wrapped: true
            }
        );
        assert_eq!(
            walk(&list, Some(1), false, true, true),
            Step {
                target: Some(2),
                skipped: 1,
                wrapped: true
            }
        );
        assert_eq!(walk(&[false], Some(0), true, true, true).target, None);
    }

    #[test]
    fn an_open_file_outside_the_list_starts_at_an_end() {
        assert_eq!(walk(&[false, false], None, true, true, false).target, Some(0));
        assert_eq!(walk(&[false, false], None, false, true, false).target, Some(1));
        assert_eq!(walk(&[], None, true, true, true).target, None);
        assert_eq!(walk(&[false, false], Some(9), true, true, false).target, Some(0));
    }

    #[test]
    fn a_million_files_with_jobs_take_one_pass() {
        let n = 1_000_000;
        let found = step(n, Some(0), true, true, false, |i| i < n - 1);
        assert_eq!(
            found,
            Step {
                target: Some(n - 1),
                skipped: n - 2,
                wrapped: false
            }
        );
    }
}
