# Builds the Rust CLI, server and window, with the Node pi harness.
# Needs Rust, the Node of harnesses/pi/.nvmrc and git.
#
#   make            build all three binaries (release) and install pi dependencies
#   make all        build every binary: those three, hamlet-workspace and hamlet_protocol_gen
#   make workspace-image  build the remote workspace manager/worker image with Docker
#   make install    put the three binaries in ~/.cargo/bin
#   make uninstall  remove all three from ~/.cargo/bin
#   make dev        try this checkout on a scratch home (HAMLET_HOME picks it)
#   make check      every check a commit needs
#   make fmt        format Rust and pi
#   make clean      remove the Rust build output
#
# The installed server finds pi in this checkout, so keep it where
# it was installed from, and run `make install` again after pulling.

CARGO ?= cargo
NPM ?= npm
DOCKER ?= docker
# Extra flags for the Cargo builds, such as `--target aarch64-apple-darwin`.
CARGO_FLAGS ?=
# Extra flags for `docker build`, such as `--platform linux/amd64`.
DOCKER_FLAGS ?=
WORKSPACE_IMAGE ?= hamlet-workspace:$(shell git rev-parse --short HEAD)

FYNIX := vendor/fynix/Cargo.toml
# npm ci rewrites this file, so it is newer than the lockfile it installed.
PI := harnesses/pi/node_modules/.package-lock.json

.DEFAULT_GOAL := build
.PHONY: build all workspace-image install uninstall dev check fmt clean

$(FYNIX):
	git submodule update --init vendor/fynix

$(PI): harnesses/pi/package.json harnesses/pi/package-lock.json
	$(NPM) ci --prefix harnesses/pi

build: $(FYNIX) $(PI)
	$(CARGO) build --release --locked -p hamlet_cli -p hamlet_server -p hamlet --bin hamlet --bin hamlet-server --bin hamlet-app $(CARGO_FLAGS)

# hamlet-harness-driver is left out: it needs the harness feature, and
# the harness tests build it. hamlet-workspace runs only on Linux.
all: $(FYNIX) $(PI)
	$(CARGO) build --release --locked --workspace --bins $(CARGO_FLAGS)

# The revision label tells which commit a deployed image was built from.
workspace-image: $(FYNIX)
	$(DOCKER) build -f crates/hamlet_workspace/Dockerfile \
		--label org.opencontainers.image.revision=$(shell git rev-parse HEAD) \
		$(DOCKER_FLAGS) -t $(WORKSPACE_IMAGE) .

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

check: $(FYNIX) $(PI)
	$(CARGO) +nightly fmt --check
	$(CARGO) build --all-targets
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test
	$(CARGO) clippy -p hamlet_acceptance --all-targets --features acceptance -- -D warnings
	$(CARGO) test -p hamlet_acceptance --features acceptance
	$(CARGO) clippy -p hamlet_server --all-targets --features harness -- -D warnings
	$(CARGO) test -p hamlet_server --features harness
	$(NPM) run check --prefix harnesses/pi
	$(NPM) test --prefix harnesses/pi

fmt: $(PI)
	$(CARGO) +nightly fmt
	$(NPM) run format --prefix harnesses/pi

clean:
	$(CARGO) clean
