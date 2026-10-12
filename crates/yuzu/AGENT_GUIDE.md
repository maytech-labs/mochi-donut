yuzu: what your environment is, before you act on it.

Status: pre-release. Commands marked (planned) exit with code 3 and say so.

When to run what
  Before acting on a cluster, cloud account, or host   yuzu                       (planned)
  Before every shell command                           yuzu check -- <command>    (planned; meant to run from a hook)
  When it works for you but not for the user           yuzu probe <url>           (planned)

Environments are declared once per user in ~/.config/yuzu/environments.toml
as mine, work, or production. Unknown targets count as risky.

Flags on every command
  --json   one JSON object on stdout

Exit codes
  0  ran, nothing wrong (or the command is allowed)
  1  a check failed (the command is blocked)
  2  bad usage       3  not built yet
