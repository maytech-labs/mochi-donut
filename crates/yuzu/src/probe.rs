//! `yuzu probe`: does it work from where the user is? Loads a URL from the
//! terminal (curl) and from the user's browser (headless, with an empty
//! temporary profile), and maps the difference to a likely cause
//! (docs/design.md, "yuzu probe"; docs/decisions/0018-yuzu-probe.md).

use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use mochi_common::time::rfc3339;
use serde::Serialize;

use crate::env::Ctx;
use crate::proc::run;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    pub url: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

/// Reads an `http://` or `https://` URL. Anything else is a usage error.
pub fn parse_url(url: &str) -> Result<Target, String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("{url:?} isn't a URL. Give the full address: https://host/path"))?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        other => {
            return Err(format!(
                "yuzu probe checks http and https URLs, not {other}://"
            ));
        }
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6
            .split_once(']')
            .ok_or_else(|| format!("{url:?} has an unclosed [ in its host"))?;
        (h.to_owned(), after.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (authority.to_owned(), None),
        }
    };
    if host.is_empty() {
        return Err(format!("{url:?} has no host"));
    }
    let port = match port {
        Some(p) => p
            .parse()
            .map_err(|_| format!("{url:?} has a port that isn't a number: {p}"))?,
        None => default_port,
    };
    Ok(Target {
        url: url.to_owned(),
        scheme,
        host: host.to_ascii_lowercase(),
        port,
    })
}

/// What the system resolver says, as curl and the browser would ask it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Dns {
    pub v4: Vec<String>,
    pub v6: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Dns {
    fn addrs(&self) -> impl Iterator<Item = &String> {
        self.v4.iter().chain(&self.v6)
    }
}

pub fn resolve(host: &str, port: u16) -> Dns {
    let mut dns = Dns::default();
    match (host, port).to_socket_addrs() {
        Ok(addrs) => {
            for a in addrs {
                let ip = a.ip().to_string();
                let list = if a.is_ipv4() {
                    &mut dns.v4
                } else {
                    &mut dns.v6
                };
                if !list.contains(&ip) {
                    list.push(ip);
                }
            }
        }
        Err(e) => dns.error = Some(e.to_string()),
    }
    dns
}

/// One load of the URL.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Attempt {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// For https: whether the certificate verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_trusted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Loads `url` with curl, as an agent's shell would. `family` forces IPv4
/// (4) or IPv6 (6). Any HTTP response counts as reached, even a 500.
pub fn curl(target: &Target, family: Option<u8>, timeout: Duration) -> Attempt {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-o", "/dev/null", "--max-time"])
        .arg(timeout.as_secs().max(1).to_string())
        .args(["-w", "%{http_code}\t%{remote_ip}\t%{ssl_verify_result}"]);
    match family {
        Some(4) => {
            cmd.arg("-4");
        }
        Some(6) => {
            cmd.arg("-6");
        }
        _ => {}
    }
    cmd.arg(&target.url);
    let out = match run(cmd, timeout + Duration::from_secs(5)) {
        Ok(out) => out,
        Err(e) => {
            return Attempt {
                error: Some(e),
                ..Attempt::default()
            };
        }
    };
    let mut fields = out.stdout.trim().split('\t');
    let status = fields
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|s| *s > 0);
    let ip = fields.next().filter(|s| !s.is_empty()).map(str::to_owned);
    let verify = fields.next().and_then(|s| s.parse::<i64>().ok());
    let https = target.scheme == "https";
    let code = out.code.unwrap_or(-1);
    Attempt {
        ok: code == 0 && status.is_some(),
        status,
        ip,
        cert_trusted: match (https, code) {
            (false, _) => None,
            // 60: peer certificate cannot be authenticated.
            (true, 60) => Some(false),
            (true, 0) => Some(verify == Some(0)),
            _ => None,
        },
        error: (code != 0).then(|| {
            // curl's first line is the error; the rest is boilerplate.
            let msg = out
                .stderr
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .trim_start_matches("curl: ");
            if msg.is_empty() {
                format!("curl exited with {code}")
            } else {
                msg.to_owned()
            }
        }),
    }
}

