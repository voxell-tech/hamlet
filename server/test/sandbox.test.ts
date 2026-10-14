import {
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { type AddressInfo, createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { registerEnvConformance } from "@earendil-works/pi-durable/testing";
import { describe, expect, it } from "vitest";
import { SandboxEnv } from "../src/sandbox/env.ts";
import { closedPolicy, openPolicy } from "../src/sandbox/reach.ts";
import { houseEnv } from "./sandbox-support.ts";
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

/** A hamlet's folder holding a repository, with an agent's house in Hamlet's home beside it. */
function folderEnv(): {
    folder: string;
    home: string;
    house: string;
    env: SandboxEnv;
} {
    const [folder, home] = [scratch(), scratch()];
    const house = join(home, "house");
    mkdirSync(house);
    mkdirSync(join(folder, ".git", "hooks"), { recursive: true });
    mkdirSync(join(folder, ".git", "info"));
    writeFileSync(join(folder, ".git", "config"), "kept");
    writeFileSync(join(home, "api_key.x"), "secret");
    return {
        folder,
        home,
        house,
        env: new SandboxEnv({
            cwd: folder,
            policy: openPolicy({ folder, house, home }),
        }),
    };
}

/** The error code of a refused file operation, or "ok". */
function outcome(result: { ok: boolean; error?: { code: string } }): string {
    return result.ok ? "ok" : (result.error?.code ?? "unknown");
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
        const { folder, home, house, env } = folderEnv();
        const outside = scratch();

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

    it("keeps a scoped temp folder from every other scope of the same house, until cleanup removes it", async () => {
        const house = scratch();
        const policy = closedPolicy(house);
        const a = new SandboxEnv({ cwd: house, policy, tmpScope: "call-a" });
        const b = new SandboxEnv({ cwd: house, policy, tmpScope: "call-b" });
        const unscoped = new SandboxEnv({ cwd: house, policy });
        const left = await sh(
            a,
            'echo secret > "$TMPDIR/note" && echo "$TMPDIR"',
        );
        const tmp = left.output.trim();
        const note = join(tmp, "note");

        expect(left.exitCode).toBe(0);
        expect(await a.readTextFile(note, context)).toEqual({
            ok: true,
            value: "secret\n",
        });
        for (const other of [b, unscoped]) {
            expect((await sh(other, `cat '${note}'`)).exitCode).not.toBe(0);
            expect(outcome(await other.readTextFile(note, context))).toBe(
                "permission_denied",
            );
        }
        await a.cleanup(context);
        expect(existsSync(tmp)).toBe(false);
    });
});

/** True when the volume of the temp folder takes names in any case as the same, as macOS's does. */
const foldsCase =
    tmpdir() !== tmpdir().toUpperCase() && existsSync(tmpdir().toUpperCase());

describe.skipIf(!foldsCase)("on a volume that ignores case", () => {
    it("keeps Hamlet's home closed through another case of its path", async () => {
        const { home, env } = folderEnv();
        const other = home.toUpperCase();

        expect(
            outcome(await env.readTextFile(join(other, "api_key.x"), context)),
        ).toBe("permission_denied");
        expect(outcome(await env.listDir(other, context))).toBe(
            "permission_denied",
        );
        expect(outcome(await env.openDirReader(other, context))).toBe(
            "permission_denied",
        );
        const ran = await sh(env, `cat '${join(other, "api_key.x")}'`);
        expect(ran.exitCode).not.toBe(0);
        expect(ran.output).not.toContain("secret");
        // The house in it is still the agent's own, in that case too.
        expect(
            outcome(
                await env.writeFile(join(other, "house", "n"), "a", context),
            ),
        ).toBe("ok");
    });

    it("makes no repository through another case of .git", async () => {
        const [folder, house, home] = [scratch(), scratch(), scratch()];
        const env = new SandboxEnv({
            cwd: folder,
            policy: openPolicy({ folder, house, home }),
        });

        expect(outcome(await env.writeFile(".GIT/HEAD", "a", context))).toBe(
            "permission_denied",
        );
        expect(outcome(await env.createDir(".Git", undefined, context))).toBe(
            "permission_denied",
        );
        expect(existsSync(join(folder, ".git"))).toBe(false);
    });
});

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
        const { home, house, env } = folderEnv();
        const outside = scratch();
        writeFileSync(join(outside, "seen.txt"), "seen");

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

    it("never writes, moves or removes git's hooks or config, in any case and in any repository", async () => {
        const { folder, env } = folderEnv();
        const nested = join(folder, "vendor", "lib", ".git");
        mkdirSync(nested, { recursive: true });
        writeFileSync(join(nested, "config"), "kept");
        mkdirSync(join(folder, "sub"));
        symlinkSync(join(folder, ".git", "hooks"), join(folder, "sub", "h"));
        symlinkSync("b", join(folder, "sub", "c"));
        symlinkSync("../.git/hooks/pre-commit", join(folder, "sub", "b"));
        writeFileSync(join(folder, "a.txt"), "a");

        for (const path of [
            ".GIT/Hooks/x",
            ".git/CONFIG",
            "vendor/lib/.git/config",
            "vendor/lib/.GIT/hooks/x",
            "sub/h/pre-commit",
            "sub/c",
        ]) {
            expect({
                path,
                write: outcome(await env.writeFile(path, "x", context)),
                into: outcome(await env.renameFile("a.txt", path, context)),
                from: outcome(await env.renameFile(path, "b.txt", context)),
                remove: outcome(
                    await env.remove(path, { force: true }, context),
                ),
            }).toEqual({
                path,
                write: "permission_denied",
                into: "permission_denied",
                from: "permission_denied",
                remove: "permission_denied",
            });
        }
        // Moved away and back, a repository would carry what was written in
        // it meanwhile, and a folder moved into its place its own hooks.
        expect(
            outcome(await env.renameFile("vendor/lib/.git", "g", context)),
        ).toBe("permission_denied");
        expect(
            outcome(await env.renameFile("sub", "vendor/.git", context)),
        ).toBe("permission_denied");
        expect(
            outcome(await env.writeFile(".git/info/exclude", "x", context)),
        ).toBe("ok");
        expect(readdirSync(join(folder, ".git", "hooks"))).toEqual([]);
        expect(readFileSync(join(folder, ".git", "config"), "utf8")).toBe(
            "kept",
        );
        expect(readFileSync(join(nested, "config"), "utf8")).toBe("kept");
        expect(existsSync(join(folder, "a.txt"))).toBe(true);
    });

    it("refuses to read a file that is not UTF-8 as text, and leaves it as it was", async () => {
        const { house, env } = houseEnv();
        const bytes = Buffer.from([0x61, 0xff, 0xfe, 0x0a]);
        writeFileSync(join(house, "data.bin"), bytes);

        const read = await env.readTextFile("data.bin", context);

        expect(read.ok || read.error).toMatchObject({
            code: "invalid",
            message: "data.bin is not UTF-8 text",
        });
        expect(readFileSync(join(house, "data.bin"))).toEqual(bytes);
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

    it("cannot reach out through a chain of symbolic links, nor through one that climbs out of another", async () => {
        const { env } = houseEnv();
        const outside = scratch();
        mkdirSync(join(outside, "deep"));
        const linked = await sh(
            env,
            `ln -s b a && ln -s '${outside}/chained' b && ln -s '${outside}/deep' d && ln -s d/../climbed c`,
        );

        expect(linked.exitCode).toBe(0);
        for (const path of ["a", "c"]) {
            expect({
                path,
                write: outcome(await env.writeFile(path, "a", context)),
            }).toEqual({ path, write: "permission_denied" });
        }
        expect(readdirSync(outside)).toEqual(["deep"]);
        expect(readdirSync(join(outside, "deep"))).toEqual([]);
    });

    it("writes nothing through a loop of symbolic links", async () => {
        const { house, env } = houseEnv();
        symlinkSync("f", join(house, "e"));
        symlinkSync("e", join(house, "f"));

        expect(outcome(await env.writeFile("e", "a", context))).not.toBe("ok");
        expect(readdirSync(house).sort()).toEqual(["e", "f"]);
    });
});
