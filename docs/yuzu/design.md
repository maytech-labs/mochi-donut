# yuzu design

<!-- matcha: links-only -->
<!-- Examples quote this Mac's contexts and other repos, so `matcha drift` checks only this doc's links. -->

yuzu answers questions about the user's environment: which cluster, account, host, and remote a command would touch and whose they are, whether something works from where the user is, which account a tool will act as, and whether a thing is done yet. It works in any directory, repo or not. The cross-tool design is in [../design.md](../design.md); how to run it is in [usage.md](usage.md), and the environment declarations are in [config.md](config.md).

| Command | Question |
|---|---|
| `yuzu` (status) | Where does my shell point, and whose is it? |
| [`check`](#yuzu-check-what-will-this-command-touch) | What will this command touch, and is it allowed? |
| [`probe`](#yuzu-probe-does-it-work-from-where-the-user-is) | Does it work from where the user is? |
| [`whoami`](#yuzu-whoami-which-account-will-this-act-as) | Which account will this act as? |
| [`wait`](#yuzu-wait-is-it-done-yet) | Is it done yet? |
| [`sessions`](#yuzu-sessions-how-do-agents-use-these-tools) | How do agents use these tools? |

## `yuzu check`: what will this command touch?

**Problem.** Agents run commands against whatever the shell is pointed at. The current Kubernetes context, cloud profile, SSH host, or git remote might be a production system the user never meant to involve. Existing tools show the current context; none of them know which contexts are whose, or stop an agent before it acts.

**What it does.** Classifies every target a command would reach, using a declaration the user writes once, and gates risky ones.

```
$ yuzu
  kube context   work-nonprod-us-east-1   WORK (nonprod)
  aws profile    default                      unset
  git remote     upstream → github.com/trammel-may/anchor   MINE
  
$ yuzu check -- kubectl apply -f openbao.yaml
  BLOCKED: kubectl would use context work-nonprod-us-east-1 (WORK).
  Your declared homelab context is k3s-pis. Re-run with --context k3s-pis, or allow it explicitly.
```

**How it works.**
- A user-level file (`~/.config/yuzu/environments.toml`) declares environments with patterns and a class: `mine`, `work`, or `production`, plus whether agents may write to it.
  ```toml
  [[env]]
  name = "homelab"
  class = "mine"
  kube_contexts = ["k3s-pis"]
  ssh_hosts = ["pi-1", "pi-2", "pi-3"]

  [[env]]
  name = "work"
  class = "production"
  kube_contexts = ["*-prod-*"]
  agents = "read-only"
  ```
- `yuzu check -- <command>` resolves what the command would act on (the effective kube context and namespace, `--context` flags, `KUBECONFIG`, AWS and GCP profiles, `ssh` hosts, `git push` remotes, `docker` contexts) and compares it with the declarations.
- **The intended use is as a pre-command hook** (for example, a Claude Code `PreToolUse` hook on Bash), so the check runs whether or not the agent remembers it. Read-only commands on work environments pass; writes to anything not declared `mine` need explicit approval.
- Unknown targets count as risky.

**Done when.** With the declarations above, an agent session that tries a write against any context other than the homelab is stopped before the command runs, and reads still work.

**Status (2026-10-12): built, on trial.** `yuzu`, `yuzu check`, and `yuzu hook claude-code` work for `kubectl`, `helm`, `ssh`, `scp`, `rsync`, `git push`, and `aws`. Writes outside `mine` ask the user rather than being denied. Replaying 2,118 past Bash commands from this Mac stopped one, correctly, and blocked nothing it shouldn't have. The two-week hook trial from [ADR 0004](../decisions/0004-prove-before-announcing.md) starts when the hook is installed. Details in [ADR 0013](../decisions/0013-yuzu-check.md) and [ADR 0014](../decisions/0014-claude-code-hook.md).

**Since (2026-10-13):** terraform, tofu, terragrunt, and pulumi are checked by the directory they run in (`infra_dirs`), and cloud CLIs (gcloud, az, doctl, flyctl, heroku, vercel, netlify, wrangler) by the tool (`cli_tools`); see [ADR 0020](../decisions/0020-yuzu-infra-and-cloud-clis.md). The hook logs its asks and denies for review ([ADR 0025](../decisions/0025-yuzu-sessions.md)). The two-week trial hasn't started: the hook isn't installed yet.

**Since (2026-10-15):** any change to yuzu's own `environments.toml` asks, from Bash or Claude Code's file tools, so an agent can't approve itself ([ADR 0036](../decisions/0036-yuzu-guards-its-config.md)).

## `yuzu probe`: does it work from where the user is?

**Problem.** An agent verifies from its own shell, but the user hits the service from a browser with its own DNS, permissions, proxy, VPN, certificate store, and cache. "Works for me" and "can't be reached" can both be true, and the agent can't see the difference.

**What it does.** Runs a check the way the user's browser would, and reports the exact failure.

```
$ yuzu probe https://openbao.local/ui/
  terminal (curl)           200   10.0.0.8   cert trusted
  Chrome (user profile)     FAIL  ERR_ADDRESS_UNREACHABLE
  likely cause: macOS Local Network permission for Google Chrome
    (Chrome updated to 155.0.8059.40 at 14:58; permission often resets on update)
```

**How it works.**
- Drives the user's installed browser in headless mode through the Chrome DevTools Protocol (and WebKit for Safari where possible), with a temporary copy of the profile's relevant settings rather than the profile itself.
- Captures the network error code, the DNS answers (IPv4 and IPv6), the certificate chain and its trust result, and the proxy decision.
- A small set of **diagnosis rules** maps symptoms to likely causes: "terminal succeeds, browser `ERR_ADDRESS_UNREACHABLE` on a private address" points at Local Network permission; "IPv6 refused, IPv4 OK" points at the name's AAAA records; a recent browser update is reported when present.
- Never reads cookies, history, or saved passwords, and never sends anything off the machine.

**Done when.** Replaying today's OpenBao session, `yuzu probe` names the Local Network permission on the first failure, without a round trip to the user.

**Status (2026-10-12): built; the bar is met on the session's facts.** curl and headless Chrome (or Edge, Brave, Chromium) with an empty temporary profile, the system resolver, and the proxy settings, mapped to causes by six exact rules. The OpenBao session's facts give exactly the Local Network permission, with Chrome's update named. Chrome has the permission on this Mac today, so the failure couldn't be reproduced live. It reads the error from headless Chrome's output instead of the DevTools Protocol, and uses an empty profile instead of a copy. See [ADR 0018](../decisions/0018-yuzu-probe.md).

**Since (2026-10-13):** a certificate both curl and the browser reject is reported as the server's problem, with a fix chosen from curl's reason ([ADR 0021](../decisions/0021-usability-from-trying-the-tools.md)).

## `yuzu whoami`: which account will this act as?

**Problem.** Opening a PR failed twice in one session because `GH_TOKEN` held a token GitHub rejected, and it silently overrode a working `gh` login. Nothing showed which credentials a tool would use, or that one hid another.

**What it does.** Lists, for the current directory, which account gh, git, ssh, and AWS will act as, where each comes from, and what overrides it. Offline by default; `--verify` asks each service, and a failure says what to do.

```
$ yuzu whoami --verify
  github  github.com: token from GH_TOKEN   (the environment)
          ! GH_TOKEN is set, so gh and the GitHub API use it for github.com instead of trammel-may's stored login.
          FAIL: gh: Bad credentials (HTTP 401)
          fix: Unset GH_TOKEN (or replace it with a valid token); gh then uses its stored login.
```

**Done when.** A credentials problem is told apart from a code problem in one call. **Status (2026-10-13): built.** On this Mac it found the rejected `GH_TOKEN` and an invalid AWS profile token. See [ADR 0022](../decisions/0022-yuzu-whoami.md).

## `yuzu wait`: is it done yet?

**Problem.** Agents wait on CI, deploys, ports, and pods constantly, and every hand-written poll loop is a new chance to get the timeout, the exit code, or the end condition wrong.

**What it does.** Waits for CI (a PR or commit, through `gh`), a port, a URL, a file, or any command, with a timeout. Exit 0 when it holds, 1 when it fails or times out; progress only when what's seen changes.

```
$ yuzu wait ci 3
Waiting up to 10m00s for CI for 3 to finish.
  [1s] 2 of 2 pending (check (macos-15), check (ubuntu-24.04))
  [32s] 1 of 2 pending (check (macos-15))
  [1m03s] all 2 passed
Done after 1m03s (3 looks): all 2 passed
```

**Status (2026-10-13): built.** GitLab CI isn't supported yet; `wait cmd` covers it meanwhile. See [ADR 0023](../decisions/0023-yuzu-wait.md).

## `yuzu sessions`: how do agents use these tools?

**Problem.** [ADR 0004](../decisions/0004-prove-before-announcing.md) judges the tools by use: whether the hook blocks legitimate commands, and whether agents reach for the tools unprompted. Both live in agent transcripts and the hook's decisions, which nothing read.

**What it does.** Counts, from Claude Code transcripts, how agents used given tools: calls, subcommands, projects, and calls made before the user mentioned the tool. Lists the hook's logged asks and denies for review.

**Status (2026-10-13): built.** Over a week, excluding this repo: one unprompted matcha call. That's the baseline for the trial. See [ADR 0025](../decisions/0025-yuzu-sessions.md).

## Open questions

1. **Sharing environment declarations.** Per user only, or can a repo ship its own ("this repo deploys to the production host")?
2. **Commands yuzu doesn't read yet:** `docker` contexts, `gh` itself, `psql` against a remote host, `kubectl --server`.
3. **Cloud CLI accounts.** A tool is one target for all its accounts; reading each tool's active account would be more precise.