/// A Chromium-based browser installed here.
#[derive(Debug, Clone, Serialize)]
pub struct Browser {
    pub name: String,
    pub path: PathBuf,
}

/// The browser to probe with: `--browser`, else `$YUZU_BROWSER`, else the
/// first of Chrome, Edge, Brave, and Chromium found in the usual places.
pub fn find_browser(over: Option<&str>, ctx: &Ctx) -> Option<Browser> {
    if let Some(p) = over.or(ctx.var("YUZU_BROWSER")) {
        let path = ctx.path(p);
        let name = path
            .file_name()
            .map_or_else(|| p.to_owned(), |n| n.to_string_lossy().into_owned());
        return Some(Browser { name, path });
    }
    let mac = [
        (
            "Google Chrome",
            "Google Chrome.app/Contents/MacOS/Google Chrome",
        ),
        (
            "Microsoft Edge",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ),
        (
            "Brave Browser",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
        ),
        ("Chromium", "Chromium.app/Contents/MacOS/Chromium"),
    ];
    for dir in [
        PathBuf::from("/Applications"),
        ctx.home.join("Applications"),
    ] {
        for (name, rel) in mac {
            let path = dir.join(rel);
            if path.is_file() {
                return Some(Browser {
                    name: name.to_owned(),
                    path,
                });
            }
        }
    }
    let linux = [
        ("Google Chrome", "google-chrome"),
        ("Google Chrome", "google-chrome-stable"),
        ("Chromium", "chromium"),
        ("Chromium", "chromium-browser"),
        ("Microsoft Edge", "microsoft-edge"),
        ("Brave Browser", "brave-browser"),
    ];
    let path_var = ctx.var("PATH").unwrap_or("");
    for (name, bin) in linux {
        for dir in path_var.split(':').filter(|d| !d.is_empty()) {
            let path = Path::new(dir).join(bin);
            if path.is_file() {
                return Some(Browser {
                    name: name.to_owned(),
                    path,
                });
            }
        }
    }
    None
}

/// The browser's load, with what's known about the browser itself.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserLoad {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// When the browser was last installed or updated, from the app's files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip)]
    pub updated_secs: Option<u64>,
    #[serde(flatten)]
    pub attempt: Attempt,
}

/// Loads `url` in the browser, headless, with a new empty profile in a
/// temporary directory: no cookies, history, passwords, or extensions are
/// read. Deleted afterward.
pub fn browser_load(browser: &Browser, target: &Target, timeout: Duration) -> BrowserLoad {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let profile = std::env::temp_dir().join(format!("yuzu-probe-{}-{nanos}", std::process::id()));
    let mut cmd = Command::new(&browser.path);
    cmd.args([
        "--headless=new",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-sync",
    ])
    .arg(format!("--user-data-dir={}", profile.display()))
    .arg(format!("--timeout={}", timeout.as_millis()))
    .arg("--dump-dom")
    .arg(&target.url);
    let result = run(cmd, timeout + Duration::from_secs(15));
    let _ = std::fs::remove_dir_all(&profile);
    let attempt = match result {
        Ok(out) => match net_error(&out.stderr, &out.stdout) {
            Some(code) => Attempt {
                error: Some(code),
                ..Attempt::default()
            },
            None if out.stdout.trim().is_empty() => Attempt {
                error: Some(format!(
                    "the browser loaded nothing and gave no error code (exit {})",
                    out.code.map_or("by signal".to_owned(), |c| c.to_string())
                )),
                ..Attempt::default()
            },
            None => Attempt {
                ok: true,
                ..Attempt::default()
            },
        },
        Err(e) => Attempt {
            error: Some(e),
            ..Attempt::default()
        },
    };
    let (updated_at, updated_secs) = match updated(&browser.path) {
        Some(secs) => (Some(rfc3339(secs)), Some(secs)),
        None => (None, None),
    };
    BrowserLoad {
        name: browser.name.clone(),
        version: version(&browser.path),
        updated_at,
        updated_secs,
        attempt,
    }
}

