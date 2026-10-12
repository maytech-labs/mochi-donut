//! Repo automation, run with `cargo xtask <task>`. Tasks are Rust so the repo
//! needs nothing but Cargo (docs/decisions/0002-rust-and-cargo.md).

use std::process::{Command, ExitCode};

const USAGE: &str = "\
cargo xtask <task>

Tasks:
  check   Everything CI runs: fmt, clippy, tests
";

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("check") => check(),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Runs the same steps as CI, stopping at the first failure.
fn check() -> ExitCode {
    let steps: &[&[&str]] = &[
        &["fmt", "--all", "--check"],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace"],
    ];
    for args in steps {
        eprintln!("==> cargo {}", args.join(" "));
        let ok = Command::new(env!("CARGO"))
            .args(*args)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("cargo {} failed", args[0]);
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
