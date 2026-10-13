//! probe against a local HTTP server and a stub browser that prints what
//! headless Chrome prints. No network beyond 127.0.0.1, no real browser.
#![cfg(unix)]
// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

/// Serves `200 ok` on 127.0.0.1 to every connection, and returns the port.
fn server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let _ =
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

/// A port nothing listens on.
fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A browser stand-in: `--version` prints a version; a load either prints a
/// DOM or logs Chrome's `Page load failed` line.
fn stub_browser(dir: &Path, error: Option<&str>) -> PathBuf {
    let load = match error {
        Some(code) => format!(
            "echo '[1:2:ERROR:headless_command_handler.cc:403] Page load failed: net::{code}' >&2"
        ),
        None => "echo '<html><body>ok</body></html>'".to_owned(),
    };
    let path = dir.join("stub-browser");
    std::fs::write(
        &path,
        format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'Stub Browser 1.2.3'; exit 0; fi\n{load}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn yuzu() -> Command {
    let mut cmd = Command::cargo_bin("yuzu").unwrap();
    cmd.env_remove("YUZU_BROWSER")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy");
    cmd
}

#[test]
fn reports_each_side_and_passes_when_both_load() {
    let dir = tempfile::tempdir().unwrap();
    let port = server();
    let out = yuzu()
        .args(["probe", "--json", "--browser"])
        .arg(stub_browser(dir.path(), None))
        .arg(format!("http://127.0.0.1:{port}/ui/"))
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(body["ok"], true);
    assert_eq!(body["terminal"]["status"], 200);
    assert_eq!(body["terminal"]["ip"], "127.0.0.1");
    assert_eq!(body["browser"]["version"], "1.2.3");
    assert_eq!(body["browser"]["ok"], true);
    assert_eq!(body["diagnosis"], serde_json::json!([]));
}

#[test]
fn names_the_browser_failure_and_doesnt_guess_a_cause() {
    let dir = tempfile::tempdir().unwrap();
    let port = server();
    yuzu()
        .args(["probe", "--browser"])
        .arg(stub_browser(dir.path(), Some("ERR_ADDRESS_UNREACHABLE")))
        .arg(format!("http://127.0.0.1:{port}/"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "terminal (curl)                 200   127.0.0.1",
        ))
        .stdout(predicate::str::contains("FAIL  ERR_ADDRESS_UNREACHABLE"))
        // 127.0.0.1 isn't on the local network, so the permission rule stays quiet.
        .stdout(predicate::str::contains("No rule matches this combination"));
}

#[test]
fn says_when_the_terminal_fails_too() {
    let dir = tempfile::tempdir().unwrap();
    let port = closed_port();
    yuzu()
        .args(["probe", "--timeout", "5", "--browser"])
        .arg(stub_browser(dir.path(), Some("ERR_CONNECTION_REFUSED")))
        .arg(format!("http://127.0.0.1:{port}/"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "likely cause: It fails from the terminal too, so it isn't the browser",
        ))
        .stdout(predicate::str::contains(format!(
            "is 127.0.0.1 port {port} open"
        )));
}

#[test]
fn a_missing_browser_is_a_failure_that_says_so() {
    let port = server();
    yuzu()
        .args(["probe", "--browser", "/nonexistent/chrome"])
        .arg(format!("http://127.0.0.1:{port}/"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("couldn't run /nonexistent/chrome"));
}

#[test]
fn a_bad_url_is_a_usage_error() {
    yuzu()
        .args(["probe", "openbao.local"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Give the full address"));
}
