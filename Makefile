# Builds Hamlet's three parts: the `hamlet` CLI and the `hamlet-app`
# window in Rust, and the agent server in TypeScript, which Node runs
# from server/ with no compile step, so its packages are its build.
# Needs Rust, the Node of server/.nvmrc and git.
#
#   make            build the CLI and the window (release) and the server
#   make install    the same, with hamlet and hamlet-app put in ~/.cargo/bin
#   make uninstall  remove both from ~/.cargo/bin
#   make dev        try this checkout on a scratch home (HAMLET_HOME picks it)
#   make check      every check a commit needs
#   make fmt        format the Rust and the server code
#   make clean      remove the Rust build output
#
# The installed CLI runs the server from this checkout, so keep it where
# it was installed from, and run `make install` again after pulling.

CARGO ?= cargo
NPM ?= npm

FYNIX := vendor/fynix/Cargo.toml
# npm ci rewrites this file, so it is newer than the lockfile it installed.
SERVER := server/node_modules/.package-lock.json

.DEFAULT_GOAL := build
.PHONY: build install uninstall dev check fmt clean

$(FYNIX):
	git submodule update --init vendor/fynix

$(SERVER): server/package.json server/package-lock.json
	$(NPM) ci --prefix server

build: $(FYNIX) $(SERVER)
	$(CARGO) build --release --locked -p hamlet_cli -p hamlet --bin hamlet --bin hamlet-app

install: $(FYNIX) $(SERVER)
	$(CARGO) install --locked --force --path crates/hamlet_cli
	$(CARGO) install --locked --force --path . --bin hamlet-app

uninstall:
	-$(CARGO) uninstall hamlet_cli
	-$(CARGO) uninstall hamlet

dev: $(FYNIX) $(SERVER)
	$(CARGO) run -p hamlet_cli -- dev

check: $(FYNIX) $(SERVER)
	$(CARGO) +nightly fmt --check
	$(CARGO) build --all-targets
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test
	$(CARGO) clippy -p hamlet_acceptance --all-targets --features acceptance -- -D warnings
	$(CARGO) test -p hamlet_acceptance --features acceptance
	$(NPM) run check --prefix server
	$(NPM) test --prefix server

fmt: $(SERVER)
	$(CARGO) +nightly fmt
	$(NPM) run format --prefix server

clean:
	$(CARGO) clean
