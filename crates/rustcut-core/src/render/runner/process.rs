//! ffmpeg's process group: signals and priorities. ffmpeg runs as the leader of its own group (`process_group(0)`), so
//! a signal to the group reaches every thread and anything it might start, and nothing reaches RustCut itself.

use rustix::process::{Pid, Signal, getpriority_process, kill_process_group, setpriority_pgrp};

use super::supervise::GroupSignal;

/// How much ffmpeg yields to the desktop (settings `render.nice`, `render.idle_io`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPriority {
    /// Niceness 0 (normal) to 19 (lowest); never above RustCut's own priority.
    pub nice: u8,
    /// The idle I/O class: ffmpeg reads and writes only when nothing else needs the drive.
    pub idle_io: bool,
}

impl Default for RenderPriority {
    fn default() -> Self {
        Self {
            nice: 10,
            idle_io: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProcessGroup(Pid);

impl ProcessGroup {
    /// The group led by the child with this process id.
    pub(super) fn led_by(child_id: u32) -> Option<Self> {
        i32::try_from(child_id).ok().and_then(Pid::from_raw).map(Self)
    }

    pub(super) fn signal(self, signal: GroupSignal) -> std::io::Result<()> {
        let signal = match signal {
            GroupSignal::Stop => Signal::STOP,
            GroupSignal::Continue => Signal::CONT,
            GroupSignal::Terminate => Signal::TERM,
            GroupSignal::Kill => Signal::KILL,
        };
        kill_process_group(self.0, signal).map_err(std::io::Error::from)
    }

    /// Lowers the CPU and I/O priority of every thread in the group; threads started later inherit it. Failures are
    /// returned as text for the job log (the render goes on at normal priority).
    pub(super) fn lower_priority(self, priority: RenderPriority) -> Vec<String> {
        let mut problems = Vec::new();
        // Lowering the niceness below our own would need privileges; a nicer RustCut keeps its value for ffmpeg too.
        let own = getpriority_process(None).unwrap_or(0);
        let nice = i32::from(priority.nice.min(19)).max(own);
        if nice != 0
            && let Err(e) = setpriority_pgrp(Some(self.0), nice)
        {
            problems.push(format!("niceness {nice} not set: {e}"));
        }
        if priority.idle_io {
            let group = ioprio::Pid::from_raw(self.0.as_raw_nonzero().get());
            let idle = ioprio::Priority::new(ioprio::Class::Idle);
            if let Err(e) = ioprio::set_priority(ioprio::Target::ProcessGroup(group), idle) {
                problems.push(format!("idle I/O priority not set: {e}"));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::*;

    /// Reads `/proc/<pid>/stat`: (state, niceness).
    fn stat(pid: u32) -> (char, i32) {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let after = text.rsplit_once(')').unwrap().1;
        let fields: Vec<&str> = after.split_whitespace().collect();
        // Field 3 is the state, field 19 the niceness (fields counted from 1 incl. pid and comm).
        (fields[0].chars().next().unwrap(), fields[16].parse().unwrap())
    }

    fn wait_for_state(pid: u32, wanted: char) -> bool {
        (0..100).any(|_| {
            if stat(pid).0 == wanted {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
            false
        })
    }

    #[test]
    fn stop_continue_and_priority_reach_the_group() {
        let mut child = std::os::unix::process::CommandExt::process_group(&mut Command::new("sleep"), 0)
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let group = ProcessGroup::led_by(child.id()).unwrap();
        let problems = group.lower_priority(RenderPriority { nice: 7, idle_io: true });
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(stat(child.id()).1, 7.max(getpriority_process(None).unwrap()));
        let io = ioprio::get_priority(ioprio::Target::Process(ioprio::Pid::from_raw(
            i32::try_from(child.id()).unwrap(),
        )))
        .unwrap();
        assert_eq!(io.class(), Some(ioprio::Class::Idle));

        group.signal(GroupSignal::Stop).unwrap();
        assert!(wait_for_state(child.id(), 'T'), "stopped");
        group.signal(GroupSignal::Continue).unwrap();
        assert!(wait_for_state(child.id(), 'S'), "running again");
        group.signal(GroupSignal::Kill).unwrap();
        assert!(!child.wait().unwrap().success());
        assert_eq!(ProcessGroup::led_by(0), None);
    }
}
