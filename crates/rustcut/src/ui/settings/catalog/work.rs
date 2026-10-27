//! Queue (processing, when done, the render process) and RustView (the link, tags and rating).

use rustcut_core::settings::{WhenDone, WriteTiming};

use super::Card;
use crate::ui::settings::field::{Field, choice, number, path_field, switch};

pub static QUEUE: &[Card] = &[
    Card {
        title: "Processing",
        description: "",
        fields: &[
            number!(
                "queue.workers",
                "Jobs at the same time",
                "1 renders one job after the other. More jobs share the CPU; each one is slower, the queue may end \
                 sooner on many cores.",
                queue.workers: u8,
                1..=16,
                1,
                ""
            ),
            switch!(
                "queue.pause_on_start",
                "Start paused",
                "When jobs are waiting at start, the queue starts paused instead of rendering at once.",
                queue.pause_on_start
            )
            .next_start(),
            switch!(
                "queue.follow_new_jobs",
                "Follow new jobs",
                "While the queue list stands at the bottom it scrolls along with each new job. Scrolled up, it stays \
                 where it is until it is scrolled to the bottom again.",
                queue.follow_new_jobs
            ),
        ],
    },
    Card {
        title: "When the queue is done",
        description: "The same choice as the queue panel's \"When the queue finishes\" button: a change in either \
                      place shows in the other. After quitting, suspending or powering off it goes back to \"Do \
                      Nothing\", so it never repeats by surprise.",
        fields: &[
            choice!(
                "queue.when_done",
                "Then",
                "What happens after the last waiting job: nothing, a notification, quit RustCut, suspend or power \
                 off the computer.",
                queue.when_done,
                [
                    (WhenDone::Nothing, "Do Nothing"),
                    (WhenDone::Notify, "Send Notification"),
                    (WhenDone::Quit, "Quit App"),
                    (WhenDone::Suspend, "Suspend Device"),
                    (WhenDone::PowerOff, "Power Off Device")
                ]
            ),
            number!(
                "queue.when_done_countdown_secs",
                "Countdown",
                "Quit, suspend and power off wait this long and can be aborted meanwhile.",
                queue.when_done_countdown_secs: u8,
                0..=120,
                5,
                "s"
            ),
        ],
    },
    Card {
        title: "Render process",
        description: "ffmpeg runs at a low priority, so the desktop and the preview stay responsive.",
        fields: &[
            number!(
                "render.nice",
                "CPU priority",
                "Niceness of ffmpeg: 0 = normal, 19 = lowest. Never above RustCut's own priority.",
                render.nice: u8,
                0..=19,
                1,
                ""
            ),
            switch!(
                "render.idle_io",
                "Idle disk priority",
                "ffmpeg reads and writes only while no other program needs the disk.",
                render.idle_io
            ),
            number!(
                "render.stall_timeout_secs",
                "Stop a stalled render after",
                "A render that makes no progress this long is stopped and fails. 0 = never.",
                render.stall_timeout_secs: u32,
                0..=86400,
                30,
                "s"
            ),
        ],
    },
];

fn link_on(s: &rustcut_core::settings::Settings) -> bool {
    s.rustview.enabled
}

pub static RUSTVIEW: &[Card] = &[
    Card {
        title: "Connection",
        description: "RustCut tags and rates its outputs in your RustView library through RustView's command line \
                      (rustview api). RustView does not have to run.",
        fields: &[
            switch!(
                "rustview.enabled",
                "Use RustView",
                "Off: no tags or rating, no known-source check in RustView's library; jobs still render.",
                rustview.enabled
            ),
            path_field!(
                "rustview.binary",
                "Program",
                "Optional: leave it empty when the command rustview works in a terminal (RustCut finds it in PATH). \
                 Else type another command name or the full path of the rustview program, or choose the file.",
                rustview.binary,
                "rustview (from PATH)",
                folder: false
            )
            .under(link_on),
            number!(
                "rustview.timeout_secs",
                "Time limit per call",
                "How long one call to RustView may take. A call that takes longer or does not reach RustView is \
                 tried again while RustCut runs: after 2 s, then twice as long each time, at most every 2 minutes. \
                 Tags and ratings that did not arrive stay with their jobs and are sent again at the next start; \
                 the Tag & Rate panel shows while RustView cannot be reached, a message when RustView refuses a job.",
                rustview.timeout_secs: u8,
                1..=120,
                1,
                "s"
            )
            .under(link_on),
            Field::button(
                "rustview.test",
                "Test the connection",
                "Asks RustView for its tags now (the Tag & Rate panel loads them again).",
                "Test",
            )
            .under(link_on),
        ],
    },
    Card {
        title: "Tags and rating",
        description: "",
        fields: &[
            choice!(
                "rustview.write_timing",
                "Write them",
                "When the job is queued (RustView shows a pending entry until the file exists), or only after the \
                 render was verified.",
                rustview.write_timing,
                [
                    (WriteTiming::AtEnqueue, "When the job is queued"),
                    (WriteTiming::AfterRender, "After the render")
                ]
            )
            .under(link_on),
            number!(
                "rustview.tag_history_size",
                "Tag history",
                "How many recently used tags the Tag & Rate panel keeps.",
                rustview.tag_history_size: u8,
                1..=100,
                1,
                "tags"
            ),
            switch!(
                "rustview.compact_tree",
                "Compact tag tree",
                "Dense rows that share one line, in PyCutRv's font size (Compact Mode). The tree's context menu \
                 switches it too.",
                rustview.compact_tree
            ),
        ],
    },
    Card {
        title: "Source hashes",
        description: "RustView recognizes a source by its MD5. RustCut takes it from RustView's library or from a sidecar \
                      file next to the video, and reads the whole video only when neither has it.",
        fields: &[switch!(
            "hashing.write_sidecar",
            "Write a sidecar",
            "When RustCut had to read a video for its MD5, it writes \".<name>.md5\" next to it (md5sum's format), so \
             neither RustCut nor other programs read it again. An existing sidecar is never changed.",
            hashing.write_sidecar
        )],
    },
];
