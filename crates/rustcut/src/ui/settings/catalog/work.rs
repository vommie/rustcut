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
        ],
    },
    Card {
        title: "When the queue is done",
        description: "The queue panel changes this for the current run; this is the choice a fresh start begins with.",
        fields: &[
            choice!(
                "queue.when_done",
                "Then",
                "What happens after the last waiting job: nothing, a notification, quit RustCut, suspend or power \
                 off the computer.",
                queue.when_done,
                [
                    (WhenDone::Nothing, "Nothing"),
                    (WhenDone::Notify, "Notify"),
                    (WhenDone::Quit, "Quit RustCut"),
                    (WhenDone::Suspend, "Suspend"),
                    (WhenDone::PowerOff, "Power off")
                ]
            )
            .next_start(),
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
                "The rustview binary. Empty = rustview from PATH.",
                rustview.binary,
                "rustview (from PATH)",
                folder: false
            )
            .under(link_on),
            number!(
                "rustview.timeout_secs",
                "Time limit per call",
                "A call that takes longer is stopped and tried again later.",
                rustview.timeout_secs: u8,
                1..=120,
                1,
                "s"
            )
            .under(link_on),
            Field::button(
                "rustview.test",
                "Test the connection",
                "Asks RustView for its tags now; the Tag & Rate panel shows the answer.",
                "Test"
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
        ],
    },
];
