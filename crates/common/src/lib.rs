//! Conventions every mochi-donut tool shares: output format, exit codes, and
//! the shape of `--json` output (docs/design.md, "Shared conventions").

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
    let message =
        format!("`{tool} {command}` isn't built yet. See docs/design.md for what it will do.");
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
