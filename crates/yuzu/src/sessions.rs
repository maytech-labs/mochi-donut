//! `yuzu sessions`: how agents used given tools, from their transcripts on
//! this machine, and what yuzu's hook asked or denied. Tells whether a tool
//! is used unprompted, which decides whether it earns its place
//! (docs/decisions/0025-yuzu-sessions.md).

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::env::Ctx;
use crate::shell;

#[derive(Debug, Serialize)]
pub struct Report {
    /// Entries at or after this time count.
    pub since: String,
    pub transcript_dirs: Vec<String>,
    pub sessions: usize,
    pub tools: Vec<ToolUse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook: Option<HookSummary>,
}

#[derive(Debug, Serialize)]
pub struct ToolUse {
    pub tool: String,
    pub sessions_using: usize,
    pub calls: usize,
    /// Calls made before the user mentioned the tool in that session.
    pub unprompted_calls: usize,
    pub unprompted_sessions: usize,
    /// Calls by subcommand, or `mcp:<tool>` for MCP calls.
    pub by_command: BTreeMap<String, usize>,
    /// Calls by the project (working directory's name) they ran in.
    pub by_project: BTreeMap<String, usize>,
    pub examples: Vec<String>,
}

/// Where the hook's decision log lives: `$XDG_STATE_HOME/yuzu`, else
/// `~/.local/state/yuzu`.
pub fn log_path(ctx: &Ctx) -> PathBuf {
    let base = ctx
        .var("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(|| ctx.home.join(".local/state"), PathBuf::from);
    base.join("yuzu").join("decisions.jsonl")
}

/// One ask or deny from the hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logged {
    pub at: String,
    pub decision: String,
    pub command: String,
    pub cwd: String,
    pub reason: String,
}

/// Appends a hook decision. Failing to log never changes the decision.
pub fn log_decision(ctx: &Ctx, entry: &Logged) {
    if ctx.var("YUZU_NO_LOG").is_some_and(|v| !v.is_empty()) {
        return;
    }
    let path = log_path(ctx);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(line) = serde_json::to_string(entry)
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

#[derive(Debug, Serialize)]
pub struct HookSummary {
    pub log: String,
    pub asks: usize,
    pub denies: usize,
    /// The most recent, newest last, to review for wrong blocks.
    pub recent: Vec<Logged>,
}

fn hook_summary(ctx: &Ctx, since: &str) -> Option<HookSummary> {
    let path = log_path(ctx);
    let text = std::fs::read_to_string(&path).ok()?;
    let entries: Vec<Logged> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Logged>(l).ok())
        .filter(|e| e.at.as_str() >= since)
        .collect();
    Some(HookSummary {
        log: path.display().to_string(),
        asks: entries.iter().filter(|e| e.decision == "ask").count(),
        denies: entries.iter().filter(|e| e.decision == "deny").count(),
        recent: entries.iter().rev().take(10).rev().cloned().collect(),
    })
}

/// Claude Code's transcript folders: `projects/` under `$CLAUDE_CONFIG_DIR`
/// and under every `~/.claude*` directory.
pub fn transcript_dirs(ctx: &Ctx) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = ctx.var("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(d).join("projects"));
    }
    if let Ok(entries) = std::fs::read_dir(&ctx.home) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".claude"))
            .map(|e| e.path().join("projects"))
            .collect();
        found.sort();
        dirs.extend(found);
    }
    // `$CLAUDE_CONFIG_DIR` is often one of the `~/.claude*` folders spelled
    // differently; compare real paths so no session counts twice.
    let mut dirs: Vec<PathBuf> = dirs
        .into_iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Scans every session file under `dirs` for calls to `tools`.
pub fn report(
    ctx: &Ctx,
    dirs: &[PathBuf],
    tools: &[String],
    since: &str,
    project: Option<&Path>,
    exclude: &[PathBuf],
) -> Report {
    let mut uses: Vec<ToolUse> = tools
        .iter()
        .map(|t| ToolUse {
            tool: t.clone(),
            sessions_using: 0,
            calls: 0,
            unprompted_calls: 0,
            unprompted_sessions: 0,
            by_command: BTreeMap::new(),
            by_project: BTreeMap::new(),
            examples: Vec::new(),
        })
        .collect();
    let mut sessions = 0;
    for dir in dirs {
        for file in session_files(dir) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let calls = scan(&text, tools, since, project, exclude);
            let Some(calls) = calls else { continue };
            sessions += 1;
            for (u, tool_calls) in uses.iter_mut().zip(calls) {
                if tool_calls.is_empty() {
                    continue;
                }
                u.sessions_using += 1;
                u.calls += tool_calls.len();
                let unprompted = tool_calls.iter().filter(|c| !c.prompted).count();
                u.unprompted_calls += unprompted;
                u.unprompted_sessions += usize::from(unprompted > 0);
                for c in tool_calls {
                    *u.by_command.entry(c.command.clone()).or_insert(0) += 1;
                    *u.by_project.entry(c.project.clone()).or_insert(0) += 1;
                    if u.examples.len() < 3 && !u.examples.contains(&c.line) {
                        u.examples.push(c.line.clone());
                    }
                }
            }
        }
    }
    Report {
        since: since.to_owned(),
        transcript_dirs: dirs.iter().map(|d| d.display().to_string()).collect(),
        sessions,
        tools: uses,
        hook: hook_summary(ctx, since),
    }
}

