# 0020. yuzu check covers infrastructure-as-code tools and cloud CLIs

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

ADR 0013 covered `kubectl`, `helm`, `ssh`, `scp`, `rsync`, `git push`, and `aws`, and let every other program pass. Trying the tools by hand, `yuzu check -- 'terraform apply'` came back "Allowed". AGENTS.md rule 4 says check treats anything unknown as risky, and an apply against unknown infrastructure is exactly the risk yuzu exists for.

## Decision

- **terraform, tofu, terragrunt, and pulumi** act on whatever their working directory configures, so the target is that directory: the working directory, or `-chdir=`, `-C`, or `--cwd`, with `..` resolved and the home directory written `~`. It's declared with `infra_dirs = ["~/homelab/infra/*"]`. `plan`, `show`, `validate`, `output`, `fmt`, `init`, `preview`, `graph`, `console`, `providers`, and `state` or `workspace` `list`/`show` are reads; everything else, `apply`, `destroy`, `import`, and `state rm` among them, is a write.
- **gcloud, az, doctl, flyctl (fly), heroku, vercel, netlify, and wrangler** act on the account in their own config, which yuzu can't read. The target is the tool itself, declared with `cli_tools = ["flyctl"]`. A call is a read when its first words include a read verb (`list`, `ls`, `describe`, `show`, `get`, `view`, `status`, `logs`, `whoami`, and similar), else a write.
- An undeclared write asks the user, like any other undeclared target.

## Results (2026-10-12)

- None of the 4,565 Bash commands in this Mac's Claude Code sessions use these tools, so no past decision changes.
- `terraform apply` in an undeclared directory, and `gcloud run deploy`, now need approval. `terraform plan` and `gcloud projects list` stay allowed.

## Consequences

- A cloud CLI is one target for all its accounts. Someone with personal and work accounts in one tool has to choose: declare it, and agents may write to whichever account is active; or leave it, and every write asks. Reading the tool's active account is the better answer when someone needs it.
- Other programs still pass unseen (`docker`, `gh`, `psql` against a remote host). Each needs its own reading of what it reaches.

## Alternatives considered

- **Ask for every unknown program:** safest on paper, but nearly every command would ask, and the user would learn to approve without reading.
- **Parse each tool's config for its account** (`gcloud config get-value project`, terraform backends): more precise, but a subprocess or a config format per tool. Worth it per tool once someone uses it.
