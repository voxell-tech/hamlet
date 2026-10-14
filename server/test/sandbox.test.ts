import { existsSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { type AddressInfo, createServer } from "node:net";
import { join, resolve } from "node:path";
import { registerEnvConformance } from "@earendil-works/pi-durable/testing";
import { describe, expect, it } from "vitest";
import { SandboxEnv } from "../src/sandbox/env.ts";
import { closedPolicy, openPolicy } from "../src/sandbox/reach.ts";
import { context, scratch, sh } from "./support.ts";

registerEnvConformance(
    { describe, expect, it },
    "Hamlet's sandboxed environment",
    async (use) => {
        const house = scratch();
        const env = new SandboxEnv({ cwd: house, policy: closedPolicy(house) });
        try {
            await use(env);
        } finally {
            await env.cleanup(context);
        }
    },
);

function houseEnv(): { house: string; env: SandboxEnv } {
    const house = scratch();
    return {
        house,
        env: new SandboxEnv({ cwd: house, policy: closedPolicy(house) }),
    };
}

describe("a sandboxed command", () => {
    it("writes only in its house and its temp folder", async () => {
        const { env } = houseEnv();
        const outside = join(scratch(), "outside.txt");

        expect(await sh(env, "echo a > in.txt && cat in.txt")).toEqual({
            exitCode: 0,
            output: "a\n",
        });
        expect((await sh(env, `echo a > ${outside}`)).exitCode).not.toBe(0);
        expect(existsSync(outside)).toBe(false);
        expect(
            await sh(env, 'echo t > "$TMPDIR/t.txt" && cat "$TMPDIR/t.txt"'),
        ).toEqual({ exitCode: 0, output: "t\n" });
    });

    it("cannot read outside its house unless granted", async () => {
        const house = scratch();
        const secret = join(scratch(), "key.txt");
        writeFileSync(secret, "secret");
        const closed = closedPolicy(house);
        const denied = await sh(
            new SandboxEnv({ cwd: house, policy: closed }),
            `cat ${secret}`,
        );
        const keys = await sh(
            new SandboxEnv({ cwd: house, policy: closed }),
            "ls ~/.ssh 2>&1; cat ~/.ssh/id_* 2>&1; true",
        );
        const granted = await sh(
            new SandboxEnv({
                cwd: house,
                policy: { ...closed, ro: [secret] },
            }),
            `cat ${secret}`,
        );

        expect(denied.exitCode).not.toBe(0);
        expect(denied.output).not.toContain("secret");
        expect(keys.output).not.toContain("PRIVATE KEY");
        expect(granted).toEqual({ exitCode: 0, output: "secret" });
    });

    it("reaches no network unless granted", async () => {
        const house = scratch();
        const listener = createServer();
        let connections = 0;
        listener.on("connection", (socket) => {
            connections++;
            socket.destroy();
        });
        await new Promise<void>((done) =>
            listener.listen(0, "127.0.0.1", done),
        );
        const { port } = listener.address() as AddressInfo;
        const curl = `curl -sS -m 2 http://127.0.0.1:${port}/ 2>&1`;
        const closed = closedPolicy(house);

        const denied = await sh(
            new SandboxEnv({ cwd: house, policy: closed }),
            curl,
        );
        const before = connections;
        await sh(
            new SandboxEnv({ cwd: house, policy: { ...closed, net: true } }),
            curl,
        );
        listener.close();

        expect(denied.exitCode).not.toBe(0);
        expect(before).toBe(0);
        expect(connections).toBe(1);
    });

    it("runs common tools", async () => {
        const { env } = houseEnv();
        for (const script of [
            "ls .",
            "cat /etc/hosts >/dev/null",
            "find . -name x",
            "git init -q . && git status --short && git --version",
            "python3 -c 'print(1+1)'",
        ]) {
            expect({ script, ...(await sh(env, script)) }).toMatchObject({
                script,
                exitCode: 0,
            });
        }
    });

    it("in a hamlet's folder reads widely, but not the home, and keeps its locks", async () => {
        const [folder, home, outside] = [scratch(), scratch(), scratch()];
        const house = join(home, "house");
        mkdirSync(house);
        mkdirSync(join(folder, ".git", "hooks"), { recursive: true });
        writeFileSync(join(folder, ".git", "config"), "");
        writeFileSync(join(home, "api_key.x"), "secret");
        const env = new SandboxEnv({
            cwd: folder,
            policy: openPolicy({ folder, house, home }),
        });

        for (const script of [
            // Outside the temp folder, which bubblewrap gives each command
            // empty.
            `cat ${resolve("package.json")}`,
            "echo a > in.txt",
            "echo a > .git/HEAD",
            `echo a > ${house}/n && cat ${house}/n`,
        ]) {
            expect({ script, ...(await sh(env, script)) }).toMatchObject({
                script,
                exitCode: 0,
            });
        }
        for (const script of [
            `echo a > ${outside}/new.txt`,
            `cat ${home}/api_key.x`,
            "echo a > .git/hooks/pre-commit",
            "echo a > .git/config",
            "mv .git .git2",
            "ln -s .git/hooks h && echo a > h/pre-commit",
        ]) {
            const ran = await sh(env, script);
            expect({ script, failed: ran.exitCode !== 0 }).toEqual({
                script,
                failed: true,
            });
            expect(ran.output).not.toContain("secret");
        }
        // Seatbelt refuses the listing; bubblewrap shows an empty folder.
        expect((await sh(env, `ls ${home}`)).output).not.toContain("api_key");
        expect(existsSync(join(folder, ".git", "hooks", "pre-commit"))).toBe(
            false,
        );
    });

    it("keeps each house's temp folder from the others", async () => {
        const a = houseEnv();
        const b = houseEnv();
        const left = await sh(
            a.env,
            'echo secret > "$TMPDIR/note" && echo "$TMPDIR"',
        );
        const note = join(left.output.trim(), "note");

        expect(left.exitCode).toBe(0);
        expect(existsSync(note)).toBe(true);
        expect((await sh(b.env, `cat '${note}'`)).exitCode).not.toBe(0);
    });
});

/** The error code of a refused file operation, or "ok". */
function outcome(result: { ok: boolean; error?: { code: string } }): string {
    return result.ok ? "ok" : (result.error?.code ?? "unknown");
}

describe("a file operation", () => {
    it("writes only in its house and its temp folder", async () => {
        const { env } = houseEnv();
        const outside = join(scratch(), "outside.txt");
        const temp = await env.createTempFile(undefined, context);

        expect(outcome(await env.writeFile("in.txt", "a", context))).toBe("ok");
        expect(outcome(await env.writeFile(outside, "a", context))).toBe(
            "permission_denied",
        );
        expect(existsSync(outside)).toBe(false);
        expect(
            temp.ok && outcome(await env.writeFile(temp.value, "t", context)),
        ).toBe("ok");
    });

    it("reads nothing outside its house unless granted", async () => {
        const house = scratch();
        const secret = join(scratch(), "key.txt");
        writeFileSync(secret, "secret");
        const closed = closedPolicy(house);
        const denied = new SandboxEnv({ cwd: house, policy: closed });
        const granted = new SandboxEnv({
            cwd: house,
            policy: { ...closed, ro: [secret] },
        });

        expect(outcome(await denied.readTextFile(secret, context))).toBe(
            "permission_denied",
        );
        expect(await granted.readTextFile(secret, context)).toEqual({
            ok: true,
            value: "secret",
        });
        expect(outcome(await granted.writeFile(secret, "x", context))).toBe(
            "permission_denied",
        );
    });

    it("in a hamlet's folder reads widely, but not the home, and never writes git's hooks or config", async () => {
        const [folder, home, outside] = [scratch(), scratch(), scratch()];
        const house = join(home, "house");
        mkdirSync(house);
        mkdirSync(join(folder, ".git", "hooks"), { recursive: true });
        writeFileSync(join(folder, ".git", "config"), "");
        writeFileSync(join(home, "api_key.x"), "secret");
        writeFileSync(join(outside, "seen.txt"), "seen");
        const env = new SandboxEnv({
            cwd: folder,
            policy: openPolicy({ folder, house, home }),
        });

        expect(
            outcome(await env.readTextFile(join(outside, "seen.txt"), context)),
        ).toBe("ok");
        expect(outcome(await env.writeFile(".git/HEAD", "a", context))).toBe(
            "ok",
        );
        expect(
            outcome(await env.writeFile(join(house, "n"), "a", context)),
        ).toBe("ok");
        expect(
            outcome(await env.readTextFile(join(home, "api_key.x"), context)),
        ).toBe("permission_denied");
        expect(outcome(await env.listDir(home, context))).toBe(
            "permission_denied",
        );
        expect(
            outcome(await env.writeFile(".git/hooks/pre-commit", "a", context)),
        ).toBe("permission_denied");
        expect(outcome(await env.writeFile(".git/config", "a", context))).toBe(
            "permission_denied",
        );
        expect(outcome(await env.renameFile(".git", ".git2", context))).toBe(
            "permission_denied",
        );
        expect(
            outcome(await env.remove(".git", { recursive: true }, context)),
        ).toBe("permission_denied");
    });

    it("cannot reach out through a symbolic link", async () => {
        const { house, env } = houseEnv();
        const outside = scratch();
        writeFileSync(join(outside, "key.txt"), "secret");
        symlinkSync(outside, join(house, "out"));

        expect(outcome(await env.writeFile("out/new.txt", "a", context))).toBe(
            "permission_denied",
        );
        expect(outcome(await env.readTextFile("out/key.txt", context))).toBe(
            "permission_denied",
        );
        expect(existsSync(join(outside, "new.txt"))).toBe(false);
    });
});
