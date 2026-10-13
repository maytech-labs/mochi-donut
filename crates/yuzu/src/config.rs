//! The user's declaration of which environments are whose, read from
//! `~/.config/yuzu/environments.toml` (docs/design.md, "yuzu check").

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::Ctx;

/// Whose an environment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Mine,
    Work,
    Production,
}

/// What agents may do in an environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Agents {
    /// Nothing: even reads are denied.
    None,
    /// Reads pass; writes need the user's approval.
    ReadOnly,
    /// Anything.
    Write,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub name: String,
    pub class: Class,
    /// Defaults to `write` for `mine` and `read-only` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Agents>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kube_contexts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ssh_hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_remotes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aws_profiles: Vec<String>,
    /// Directories terraform, tofu, terragrunt, or pulumi run in, with `~`
    /// for the home directory: `~/homelab/infra/*`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub infra_dirs: Vec<String>,
    /// Cloud CLIs whose configured account is this environment's: `flyctl`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cli_tools: Vec<String>,
}

impl Environment {
    pub fn agents(&self) -> Agents {
        self.agents.unwrap_or(match self.class {
            Class::Mine => Agents::Write,
            Class::Work | Class::Production => Agents::ReadOnly,
        })
    }

    fn patterns(&self, kind: Kind) -> &[String] {
        match kind {
            Kind::KubeContext => &self.kube_contexts,
            Kind::SshHost => &self.ssh_hosts,
            Kind::GitRemote => &self.git_remotes,
            Kind::AwsProfile => &self.aws_profiles,
            Kind::InfraDir => &self.infra_dirs,
            Kind::CliTool => &self.cli_tools,
        }
    }
}

/// The kinds of target yuzu knows how to find in a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    KubeContext,
    SshHost,
    GitRemote,
    AwsProfile,
    /// The directory an infrastructure-as-code tool runs in.
    InfraDir,
    /// A cloud CLI that acts on whatever account its own config holds.
    CliTool,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::KubeContext => "kube context",
            Kind::SshHost => "ssh host",
            Kind::GitRemote => "git remote",
            Kind::AwsProfile => "aws profile",
            Kind::InfraDir => "infra dir",
            Kind::CliTool => "cloud cli",
        }
    }

    /// The config key that declares targets of this kind.
    pub fn key(self) -> &'static str {
        match self {
            Kind::KubeContext => "kube_contexts",
            Kind::SshHost => "ssh_hosts",
            Kind::GitRemote => "git_remotes",
            Kind::AwsProfile => "aws_profiles",
            Kind::InfraDir => "infra_dirs",
            Kind::CliTool => "cli_tools",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default, rename = "env")]
    envs: Vec<Environment>,
}

/// The loaded declarations. Empty when the file doesn't exist yet.
#[derive(Debug, Default)]
pub struct Config {
    pub path: PathBuf,
    pub found: bool,
    pub envs: Vec<Environment>,
}

impl Config {
    /// `$YUZU_CONFIG`, else `$XDG_CONFIG_HOME/yuzu/environments.toml`, else
    /// `~/.config/yuzu/environments.toml`.
    pub fn path(ctx: &Ctx) -> PathBuf {
        if let Some(p) = ctx.var("YUZU_CONFIG").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        let base = ctx
            .var("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map_or_else(|| ctx.home.join(".config"), PathBuf::from);
        base.join("yuzu").join("environments.toml")
    }

    /// Reads the declarations. A missing file is an empty config; a file that
    /// doesn't parse is an error, so a typo can't quietly allow everything.
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config {
                    path: path.to_path_buf(),
                    ..Config::default()
                });
            }
            Err(e) => return Err(format!("couldn't read {}: {e}", path.display())),
        };
        let file: File = toml::from_str(&text)
            .map_err(|e| format!("{} isn't valid: {}", path.display(), e.message()))?;
        Ok(Config {
            path: path.to_path_buf(),
            found: true,
            envs: file.envs,
        })
    }

    /// The first environment with a pattern that matches `name`. Order in the
    /// file decides ties, so put narrow patterns before broad ones.
    pub fn classify(&self, kind: Kind, name: &str) -> Option<&Environment> {
        self.envs
            .iter()
            .find(|e| e.patterns(kind).iter().any(|p| glob(p, name)))
    }

    /// Names (or patterns) of this kind declared in environments agents may
    /// write to, for suggesting a safe alternative.
    pub fn writable(&self, kind: Kind) -> Vec<&str> {
        self.envs
            .iter()
            .filter(|e| e.agents() == Agents::Write)
            .flat_map(|e| e.patterns(kind))
            .map(String::as_str)
            .collect()
    }
}

/// Shell-style matching: `*` is any run of characters, `?` is one. Case
/// matters.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    // Where the last `*` was, and the text position it's matched up to.
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(&c) if c == '?' || c == t[ti] => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(toml: &str) -> Config {
        let file: File = toml::from_str(toml).unwrap();
        Config {
            found: true,
            envs: file.envs,
            ..Config::default()
        }
    }

    const EXAMPLE: &str = r#"
        [[env]]
        name = "homelab"
        class = "mine"
        kube_contexts = ["k3s-pis"]
        ssh_hosts = ["samwise", "10.0.0.*"]

        [[env]]
        name = "work-prod"
        class = "production"
        kube_contexts = ["*-prod-*"]

        [[env]]
        name = "work"
        class = "work"
        kube_contexts = ["*"]
    "#;

    #[test]
    fn globs() {
        assert!(glob("*-prod-*", "cp1-prod-main-us-east-1"));
        assert!(!glob("*-prod-*", "cp1-nonprod-main-us-east-1"));
        assert!(glob("10.0.0.?", "10.0.0.8"));
        assert!(!glob("10.0.0.?", "10.0.0.27"));
        assert!(glob("*", ""));
        assert!(glob("a*b*c", "aXXbYYc"));
        assert!(!glob("a*b*c", "aXXbYY"));
        assert!(glob("exact", "exact"));
    }

    #[test]
    fn first_matching_environment_wins() {
        let c = config(EXAMPLE);
        let name = |ctx| c.classify(Kind::KubeContext, ctx).map(|e| e.name.as_str());
        assert_eq!(name("k3s-pis"), Some("homelab"));
        assert_eq!(name("anything-else"), Some("work"));
        assert_eq!(name("cp1-prod-main-us-east-1"), Some("work-prod"));
        assert_eq!(
            c.classify(Kind::SshHost, "10.0.0.8").unwrap().name,
            "homelab"
        );
        assert!(c.classify(Kind::SshHost, "bastion").is_none());
    }

    #[test]
    fn agents_default_by_class() {
        let c = config(EXAMPLE);
        assert_eq!(c.envs[0].agents(), Agents::Write);
        assert_eq!(c.envs[1].agents(), Agents::ReadOnly);
        assert_eq!(c.writable(Kind::KubeContext), ["k3s-pis"]);
    }

    #[test]
    fn typos_are_errors_not_silent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("environments.toml");
        std::fs::write(
            &path,
            "[[env]]\nname = \"x\"\nclass = \"mine\"\nkube_context = \"a\"\n",
        )
        .unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(err.contains("kube_context"), "{err}");
        std::fs::write(&path, "[[env]]\nname = \"x\"\nclass = \"ours\"\n").unwrap();
        assert!(Config::load(&path).is_err());
    }

    #[test]
    fn a_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert!(!c.found);
        assert!(c.envs.is_empty());
    }
}
