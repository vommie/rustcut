//! Target file names: templates (minijinja), the counter suffix, sanitizing, free "(n)" variants, path identity, and
//! the conflict check of the export flow.

mod conflict;
mod template;
mod variant;

pub use conflict::{
    CheckInput, Conflicts, ExistingFile, FolderScan, MAX_SCANNED, MAX_SIMILAR, QueuedTarget, Taken, TypedNameError,
    check, existing_file, free_variant, next_free_counter, scan_folder, typed_stem,
};
pub use template::{DEFAULT_TEMPLATE, NameContext, NameError, render_stem, used_variables};
pub use variant::{MAX_STEM_BYTES, file_name, next_free_variant, same_path, sanitize_stem};
