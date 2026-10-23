//! Entry point of the `rustcut` binary; everything happens in `rustcut::app`.

use std::process::ExitCode;

fn main() -> ExitCode {
    rustcut::app::main()
}
