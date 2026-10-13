# 0018. yuzu probe: curl and headless Chrome with an empty profile, and a few exact rules

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The design wants `yuzu probe <url>` to load a URL from the terminal and from the user's browser, report the exact failure on each side, and name the likely cause. Its bar: replaying the OpenBao session, name the macOS Local Network permission on the first failure. It suggests driving the browser through the Chrome DevTools Protocol with a temporary copy of the profile's relevant settings, and leaves Chrome-only against Chrome and Safari as an open question.

## Decision

- **The terminal side is `curl`**, because that's what an agent's shell runs: the same resolver, CA bundle, and proxy variables. Any HTTP response counts as reached. When the name has both IPv4 and IPv6 addresses, it also loads over each with `-4` and `-6`.
- **DNS is the system resolver** (`getaddrinfo`, through Rust's standard library), the one both sides start from.
- **The browser side is headless Chrome's `--dump-dom`, not the DevTools Protocol.** New headless mode logs `Page load failed: net::ERR_X` on stderr when a load fails and prints the DOM when it works; old headless renders an error page whose DOM holds the code, and both are read. That's every fact the rules need, without a websocket client and with no new dependency. yuzu grew from 851 KB to 918 KB.
- **An empty profile in a temporary directory, deleted afterward**, not a copy of the user's. The causes the design names belong to the app or the system, not the profile: the macOS Local Network grant is per app, the certificate store and the proxy settings are system-wide, and an update replaces the app. Copying a profile risks copying cookies and passwords, which the design rules out. The cost: per-profile settings such as Chrome's Secure DNS and proxy extensions aren't seen.
- **Browsers:** `--browser <path>`, else `$YUZU_BROWSER`, else the first of Chrome, Edge, Brave, and Chromium in `/Applications`, `~/Applications`, or on `PATH`. **Not Safari:** it has no headless mode, and driving it means `safaridriver`, which the user has to enable by hand. This answers the design's open question 1.
- **The browser's update time** is the app bundle's `Info.plist` time (macOS), which an update replaces, else the binary's.
- **Diagnosis rules fire only when all their symptoms match:**
  - *macOS Local Network permission*: on macOS, curl reached the host, an address is private (RFC 1918, 100.64/10, link-local, IPv6 ULA) or the name ends in `.local`, and the browser got `ERR_ADDRESS_UNREACHABLE`. A browser update in the last 7 days is named in the evidence.
  - *The browser resolves differently*: the system resolver answered, the browser got `ERR_NAME_NOT_RESOLVED`.
  - *The browser doesn't trust the certificate curl does*: curl verified it, the browser got `ERR_CERT_*`. A CA bundle only curl reads (`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `SSL_CERT_DIR`) is named.
  - *The IPv6 address doesn't answer*: `-4` loads and `-6` doesn't.
  - *Different proxies*: one side loads and the other doesn't, and only one of them has a proxy (shell variables against `scutil --proxy`).
  - *It doesn't resolve anywhere*, or *it fails from the terminal too*: both sides fail, so it isn't the browser.
  With none, the output says no rule matches and that it won't guess.
- **Exit 0 only when both sides load.** A missing browser exits 1: the probe couldn't show it works from where the user is. A bad URL exits 2.

## Results (2026-10-12)

- The OpenBao session as facts (curl 200 from 10.0.0.8; Chrome `ERR_ADDRESS_UNREACHABLE`; Chrome updated at 14:58) gives exactly one cause, the Local Network permission, with the update named. It's a unit test.
- Against this Mac's Chrome 155: a local server loads on both sides in about a second; `nonexistent.invalid` is reported as not resolving from this machine. `10.0.0.8:8200` gives `ERR_CONNECTION_REFUSED` in Chrome, so Chrome has the Local Network permission today, and the original failure couldn't be reproduced live.
- Tests use a local server and a stub browser script, so they need neither the network nor Chrome.

## Consequences

- The design's bar is met on the session's facts, not live. A live check needs the permission off, which is the user's to toggle.
- Rules are few and exact. New ones should come from real failures, each with a test built from that failure's facts.
- Each probe takes the time of two to four curl loads and one browser launch: about a second when things work, up to `--timeout` per side when they don't.

## Alternatives considered

- **The DevTools Protocol:** richer (the certificate chain, the proxy decision, timings), but needs a websocket client. Chrome's `--remote-debugging-pipe` avoids websockets, but needs extra file descriptors set up in the child, which takes `unsafe` code this repo forbids.
- **A Rust HTTP client for the terminal side:** no dependency on curl, but it wouldn't see what the agent's shell sees, which is the point of the comparison.
- **Copying the user's profile:** catches profile-level settings, at the risk of reading cookies and passwords.
