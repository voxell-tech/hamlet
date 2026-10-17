import { expect, it } from "vitest";
import { hamlet } from "./cli-support.ts";
import { scratch } from "./support.ts";

it("runs help and usage failures with isolated streams and environment", async () => {
    const home = scratch();
    const help = hamlet(["--help"], home);
    expect(await help.ended).toBe(0);
    expect(help.out()).toMatch(/^usage: hamlet/);
    expect(help.err()).toBe("");
    const bad = hamlet(["server", "--port", "high"], home);
    expect(await bad.ended).toBe(1);
    expect(bad.err()).toContain("--port needs a port number, not `high`");
    expect(bad.out()).toBe("");
});
