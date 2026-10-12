# 0010. `make install` into /usr/local/bin

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The owner wants `make install` to build the binaries and install them, and asked for `/usr/bin`. ADR 0002 chose `cargo xtask` over Make for repo automation.

## Decision

- A small `Makefile` wraps Cargo for the Unix install convention: `make build`, `make install`, `make uninstall`, `make check`. Repo automation stays in `cargo xtask`; the Makefile adds no logic of its own.
- **The default install location is `/usr/local/bin`, not `/usr/bin`.** On macOS, `/usr/bin` is on the sealed system volume: System Integrity Protection makes it read-only even for root (`touch /usr/bin/x` fails with "Operation not permitted"). On Linux, `/usr/bin` belongs to the package manager, and `/usr/local/bin` is the standard place for software you build yourself.
- `PREFIX` and `DESTDIR` work as usual: `make install PREFIX=~/.local`, or `DESTDIR` for staging a package.

## Consequences

- `make install` works without `sudo` on a Mac where `/usr/local/bin` is owned by the user, and with `sudo` elsewhere.
- Installing with both `make install` and Homebrew puts two copies on the `PATH`; whichever comes first wins. The README says to pick one.

## Alternatives considered

- **`/usr/bin`:** impossible on macOS and wrong on Linux.
- **Only `cargo install`:** puts binaries in `~/.cargo/bin`, but the owner wanted the familiar `make install`.
