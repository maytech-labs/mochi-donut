//! Commands that would change yuzu's own config. The environments file
//! decides what agents may change without asking, so an agent that edits it
//! could approve itself. Every write to it asks the user
//! (docs/decisions/0036-yuzu-guards-its-config.md).
//!
//! This reads the command line as text as well as words, because the shell
//! splitter drops redirections. It errs toward asking: anything that names
//! the file and isn't a known reader counts as a write.

use std::path::Path;

use crate::config::Kind;
use crate::shell;
use crate::targets::{Access, Target};

/// Programs that only read the files they're given.
const READERS: &[&str] = &[
    "cat", "less", "more", "head", "tail", "grep", "egrep", "fgrep", "rg", "bat", "wc", "diff",
    "ls", "stat", "file", "test", "[",
];

/// A target for each simple command in `src` that would read or change the
/// config at `path`, and for each redirection into it.
pub fn find(src: &str, path: &Path) -> Vec<Target> {
    let names = Names::new(path);
    let mut out = Vec::new();
    for words in shell::commands(src) {
        let access = if is_env_add(&words) {
            Some(Access::Write)
        } else if words.iter().any(|w| names.mentions(w)) {
            let program = program(&words);
            Some(if READERS.contains(&program) {
                Access::Read
            } else {
                Access::Write
            })
        } else {
            None
        };
        if let Some(access) = access {
            out.push(target(path, access, words.join(" ")));
        }
    }
    for redirect in redirect_targets(src) {
        if names.mentions(&redirect) {
            out.push(target(path, Access::Write, format!("> {redirect}")));
        }
    }
    out
}

fn target(path: &Path, access: Access, command: String) -> Target {
    Target {
        kind: Kind::YuzuConfig,
        name: path.display().to_string(),
        access,
        command,
        source: None,
    }
}

/// The program a simple command runs, after `NAME=value` prefixes.
fn program(words: &[String]) -> &str {
    let first = words
        .iter()
        .find(|w| !is_assignment(w))
        .map_or("", String::as_str);
    first.rsplit('/').next().unwrap_or(first)
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// `yuzu env add`, which writes the config by design.
fn is_env_add(words: &[String]) -> bool {
    let start = words.iter().position(|w| !is_assignment(w));
    let Some(rest) = start.map(|i| &words[i..]) else {
        return false;
    };
    program(rest) == "yuzu"
        && rest
            .iter()
            .skip(1)
            .filter(|w| !w.starts_with('-'))
            .take(2)
            .eq(["env", "add"].iter())
}

/// The ways a command line can name the config.
struct Names {
    full: String,
    file: String,
}

impl Names {
    fn new(path: &Path) -> Names {
        Names {
            full: path.display().to_string(),
            file: path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    /// Whether `word` names the file, its directory, or the variable that
    /// moves it, however the path is spelled.
    fn mentions(&self, word: &str) -> bool {
        (!self.file.is_empty() && word.contains(&self.file))
            || word.contains(&self.full)
            || word.contains(".config/yuzu")
            || word.contains("CONFIG_HOME/yuzu")
            || word.contains("CONFIG_HOME}/yuzu")
            || word.contains("YUZU_CONFIG")
    }
}

/// The word after each `>` in `src`, with quotes removed. Rough on purpose:
/// a `>` inside a quoted string can only add an ask, never remove one.
fn redirect_targets(src: &str) -> Vec<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '>' {
            i += 1;
            continue;
        }
        i += 1;
        while i < chars.len() && (matches!(chars[i], '>' | '|' | '&') || chars[i].is_whitespace()) {
            i += 1;
        }
        let mut word = String::new();
        while i < chars.len()
            && !chars[i].is_whitespace()
            && !matches!(chars[i], ';' | '&' | '|' | '(' | ')' | '<' | '>')
        {
            if !matches!(chars[i], '\'' | '"') {
                word.push(chars[i]);
            }
            i += 1;
        }
        if !word.is_empty() {
            out.push(word);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "/home/me/.config/yuzu/environments.toml";

    fn accesses(src: &str) -> Vec<Access> {
        find(src, Path::new(PATH))
            .into_iter()
            .map(|t| t.access)
            .collect()
    }

    #[test]
    fn edits_are_writes_however_the_path_is_spelled() {
        for src in [
            "sed -i '' 's/a/b/' ~/.config/yuzu/environments.toml",
            "sed -i 's/a/b/' $HOME/.config/yuzu/environments.toml",
            "cp /tmp/x /home/me/.config/yuzu/environments.toml",
            "rm -rf ~/.config/yuzu",
            "rm -rf \"$XDG_CONFIG_HOME/yuzu\"",
            "cd ~/.config/yuzu && vim environments.toml",
            "python3 -c 'open(\"environments.toml\",\"w\")'",
            "bash -c 'echo x >> ~/.config/yuzu/environments.toml'",
        ] {
            assert!(accesses(src).contains(&Access::Write), "{src}");
        }
    }

    #[test]
    fn redirections_into_it_are_writes() {
        assert_eq!(
            accesses("echo '[[env]]' >> ~/.config/yuzu/environments.toml"),
            [Access::Write]
        );
        assert_eq!(
            accesses("cat /tmp/new > \"$HOME/.config/yuzu/environments.toml\""),
            [Access::Write]
        );
    }

    #[test]
    fn reading_it_is_a_read() {
        assert_eq!(
            accesses("cat ~/.config/yuzu/environments.toml"),
            [Access::Read]
        );
        assert_eq!(
            accesses("grep ssh_hosts ~/.config/yuzu/environments.toml 2>/dev/null"),
            [Access::Read]
        );
    }

    #[test]
    fn env_add_is_a_write() {
        assert_eq!(
            accesses("yuzu env add homelab ssh_hosts balin"),
            [Access::Write]
        );
        assert_eq!(
            accesses("/usr/local/bin/yuzu --json env add homelab ssh_hosts balin"),
            [Access::Write]
        );
        assert!(accesses("yuzu check -- 'ssh balin'").is_empty());
    }

    #[test]
    fn other_commands_are_left_alone() {
        assert!(accesses("ssh balin docker ps > out.txt").is_empty());
        assert!(accesses("cargo test -p mochi-yuzu").is_empty());
    }

    #[test]
    fn a_custom_path_is_found_by_name() {
        let found = find(
            "vim /etc/agents/envs.toml",
            Path::new("/etc/agents/envs.toml"),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].access, Access::Write);
    }
}