fn session_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(dir) else {
        return out;
    };
    for p in projects.flatten() {
        if let Ok(files) = std::fs::read_dir(p.path()) {
            out.extend(
                files
                    .flatten()
                    .map(|f| f.path())
                    .filter(|f| f.extension().is_some_and(|e| e == "jsonl")),
            );
        }
    }
    out.sort();
    out
}

#[derive(Debug, Clone)]
struct Call {
    command: String,
    project: String,
    line: String,
    prompted: bool,
}

/// The calls to each of `tools` in one session, or `None` if the session
/// has nothing in the window or outside `project`.
fn scan(
    text: &str,
    tools: &[String],
    since: &str,
    project: Option<&Path>,
    exclude: &[PathBuf],
) -> Option<Vec<Vec<Call>>> {
    let mut calls: Vec<Vec<Call>> = vec![Vec::new(); tools.len()];
    let mut mentioned: HashSet<usize> = HashSet::new();
    let mut any = false;
    for line in text.lines() {
        let Ok(d) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = d.get("timestamp").and_then(Value::as_str).unwrap_or("");
        if !at.is_empty() && at < since {
            continue;
        }
        let cwd = d.get("cwd").and_then(Value::as_str).unwrap_or("");
        if project.is_some_and(|p| !Path::new(cwd).starts_with(p))
            || exclude.iter().any(|x| Path::new(cwd).starts_with(x))
        {
            continue;
        }
        let project_name = Path::new(cwd).file_name().map_or_else(
            || "(unknown)".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let kind = d.get("type").and_then(Value::as_str).unwrap_or("");
        let content = d.pointer("/message/content");
        match kind {
            "user" if d.get("isMeta").and_then(Value::as_bool) != Some(true) => {
                any = true;
                for t in user_texts(content) {
                    let lower = t.to_lowercase();
                    for (i, tool) in tools.iter().enumerate() {
                        if lower.contains(&tool.to_lowercase()) {
                            mentioned.insert(i);
                        }
                    }
                }
            }
            "assistant" => {
                any = true;
                let Some(items) = content.and_then(Value::as_array) else {
                    continue;
                };
                for item in items
                    .iter()
                    .filter(|i| i.get("type").and_then(Value::as_str) == Some("tool_use"))
                {
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    for (i, tool) in tools.iter().enumerate() {
                        let found = if name == "Bash" {
                            let cmd = item
                                .pointer("/input/command")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            cli_calls(cmd, tool)
                        } else {
                            mcp_call(name, tool).into_iter().collect()
                        };
                        for (command, line) in found {
                            calls[i].push(Call {
                                command,
                                project: project_name.clone(),
                                line,
                                prompted: mentioned.contains(&i),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    any.then_some(calls)
}

/// The text a person typed: a string, or `text` items, but not tool
/// results or injected `<system-reminder>`-style blocks.
fn user_texts(content: Option<&Value>) -> Vec<&str> {
    let typed = |s: &&str| !s.trim_start().starts_with('<');
    match content {
        Some(Value::String(s)) => vec![s.as_str()].into_iter().filter(typed).collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|i| i.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .filter(typed)
            .collect(),
        _ => Vec::new(),
    }
}

/// Runs of `tool` in a shell line: its subcommand and the simple command.
/// Uses yuzu's own splitter, so a name in a heredoc or a path isn't a call.
fn cli_calls(src: &str, tool: &str) -> Vec<(String, String)> {
    const WRAPPERS: &[&str] = &[
        "env", "sudo", "time", "nice", "timeout", "xargs", "command", "exec",
    ];
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_owned();
    shell::commands(src)
        .into_iter()
        .filter_map(|words| {
            let start = words.iter().position(|w| {
                !WRAPPERS.contains(&base(w).as_str())
                    && !w.contains('=')
                    && !w.chars().all(|c| c.is_ascii_digit())
            })?;
            (base(&words[start]) == tool).then(|| {
                let sub = words[start + 1..]
                    .iter()
                    .find(|w| {
                        // The first plain word: skips flags and path-like flag values.
                        !w.starts_with('-')
                            && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    })
                    .cloned()
                    .unwrap_or_else(|| "(none)".to_owned());
                (sub, words[start..].join(" "))
            })
        })
        .collect()
}

/// `mcp__plugin_mochi-donut_matcha__census` is matcha's `census`.
fn mcp_call(name: &str, tool: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, mcp_tool) = rest.rsplit_once("__")?;
    let ours = server == tool
        || server.ends_with(&format!("_{tool}"))
        || server.ends_with(&format!("-{tool}"));
    ours.then(|| (format!("mcp:{mcp_tool}"), name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_cli_calls_but_not_mentions() {
        let tools = |s: &str| cli_calls(s, "matcha");
        assert_eq!(
            tools("cd repo && ./target/release/matcha drift --in docs | tail -1"),
            [(
                "drift".to_owned(),
                "./target/release/matcha drift --in docs".to_owned()
            )]
        );
        assert_eq!(tools("env FOO=1 matcha --json census x")[0].0, "census");
        assert_eq!(tools("matcha --repo ../anchor census forms")[0].0, "census");
        assert!(tools("cat crates/matcha/src/main.rs").is_empty());
        assert!(tools("python3 - <<'EOF'\nmatcha drift\nEOF").is_empty());
        assert!(tools("echo matcha drift").is_empty());
    }

    #[test]
    fn finds_mcp_calls_by_server_name() {
        assert_eq!(
            mcp_call("mcp__plugin_mochi-donut_matcha__census", "matcha"),
            Some((
                "mcp:census".to_owned(),
                "mcp__plugin_mochi-donut_matcha__census".to_owned()
            ))
        );
        assert_eq!(
            mcp_call("mcp__matcha__drift", "matcha")
                .map(|c| c.0)
                .as_deref(),
            Some("mcp:drift")
        );
        assert_eq!(
            mcp_call("mcp__plugin_mochi-donut_yuzu__check", "matcha"),
            None
        );
        assert_eq!(mcp_call("Bash", "matcha"), None);
    }

    fn entry(kind: &str, at: &str, content: Value) -> String {
        serde_json::json!({"type": kind, "timestamp": at, "cwd": "/repo", "message": {"content": content}}).to_string()
    }

    fn bash(cmd: &str) -> Value {
        serde_json::json!([{"type": "tool_use", "name": "Bash", "input": {"command": cmd}}])
    }

    #[test]
    fn calls_before_the_user_names_the_tool_are_unprompted() {
        let session = [
            entry(
                "user",
                "2026-10-12T10:00:00Z",
                serde_json::json!("how are forms done here?"),
            ),
            entry(
                "assistant",
                "2026-10-12T10:00:05Z",
                bash("matcha census forms --alt x"),
            ),
            entry(
                "user",
                "2026-10-12T10:01:00Z",
                serde_json::json!([{"type": "text", "text": "now run matcha drift"}]),
            ),
            entry("assistant", "2026-10-12T10:01:05Z", bash("matcha drift")),
            entry(
                "user",
                "2026-10-12T10:02:00Z",
                serde_json::json!("<system-reminder>yuzu</system-reminder>"),
            ),
            entry(
                "assistant",
                "2026-10-12T10:02:05Z",
                bash("yuzu check -- ls"),
            ),
        ]
        .join("\n");
        let tools = vec!["matcha".to_owned(), "yuzu".to_owned()];
        let calls = scan(&session, &tools, "2026-10-01", None, &[]).unwrap();
        let prompted: Vec<bool> = calls[0].iter().map(|c| c.prompted).collect();
        assert_eq!(prompted, [false, true]);
        // An injected reminder isn't the user mentioning yuzu.
        assert!(!calls[1][0].prompted);
        // Outside the window or the project, nothing counts.
        assert!(scan(&session, &tools, "2026-11-01", None, &[]).is_none());
        assert!(
            scan(
                &session,
                &tools,
                "2026-10-01",
                Some(Path::new("/other")),
                &[]
            )
            .is_none()
        );
        assert!(
            scan(
                &session,
                &tools,
                "2026-10-01",
                None,
                &[PathBuf::from("/repo")]
            )
            .is_none()
        );
        assert_eq!(calls[0][0].project, "repo");
    }
}
