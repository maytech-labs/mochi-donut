# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- Cargo workspace with `matcha` and `yuzu`. Every planned command parses and reports that it isn't built yet (exit code 3, or a JSON body with `--json`).
- `matcha agent-guide` and `yuzu agent-guide`.
- `matcha census`: compare alternatives (`--alt`) or spellings (`--spelling`) across tracked files, by language, with examples.
- `matcha recipe`: `--check` the working tree or a past `--commit` for missing companion files, with the usual fix; `--like` for what similar changes touched; `--replay` to measure its own precision on the repo's history.
- Design doc and ADRs 0001–0006.
