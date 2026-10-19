# Releases

`.github/workflows/release.yml` builds Hamlet for macOS (Apple Silicon),
Windows (x64) and Linux (x64). It runs only when a version tag is pushed
or when it is started by hand from the Actions tab:

```sh
git tag v0.1.0 && git push origin v0.1.0   # builds, and publishes a release
```

A run by hand keeps the archives as the run's artifacts and publishes
nothing. `.github/scripts/package.sh` packs the app, and can be run on a
build of your own.

## The compiler

Every job, the release's and CI's, builds with the compiler
`rust-toolchain.toml` pins (`.github/actions/toolchain` reads it), and
formatting runs on nightly. A native plugin is loaded only by a Hamlet
built with its very compiler, so moving the pin moves what every plugin is
built with: it is a release of its own, with new kits.

## What a release holds

For each target, `<version>` being the tag (`v0.1.0`), or the first seven
letters of the commit in a run by hand:

| File | What |
|---|---|
| `hamlet-<version>-<target>.zip` (`.tar.gz` on Linux) | The app |
| `hamlet-sdk-<version>-<target>.tar.gz` | The SDK kit native plugins for that app are built with |

The kit is made in the job that builds the app, right after it, by
`scripts/make-native-sdk.sh` with the options of that build
(`--release --locked -p hamlet --target <target>`), so it is of the very
build that is released and nothing is compiled again. Its `sdk.toml` says what it is
for: the Hamlet version, the toolchain, the target, the profile and the
values the gate compares, as that build's `hamlet --native-abi` printed
them.

A plugin author gets it with
`scripts/fetch-native-sdk.sh <version> <target> <kit>`, which downloads it
with `gh`, or with `curl` when `gh` is not signed in, and unpacks it. [`native.md`](native.md) says how a plugin is
built with it and what must match.
