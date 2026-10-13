//! `yuzu status`: where the shell points right now, and whose it is.

use serde::Serialize;

use crate::config::{Agents, Config, Kind};
use crate::env::Ctx;
use crate::policy::class_name;
use crate::targets::{self, current_context, kubeconfig_files, normalize_remote};

#[derive(Debug, Serialize)]
pub struct Status {
    pub config: ConfigSummary,
    pub targets: Vec<Current>,
}

#[derive(Debug, Serialize)]
pub struct ConfigSummary {
    pub path: String,
    pub found: bool,
    pub environments: usize,
}

/// Something commands would use by default.
#[derive(Debug, Serialize)]
pub struct Current {
    pub kind: Kind,
    pub name: String,
    /// Where the default comes from, such as the kubeconfig file or the remote name.
    pub source: String,
    pub env: Option<String>,
    pub class: &'static str,
    pub agents: Agents,
}

pub fn status(config: &Config, ctx: &Ctx) -> Status {
    let mut found: Vec<(Kind, String, String)> = Vec::new();

    match current_context(&kubeconfig_files(ctx)) {
        Ok(Some((name, file))) => found.push((
            Kind::KubeContext,
            name,
            format!("current-context in {}", file.display()),
        )),
        Ok(None) => {}
        Err(e) => found.push((Kind::KubeContext, "(unknown)".to_owned(), e)),
    }

    let profile = ctx
        .var("AWS_PROFILE")
        .or_else(|| ctx.var("AWS_DEFAULT_PROFILE"));
    match profile.filter(|p| !p.is_empty()) {
        Some(p) => found.push((Kind::AwsProfile, p.to_owned(), "AWS_PROFILE".to_owned())),
        None if ctx.home.join(".aws").exists() => found.push((
            Kind::AwsProfile,
            "default".to_owned(),
            "no AWS_PROFILE set".to_owned(),
        )),
        None => {}
    }

    if let Some(names) = targets::git_out(&ctx.cwd, &["remote"]) {
        for remote in names.lines() {
            if let Some(url) = targets::remote_url(&ctx.cwd, remote) {
                found.push((
                    Kind::GitRemote,
                    normalize_remote(&url),
                    format!("remote {remote}"),
                ));
            }
        }
    }

    Status {
        config: ConfigSummary {
            path: config.path.display().to_string(),
            found: config.found,
            environments: config.envs.len(),
        },
        targets: found
            .into_iter()
            .map(|(kind, name, source)| {
                let env = config.classify(kind, &name);
                Current {
                    kind,
                    env: env.map(|e| e.name.clone()),
                    class: class_name(env.map(|e| e.class)),
                    agents: env.map_or(Agents::ReadOnly, |e| e.agents()),
                    name,
                    source,
                }
            })
            .collect(),
    }
}
