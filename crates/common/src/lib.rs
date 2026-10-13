//! Conventions every mochi-donut tool shares: output format, exit codes, and
//! the shape of `--json` output (docs/matcha/usage.md and docs/yuzu/usage.md, "exit codes").

pub mod mcp;
pub mod paths;
pub mod proc;

use std::io::{self, Write};

use serde::Serialize;

/// Exit codes. Stable: agents and hooks act on them.
pub mod exit {
    /// The command ran and found nothing wrong.
    pub const OK: u8 = 0;
    /// The command ran and a check failed (a missing companion file, a blocked command).
    pub const CHECK_FAILED: u8 = 1;
    /// The command line was wrong. Clap uses this code for usage errors too.
    pub const USAGE: u8 = 2;
    /// The command exists but isn't built yet.
    pub const NOT_IMPLEMENTED: u8 = 3;
}

/// Times, as RFC 3339 in UTC, without a date library.
pub mod time {
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The current time: `2026-10-12T14:58:03Z`.
    pub fn now_rfc3339() -> String {
        rfc3339(unix_now())
    }

    /// Seconds since 1970-01-01 UTC.
    pub fn unix_now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// `secs` since 1970-01-01 UTC as RFC 3339.
    pub fn rfc3339(secs: u64) -> String {
        let days = (secs / 86_400) as i64;
        let rem = secs % 86_400;
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    }
}

/// How a command prints its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Short, plain text for people.
    Text,
    /// One JSON object on stdout, for agents and scripts.
    Json,
}

impl Format {
    pub fn from_json_flag(json: bool) -> Self {
        if json { Format::Json } else { Format::Text }
    }
}

/// The JSON body for a command that isn't built yet.
#[derive(Debug, Serialize)]
pub struct NotImplemented<'a> {
    pub status: &'static str,
    pub tool: &'a str,
    pub command: &'a str,
    pub message: String,
}

/// Reports that `tool command` isn't built yet, in the requested format, and
/// returns the exit code to use.
pub fn not_implemented(format: Format, tool: &str, command: &str) -> u8 {
    let message = format!("`{tool} {command}` isn't built yet. See docs/ for what it will do.");
    match format {
        Format::Text => eprintln!("{message}"),
        Format::Json => print_json(&NotImplemented {
            status: "not_implemented",
            tool,
            command,
            message,
        }),
    }
    exit::NOT_IMPLEMENTED
}

/// Prints `value` as one line of JSON on stdout. Serializing our own types
/// can't fail, so a failure here is a bug and is reported on stderr.
pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(line) => {
            let _ = writeln!(io::stdout(), "{line}");
        }
        Err(e) => eprintln!("internal error: couldn't serialize output: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(time::rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(time::rfc3339(1_791_816_243), "2026-10-12T14:44:03Z");
        assert_eq!(time::rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn json_flag_picks_the_format() {
        assert_eq!(Format::from_json_flag(true), Format::Json);
        assert_eq!(Format::from_json_flag(false), Format::Text);
    }

    #[test]
    fn not_implemented_body_names_the_command() {
        let body = NotImplemented {
            status: "not_implemented",
            tool: "matcha",
            command: "census",
            message: "x".into(),
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["status"], "not_implemented");
        assert_eq!(v["command"], "census");
    }
}
