import { createServer } from "node:http";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { gitProxy } from "../src/git/proxy.ts";
import { closedPolicy } from "../src/sandbox/reach.ts";
import { SandboxEnv } from "../src/sandbox.ts";
import { gitHost, hostRef, serve } from "./git-host.ts";
import { closeAfter, scratch, sh } from "./support.ts";

describe("a sandboxed command with no network", () => {
    it("reaches the git proxy, and nothing else on this machine", async () => {
        const host = await gitHost("chief-token");
        const proxy = gitProxy({
            upstream: `${host.url}/project.git`,
            token: "chief-token",
            credential: "hamlet-credential",
        });
        const tcp = await serve(proxy);
        const socket = join(scratch(), "git.sock");
        const unix = createServer(proxy);
        await new Promise<void>((done) => unix.listen(socket, done));
        closeAfter(
            host.close,
            () => tcp.server.close(),
            () => unix.close(),
        );
        const port = Number(new URL(tcp.url).port);
        const house = scratch();
        const env = new SandboxEnv({
            cwd: house,
            policy: { ...closedPolicy(house), services: [{ port, socket }] },
        });

        const pushed = await sh(
            env,
            [
                `git clone -q http://hamlet:hamlet-credential@127.0.0.1:${port} repo`,
                "cd repo",
                "git -c user.name=Agent -c user.email=agent@hamlet commit -q --allow-empty -m Work",
                "git push -q origin HEAD:from-the-sandbox",
            ].join(" && "),
        );
        const elsewhere = await sh(env, `curl -sS -m 2 ${host.url}/ 2>&1`);

        expect(pushed).toMatchObject({ exitCode: 0 });
        expect(
            await hostRef(host.repo, "refs/heads/from-the-sandbox"),
        ).not.toBe("");
        expect(elsewhere.exitCode).not.toBe(0);
    });
});