/// The `net::ERR_*` code from a headless load: new headless logs
/// `Page load failed: net::ERR_X` on stderr; old headless renders an error
/// page whose DOM holds the code.
fn net_error(stderr: &str, dom: &str) -> Option<String> {
    let code_after = |s: &str, marker: &str| -> Option<String> {
        let at = s.find(marker)? + marker.len();
        let code: String = s[at..]
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        (code.len() > 4).then_some(code)
    };
    if let Some(line) = stderr.lines().find(|l| l.contains("Page load failed")) {
        return code_after(line, "net::");
    }
    if dom.contains("neterror") || dom.contains("main-frame-error") {
        return code_after(dom, "net::")
            .or_else(|| code_after(dom, ">ERR_").map(|c| format!("ERR_{c}")));
    }
    None
}

fn version(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    let out = run(cmd, Duration::from_secs(10)).ok()?;
    out.stdout
        .split_whitespace()
        .rev()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_owned)
}

/// When the browser was last updated: the app bundle's `Info.plist` on
/// macOS, which an update replaces, else the binary.
fn updated(path: &Path) -> Option<u64> {
    let plist = path
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(|app| app.join("Contents/Info.plist"))
        .filter(|p| p.exists());
    let meta = std::fs::metadata(plist.as_deref().unwrap_or(path)).ok()?;
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Proxies each side would use.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Proxies {
    /// `HTTPS_PROXY=host:port` and friends, as curl reads them. Credentials
    /// are left out.
    pub terminal: Vec<String>,
    /// The system's proxy settings, as the browser reads them (macOS).
    pub system: Vec<String>,
}

pub fn proxies(ctx: &Ctx) -> Proxies {
    let mut terminal = Vec::new();
    for name in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        if let Some(v) = ctx.var(name).filter(|v| !v.is_empty()) {
            terminal.push(format!("{name}={}", strip_credentials(v)));
        }
    }
    let system = if cfg!(target_os = "macos") {
        let mut cmd = Command::new("scutil");
        cmd.arg("--proxy");
        run(cmd, Duration::from_secs(5))
            .map(|o| parse_scutil(&o.stdout))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Proxies { terminal, system }
}

fn strip_credentials(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(s), Some(at)) if at > s => format!("{}{}", &url[..s + 3], &url[at + 1..]),
        _ => url.to_owned(),
    }
}

/// Enabled proxies from `scutil --proxy`: `HTTPS 10.0.0.1:3128`, `PAC <url>`.
fn parse_scutil(out: &str) -> Vec<String> {
    let get = |key: &str| {
        out.lines()
            .filter_map(|l| l.trim().split_once(" : "))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.trim().to_owned())
    };
    let on = |key: &str| get(key).as_deref() == Some("1");
    let mut v = Vec::new();
    for (label, prefix) in [("HTTP", "HTTP"), ("HTTPS", "HTTPS"), ("SOCKS", "SOCKS")] {
        if on(&format!("{prefix}Enable")) {
            v.push(format!(
                "{label} {}:{}",
                get(&format!("{prefix}Proxy")).unwrap_or_default(),
                get(&format!("{prefix}Port")).unwrap_or_default()
            ));
        }
    }
    if on("ProxyAutoConfigEnable") {
        v.push(format!(
            "PAC {}",
            get("ProxyAutoConfigURLString").unwrap_or_default()
        ));
    }
    if on("ProxyAutoDiscoveryEnable") {
        v.push("WPAD (auto-discovery)".into());
    }
    v
}

/// Everything the diagnosis rules look at.
pub struct Facts<'a> {
    pub target: &'a Target,
    pub dns: &'a Dns,
    pub terminal: &'a Attempt,
    /// Separate IPv4 and IPv6 loads, when the name has both.
    pub terminal_v4: Option<&'a Attempt>,
    pub terminal_v6: Option<&'a Attempt>,
    pub browser: Option<&'a BrowserLoad>,
    pub proxies: &'a Proxies,
    pub macos: bool,
    /// `SSL_CERT_FILE=...` and the like: CA bundles only curl sees.
    pub ca_env: Vec<String>,
    pub now: u64,
}

/// A likely cause, with the evidence for it and what to do.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub cause: String,
    pub evidence: String,
    pub fix: String,
}

