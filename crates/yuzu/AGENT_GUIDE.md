yuzu: what your environment is, before you act on it.

When to run what
  Before acting on a cluster, cloud account, or host   yuzu                       where the shell points, and whose it is
  Before a command that might change one               yuzu check -- '<command>'  exit 1 if it needs the user's approval
  Every Bash command, automatically                    yuzu hook claude-code      a Claude Code PreToolUse hook
  When it works from your shell but the user says it doesn't  yuzu probe <url>
                                                       curl and the user's browser side by side, and the likely cause

What check looks at
  kubectl, helm      the kube context: --context, else current-context in $KUBECONFIG or ~/.kube/config
  ssh, scp, rsync    the host
  git push           the remote's URL, normalized (github.com/owner/repo)
  aws                the profile: --profile, else $AWS_PROFILE, else default
  It reads through ;, &&, |, $( ), sudo, env, bash -c, and VAR=value prefixes.

Decisions
  write to an env agents may write to (class mine)   allowed
  read anything not marked agents = "none"           allowed
  write to work, production, or anything undeclared  needs the user's approval
  anything in an env with agents = "none"            denied
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
  # agents = "write" | "read-only" | "none"  (default: write for mine, else read-only)
  The first env with a matching pattern wins.

Flags on every command
  --json   one JSON object on stdout

Exit codes
  0  ran, nothing wrong (or the command is allowed)
  1  a check failed (the command needs approval or is denied)
  2  bad usage
  A config that doesn't parse blocks every check until it's fixed.
