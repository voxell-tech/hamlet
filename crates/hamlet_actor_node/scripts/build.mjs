#!/usr/bin/env node
// This file was changed for Hamlet. See NOTICE in this crate.
/**
 * Build wrapper for actor-node.
 */
import { execFileSync } from "node:child_process";

const args = process.argv.slice(2);
const extraFlags = args.includes("--release") ? ["--release"] : [];

// Explicit skip for environments that don't need the native binary (e.g.
// Docker engine-frontend build which only consumes TypeScript types).
if (process.env.SKIP_NAPI_BUILD === "1") {
    console.log("[actor-node/build] SKIP_NAPI_BUILD=1 — skipping napi build");
    process.exit(0);
}

// The per-actor inspector UI (frontend/dist/inspector-ui, embedded into
// hamlet_actor_core by its build.rs) must be built before this napi build runs.
// It is NOT built here: hamlet_actor_core's embed needs the inspector-tab package,
// which is downstream of this package in the build graph, so building it from
// the napi build would invert the dependency order. CI builds it via
// `turbo build:inspector-ui` in docker/build/*.Dockerfile before `napi build`;
// for local builds run `pnpm -F @hamlet/engine-frontend build:inspector-ui`
// (or `turbo build:inspector-ui`) first.
const cmd = ["build", "--platform", ...extraFlags];
console.log(`[actor-node/build] running: napi ${cmd.join(" ")}`);
execFileSync("napi", cmd, { stdio: "inherit" });
