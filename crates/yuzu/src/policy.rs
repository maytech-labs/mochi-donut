//! Deciding whether a command may run, from its targets and the user's
//! declarations (docs/decisions/0013-yuzu-check.md):
//!
//! - an environment agents may write to: anything goes;
//! - `read-only` (the default for `work` and `production`): reads pass,
//!   writes need the user's approval;
//! - `none`: denied, reads too;
//! - not declared at all: treated like `read-only`, since unknown is risky.

use serde::Serialize;

use crate::config::{Agents, Class, Config};
use crate::targets::{Access, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    /// The user has to approve it.
    Ask,
    Deny,
}

/// One target, classified.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    #[serde(flatten)]
    pub target: Target,
    /// The declared environment it belongs to, if any.
    pub env: Option<String>,
    /// `mine`, `work`, `production`, or `unknown`.
    pub class: &'static str,
    pub agents: Agents,
    pub decision: Decision,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub decision: Decision,
    pub targets: Vec<Verdict>,
    /// What to tell the agent or person, when the decision isn't `allow`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn class_name(class: Option<Class>) -> &'static str {
    match class {
        Some(Class::Mine) => "mine",
        Some(Class::Work) => "work",
        Some(Class::Production) => "production",
        None => "unknown",
    }
}

pub fn classify(config: &Config, target: Target) -> Verdict {
    let env = config.classify(target.kind, &target.name);
    let agents = env.map_or(Agents::ReadOnly, |e| e.agents());
    let decision = match (agents, target.access) {
        (Agents::Write, _) | (Agents::ReadOnly, Access::Read) => Decision::Allow,
        (Agents::ReadOnly, Access::Write) => Decision::Ask,
        (Agents::None, _) => Decision::Deny,
    };
    Verdict {
        env: env.map(|e| e.name.clone()),
        class: class_name(env.map(|e| e.class)),
        agents,
        decision,
        target,
    }
}

pub fn check(config: &Config, targets: Vec<Target>) -> Check {
    let targets: Vec<Verdict> = targets.into_iter().map(|t| classify(config, t)).collect();
    let decision = targets
        .iter()
        .map(|v| v.decision)
        .max()
        .unwrap_or(Decision::Allow);
    let message = (decision != Decision::Allow).then(|| message(config, &targets));
    Check {
        decision,
        targets,
        message,
    }
}

/// Says what's blocked, why, and what to do instead.
fn message(config: &Config, targets: &[Verdict]) -> String {
    let mut lines = Vec::new();
    for v in targets.iter().filter(|v| v.decision != Decision::Allow) {
        let t = &v.target;
        let whose = match &v.env {
            Some(env) if env == v.class => env.clone(),
            Some(env) => format!("{} ({})", env, v.class),
            None => "not declared in yuzu's environments".to_owned(),
        };
        let what = match t.access {
            Access::Write => "change",
            Access::Read => "read",
        };
        let from = t
            .source
            .as_ref()
            .map(|s| format!(", from {s}"))
            .unwrap_or_default();
        lines.push(format!(
            "`{}` would {what} {} {}{from}: {whose}.",
            t.command,
            t.kind.label(),
            t.name,
        ));
        lines.push(advice(config, v));
    }
    if !config.found {
        lines.push(format!(
            "No environments are declared yet. Write them in {} (see `yuzu agent-guide`).",
            config.path.display()
        ));
    }
    lines.join("\n")
}

fn advice(config: &Config, v: &Verdict) -> String {
    use crate::config::Kind;
    let t = &v.target;
    if v.decision == Decision::Deny {
        return "Agents may not touch it at all. Don't retry; ask the user to run it themselves."
            .to_owned();
    }
    let safe = config.writable(t.kind);
    let alternative = match (t.kind, safe.is_empty()) {
        (_, true) => String::new(),
        (Kind::KubeContext, false) => format!(
            " If you meant your own cluster, re-run with --context {}.",
            safe.join(" or ")
        ),
        (Kind::SshHost, false) => format!(" Hosts agents may change: {}.", safe.join(", ")),
        (Kind::GitRemote, false) => format!(" Remotes agents may push to: {}.", safe.join(", ")),
        (Kind::AwsProfile, false) => format!(
            " If you meant your own account, use --profile {}.",
            safe.join(" or ")
        ),
        (Kind::InfraDir, false) => {
            format!(" Directories agents may apply from: {}.", safe.join(", "))
        }
        (Kind::CliTool, false) => {
            format!(" CLIs agents may change things with: {}.", safe.join(", "))
        }
    };
    let declare = if v.env.is_none() {
        format!(
            " If it's the user's own, they can add it to {} under an env with class = \"mine\".",
            t.kind.key()
        )
    } else {
        String::new()
    };
    format!("It needs the user's approval.{alternative}{declare}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Environment, Kind};

    fn env(name: &str, class: Class, agents: Option<Agents>, contexts: &[&str]) -> Environment {
        Environment {
            name: name.into(),
            class,
            agents,
            kube_contexts: contexts.iter().map(|c| (*c).to_owned()).collect(),
            ssh_hosts: vec![],
            git_remotes: vec![],
            aws_profiles: vec![],
            infra_dirs: vec![],
            cli_tools: vec![],
        }
    }

    fn config() -> Config {
        Config {
            found: true,
            envs: vec![
                env("homelab", Class::Mine, None, &["k3s-pis"]),
                env(
                    "payments",
                    Class::Production,
                    Some(Agents::None),
                    &["*-payments-*"],
                ),
                env("work", Class::Work, None, &["cp*"]),
            ],
            ..Config::default()
        }
    }

    fn kube(name: &str, access: Access) -> Target {
        Target {
            kind: Kind::KubeContext,
            name: name.into(),
            access,
            command: "kubectl x".into(),
            source: None,
        }
    }

    #[test]
    fn decides_by_class_and_access() {
        let c = config();
        let d = |name, access| check(&c, vec![kube(name, access)]).decision;
        assert_eq!(d("k3s-pis", Access::Write), Decision::Allow);
        assert_eq!(d("cp3-nonprod", Access::Read), Decision::Allow);
        assert_eq!(d("cp3-nonprod", Access::Write), Decision::Ask);
        assert_eq!(d("prod-payments-1", Access::Read), Decision::Deny);
        assert_eq!(d("somewhere-new", Access::Read), Decision::Allow);
        assert_eq!(d("somewhere-new", Access::Write), Decision::Ask);
        assert_eq!(check(&c, vec![]).decision, Decision::Allow);
    }

    #[test]
    fn the_strictest_target_wins() {
        let c = config();
        let r = check(
            &c,
            vec![kube("k3s-pis", Access::Write), kube("cp1", Access::Write)],
        );
        assert_eq!(r.decision, Decision::Ask);
    }

    #[test]
    fn messages_name_the_target_and_the_safe_alternative() {
        let c = config();
        let r = check(&c, vec![kube("cp3-nonprod", Access::Write)]);
        let m = r.message.unwrap();
        assert!(m.contains("kube context cp3-nonprod: work."), "{m}");
        assert!(m.contains("--context k3s-pis"), "{m}");
        let r = check(&c, vec![kube("new-one", Access::Write)]);
        assert!(r.message.unwrap().contains("kube_contexts"));
    }
}
