//! `yuzu check` decisions for real command lines, through the binary: the
//! contract the hook relies on. One table, so a change in any decision shows
//! up as a failing row (AGENTS.md, Testing).

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use serde_json::Value;

const ENVIRONMENTS: &str = r#"
[[env]]
name = "homelab"
class = "mine"
kube_contexts = ["k3s-pis"]
ssh_hosts = ["samwise", "10.0.0.*"]
git_remotes = ["github.com/me/*"]
aws_profiles = ["personal"]

[[env]]
name = "payments"
class = "production"
agents = "none"
kube_contexts = ["*-payments-*"]

[[env]]
name = "work"
class = "work"
kube_contexts = ["work-*"]
git_remotes = ["github.com/acme/*"]
"#;

/// A home whose kube context is a work cluster, and a repo whose `origin` is
/// the user's and whose `work` remote is the employer's.
fn world() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for d in [".kube", ".config/yuzu", "repo"] {
        std::fs::create_dir_all(home.path().join(d)).unwrap();
    }
    std::fs::write(
        home.path().join(".kube/config"),
        "apiVersion: v1\ncurrent-context: work-nonprod-us-east-1\nkind: Config\n",
    )
    .unwrap();
    std::fs::write(
        home.path().join(".config/yuzu/environments.toml"),
        ENVIRONMENTS,
    )
    .unwrap();
    let repo = home.path().join("repo");
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["remote", "add", "origin", "git@github.com:me/r.git"],
        vec!["remote", "add", "work", "https://github.com/acme/app.git"],
    ] {
        assert!(
            Process::new("git")
                .arg("-C")
                .arg(&repo)
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    home
}

fn decide(home: &Path, line: &str) -> (i32, Value) {
    let out = Command::cargo_bin("yuzu")
        .unwrap()
        .current_dir(home.join("repo"))
        .env("HOME", home)
        .env_remove("KUBECONFIG")
        .env_remove("YUZU_CONFIG")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_DEFAULT_PROFILE")
        .args(["check", "--json", "--", line])
        .output()
        .unwrap();
    let body: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{line}: not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    (out.status.code().unwrap_or(-1), body)
}

#[test]
fn decisions_for_real_command_lines() {
    let home = world();
    let table: &[(&str, &str)] = &[
        // Kubernetes: the current context is work.
        ("kubectl get pods -A", "allow"),
        ("kubectl delete pod api-1", "ask"),
        ("kubectl exec -it api-1 -- sh", "ask"),
        ("kubectl --context k3s-pis delete pod x", "allow"),
        ("kubectl --context prod-payments-1 get pods", "deny"),
        ("helm list", "allow"),
        ("helm upgrade api ./chart", "ask"),
        ("kubectl apply -f x.yaml --dry-run=client", "allow"),
        // Wrappers and compound lines.
        ("sudo kubectl delete ns x", "ask"),
        ("timeout 30 kubectl delete ns x", "ask"),
        ("bash -c 'kubectl delete ns x'", "ask"),
        ("echo $(kubectl delete ns x)", "ask"),
        (
            "cd /tmp && kubectl --context k3s-pis apply -f a.yaml",
            "allow",
        ),
        ("ls; kubectl --context prod-payments-1 delete ns x", "deny"),
        ("cat <<EOF\nkubectl delete ns x\nEOF", "allow"),
        // Hosts.
        ("ssh samwise uptime", "allow"),
        ("ssh bastion uptime", "ask"),
        ("scp a.txt 10.0.0.8:/tmp/", "allow"),
        ("rsync -a . deploy@prod-host:/srv/", "ask"),
        // Git remotes.
        ("git push origin main", "allow"),
        ("git push work main", "ask"),
        ("git status", "allow"),
        // AWS: no profile set means `default`, which isn't declared.
        ("aws s3 ls", "allow"),
        ("aws ec2 terminate-instances --instance-ids i-1", "ask"),
        (
            "aws --profile personal ec2 terminate-instances --instance-ids i-1",
            "allow",
        ),
        (
            "AWS_PROFILE=personal aws ec2 terminate-instances --instance-ids i-1",
            "allow",
        ),
    ];
    let mut wrong = Vec::new();
    for (line, want) in table {
        let (code, body) = decide(home.path(), line);
        let got = body["decision"].as_str().unwrap_or("?");
        let want_code = if *want == "allow" { 0 } else { 1 };
        if got != *want || code != want_code {
            wrong.push(format!(
                "{line:?}: want {want} (exit {want_code}), got {got} (exit {code})"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
