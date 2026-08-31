//! Runs every fixture under `tests/`.
//!
//! Each fixture becomes one named test, so a report names the file that failed.
//! Set `LARK_BLESS=1` to rewrite every expected file from the actual output.

use std::process::ExitCode;

use libtest_mimic::Arguments;

fn main() -> ExitCode {
    let arguments = Arguments::from_args();
    let root = lark_test::repository_root();

    let trials = match lark_test::trials(&root) {
        Ok(trials) => trials,
        Err(error) => {
            eprintln!("cannot read the fixtures under {}: {error}", root.display());
            return ExitCode::FAILURE;
        }
    };

    if trials.is_empty() {
        eprintln!("no fixture found under {}", root.display());
        return ExitCode::FAILURE;
    }

    libtest_mimic::run(&arguments, trials).exit_code()
}
