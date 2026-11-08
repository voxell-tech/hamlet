# Releases

`.github/workflows/release.yml` builds Hamlet for macOS (Apple Silicon),
Linux (x64) and Windows (x64). For each platform it makes what a chief
installs without a terminal, and an archive of the same programs:

| Platform | Installs with | Archive |
|---|---|---|
| macOS | `Hamlet-<version>.dmg`: drag the app onto Applications | `.zip` |
| Linux | `hamlet_<version>_amd64.deb` and `hamlet-<version>-1.x86_64.rpm`, opened in the system's software app | `.tar.gz` |
| Windows | `Hamlet-<version>-Setup.exe` | `.zip` |

Each holds:

- `hamlet`, the CLI;
- `hamlet_launch`, what the installed app opens;
- `hamlet_server`, the server;
- `hamlet_app`, the window;
- `node`, the Node of `harnesses/pi/.nvmrc` from nodejs.org, checked
  against its published sums;
- `pi/` with its source, lockfile and production `node_modules`;
- licenses, Node's among them, and a README.

Nothing else is installed first: no Node, no checkout, no npm install.
The server finds sibling `pi/src/main.ts`, then the macOS bundle's
`Contents/Resources/pi/src/main.ts`, then the checkout it was built from,
and refuses to start, naming `HAMLET_PI_HARNESS`, when none of them
exists. It runs pi with `HAMLET_NODE`, else the `node` beside it, else
Node on `PATH`.

## The launcher

`hamlet_launch` does what bare `hamlet` does, for a chief with no
terminal: it starts the home's server, or reuses the one running, and
opens the window. On Linux and macOS the window takes the launcher's
process, so the dock shows the app that was opened; on Windows the
window is opened apart and the launcher, which asks for no console,
ends. When the server does not start, a dialog says why, from the
`Error:` the server wrote last in `server.log`, and where that log is.
Linux shows it with zenity, which the packages recommend.

## What each installs

macOS: the disk image holds `Hamlet.app` beside a link to
Applications. The bundle opens `hamlet_launch`; the binaries and `node`
live in `Contents/MacOS`, and pi and the licenses in `Contents/Resources`.
With a Developer ID the release signs every program with the hardened
runtime, pi's native modules too, gives Node the JIT entitlements it
needs, and has Apple notarize and staple the image. Without one, the
programs are ad-hoc signed and Gatekeeper refuses the app until the
chief chooses Open Anyway in Privacy & Security.

Linux: the `.deb` and the `.rpm` install the programs in `/opt/hamlet`,
`hamlet` in `/usr/bin`, and `hamlet.desktop` in the menu, whose
`StartupWMClass` is the window's app id. Both depend on bubblewrap, which
the server probes before it starts and has no insecure fallback for, and
on the libraries the window loads. The tarball holds the same programs:
run `./hamlet`, with working bubblewrap.

Windows: an Inno Setup installer, `.github/scripts/hamlet.iss`, installs
for the current account in `%LOCALAPPDATA%\Programs\Hamlet`, with no
administrator, and adds Hamlet to the Start menu. An update stops the
running server first, since it holds its programs open, and replaces
pi whole; uninstalling stops it too. The home is never touched. Windows
has no server sandbox, so a server starts only in insecure mode,
which needs `hamlet.exe --insecure` in a terminal: the installed app
says so in its dialog. Agents' commands then reach whatever the account
running it can. Agents get `powershell`, and `bash` where Git for
Windows is installed. The binaries link the C runtime statically and
need no Visual C++ runtime.

Neither packaging nor Linux tests prove macOS or Windows behavior.
Remote workspaces require a separately run workspace worker; it is not
included.

## Building them

Build natively on the target platform: npm's optional/native dependencies
must match the target. The release job uses the pinned Node version and
builds the four binaries with `cargo xtask build` before invoking
`.github/scripts/package.sh`:

```sh
cargo xtask build --target x86_64-unknown-linux-gnu
.github/scripts/package.sh x86_64-unknown-linux-gnu v0.1.0
```

For macOS use `aarch64-apple-darwin`. The script accepts an optional third
argument for the profile directory, defaults to `release`, and respects
`CARGO_TARGET_DIR`. It creates a clean staging directory, downloads Node,
installs pi with `npm ci --omit=dev`, and writes everything under `dist/`.
It does not compile TypeScript. Linux needs `dpkg-deb` and `rpmbuild`.
A tag's version names the packages; a commit's is the workspace's
version followed by `~g` and the commit, which sorts before the tag's.

For Windows, build the four binaries with `cargo build --release --locked
-p hamlet_cli --bin hamlet --bin hamlet_launch -p hamlet_server --bin
hamlet_server -p hamlet_app --bin hamlet_app --target
x86_64-pc-windows-msvc`, with `RUSTFLAGS` set to
`-C target-feature=+crt-static`, then run the script in Git Bash with that
target. It needs Node, npm, 7-Zip and Inno Setup 6.
`brand/hamlet-icon.ico` is the installer's and the shortcuts' icon, made
from `brand/hamlet-icon.png`.

`cargo xtask install` is a different, source-development workflow: it installs
the CLI, server and window into Cargo's bin directory but leaves pi in the build
checkout. Retain that checkout, or set `HAMLET_PI_HARNESS` to another fully
installed pi entry. `cargo xtask uninstall` removes the three binaries, not homes.

## Signing

The release signs when the repository has the secrets the workflow's
header names, and skips signing without them. macOS imports a Developer
ID Application certificate into a keychain of the run's own, and
`package.sh` signs with `APPLE_SIGNING_IDENTITY` and notarizes with
`APPLE_ID`, `APPLE_TEAM_ID` and an app-specific `APPLE_APP_PASSWORD`.
Windows signs through Azure Trusted Signing, as certificates are now
kept in hardware and cannot be exported to a file: the four programs
before they are packed, and the installer after.

## Publishing

The release workflow runs only for pushed `v*` tags or an explicit manual
run. A manual run uploads one workflow artifact per platform and publishes
no release.
A tag run first checks that the tag, less its leading `v`, is the version
in `Cargo.toml`'s `[workspace.package]`, then creates the GitHub release or
replaces matching assets on an existing release. Pushing a tag is
therefore a publishing action, not a local packaging check.

Validate an extracted archive from outside the checkout on a fresh
`HAMLET_HOME`: start the server, complete a faux chat, stop it and check
that its publication is removed. Do not point release smoke tests at a
personal home. This checks discovery and dependencies, not exhaustive
behavior, real-provider access, or every platform.
