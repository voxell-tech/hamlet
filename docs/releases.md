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

pi's optional packages are left out: they carry Anthropic's Claude program,
whose terms are Anthropic's, so a released pi offers no Claude
([ADR 0014](adr/0014-claude-subscriptions-run-in-anthropics-own-program-under-pi.md)).

## The launcher

`hamlet_launch` does what bare `hamlet` does, for a chief with no
terminal: it starts the home's server, or reuses the one running, and
opens the window. On Linux and macOS the window takes the launcher's
process, so the dock shows the app that was opened; on Windows the
window is opened apart and the launcher, which asks for no console,
ends. When the server does not start, a dialog says why, from the
`Error:` the server wrote last in `server.log`, and where that log is.
Linux shows it with zenity, which the packages recommend. Given
`--update <file>`, it first puts that update in place (see
[Updates](#updates)).

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
installs pi with `npm ci --omit=dev --omit=optional`, and writes everything
under `dist/`. It does not compile TypeScript. Linux needs `dpkg-deb` and
`rpmbuild`. A tag's version names the packages; a commit's is the
workspace's version followed by `~g` and the commit, which sorts before the
tag's.

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

## Updates

An installed release looks for a newer one while its window is open:
15 seconds after the window opens, then every hour. `hamlet_update`
holds the looking, the download and the putting in place; the window
and `hamlet_launch` use it.

Each release publishes `latest.json` beside its files: its version,
notes and page, and for each kind of install (`macos`, `windows`,
`deb`, `rpm`) its file's address, SHA-256 and length.
`latest.json.sig` is its Ed25519 signature, in base64. The window
reads both from the latest release, takes the manifest only when the
signature holds under the public key built into it and the version is
newer than its own, then downloads its install's file into the cache
folder (`~/Library/Caches/Hamlet/updates`, `%LOCALAPPDATA%\Hamlet\updates`
or `~/.cache/hamlet/updates`) and keeps it only if its sum and length
match. A toast offers it until the chief restarts or closes it; a
closed offer comes back when the window next opens.

Restart notes the version being installed and hands the file to
`hamlet_launch --update <file>`, then the window closes. The launcher
stops the home's server and puts the update in place:

- macOS: the zip is unpacked beside the app, and the new app takes the
  old one's place once `codesign` finds it whole and, when the old app
  was signed by a developer, signed by the same team.
- Windows: the installer runs silently, closes what holds Hamlet's
  files, and opens Hamlet when it is done.
- Linux: `pkexec` runs `apt-get install` on the `.deb` or `dnf install`
  on the `.rpm`, and the desktop asks for the password.

The launcher then opens Hamlet as it always does, and the agents carry
on. When the update fails, it says why in a dialog and opens the copy
that is there; the next window then says the update did not install.
An archive or a build from source cannot replace itself: its toast
links to the release's page instead.

Only a release looks: the workflow gives a tag's build the public key,
from the repository variable `HAMLET_UPDATE_KEY`, and a build without
it never looks. `HAMLET_NO_UPDATE` set to anything stops a release
from looking. `HAMLET_UPDATE_FEED` names another manifest, for trying
an update before it is published. The key pair is made once with
OpenSSL; the private key goes in the secret `HAMLET_UPDATE_SIGNING_KEY`
and the public one in the variable:

```sh
openssl genpkey -algorithm ed25519 -out hamlet-update.pem
openssl pkey -in hamlet-update.pem -pubout -outform DER | tail -c 32 | base64
```

Keep the private key out of the repository: whoever holds it can
offer every installed Hamlet a program to run.

## Publishing

The release workflow runs only for pushed `v*` tags or an explicit manual
run. A manual run uploads one workflow artifact per platform and publishes
no release.
A tag run first checks that the tag, less its leading `v`, is the version
in `Cargo.toml`'s `[workspace.package]`, or a pre-release of it such as
`v0.1.0-dev.1`, then creates the GitHub release or replaces matching
assets on an existing release. A pre-release tag makes a GitHub
pre-release, which installed copies never update to, since they read
the latest release. With
`HAMLET_UPDATE_SIGNING_KEY` set, `.github/scripts/manifest.sh` then
writes and signs `latest.json`, uploaded last, so installed copies
find the update only once every file it names is there. Pushing a tag
is therefore a publishing action, not a local packaging check.

Validate an extracted archive from outside the checkout on a fresh
`HAMLET_HOME`: start the server, complete a faux chat, stop it and check
that its publication is removed. Do not point release smoke tests at a
personal home. This checks discovery and dependencies, not exhaustive
behavior, real-provider access, or every platform.
