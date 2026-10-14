import { randomBytes } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { gitProxy } from "../src/git/proxy.ts";
import { git, gitHost, hostRef, serve } from "./git-host.ts";
import { closeAfter, scratch } from "./support.ts";

const TOKEN = "chief-token";
const CREDENTIAL = "hamlet-credential";

/** A git host, the proxy in front of it, and a clone made through the proxy. */
async function setUp(credential = CREDENTIAL) {
    const host = await gitHost(TOKEN);
    const proxy = await serve(
        gitProxy({
            upstream: `${host.url}/project.git`,
            token: TOKEN,
            credential: CREDENTIAL,
        }),
    );
    closeAfter(host.close, () => proxy.server.close());
    const clone = scratch();
    const remote = proxy.url.replace("http://", `http://hamlet:${credential}@`);
    const cloned = await git(["clone", "-q", remote, clone]);
    return { host, clone, cloned };
}

const pushes = (requests: string[]) =>
    requests.filter(
        (request) => request === "POST /project.git/git-receive-pack",
    );

describe("the git proxy", () => {
    it("lets an agent clone and push a branch, adding the chief's token", async () => {
        const { host, clone, cloned } = await setUp();
        writeFileSync(join(clone, "notes.md"), "Notes\n");
        await git(["add", "notes.md"], clone);
        await git(["commit", "-q", "-m", "Notes"], clone);

        const pushed = await git(
            ["push", "-q", "origin", "HEAD:agent-work"],
            clone,
        );

        expect(cloned.code).toBe(0);
        expect(pushed).toMatchObject({ code: 0 });
        expect(await hostRef(host.repo, "refs/heads/agent-work")).toBe(
            (await git(["rev-parse", "HEAD"], clone)).stdout.trim(),
        );
    });

    it("lets a push larger than git sends in one request through", async () => {
        const { host, clone } = await setUp();
        writeFileSync(join(clone, "data.bin"), randomBytes(3 * 1024 * 1024));
        await git(["add", "data.bin"], clone);
        await git(["commit", "-q", "-m", "Data"], clone);

        const pushed = await git(["push", "-q", "origin", "HEAD:large"], clone);

        expect(pushed).toMatchObject({ code: 0 });
        expect(await hostRef(host.repo, "refs/heads/large")).toBe(
            (await git(["rev-parse", "HEAD"], clone)).stdout.trim(),
        );
    });

    it("refuses deleting a branch before the git host hears of it", async () => {
        const { host, clone } = await setUp();
        const before = await hostRef(host.repo, "refs/heads/feature");

        const pushed = await git(
            ["push", "origin", "--delete", "feature"],
            clone,
        );

        expect(pushed.code).not.toBe(0);
        expect(pushed.stderr).toContain("refused");
        expect(await hostRef(host.repo, "refs/heads/feature")).toBe(before);
        expect(pushes(host.requests)).toEqual([]);
    });

    it("refuses pushing a tag before the git host hears of it", async () => {
        const { host, clone } = await setUp();
        await git(["tag", "v1"], clone);

        const pushed = await git(["push", "origin", "v1"], clone);

        expect(pushed.code).not.toBe(0);
        expect(pushed.stderr).toContain("refused");
        expect(await hostRef(host.repo, "refs/tags/v1")).toBe("");
        expect(pushes(host.requests)).toEqual([]);
    });

    it("refuses anyone without the hamlet's credential", async () => {
        const { host, cloned } = await setUp("guess");

        expect(cloned.code).not.toBe(0);
        expect(host.requests).toEqual([]);
    });
});
