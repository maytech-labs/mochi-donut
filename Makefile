# Build and install matcha and yuzu. Repo automation lives in `cargo xtask`;
# this file only wraps Cargo for the `make install` convention
# (docs/decisions/0010-make-install.md).

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO  ?= cargo
BINS   := matcha yuzu

.PHONY: help build install uninstall check

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  %-10s %s\n", $$1, $$2}'

build: ## Build release binaries into target/release
	$(CARGO) build --release --locked -p mochi-matcha -p mochi-yuzu

install: build ## Install matcha and yuzu into $(BINDIR) (default /usr/local/bin)
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 0755 $(addprefix target/release/,$(BINS)) "$(DESTDIR)$(BINDIR)/"
	@echo "Installed $(BINS) to $(DESTDIR)$(BINDIR)"

uninstall: ## Remove matcha and yuzu from $(BINDIR)
	rm -f $(foreach b,$(BINS),"$(DESTDIR)$(BINDIR)/$(b)")

check: ## Everything CI runs: fmt, clippy, tests
	$(CARGO) xtask check