/// Maps the symptoms to likely causes. Only rules whose symptoms all match
/// fire; with none, the report says so instead of guessing.
pub fn diagnose(f: &Facts) -> Vec<Diagnosis> {
    let mut out = Vec::new();
    let browser_error = f.browser.and_then(|b| b.attempt.error.as_deref());
    let name = f.browser.map_or("the browser", |b| b.name.as_str());
    let reached = f
        .terminal
        .ip
        .clone()
        .or_else(|| f.dns.addrs().next().cloned())
        .unwrap_or_else(|| f.target.host.clone());

    // macOS asks per app before it may reach the local network; the terminal
    // has its own grant. An update can reset the browser's.
    let private = f.dns.addrs().any(|a| is_private(a)) || f.target.host.ends_with(".local");
    if f.macos && f.terminal.ok && private && browser_error == Some("ERR_ADDRESS_UNREACHABLE") {
        let mut evidence = format!(
            "The terminal reached {reached}, a local-network address; {name} got ERR_ADDRESS_UNREACHABLE."
        );
        if let Some(b) = f.browser
            && let (Some(at), Some(secs)) = (&b.updated_at, b.updated_secs)
            && f.now.saturating_sub(secs) < 7 * 86_400
        {
            let version = b.version.as_deref().unwrap_or("a new version");
            evidence.push_str(&format!(
                " {name} updated to {version} at {at}; the permission often resets on update."
            ));
        }
        out.push(Diagnosis {
            cause: format!("macOS Local Network permission for {name}"),
            evidence,
            fix: format!(
                "System Settings > Privacy & Security > Local Network: turn {name} on (or off and on), then quit and reopen {name}."
            ),
        });
    }

    if browser_error == Some("ERR_NAME_NOT_RESOLVED") && f.dns.addrs().next().is_some() {
        out.push(Diagnosis {
            cause: format!("{name} resolves {} differently from the system", f.target.host),
            evidence: format!(
                "The system resolver gave {}; {name} got ERR_NAME_NOT_RESOLVED.",
                f.dns.addrs().cloned().collect::<Vec<_>>().join(", ")
            ),
            fix: format!(
                "Check {name}'s Secure DNS setting (chrome://settings/security), which skips the system resolver and /etc/hosts, and any VPN's split DNS."
            ),
        });
    }

    if let Some(code) = browser_error.filter(|c| c.starts_with("ERR_CERT"))
        && f.terminal.cert_trusted == Some(true)
    {
        let mut evidence = format!("curl verified the certificate; {name} got {code}.");
        if !f.ca_env.is_empty() {
            evidence.push_str(&format!(
                " curl trusts a CA bundle the browser doesn't read: {}.",
                f.ca_env.join(", ")
            ));
        }
        out.push(Diagnosis {
            cause: format!("{name} doesn't trust the certificate the terminal does"),
            evidence,
            fix: "Add the issuing CA to the system trust store (macOS: Keychain Access, System keychain, Always Trust), not only to curl's bundle.".into(),
        });
    }

    if let (Some(v4), Some(v6)) = (f.terminal_v4, f.terminal_v6)
        && v4.ok
        && !v6.ok
    {
        out.push(Diagnosis {
            cause: format!("{}'s IPv6 address doesn't answer", f.target.host),
            evidence: format!(
                "Over IPv4 ({}) it loads; over IPv6 ({}) it fails: {}",
                f.dns.v4.join(", "),
                f.dns.v6.join(", "),
                v6.error.as_deref().unwrap_or("no response")
            ),
            fix: "Fix or remove the AAAA record, or the IPv6 route to it. Clients that try IPv6 first stall or fail.".into(),
        });
    }

    let terminal_proxied = f
        .proxies
        .terminal
        .iter()
        .any(|p| !p.to_lowercase().starts_with("no_proxy"));
    let browser_proxied = !f.proxies.system.is_empty();
    if let Some(b) = f.browser
        && b.attempt.ok != f.terminal.ok
        && terminal_proxied != browser_proxied
    {
        out.push(Diagnosis {
            cause: "The terminal and the browser use different proxies".into(),
            evidence: format!(
                "Terminal: {}. System (browser): {}.",
                list_or_none(&f.proxies.terminal),
                list_or_none(&f.proxies.system)
            ),
            fix: "Make them agree: set the shell's proxy variables to match the system settings, or the other way around.".into(),
        });
    }

    let unresolved = f.dns.addrs().next().is_none();
    let both_fail = !f.terminal.ok && f.browser.is_some_and(|b| !b.attempt.ok);
    let browser_cert =
        browser_error.is_some_and(|c| c.starts_with("ERR_CERT") || c.contains("SSL"));
    if both_fail && f.terminal.cert_trusted == Some(false) && browser_cert {
        out.push(Diagnosis {
            cause: format!(
                "{}'s certificate is rejected everywhere, not just here",
                f.target.host
            ),
            evidence: format!(
                "curl: {}. {name}: {}.",
                f.terminal
                    .error
                    .as_deref()
                    .unwrap_or("certificate not trusted"),
                browser_error.unwrap_or("certificate error")
            ),
            fix: cert_fix(f.terminal.error.as_deref().unwrap_or("")),
        });
    } else if both_fail && unresolved {
        out.push(Diagnosis {
            cause: format!("{} doesn't resolve from this machine", f.target.host),
            evidence: format!(
                "The system resolver: {}. curl: {}.",
                f.dns.error.as_deref().unwrap_or("no addresses"),
                f.terminal.error.as_deref().unwrap_or("no response")
            ),
            fix: format!(
                "Check {}'s DNS record, /etc/hosts, and whether this machine should be on a VPN or tailnet that serves the name.",
                f.target.host
            ),
        });
    } else if both_fail {
        out.push(Diagnosis {
            cause: "It fails from the terminal too, so it isn't the browser".into(),
            evidence: format!(
                "curl: {}. {name}: {}.",
                f.terminal.error.as_deref().unwrap_or("no response"),
                browser_error.unwrap_or("failed")
            ),
            fix: format!(
                "Check the service and the path to it first: is it running, and is {reached} port {} open from here?",
                f.target.port
            ),
        });
    }
    out
}

