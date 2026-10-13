# Using yuzu

Every command takes `--json` (one JSON object on stdout). Exit codes:

| Code | Meaning |
|---|---|
| 0 | Ran, and nothing's wrong: the command is allowed, the probe loaded on both sides, the wait was met |
| 1 | A check failed: the command needs approval or is denied, a side failed to load, a verification failed, a wait timed out or CI failed |
| 2 | Bad usage, or a config file that doesn't parse |

`yuzu agent-guide` prints a short version of this page, written for agents.

## status

```sh
yuzu
```

The current kube context, AWS profile, and git remotes, and which declared environment each belongs to. Always exit 0, unless the config doesn't parse.

## check

```sh
yuzu check -- 'kubectl delete ns payments'
yuzu check -- git push origin main
```

Finds every target the command line would reach and decides. One argument is read as a shell line: yuzu splits `;`, `&&`, `||`, `|`, subshells, `$( )`, and backticks; looks through `sudo`, `env`, `timeout`, `xargs`, `bash -c`, and `eval`; follows `cd` and `VAR=value`; and skips heredoc bodies.

| Program | Target |
|---|---|
| `kubectl`, `oc`, `helm` | The kube context: `--context`, else `current-context` in `$KUBECONFIG` or `~/.kube/config` |
| `ssh`, `mosh`, `sftp`, `scp`, `rsync` | The host |
| `git push` | The remote's URL, normalized to `host/owner/repo` |
| `aws` | The profile: `--profile`, else `$AWS_PROFILE`, else `default` |
| `terraform`, `tofu`, `terragrunt`, `pulumi` | The directory: `-chdir`, `-C`, `--cwd`, else the working directory |
| `gcloud`, `az`, `doctl`, `flyctl`, `heroku`, `vercel`, `netlify`, `wrangler` | The tool itself |

| Decision | When | Exit |
|---|---|---|
| allow | A read, or a write to an environment agents may write to | 0 |
| ask | A write to a read-only environment, or to anything undeclared | 1 |
| deny | Anything in an environment with `agents = "none"` | 1 |

The strictest target in the line decides. The message names a safe alternative when there is one, such as `--context k3s-pis`.

## probe

```sh
yuzu probe https://openbao.local/ui/
yuzu probe https://example.com/ --browser "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
```

| Flag | |
|---|---|
| `<URL>` | The http or https URL the user opens |
| `--browser <PATH>` | A Chromium-based browser; default `$YUZU_BROWSER`, else the first of Chrome, Edge, Brave, Chromium |
| `--timeout <SECS>` | Per load (default 15) |

Resolves the name, loads it with curl (and over IPv4 and IPv6 separately when it has both), loads it in the browser headless with an empty temporary profile, and names a likely cause when they differ: macOS Local Network permission, the browser resolving differently, a CA only curl trusts, a dead IPv6 address, mismatched proxies, a certificate both reject, or a name that resolves nowhere. Exit 0 only when both sides load.

## whoami

```sh
yuzu whoami
yuzu whoami --verify
```

Which account gh (per host), git (commit author), ssh (key per remote), and AWS will act as here, where each comes from, and what overrides it. `--verify` asks each service (`gh api user`, `ssh -T`, `aws sts get-caller-identity`) and gives a fix for each failure; exit 1 if any fails.

## wait

```sh
yuzu wait ci 3                     # PR 3's checks
yuzu wait ci                       # runs for HEAD
yuzu wait port 127.0.0.1:8200
yuzu wait url https://app.local/healthz --status 200
yuzu wait file ./build/done
yuzu wait cmd -- 'kubectl rollout status deploy/api'
yuzu wait --not --timeout 2m port 127.0.0.1:8200     # until it closes
```

| Flag | |
|---|---|
| `--timeout <DURATION>` | Give up after this long: `90s`, `10m`, `1h` (default 10m) |
| `--every <DURATION>` | How often to look (default 5s; 10s for `url`; 30s for `ci`) |
| `--not` | Until it stops being true |
| `ci [WHAT]` | A PR number, or a commit, branch, or tag (default `HEAD`); `--repo host/owner/repo` overrides the remote |
| `url --status <N>` | Wait for exactly this status instead of any 2xx or 3xx |

Progress goes to stderr, one line each time what's seen changes. Exit 0 when met; 1 when timed out or settled as failed (CI failed, with each failed check's link, or `gh` can't authenticate). CI goes through `gh`, for GitHub and GitHub Enterprise; GitLab isn't supported yet.

## sessions

```sh
yuzu sessions
yuzu sessions --tool matcha --since 7d --exclude ~/projects/mochi-donut
```

| Flag | |
|---|---|
| `--tool <NAME>` | A CLI or MCP server name to count; repeatable (default `matcha` and `yuzu`) |
| `--since <AGE>` | `14d`, `24h`, `90m` (default 14d) |
| `--project <PATH>` | Only sessions in this directory or below |
| `--exclude <PATH>` | Leave out sessions there; repeatable |
| `--dir <PATH>` | Read this transcript folder instead of finding them; repeatable |

Per tool: sessions using it, calls, calls by subcommand (`mcp:<tool>` for MCP calls) and by project, and calls made before the user mentioned the tool. Then the hook's logged asks and denies, the last ten listed for review. Always exit 0.

## Serving agents

`yuzu mcp` serves status, check, probe, whoami, and wait as MCP tools, and `yuzu hook claude-code` is the check hook. Both are described in [agents.md](agents.md).
