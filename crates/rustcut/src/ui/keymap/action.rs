//! The actions a key can trigger, with their ids in `config.toml`, their titles and their default keys (PyCutRv's keys plus
//! NLE-style keys and the filter keys C, Shift+C, P, R, Shift+R; user decisions 2026-10-23), plus T for the probe render.

/// Everything the keymap can bind. The order is the order of the Settings window's list and decides which binding wins
/// when two defaults would collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    Open,
    Save,
    Quit,
    TogglePause,
    ShuttleReverse,
    ShuttleStop,
    ShuttleForward,
    FrameBack,
    FrameForward,
    SeekBack,
    SeekForward,
    SeekBackLong,
    SeekForwardLong,
    PreviousCut,
    NextCut,
    ToggleMute,
    Reload,
    SetStart,
    SetEnd,
    GoToStart,
    GoToEnd,
    AddSection,
    DeleteSection,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    CropEdit,
    AutoCrop,
    ToggleFilterPreview,
    ToggleKeepFilters,
    RotateLeft,
    RotateRight,
    MirrorHorizontal,
    MirrorVertical,
    ResetFilters,
    ProbeRender,
    QueueMoveUp,
    QueueMoveDown,
    QueueMoveTop,
    QueueMoveBottom,
    QueueSelectAll,
}

impl Action {
    pub const ALL: [Self; 41] = [
        Self::Open,
        Self::Save,
        Self::Quit,
        Self::TogglePause,
        Self::ShuttleReverse,
        Self::ShuttleStop,
        Self::ShuttleForward,
        Self::FrameBack,
        Self::FrameForward,
        Self::SeekBack,
        Self::SeekForward,
        Self::SeekBackLong,
        Self::SeekForwardLong,
        Self::PreviousCut,
        Self::NextCut,
        Self::ToggleMute,
        Self::Reload,
        Self::SetStart,
        Self::SetEnd,
        Self::GoToStart,
        Self::GoToEnd,
        Self::AddSection,
        Self::DeleteSection,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ZoomFit,
        Self::CropEdit,
        Self::AutoCrop,
        Self::ToggleFilterPreview,
        Self::ToggleKeepFilters,
        Self::RotateLeft,
        Self::RotateRight,
        Self::MirrorHorizontal,
        Self::MirrorVertical,
        Self::ResetFilters,
        Self::ProbeRender,
        Self::QueueMoveUp,
        Self::QueueMoveDown,
        Self::QueueMoveTop,
        Self::QueueMoveBottom,
        Self::QueueSelectAll,
    ];

