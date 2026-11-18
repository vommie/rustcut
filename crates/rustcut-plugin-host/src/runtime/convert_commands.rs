//! Between the generated `command-host` types (API 1.2) and RustCut's own (`crate::commands`).

use super::bindings::with_commands::rustcut::plugin::command_host as wit;
use crate::commands::{
    FileManagerMode, FolderFile, FolderListing, FolderOrder, FolderRequest, FolderStep, HiddenFiles, OrderSource,
};

pub(super) fn request_from_wit(request: wit::FolderRequest) -> FolderRequest {
    FolderRequest {
        file_manager: match request.file_manager {
            wit::FileManagerMode::Off => FileManagerMode::Off,
            wit::FileManagerMode::OpenTab => FileManagerMode::OpenTab,
            wit::FileManagerMode::Running => FileManagerMode::Running,
            wit::FileManagerMode::Always => FileManagerMode::Always,
        },
        hidden: match request.hidden {
            wit::HiddenFiles::Skip => HiddenFiles::Skip,
            wit::HiddenFiles::Show => HiddenFiles::Show,
            wit::HiddenFiles::LikeFileManager => HiddenFiles::LikeFileManager,
        },
        videos_only: request.videos_only,
    }
}

fn file_to_wit(file: FolderFile) -> wit::FolderFile {
    wit::FolderFile {
        path: file.path.to_string_lossy().into_owned(),
        name: file.name,
        has_job: file.has_job,
    }
}

fn order_to_wit(order: FolderOrder) -> wit::FolderOrder {
    wit::FolderOrder {
        source: match order.source {
            OrderSource::FileManager => wit::OrderSource::FileManager,
            OrderSource::NamesOff => wit::OrderSource::NamesOff,
            OrderSource::NamesNotRunning => wit::OrderSource::NamesNotRunning,
            OrderSource::NamesNotOpen => wit::OrderSource::NamesNotOpen,
            OrderSource::NamesUnsupported => wit::OrderSource::NamesUnsupported,
            OrderSource::NamesFailed => wit::OrderSource::NamesFailed,
        },
        role: order.role,
        descending: order.descending,
        detail: order.detail,
    }
}

pub(super) fn view_from_wit(view: wit::FolderView) -> crate::commands::FolderView {
    crate::commands::FolderView {
        request: request_from_wit(view.request),
        progress: view.progress,
        neighbors: view.neighbors,
    }
}

pub(super) fn step_to_wit(step: FolderStep) -> wit::FolderStep {
    wit::FolderStep {
        file: step.file.map(file_to_wit),
        position: step.position,
        total: step.total,
        skipped: step.skipped,
        wrapped: step.wrapped,
        order: order_to_wit(step.order),
    }
}

pub(super) fn listing_to_wit(listing: FolderListing) -> wit::FolderListing {
    wit::FolderListing {
        files: listing.files.into_iter().map(file_to_wit).collect(),
        current: listing.current,
        order: order_to_wit(listing.order),
    }
}
