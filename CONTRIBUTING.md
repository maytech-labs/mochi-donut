# Contributing

Thanks for your interest. mochi-donut is pre-release, so the design is still moving; open an issue before starting anything large.

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md). Report security bugs privately, as [SECURITY.md](SECURITY.md) says, not in an issue.

## Setup

You need [rustup](https://rustup.rs). The toolchain version is pinned in `rust-toolchain.toml` and installs on first build.

```sh
cargo build
cargo xtask check   # fmt, clippy, tests, notices: the same as CI (or: make check)
make install        # build and install matcha, yuzu, ube, and taro into /usr/local/bin
```

## Making a change

- Read [AGENTS.md](AGENTS.md): it's the working guide for people too.
- Add tests with the change: unit tests beside the code to drive it, and integration tests in `crates/<tool>/tests/` that pin what users rely on. The two kinds have different rules; see [Testing in AGENTS.md](AGENTS.md#testing).
- Update `CHANGELOG.md` under **Unreleased**.
- Write commit messages as terse [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `fix(yuzu): treat unknown contexts as risky`. Details are in [AGENTS.md](AGENTS.md#commits-and-pull-requests).
- If you made a design decision, add an ADR in `docs/decisions/`.
- Open the pull request against `main`. The template asks what changed, why, which ADRs it adds, and how you tested it. CI must pass.

## License

By contributing, you agree that your contributions are licensed under the [Reciprocal Ethical AI License (REAL) 1.0](LICENSE.md), the same as the rest of the project.
