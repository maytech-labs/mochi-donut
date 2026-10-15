//! yuzu against the programs it talks to: gh, ssh, aws, curl, a browser,
//! a local server. Programs we don't control are stub scripts put first on
//! PATH (or passed as --browser), so yuzu runs unchanged and is judged only
//! by its output, exit code, and what it wrote (AGENTS.md, Testing).
#![cfg(unix)]
// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

/// Writes an executable `name` in `dir` that runs `body` under sh.
fn stub(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A temp home with a stub bin dir first on PATH, and yuzu's config,
/// state, and gh config inside it.
struct World {
    home: tempfile::TempDir,
}

impl World {
    fn new(environments: &str) -> World {
        let home = tempfile::tempdir().unwrap();
        for d in ["bin", ".config/yuzu", ".kube"] {
            std::fs::create_dir_all(home.path().join(d)).unwrap();
        }
        std::fs::write(
            home.path().join(".config/yuzu/environments.toml"),
            environments,
        )
        .unwrap();
        World { home }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.home.path().join(rel)
    }

    fn stub(&self, name: &str, body: &str) {
        stub(&self.path("bin"), name, body);
    }

    /// A git repo at `rel` with an `origin` remote.
    fn repo(&self, rel: &str, origin: &str) -> PathBuf {
        let dir = self.path(rel);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["remote", "add", "origin", origin],
        ] {
            assert!(
                Process::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        dir
    }

    fn yuzu(&self, cwd: &Path) -> Command {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::cargo_bin("yuzu").unwrap();
        cmd.current_dir(cwd)
            .env("HOME", self.home.path())
            .env("PATH", path)
            .env("GH_CONFIG_DIR", self.path(".gh"))
            .env("XDG_STATE_HOME", self.path(".state"))
            .env("KUBECONFIG", self.path(".kube/config"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("YUZU_CONFIG")
            .env_remove("YUZU_NO_LOG")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_ENTERPRISE_TOKEN")
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_DEFAULT_PROFILE")
            .env("AWS_PROFILE", "personal");
        cmd
    }
}

fn json_of(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(out)))
}

const ENVS: &str = r#"
[[env]]
name = "homelab"
class = "mine"
infra_dirs = ["~/infra/*"]
cli_tools = ["flyctl"]
"#;

// whoami

#[test]
fn whoami_verify_asks_each_service_and_says_how_to_fix_a_failure() {
    let w = World::new(ENVS);
    std::fs::create_dir_all(w.path(".gh")).unwrap();
    std::fs::write(
        w.path(".gh/hosts.yml"),
        "github.com:\n    users:\n        me:\n    user: me\n",
    )
    .unwrap();
    std::fs::create_dir_all(w.path(".ssh")).unwrap();
    std::fs::write(w.path(".ssh/id_ed25519"), "key").unwrap();
    let repo = w.repo("work/r", "git@github.com:me/r.git");
    w.stub(
        "gh",
        r#"[ "$1 $2" = "api user" ] && { echo me; exit 0; }; exit 1"#,
    );
    w.stub(
        "ssh",
        r#"case "$1" in
  -G) printf 'user git\nidentityfile ~/.ssh/id_ed25519\nidentityfile ~/.ssh/id_rsa\n';;
  -T) echo "Hi me! You've successfully authenticated, but GitHub does not provide shell access." >&2; exit 1;;
esac"#,
    );
    w.stub("aws", "echo 'An error occurred (ExpiredToken) when calling the GetCallerIdentity operation' >&2; exit 254");

    let out = w
        .yuzu(&repo)
        .args(["whoami", "--verify", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let body = json_of(&out);
    let ids = body["identities"].as_array().unwrap();
    let by = |service: &str| ids.iter().find(|i| i["service"] == service).unwrap();

    assert_eq!(by("github")["who"], "me");
    assert_eq!(by("github")["verified"]["ok"], true);
    assert_eq!(by("github")["verified"]["detail"], "authenticates as me");
    assert_eq!(by("ssh")["who"], "~/.ssh/id_ed25519");
    assert_eq!(by("ssh")["verified"]["detail"], "authenticates as me");
    assert_eq!(by("aws")["who"], "profile personal");
    assert_eq!(by("aws")["verified"]["ok"], false);
    assert!(
        by("aws")["verified"]["fix"]
            .as_str()
            .unwrap()
            .contains("aws sso login --profile personal")
    );
    assert_eq!(body["failed"], 1);
}

#[test]
fn whoami_verify_blames_an_overriding_token() {
    let w = World::new(ENVS);
    std::fs::create_dir_all(w.path(".gh")).unwrap();
    std::fs::write(w.path(".gh/hosts.yml"), "github.com:\n    user: me\n").unwrap();
    w.stub(
        "gh",
        "echo 'HTTP 401: Bad credentials (https://api.github.com/user)' >&2; exit 1",
    );
    let out = w
        .yuzu(w.home.path())
        .env("GH_TOKEN", "stale")
        .args(["whoami", "--verify", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let body = json_of(&out);
    let gh = body["identities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["service"] == "github")
        .unwrap()
        .clone();
    assert_eq!(
        gh["verified"]["detail"],
        "HTTP 401: Bad credentials (https://api.github.com/user)"
    );
    assert!(
        gh["verified"]["fix"]
            .as_str()
            .unwrap()
            .starts_with("Unset GH_TOKEN")
    );
}

// wait

/// A stub gh that answers `pr checks` with each JSON in `answers` in turn,
/// then the last one again, and records its arguments.
fn gh_answers(w: &World, answers: &[&str]) {
    let mut body = String::from(
        "n=$(cat \"$HOME/gh-calls\" 2>/dev/null | wc -l | tr -d ' ')\necho \"$*\" >> \"$HOME/gh-calls\"\ncase $n in\n",
    );
    for (i, a) in answers.iter().enumerate() {
        let pat = if i + 1 == answers.len() {
            "*".to_owned()
        } else {
            i.to_string()
        };
        body.push_str(&format!("  {pat}) echo '{a}';;\n"));
    }
    body.push_str("esac");
    w.stub("gh", &body);
}

#[test]
fn wait_ci_waits_for_pending_checks_to_pass() {
    let w = World::new(ENVS);
    let repo = w.repo("work/r", "git@github.com:me/r.git");
    gh_answers(
        &w,
        &[
            r#"[{"name":"linux","bucket":"pass","link":"l1"},{"name":"macos","bucket":"pending","link":"l2"}]"#,
            r#"[{"name":"linux","bucket":"pass","link":"l1"},{"name":"macos","bucket":"pass","link":"l2"}]"#,
        ],
    );
    let out = w
        .yuzu(&repo)
        .args([
            "wait",
            "--every",
            "1s",
            "--timeout",
            "20s",
            "ci",
            "7",
            "--json",
        ])
        .assert()
        .code(0)
        .stderr(predicate::str::contains("1 of 2 pending (macos)"))
        .get_output()
        .stdout
        .clone();
    let body = json_of(&out);
    assert_eq!(body["met"], true);
    assert_eq!(body["last"]["detail"], "all 2 passed");
    assert_eq!(body["looks"], 2);
    // The repo came from the remote.
    let calls = std::fs::read_to_string(w.path("gh-calls")).unwrap();
    assert!(calls.contains("pr checks 7 -R github.com/me/r"), "{calls}");
}

#[test]
fn wait_ci_stops_on_a_failed_check_with_its_link() {
    let w = World::new(ENVS);
    let repo = w.repo("work/r", "git@github.com:me/r.git");
    gh_answers(
        &w,
        &[
            r#"[{"name":"linux","bucket":"fail","link":"https://ci/linux"},{"name":"macos","bucket":"pass","link":"l2"}]"#,
        ],
    );
    w.yuzu(&repo)
        .args(["wait", "--every", "1s", "--timeout", "20s", "ci", "7"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Failed after"))
        .stdout(predicate::str::contains("1 of 2 failed"))
        .stdout(predicate::str::contains("linux: https://ci/linux"));
}

#[test]
fn wait_ci_stops_at_once_when_gh_cant_authenticate() {
    let w = World::new(ENVS);
    let repo = w.repo("work/r", "git@github.com:me/r.git");
    w.stub(
        "gh",
        "echo 'HTTP 401: Bad credentials (https://api.github.com/graphql)' >&2; exit 1",
    );
    w.yuzu(&repo)
        .args(["wait", "--timeout", "5m", "ci", "7"])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("yuzu whoami --verify"));
}

#[test]
fn wait_ci_for_a_commit_lists_its_runs() {
    let w = World::new(ENVS);
    let repo = w.repo("work/r", "https://github.com/me/r");
    let args = [
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "x",
    ];
    assert!(
        Process::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
    let sha = String::from_utf8(
        Process::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    w.stub("gh", r#"echo "$*" >> "$HOME/gh-calls"; echo '[{"name":"CI","status":"completed","conclusion":"success","url":"u"}]'"#);
    w.yuzu(&repo)
        .args(["wait", "--timeout", "20s", "ci"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("all 1 passed"));
    let calls = std::fs::read_to_string(w.path("gh-calls")).unwrap();
    assert!(
        calls.contains(&format!(
            "run list -R github.com/me/r --commit {}",
            sha.trim()
        )),
        "{calls}"
    );
}

#[test]
fn wait_port_url_and_cmd() {
    let w = World::new(ENVS);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { continue };
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
        }
    });
    let here = w.home.path();
    w.yuzu(here)
        .args(["wait", "--timeout", "10s", "port"])
        .arg(format!("127.0.0.1:{port}"))
        .assert()
        .code(0);
    w.yuzu(here)
        .args(["wait", "--timeout", "10s", "url"])
        .arg(format!("http://127.0.0.1:{port}/"))
        .assert()
        .code(0);
    w.yuzu(here)
        .args([
            "wait",
            "--timeout",
            "2s",
            "--every",
            "1s",
            "url",
            "--status",
            "200",
        ])
        .arg(format!("http://127.0.0.1:{port}/"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("answered 204"));
    w.yuzu(here)
        .args(["wait", "--timeout", "10s", "cmd", "--", "test -d ."])
        .assert()
        .code(0);
    w.yuzu(here)
        .args([
            "wait",
            "--timeout",
            "2s",
            "--every",
            "1s",
            "cmd",
            "--",
            "echo nope >&2; exit 3",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("exited 3: nope"));
    // --not: until the port closes, which it doesn't.
    w.yuzu(here)
        .args(["wait", "--not", "--timeout", "2s", "--every", "1s", "port"])
        .arg(format!("127.0.0.1:{port}"))
        .assert()
        .code(1);
}

// check: infra dirs and cloud CLIs

#[test]
fn check_infra_dirs_and_cloud_clis_follow_the_declarations() {
    let w = World::new(ENVS);
    let mine = w.path("infra/homelab");
    let other = w.path("work/prod");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    w.yuzu(&mine)
        .args(["check", "--", "terraform apply"])
        .assert()
        .code(0);
    w.yuzu(&mine)
        .args(["check", "--", "terraform plan"])
        .assert()
        .code(0);
    w.yuzu(&other)
        .args(["check", "--", "terraform apply"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("infra dir ~/work/prod"))
        .stdout(predicate::str::contains(
            "Directories agents may apply from: ~/infra/*",
        ));
    w.yuzu(&other)
        .args(["check", "--", "terraform -chdir=../../infra/homelab apply"])
        .assert()
        .code(0);
    w.yuzu(&other)
        .args(["check", "--", "fly deploy"])
        .assert()
        .code(0);
    w.yuzu(&other)
        .args(["check", "--", "gcloud run deploy api"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "yuzu env add homelab cli_tools gcloud",
        ));
    w.yuzu(&other)
        .args(["check", "--", "gcloud projects list"])
        .assert()
        .code(0);
}

// probe: a certificate both sides reject

#[test]
fn probe_names_a_certificate_both_sides_reject() {
    let w = World::new(ENVS);
    w.stub(
        "curl",
        "echo 'curl: (60) SSL certificate problem: certificate has expired' >&2\necho 'More details here: https://curl.se/docs/sslcerts.html' >&2\nexit 60",
    );
    stub(
        &w.path("bin"),
        "browser",
        "[ \"$1\" = --version ] && { echo 'Stub 1.0'; exit 0; }\necho 'Page load failed: net::ERR_CERT_DATE_INVALID' >&2",
    );
    w.yuzu(w.home.path())
        .args(["probe", "--timeout", "5", "--browser"])
        .arg(w.path("bin/browser"))
        .arg("https://localhost/")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "FAIL  (60) SSL certificate problem: certificate has expired",
        ))
        .stdout(predicate::str::contains("More details").not())
        .stdout(predicate::str::contains(
            "certificate is rejected everywhere",
        ))
        .stdout(predicate::str::contains(
            "fix: Renew the server's certificate.",
        ));
}

// MCP: the tools added after check

#[test]
fn mcp_serves_whoami_and_wait() {
    let w = World::new(ENVS);
    let ready = w.path("ready");
    std::fs::write(&ready, "").unwrap();
    let msgs = [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"whoami","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"wait","arguments":{"condition":"file","target":ready.display().to_string(),"timeout":"5s"}}}),
    ];
    let input: String = msgs.iter().map(|m| format!("{m}\n")).collect();
    let out = w
        .yuzu(w.home.path())
        .arg("mcp")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let text = |i: usize| {
        json_of(
            replies[i]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .as_bytes(),
        )
    };
    assert_eq!(replies[0]["result"]["isError"], false);
    assert!(text(0)["identities"].is_array());
    assert_eq!(text(1)["met"], true);
}

/// A curl stand-in that prints probe's `-w` line for `ip`, and fails over
/// IPv6 when `v6_fails`.
fn stub_curl(w: &World, ip: &str, v6_fails: bool) {
    let v6 = if v6_fails {
        "for a in \"$@\"; do [ \"$a\" = -6 ] && { echo 'curl: (7) Failed to connect to ::1 port 1' >&2; exit 7; }; done\n"
    } else {
        ""
    };
    w.stub("curl", &format!("{v6}printf '200\\t{ip}\\t0'"));
}

fn stub_browser_error(w: &World, code: Option<&str>) -> PathBuf {
    let load = match code {
        Some(c) => format!("echo 'Page load failed: net::{c}' >&2"),
        None => "echo '<html>ok</html>'".to_owned(),
    };
    w.stub(
        "browser",
        &format!("[ \"$1\" = --version ] && {{ echo 'Stub 1.0'; exit 0; }}\n{load}"),
    );
    w.path("bin/browser")
}

#[test]
fn probe_names_local_network_permission_on_macos() {
    let w = World::new(ENVS);
    stub_curl(&w, "10.0.0.8", false);
    let browser = stub_browser_error(&w, Some("ERR_ADDRESS_UNREACHABLE"));
    let assert = w
        .yuzu(w.home.path())
        .args(["probe", "--timeout", "5", "--browser"])
        .arg(&browser)
        .arg("http://10.0.0.8:8200/ui/")
        .assert()
        .code(1);
    if cfg!(target_os = "macos") {
        // The stub was just written, so it counts as a fresh update.
        assert
            .stdout(predicate::str::contains(
                "likely cause: macOS Local Network permission for browser",
            ))
            .stdout(predicate::str::contains("updated to 1.0 at"))
            .stdout(predicate::str::contains(
                "Privacy & Security > Local Network",
            ));
    } else {
        assert.stdout(predicate::str::contains("No rule matches this combination"));
    }
}

#[test]
fn probe_names_a_browser_that_resolves_differently() {
    let w = World::new(ENVS);
    stub_curl(&w, "127.0.0.1", false);
    let browser = stub_browser_error(&w, Some("ERR_NAME_NOT_RESOLVED"));
    w.yuzu(w.home.path())
        .args(["probe", "--timeout", "5", "--browser"])
        .arg(&browser)
        .arg("http://localhost:1/")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "resolves localhost differently from the system",
        ))
        .stdout(predicate::str::contains("Secure DNS"));
}

#[test]
fn probe_names_a_dead_ipv6_address() {
    let w = World::new(ENVS);
    stub_curl(&w, "127.0.0.1", true);
    let browser = stub_browser_error(&w, None);
    let out = w
        .yuzu(w.home.path())
        .args(["probe", "--json", "--timeout", "5", "--browser"])
        .arg(&browser)
        .arg("http://localhost:1/")
        .output()
        .unwrap();
    let body = json_of(&out.stdout);
    // Only meaningful where localhost has both addresses, as on macOS and
    // most Linux images.
    if body["dns"]["v6"].as_array().is_some_and(|v| !v.is_empty())
        && body["dns"]["v4"].as_array().is_some_and(|v| !v.is_empty())
    {
        assert_eq!(body["terminal_ipv6"]["ok"], false, "{body}");
        let causes: Vec<&str> = body["diagnosis"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["cause"].as_str().unwrap())
            .collect();
        assert!(
            causes.contains(&"localhost's IPv6 address doesn't answer"),
            "{body}"
        );
    }
}
