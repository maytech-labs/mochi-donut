//! `ube run`: run a plan's checks, and report failures in a few lines each
//! (docs/ube/design.md, "ube run").

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use mochi_common::proc::run;

use crate::parse;
use crate::plan::Check;
use crate::store::CheckResult;

/// Runs one check in `root`/`check.dir`, through `sh -c`, with extra
/// environment variables.
pub fn run_check(
    root: &Path,
    check: &Check,
    timeout: Duration,
    env: &[(&str, &str)],
) -> CheckResult {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", &check.command])
        .current_dir(root.join(&check.dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let start = Instant::now();
    let out = run(cmd, timeout);
    let secs = (start.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    let at = mochi_common::time::now_rfc3339();
    match out {
        Ok(o) => {
            let passed = o.code == Some(0);
            let text = format!("{}\n{}", o.stdout, o.stderr);
            let failures = if passed {
                Vec::new()
            } else {
                parse::failures(&check.command, &text)
            };
            let tail = if !passed && failures.is_empty() {
                let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                lines[lines.len().saturating_sub(15)..]
                    .iter()
                    .map(|l| (*l).to_owned())
                    .collect()
            } else {
                Vec::new()
            };
            CheckResult {
                command: check.command.clone(),
                dir: check.dir.clone(),
                passed,
                exit: o.code,
                secs,
                failures,
                tail,
                at,
            }
        }
        Err(e) => CheckResult {
            command: check.command.clone(),
            dir: check.dir.clone(),
            passed: false,
            exit: None,
            secs,
            failures: Vec::new(),
            tail: vec![e],
            at,
        },
    }
}
