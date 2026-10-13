# Configuring yuzu

## environments.toml

yuzu reads `$YUZU_CONFIG`, else `$XDG_CONFIG_HOME/yuzu/environments.toml`, else `~/.config/yuzu/environments.toml`. Without the file, nothing is declared, so every write asks.

```toml
[[env]]
name = "homelab"
class = "mine"
kube_contexts = ["k3s-pis"]
ssh_hosts = ["pi-1", "pi-2", "10.0.0.*"]
git_remotes = ["github.com/me/*"]
aws_profiles = ["personal"]
infra_dirs = ["~/homelab/infra/*"]
cli_tools = ["flyctl"]

[[env]]
name = "payments"
class = "production"
agents = "none"
kube_contexts = ["*-payments-*"]

[[env]]
name = "work"
class = "work"
kube_contexts = ["*-nonprod-*"]
git_remotes = ["github.com/acme/*"]
```

| Key | |
|---|---|
| `name` | Shown in messages |
| `class` | `mine`, `work`, or `production` |
| `agents` | `write`, `read-only`, or `none`. Default: `write` for `mine`, `read-only` otherwise. |
| `kube_contexts` | Context names |
| `ssh_hosts` | Hosts as written on the command line |
| `git_remotes` | Normalized remotes: `github.com/owner/repo`, matching SSH and HTTPS spellings alike |
| `aws_profiles` | Profile names |
| `infra_dirs` | Directories terraform, tofu, terragrunt, or pulumi run in, with `~` for home |
| `cli_tools` | Cloud CLIs whose configured account is this environment's |

Patterns take `*` and `?`. The first environment with a matching pattern wins. Unknown keys are errors, and a file that doesn't parse makes every check ask until it's fixed, so a typo can't quietly allow everything.

## Environment variables

| Variable | Used for |
|---|---|
| `YUZU_CONFIG` | The environments file |
| `KUBECONFIG` | Kubeconfig files, as kubectl reads them |
| `AWS_PROFILE`, `AWS_DEFAULT_PROFILE`, `AWS_ACCESS_KEY_ID` | The AWS identity `check` and `whoami` report |
| `GH_TOKEN`, `GITHUB_TOKEN`, `GH_ENTERPRISE_TOKEN`, `GITHUB_ENTERPRISE_TOKEN`, `GH_CONFIG_DIR` | gh's identity, for `whoami` |
| `YUZU_BROWSER` | The browser `probe` uses |
| `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` | The terminal's proxies, compared by `probe` |
| `CURL_CA_BUNDLE`, `SSL_CERT_FILE`, `SSL_CERT_DIR` | CA bundles only curl reads, named by `probe` |
| `CLAUDE_CONFIG_DIR` | Where `sessions` finds transcripts, besides `~/.claude*` |
| `XDG_STATE_HOME` | Where the hook's decision log goes (default `~/.local/state`) |
| `YUZU_NO_LOG` | Set to turn the decision log off |

## Files yuzu writes

Only `~/.local/state/yuzu/decisions.jsonl` (or under `$XDG_STATE_HOME`): one line per ask or deny from the hook, with the time, decision, command, directory, and reason. `probe` makes a temporary browser profile and deletes it.