/// What to do about a certificate both sides reject, from curl's reason.
fn cert_fix(curl_error: &str) -> String {
    let e = curl_error.to_lowercase();
    if e.contains("expired") {
        "Renew the server's certificate.".into()
    } else if e.contains("self signed") || e.contains("self-signed") || e.contains("local issuer") {
        "If the server uses your own or your company's CA (a homelab, an internal service), add that CA to the system trust store; then curl and the browser both accept it. Otherwise, give the server a certificate from a public CA.".into()
    } else if e.contains("name") || e.contains("subject") {
        "The certificate doesn't cover this name: reissue it with the name as a subject alternative name, or use a name it covers.".into()
    } else {
        "Fix the server's certificate: its names, chain, and dates.".into()
    }
}

fn list_or_none(v: &[String]) -> String {
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}

/// RFC 1918, carrier-grade NAT (Tailscale), link-local, and IPv6 unique
/// local and link-local addresses: the ones macOS counts as local network.
pub fn is_private(addr: &str) -> bool {
    match addr.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            let o = v4.octets();
            v4.is_private() || v4.is_link_local() || (o[0] == 100 && (64..128).contains(&o[1]))
        }
        Ok(IpAddr::V6(v6)) => {
            let first = v6.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        let t = parse_url("https://openbao.local/ui/").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("openbao.local", 443));
        let t = parse_url("http://user:pw@10.0.0.8:8200/v1?x=1").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("10.0.0.8", 8200));
        let t = parse_url("http://[fd00::1]:8080/").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("fd00::1", 8080));
        assert!(
            parse_url("openbao.local")
                .unwrap_err()
                .contains("isn't a URL")
        );
        assert!(parse_url("ftp://x").unwrap_err().contains("not ftp://"));
        assert!(parse_url("http://x:http").unwrap_err().contains("port"));
    }

    #[test]
    fn reads_chrome_errors_both_ways() {
        let stderr = "[1:2:ERROR:headless_command_handler.cc:403] Page load failed: net::ERR_ADDRESS_UNREACHABLE\n";
        assert_eq!(
            net_error(stderr, "").as_deref(),
            Some("ERR_ADDRESS_UNREACHABLE")
        );
        let dom = r#"<div id="main-frame-error" class="neterror"><div class="error-code">ERR_NAME_NOT_RESOLVED</div>"#;
        assert_eq!(net_error("", dom).as_deref(), Some("ERR_NAME_NOT_RESOLVED"));
        assert_eq!(
            net_error("noise", "<html>ERR_FOO in a normal page</html>"),
            None
        );
    }

    #[test]
    fn knows_local_network_addresses() {
        for a in [
            "10.0.0.8",
            "192.168.1.2",
            "172.20.0.1",
            "100.101.102.103",
            "169.254.1.1",
            "fd7a::1",
            "fe80::1",
        ] {
            assert!(is_private(a), "{a}");
        }
        for a in [
            "8.8.8.8",
            "127.0.0.1",
            "100.128.0.1",
            "2001:db8::1",
            "openbao.local",
        ] {
            assert!(!is_private(a), "{a}");
        }
    }

    #[test]
    fn reads_system_proxies() {
        let out = "<dictionary> {\n  HTTPEnable : 0\n  HTTPSEnable : 1\n  HTTPSPort : 3128\n  HTTPSProxy : 10.0.0.1\n  ProxyAutoConfigEnable : 1\n  ProxyAutoConfigURLString : http://wpad/proxy.pac\n}\n";
        assert_eq!(
            parse_scutil(out),
            ["HTTPS 10.0.0.1:3128", "PAC http://wpad/proxy.pac"]
        );
        assert_eq!(
            strip_credentials("http://u:p@proxy:3128"),
            "http://proxy:3128"
        );
    }

    fn ok(ip: &str) -> Attempt {
        Attempt {
            ok: true,
            status: Some(200),
            ip: Some(ip.into()),
            cert_trusted: Some(true),
            error: None,
        }
    }

    fn failed(code: &str) -> Attempt {
        Attempt {
            error: Some(code.into()),
            ..Attempt::default()
        }
    }

    fn chrome(attempt: Attempt, updated_secs: u64) -> BrowserLoad {
        BrowserLoad {
            name: "Google Chrome".into(),
            version: Some("155.0.8059.40".into()),
            updated_at: Some(rfc3339(updated_secs)),
            updated_secs: Some(updated_secs),
            attempt,
        }
    }

    /// The OpenBao session: curl got 200 from 10.0.0.8, Chrome got
    /// ERR_ADDRESS_UNREACHABLE, and Chrome had updated that afternoon.
    #[test]
    fn names_local_network_permission_for_the_openbao_session() {
        let target = parse_url("https://openbao.local/ui/").unwrap();
        let dns = Dns {
            v4: vec!["10.0.0.8".into()],
            ..Dns::default()
        };
        let terminal = ok("10.0.0.8");
        let now = 1_791_817_200; // 2026-10-12T15:00:00Z
        let browser = chrome(failed("ERR_ADDRESS_UNREACHABLE"), now - 120); // 14:58
        let d = diagnose(&Facts {
            target: &target,
            dns: &dns,
            terminal: &terminal,
            terminal_v4: None,
            terminal_v6: None,
            browser: Some(&browser),
            proxies: &Proxies::default(),
            macos: true,
            ca_env: Vec::new(),
            now,
        });
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(
            d[0].cause,
            "macOS Local Network permission for Google Chrome"
        );
        assert!(d[0].evidence.contains("reached 10.0.0.8"));
        assert!(
            d[0].evidence
                .contains("updated to 155.0.8059.40 at 2026-10-12T14:58:00Z")
        );
        assert!(d[0].fix.contains("Privacy & Security > Local Network"));
    }

    #[test]
    fn other_rules_fire_on_their_symptoms_only() {
        let target = parse_url("https://vault.example.test/").unwrap();
        let dns = Dns {
            v4: vec!["203.0.113.5".into()],
            v6: vec!["2001:db8::5".into()],
            ..Dns::default()
        };
        let terminal = ok("203.0.113.5");
        let v6 = failed("curl: (7) Failed to connect");
        let now = 2_000_000_000;
        let facts = |browser: &BrowserLoad, macos: bool| -> Vec<String> {
            diagnose(&Facts {
                target: &target,
                dns: &dns,
                terminal: &terminal,
                terminal_v4: Some(&terminal),
                terminal_v6: Some(&v6),
                browser: Some(browser),
                proxies: &Proxies::default(),
                macos,
                ca_env: vec!["SSL_CERT_FILE=/etc/corp-ca.pem".into()],
                now,
            })
            .into_iter()
            .map(|d| d.cause)
            .collect()
        };
        // A public address: not the Local Network permission, but IPv6 is broken.
        let unreachable = chrome(failed("ERR_ADDRESS_UNREACHABLE"), 0);
        assert_eq!(
            facts(&unreachable, true),
            ["vault.example.test's IPv6 address doesn't answer"]
        );
        let cert = chrome(failed("ERR_CERT_AUTHORITY_INVALID"), 0);
        assert!(facts(&cert, false)[0].contains("doesn't trust the certificate"));
        let dns_fail = chrome(failed("ERR_NAME_NOT_RESOLVED"), 0);
        assert!(facts(&dns_fail, false)[0].contains("resolves vault.example.test differently"));
    }

    #[test]
    fn a_name_that_resolves_nowhere_is_a_dns_problem() {
        let target = parse_url("http://nonexistent.invalid/").unwrap();
        let dns = Dns {
            error: Some("nodename nor servname provided".into()),
            ..Dns::default()
        };
        let terminal = failed("(6) Could not resolve host: nonexistent.invalid");
        let browser = chrome(failed("ERR_NAME_NOT_RESOLVED"), 0);
        let d = diagnose(&Facts {
            target: &target,
            dns: &dns,
            terminal: &terminal,
            terminal_v4: None,
            terminal_v6: None,
            browser: Some(&browser),
            proxies: &Proxies::default(),
            macos: true,
            ca_env: Vec::new(),
            now: 0,
        });
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(
            d[0].cause,
            "nonexistent.invalid doesn't resolve from this machine"
        );
    }

    #[test]
    fn a_certificate_both_sides_reject_is_the_servers_problem() {
        let target = parse_url("https://expired.badssl.com/").unwrap();
        let dns = Dns {
            v4: vec!["104.154.89.105".into()],
            ..Dns::default()
        };
        let terminal = Attempt {
            cert_trusted: Some(false),
            error: Some("(60) SSL certificate problem: certificate has expired".into()),
            ..Attempt::default()
        };
        let browser = chrome(failed("ERR_CERT_DATE_INVALID"), 0);
        let d = diagnose(&Facts {
            target: &target,
            dns: &dns,
            terminal: &terminal,
            terminal_v4: None,
            terminal_v6: None,
            browser: Some(&browser),
            proxies: &Proxies::default(),
            macos: true,
            ca_env: Vec::new(),
            now: 0,
        });
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].cause.contains("certificate is rejected everywhere"));
        assert!(d[0].evidence.contains("certificate has expired"));
        assert_eq!(d[0].fix, "Renew the server's certificate.");
        assert!(
            cert_fix("(60) SSL certificate problem: self signed certificate")
                .contains("add that CA")
        );
    }

    #[test]
    fn says_when_it_fails_everywhere() {
        let target = parse_url("http://10.0.0.9/").unwrap();
        let dns = Dns {
            v4: vec!["10.0.0.9".into()],
            ..Dns::default()
        };
        let terminal = failed("(7) Failed to connect to 10.0.0.9 port 80");
        let browser = chrome(failed("ERR_CONNECTION_REFUSED"), 0);
        let d = diagnose(&Facts {
            target: &target,
            dns: &dns,
            terminal: &terminal,
            terminal_v4: None,
            terminal_v6: None,
            browser: Some(&browser),
            proxies: &Proxies::default(),
            macos: true,
            ca_env: Vec::new(),
            now: 0,
        });
        assert_eq!(d.len(), 1);
        assert_eq!(
            d[0].cause,
            "It fails from the terminal too, so it isn't the browser"
        );
    }
}
