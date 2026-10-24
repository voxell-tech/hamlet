# Releases

`.github/workflows/release.yml` builds the complete installation for macOS
(Apple Silicon) and Linux (x64), plus a Windows x64 window-only archive.
The macOS and Linux archives include:

- `hamlet`, the CLI and launcher;
- `hamlet-server`, the server;
- `hamlet-app`, the window;
- `pi/` with its source, lockfile and production `node_modules`;
- licenses and a runtime setup README.

Node is **not** bundled. Install the version in `harnesses/pi/.nvmrc`
(currently 24.1.0). It must be on PATH or selected by `HAMLET_NODE`.
No checkout or npm install is needed after extraction. Keep the archive's
layout. The server finds sibling `pi/src/main.ts`, then the macOS bundle's
`Contents/Resources/pi/src/main.ts`, then the checkout it was built from,
and refuses to start, naming `HAMLET_PI_HARNESS`, when none of them exists.

## Archive layout

Linux uses a tarball with the binaries and pi in its top-level named
directory. Run `./hamlet`; working bubblewrap is required. The server
probes it before starting and has no insecure fallback.

macOS uses a zip containing a named directory with `Hamlet.app`, licenses
and setup notes. The binaries live in `Hamlet.app/Contents/MacOS`, and pi
in `Contents/Resources/pi`. Helper binaries are signed before the app;
scripts and package data are sealed as resources.
The bundle launches `hamlet`, which starts or reuses the server and opens
`hamlet-app`. It is ad-hoc signed, not developer-signed or notarized; macOS
may ask before opening it. Finder may not see a shell-installed Node, so
launch `Contents/MacOS/hamlet` from a terminal with `HAMLET_NODE` if needed.

The Windows zip contains `hamlet-app.exe`, licenses and setup notes, with
no server, Pi or Node requirement on the Windows machine. Set `HAMLET_SERVER`
to a remote server's WebSocket URL through an SSH tunnel and `HAMLET_TOKEN`
to its connection token, then run the window. Keep the token private.
Windows has no supported server sandbox; this is a remote client, not a
local server installation. Neither packaging nor Linux tests prove macOS or Windows
behavior. Remote workspaces require a separately run workspace worker;
it is not included in these archives.

## Building an archive

Build natively on the target platform: npm's optional/native dependencies
must match the target. The release job uses the pinned Node version and
builds all three binaries with `cargo xtask build` before invoking
`.github/scripts/package.sh`:

```sh
cargo xtask build --target x86_64-unknown-linux-gnu
.github/scripts/package.sh x86_64-unknown-linux-gnu v0.1.0
```

For macOS use `aarch64-apple-darwin`. The script accepts an optional third
argument for the profile directory, defaults to `release`, and respects
`CARGO_TARGET_DIR`. It creates a clean staging directory, installs pi with
`npm ci --omit=dev`, and writes the archive under `dist/`. It does not
compile TypeScript.

For Windows, build only `cargo build --release --locked -p hamlet_app --bin
hamlet-app --target x86_64-pc-windows-msvc`, then run the script in Bash with
that target. This branch needs 7-Zip, not Node or npm. It packages only the
window; no Windows server build is attempted.

`cargo xtask install` is a different, source-development workflow: it installs
all three binaries into Cargo's bin directory but leaves pi in the build
checkout. Retain that checkout, or set `HAMLET_PI_HARNESS` to another fully
installed pi entry. `cargo xtask uninstall` removes the three binaries, not homes.

## Publishing

The release workflow runs only for pushed `v*` tags or an explicit manual
run. A manual run uploads three workflow artifacts and publishes no release.
A tag run first checks that the tag, less its leading `v`, is the version
in `Cargo.toml`'s `[workspace.package]`, then creates the GitHub release or
replaces matching assets on an existing release. Pushing a tag is
therefore a publishing action, not a local packaging check.

Validate an extracted archive from outside the checkout on a fresh
`HAMLET_HOME`: start the server, complete a faux chat, stop it and check
that its publication is removed. Do not point release smoke tests at a
personal home. This checks discovery and dependencies, not exhaustive
behavior, real-provider access, or every platform.