    /// The key in `config.toml`'s `[keymap]` table.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Save => "save",
            Self::Quit => "quit",
            Self::TogglePause => "toggle_pause",
            Self::ShuttleReverse => "shuttle_reverse",
            Self::ShuttleStop => "shuttle_stop",
            Self::ShuttleForward => "shuttle_forward",
            Self::FrameBack => "frame_back",
            Self::FrameForward => "frame_forward",
            Self::SeekBack => "seek_back",
            Self::SeekForward => "seek_forward",
            Self::SeekBackLong => "seek_back_long",
            Self::SeekForwardLong => "seek_forward_long",
            Self::PreviousCut => "previous_cut",
            Self::NextCut => "next_cut",
            Self::ToggleMute => "toggle_mute",
            Self::Reload => "reload",
            Self::SetStart => "set_start",
            Self::SetEnd => "set_end",
            Self::GoToStart => "go_to_start",
            Self::GoToEnd => "go_to_end",
            Self::AddSection => "add_section",
            Self::DeleteSection => "delete_section",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ZoomFit => "zoom_fit",
            Self::CropEdit => "crop_edit",
            Self::AutoCrop => "auto_crop",
            Self::ToggleFilterPreview => "toggle_filter_preview",
            Self::ToggleKeepFilters => "toggle_keep_filters",
            Self::RotateLeft => "rotate_left",
            Self::RotateRight => "rotate_right",
            Self::MirrorHorizontal => "mirror_horizontal",
            Self::MirrorVertical => "mirror_vertical",
            Self::ResetFilters => "reset_filters",
            Self::ProbeRender => "probe_render",
            Self::QueueMoveUp => "queue_move_up",
            Self::QueueMoveDown => "queue_move_down",
            Self::QueueMoveTop => "queue_move_top",
            Self::QueueMoveBottom => "queue_move_bottom",
            Self::QueueSelectAll => "queue_select_all",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The name shown in the Settings window (M4.2).
    pub const fn title(self) -> &'static str {
        match self {
            Self::Open => "Open file",
            Self::Save => "Save job",
            Self::Quit => "Quit",
            Self::TogglePause => "Play / pause",
            Self::ShuttleReverse => "Play backward (again: faster)",
            Self::ShuttleStop => "Stop",
            Self::ShuttleForward => "Play forward (again: faster)",
            Self::FrameBack => "Previous frame",
            Self::FrameForward => "Next frame",
            Self::SeekBack => "Seek back",
            Self::SeekForward => "Seek forward",
            Self::SeekBackLong => "Seek back (long step)",
            Self::SeekForwardLong => "Seek forward (long step)",
            Self::PreviousCut => "Previous cut point",
            Self::NextCut => "Next cut point",
            Self::ToggleMute => "Mute / unmute",
            Self::Reload => "Reload video",
            Self::SetStart => "Set section start",
            Self::SetEnd => "Set section end",
            Self::GoToStart => "Go to section start",
            Self::GoToEnd => "Go to section end",
            Self::AddSection => "Add section",
            Self::DeleteSection => "Delete selected section",
            Self::ZoomIn => "Zoom timeline in",
            Self::ZoomOut => "Zoom timeline out",
            Self::ZoomFit => "Fit timeline",
            Self::CropEdit => "Edit the crop on the video",
            Self::AutoCrop => "Detect black borders (auto-crop)",
            Self::ToggleFilterPreview => "Show / hide the filters in the player",
            Self::ToggleKeepFilters => "Keep the filters for the next video",
            Self::RotateLeft => "Rotate left",
            Self::RotateRight => "Rotate right",
            Self::MirrorHorizontal => "Mirror left and right",
            Self::MirrorVertical => "Mirror top and bottom",
            Self::ResetFilters => "Reset all filters",
            Self::ProbeRender => "Probe render around the playhead",
            Self::QueueMoveUp => "Move the selected jobs up",
            Self::QueueMoveDown => "Move the selected jobs down",
            Self::QueueMoveTop => "Move the selected jobs to the top",
            Self::QueueMoveBottom => "Move the selected jobs to the bottom",
            Self::QueueSelectAll => "Select every job shown in the queue",
        }
    }

    /// The keys a fresh profile has, in Slint key names (`Control+S`, `Home`, `Plus`).
    pub const fn default_keys(self) -> &'static [&'static str] {
        match self {
            Self::Open => &["Control+O"],
            Self::Save => &["Control+S", "F9"],
            Self::Quit => &["Control+Q"],
            Self::TogglePause => &["Space"],
            Self::ShuttleReverse => &["J"],
            Self::ShuttleStop => &["K"],
            Self::ShuttleForward => &["L"],
            Self::FrameBack => &["Comma", "PageUp"],
            Self::FrameForward => &["Period", "PageDown"],
            Self::SeekBack => &["LeftArrow"],
            Self::SeekForward => &["RightArrow"],
            Self::SeekBackLong => &["Shift+LeftArrow"],
            Self::SeekForwardLong => &["Shift+RightArrow"],
            Self::PreviousCut => &["UpArrow"],
            Self::NextCut => &["DownArrow"],
            Self::ToggleMute => &["M"],
            Self::Reload => &["F5"],
            Self::SetStart => &["I", "Home"],
            Self::SetEnd => &["O", "End"],
            Self::GoToStart => &["Shift+I"],
            Self::GoToEnd => &["Shift+O"],
            Self::AddSection => &["Plus", "ScrollLock"],
            Self::DeleteSection => &["Delete"],
            Self::ZoomIn => &["Control+Plus"],
            Self::ZoomOut => &["Control+HyphenMinus"],
            Self::ZoomFit => &["Control+Digit0"],
            // Filter keys (user decision 2026-10-23; PyCutRv had none).
            Self::CropEdit => &["C"],
            Self::AutoCrop => &["Shift+C"],
            Self::ToggleFilterPreview => &["P"],
            Self::RotateLeft => &["Shift+R"],
            Self::RotateRight => &["R"],
            // A test render (M2.5); T is free in PyCutRv's and the NLE keys.
            Self::ProbeRender => &["T"],
            // The queue (M4.1, HANDOFF 8.Q): Ctrl with the keys that move elsewhere.
            Self::QueueMoveUp => &["Control+UpArrow"],
            Self::QueueMoveDown => &["Control+DownArrow"],
            Self::QueueMoveTop => &["Control+Home"],
            Self::QueueMoveBottom => &["Control+End"],
            Self::QueueSelectAll => &["Control+A"],
            Self::ToggleKeepFilters | Self::MirrorHorizontal | Self::MirrorVertical | Self::ResetFilters => &[],
        }
    }

    /// Whether the action works on the source's timeline, marks or sections (refused while the probe render plays).
    pub const fn edits_the_source(self) -> bool {
        matches!(
            self,
            Self::PreviousCut
                | Self::NextCut
                | Self::SetStart
                | Self::SetEnd
                | Self::GoToStart
                | Self::GoToEnd
                | Self::AddSection
                | Self::DeleteSection
                | Self::ZoomIn
                | Self::ZoomOut
                | Self::ZoomFit
                | Self::CropEdit
                | Self::AutoCrop
        )
    }

    /// Whether a held key repeats the action (steps and seeks do; toggles and edits would flip or duplicate).
    pub const fn repeats(self) -> bool {
        matches!(
            self,
            Self::FrameBack
                | Self::FrameForward
                | Self::SeekBack
                | Self::SeekForward
                | Self::SeekBackLong
                | Self::SeekForwardLong
                | Self::PreviousCut
                | Self::NextCut
                | Self::ZoomIn
                | Self::ZoomOut
                | Self::QueueMoveUp
                | Self::QueueMoveDown
        )
    }
}
