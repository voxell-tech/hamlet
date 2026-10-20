# Builds the Rust CLI, server and window, with the Node pi harness.
# Needs Rust, the Node of harnesses/pi/.nvmrc and git.
#
#   make            build all three binaries (release) and install pi dependencies
#   make install    put the three binaries in ~/.cargo/bin
#   make uninstall  remove all three from ~/.cargo/bin
#   make dev        try this checkout on a scratch home (HAMLET_HOME picks it)
#   make check      every check a commit needs
#   make fmt        format Rust, the protocol schema and pi
#   make clean      remove the Rust build output
#
# The installed server finds pi in this checkout, so keep it where
# it was installed from, and run `make install` again after pulling.

CARGO ?= cargo
NPM ?= npm

FYNIX := vendor/fynix/Cargo.toml
# npm ci rewrites this file, so it is newer than the lockfile it installed.
SCHEMA := crates/hamlet_protocol/schema/node_modules/.package-lock.json
PI := harnesses/pi/node_modules/.package-lock.json

.DEFAULT_GOAL := build
.PHONY: build install uninstall dev check fmt clean

$(FYNIX):
	git submodule update --init vendor/fynix

$(SCHEMA): crates/hamlet_protocol/schema/package.json crates/hamlet_protocol/schema/package-lock.json
	$(NPM) ci --prefix crates/hamlet_protocol/schema

$(PI): harnesses/pi/package.json harnesses/pi/package-lock.json
	$(NPM) ci --prefix harnesses/pi

build: $(FYNIX) $(PI)
	$(CARGO) build --release --locked -p hamlet_cli -p hamlet_server -p hamlet --bin hamlet --bin hamlet-server --bin hamlet-app

install: $(FYNIX) $(PI)
	$(CARGO) install --locked --force --path crates/hamlet_cli
	$(CARGO) install --locked --force --path crates/hamlet_server --bin hamlet-server
	$(CARGO) install --locked --force --path . --bin hamlet-app

uninstall:
	-$(CARGO) uninstall hamlet_cli
	-$(CARGO) uninstall hamlet_server
	-$(CARGO) uninstall hamlet

dev: $(FYNIX) $(PI)
	$(CARGO) run -p hamlet_cli -- dev

check: $(FYNIX) $(SCHEMA) $(PI)
	$(CARGO) +nightly fmt --check
	$(CARGO) build --all-targets
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test
	$(CARGO) clippy -p hamlet_acceptance --all-targets --features acceptance -- -D warnings
	$(CARGO) test -p hamlet_acceptance --features acceptance
	$(CARGO) clippy -p hamlet_server --all-targets --features harness -- -D warnings
	$(CARGO) test -p hamlet_server --features harness
	$(NPM) run check --prefix crates/hamlet_protocol/schema
	$(NPM) test --prefix crates/hamlet_protocol/schema
	$(NPM) run check --prefix harnesses/pi
	$(NPM) test --prefix harnesses/pi

fmt: $(SCHEMA) $(PI)
	$(CARGO) +nightly fmt
	$(NPM) run format --prefix crates/hamlet_protocol/schema
	$(NPM) run format --prefix harnesses/pi

clean:
	$(CARGO) clean
