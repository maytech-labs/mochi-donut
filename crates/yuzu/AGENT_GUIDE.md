yuzu: what your environment is, before you act on it.

When to run what
  Before acting on a cluster, cloud account, or host   yuzu                       where the shell points, and whose it is
  Before a command that might change one               yuzu check -- '<command>'  exit 1 if it needs the user's approval
  Every Bash command, automatically                    yuzu hook claude-code      a Claude Code PreToolUse hook
  When it works from your shell but the user says it doesn't  yuzu probe <url>
                                                       curl and the user's browser side by side, and the likely cause
  Before pushing, opening a PR, or calling a cloud API; when auth fails
    yuzu whoami                which account gh, git, ssh, aws act as here, and what overrides it
    yuzu whoami --verify       ask each service (network); exit 1 if one fails, with the fix
  Instead of sleep or a poll loop
    yuzu wait ci [<pr|ref>]    until CI finishes: exit 0 passed, 1 failed (with links) or timed out
    yuzu wait port|url|file <target>, yuzu wait cmd -- '<command>'
                               --timeout 10m (default), --every 30s, --not
  To see how agents use a tool, and what the hook blocked
    yuzu sessions [--tool <name>] [--since 14d] [--exclude <repo>]

What check looks at
  kubectl, helm      the kube context: --context, else current-context in $KUBECONFIG or ~/.kube/config
  ssh, scp, rsync    the host
  git push           the remote's URL, normalized (github.com/owner/repo)
  aws                the profile: --profile, else $AWS_PROFILE, else default
  terraform, tofu,   the directory: -chdir/--cwd, else the working directory
    terragrunt, pulumi
  gcloud, az, fly,   the tool itself (its own config holds the account)
    doctl, heroku, vercel, netlify, wrangler
  It reads through ;, &&, |, $( ), sudo, env, bash -c, and VAR=value prefixes.

Decisions
  write to an env agents may write to (class mine)   allowed
  read anything not marked agents = "none"           allowed
  write to work, production, or anything undeclared  needs the user's approval
  anything in an env with agents = "none"            denied
  changing yuzu's own environments.toml              needs the user's approval
  When blocked, don't look for another way to run it. Use the safe target the
  message names, or ask the user.

What probe does
  Loads the URL with curl and with headless Chrome (or Edge, Brave, Chromium;
  --browser <path> to pick) in an empty temporary profile: no cookies,
  history, or passwords are read. Reports DNS, each side's result, and a
  likely cause when they differ: macOS Local Network permission, Secure DNS,
  a CA only curl trusts, a dead IPv6 address, mismatched proxies. Exit 0 only
  when both sides load. Don't ask the user to check their browser first; run
  this and tell them the cause and the fix.

Declaring environments: ~/.config/yuzu/environments.toml (or $YUZU_CONFIG)
  [[env]]
  name = "homelab"
  class = "mine"                     # mine, work, or production
  kube_contexts = ["k3s-pis"]        # patterns: * and ?
  ssh_hosts = ["samwise", "10.0.0.*"]
  git_remotes = ["github.com/me/*"]
  aws_profiles = ["personal"]
  infra_dirs = ["~/homelab/infra/*"] # where terraform and friends run
  cli_tools = ["flyctl"]             # cloud CLIs whose account is this env's
  # agents = "write" | "read-only" | "none"  (default: write for mine, else read-only)
  The first env with a matching pattern wins.
  To add one: yuzu env add <env> <key> <pattern>... [--class mine]
  Changing this file asks the user. Don't run env add to get past an ask;
  suggest it, and let the user approve it.

Flags on every command
  --json   one JSON object on stdout

Exit codes
  0  ran, nothing wrong (or the command is allowed)
  1  a check failed (the command needs approval or is denied)
  2  bad usage
  A config that doesn't parse blocks every check until it's fixed.
