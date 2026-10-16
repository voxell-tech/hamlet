import { PassThrough } from "node:stream";
import { expect, it } from "vitest";
import { main } from "../src/cli.ts";

it("runs help and usage failures in process with isolated streams and environment", async () => {
    const stdin = new PassThrough();
    const stdout = new PassThrough();
    const stderr = new PassThrough();
    let out = "",
        err = "";
    stdout.on("data", (chunk) => {
        out += chunk;
    });
    stderr.on("data", (chunk) => {
        err += chunk;
    });
    expect(await main(["--help"], {}, { stdin, stdout, stderr })).toBe(0);
    expect(out).toMatch(/^usage: hamlet/);
    expect(err).toBe("");
    out = "";
    expect(
        await main(["server", "--port", "high"], {}, { stdin, stdout, stderr }),
    ).toBe(1);
    expect(err).toContain("--port needs a port number, not `high`");
    expect(out).toBe("");
});
