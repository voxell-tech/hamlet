import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import {
    type Locked,
    type Machine,
    needed,
    roots,
} from "../src/claude-install.ts";

const LOCK = JSON.parse(
    readFileSync(join(import.meta.dirname, "..", "package-lock.json"), "utf8"),
) as { packages: Record<string, Locked> };

test.each<[Machine, string]>([
    [{ os: "darwin", cpu: "arm64" }, "darwin-arm64"],
    [{ os: "linux", cpu: "x64", libc: "glibc" }, "linux-x64"],
    [{ os: "linux", cpu: "x64", libc: "musl" }, "linux-x64-musl"],
    [{ os: "win32", cpu: "x64" }, "win32-x64"],
])(
    "%o downloads its own Claude program and what drives it, and nothing pi has",
    (on, build) => {
        const keys = needed(LOCK.packages, roots(), on).map(([key]) => key);
        const programs = keys.filter((key) =>
            key.startsWith("node_modules/@anthropic-ai/claude-agent-sdk-"),
        );
        expect(programs).toEqual([
            `node_modules/@anthropic-ai/claude-agent-sdk-${build}`,
        ]);
        expect(keys).toContain("node_modules/@modelcontextprotocol/sdk");
        expect(keys).toContain("node_modules/cc-session-io");
        expect(keys.filter((key) => LOCK.packages[key]?.dev === true)).toEqual(
            [],
        );
    },
);
