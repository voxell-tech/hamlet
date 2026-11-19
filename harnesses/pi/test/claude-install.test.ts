import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import { type Locked, needed, roots } from "../src/claude-install.ts";

const LOCK = JSON.parse(
    readFileSync(join(import.meta.dirname, "..", "package-lock.json"), "utf8"),
) as { packages: Record<string, Locked> };

test("a machine downloads its own Claude program and what drives it, and nothing pi has", () => {
    const keys = needed(LOCK.packages, roots(), {
        os: "darwin",
        cpu: "arm64",
    }).map(([key]) => key);
    const programs = keys.filter((key) =>
        key.startsWith("node_modules/@anthropic-ai/claude-agent-sdk-"),
    );
    expect(programs).toEqual([
        "node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64",
    ]);
    expect(keys).toContain("node_modules/@modelcontextprotocol/sdk");
    expect(keys).toContain("node_modules/cc-session-io");
    expect(keys.filter((key) => LOCK.packages[key]?.dev === true)).toEqual([]);
});
