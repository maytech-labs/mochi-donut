# Contributing

Thanks for your interest. mochi-donut is pre-release, so the design is still moving; open an issue before starting anything large.

## Setup

You need [rustup](https://rustup.rs). The toolchain version is pinned in `rust-toolchain.toml` and installs on first build.

```sh
cargo build
cargo xtask check   # fmt, clippy, tests: the same as CI
```

## Making a change

- Read [AGENTS.md](AGENTS.md): it's the working guide for people too.
- Add tests with the change. Integration tests in `crates/<tool>/tests/` run the real binary.
- Update `CHANGELOG.md` under **Unreleased**.
- Write commit messages as terse [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `fix(yuzu): treat unknown contexts as risky`. Details are in [AGENTS.md](AGENTS.md#commits-and-pull-requests).
- If you made a design decision, add an ADR in `docs/decisions/`.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
