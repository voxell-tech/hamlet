# Releases

`.github/workflows/release.yml` builds the complete installation for macOS
(Apple Silicon), Linux (x64) and Windows (x64). Each archive includes:

- `hamlet`, the CLI and launcher;
- `hamlet_server`, the server;
- `hamlet_app`, the window;
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
`hamlet_app`. It is ad-hoc signed, not developer-signed or notarized; macOS
may ask before opening it. Finder may not see a shell-installed Node, so
launch `Contents/MacOS/hamlet` from a terminal with `HAMLET_NODE` if needed.

Windows uses a zip laid out as the Linux tarball, with `.exe` binaries.
Run `hamlet.exe --insecure` in a terminal. Windows has no server sandbox
yet, so a Windows server starts only in insecure mode, and agents'
commands reach whatever the account running it can. Agents get
`powershell`, and `bash` where Git for Windows is installed. The binaries
link the C runtime statically and need no Visual C++ runtime. Neither
packaging nor Linux tests prove macOS or Windows behavior. Remote
workspaces require a separately run workspace worker; it is not included
in these archives.

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

For Windows, build the three binaries with `cargo build --release --locked
-p hamlet_cli --bin hamlet -p hamlet_server --bin hamlet_server -p hamlet_app
--bin hamlet_app --target x86_64-pc-windows-msvc`, with `RUSTFLAGS` set to
`-C target-feature=+crt-static`, then run the script in Git Bash with that
target. It needs Node, npm and 7-Zip.

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
