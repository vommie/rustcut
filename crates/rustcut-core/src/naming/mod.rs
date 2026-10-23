//! Target file names: templates (minijinja), the counter suffix, sanitizing, free "(n)" variants and path identity.

mod template;
mod variant;

pub use template::{DEFAULT_TEMPLATE, NameContext, NameError, render_stem, used_variables};
pub use variant::{MAX_STEM_BYTES, file_name, next_free_variant, same_path, sanitize_stem};
