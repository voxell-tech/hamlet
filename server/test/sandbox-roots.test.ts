import {
    lstat,
    mkdir,
    readdir,
    readFile,
    rename,
    rm,
    symlink,
    writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { expect, it } from "vitest";
import { houseOf } from "../src/home/layout.ts";
import {
    deliver as deliverTree,
    list as listTree,
    remove as removeTree,
    stage as stageTree,
} from "../src/house.ts";
import { closedPolicy, openPolicy } from "../src/sandbox/reach.ts";
import { SandboxEnv } from "../src/sandbox.ts";
import {
    context,
    fauxModels,
    MAYOR,
    scratch,
    sh,
    testHome,
} from "./support.ts";

it.each([false, true])(
    "keeps the private temp root fixed with open policy %s",
    async (open) => {
        const house = scratch(),
            home = scratch(),
            folder = scratch(),
            outside = scratch();
        await writeFile(join(outside, "secret"), "chief-credentials");
        const env = new SandboxEnv({
            cwd: house,
            policy: open
                ? openPolicy({ house, home, folder })
                : closedPolicy(house),
        });
        const tmp = (await sh(env, 'printf %s "$TMPDIR"')).output;
        try {
            await sh(env, `rm -rf -- "$TMPDIR"; ln -s '${outside}' "$TMPDIR"`);
            const next = await sh(
                env,
                'cat "$TMPDIR/secret"; echo clobbered > "$TMPDIR/secret"',
            );
            expect.soft(next.output).not.toContain("chief-credentials");
            expect
                .soft(await readFile(join(outside, "secret"), "utf8"))
                .toBe("chief-credentials");
            expect((await lstat(tmp)).isDirectory()).toBe(true);
            expect(
                await sh(
                    env,
                    'echo ordinary > "$TMPDIR/note"; cat "$TMPDIR/note"',
                ),
            ).toEqual({ exitCode: 0, output: "ordinary\n" });
            expect(
                (await env.remove(tmp, { recursive: true }, context)).ok,
            ).toBe(false);
            expect(
                (await env.renameFile(tmp, `${tmp}-moved`, context)).ok,
            ).toBe(false);
        } finally {
            await env.cleanup(context);
            if ((await lstat(tmp)).isSymbolicLink()) {
                await rm(tmp);
                await mkdir(tmp);
            }
        }
    },
);

it.each(["closed house", "open house", "open folder"])(
    "cannot replace the %s with a link in a command",
    async (kind) => {
        const home = scratch(),
            house = join(home, "house"),
            folder = scratch(),
            outside = scratch();
        await mkdir(house);
        await writeFile(join(outside, "canary"), "private");
        const root = kind === "open folder" ? folder : house;
        const env = new SandboxEnv({
            cwd: root,
            policy:
                kind === "closed house"
                    ? closedPolicy(house)
                    : openPolicy({ house, folder, home }),
        });
        try {
            const attack = await sh(
                env,
                `cd / && rm -rf -- '${root}' && ln -s '${outside}' '${root}'`,
            );
            expect(attack.exitCode).not.toBe(0);
            expect((await lstat(root)).isDirectory()).toBe(true);
            expect(await sh(env, "echo ordinary > note && cat note")).toEqual({
                exitCode: 0,
                output: "ordinary\n",
            });
            expect(
                (await sh(env, `echo changed > '${root}/canary'`)).exitCode,
            ).toBe(0);
            expect(await readFile(join(outside, "canary"), "utf8")).toBe(
                "private",
            );
            expect(
                (await env.remove(root, { recursive: true }, context)).ok,
            ).toBe(false);
            expect(
                (await env.renameFile(root, `${root}-moved`, context)).ok,
            ).toBe(false);
        } finally {
            await env.cleanup(context);
        }
    },
);

it("refuses commands and file tools if a previously granted root becomes a link", async () => {
    const parent = scratch(),
        house = join(parent, "house"),
        outside = scratch();
    await mkdir(house);
    await writeFile(join(outside, "canary"), "private");
    const env = new SandboxEnv({ cwd: house, policy: closedPolicy(house) });
    expect((await env.writeFile("ordinary", "ok", context)).ok).toBe(true);
    await rename(house, join(parent, "old"));
    await symlink(outside, house);
    try {
        const read = await env.exec(
            `cat '${house}/canary'; echo changed > '${house}/canary'`,
            undefined,
            context,
        );
        expect(read.ok).toBe(false);
        expect((await env.readTextFile("canary", context)).ok).toBe(false);
        expect((await env.writeFile("canary", "changed", context)).ok).toBe(
            false,
        );
        expect((await env.createTempFile(undefined, context)).ok).toBe(false);
        expect(await readFile(join(outside, "canary"), "utf8")).toBe("private");
    } finally {
        await env.cleanup(context);
    }
});

it.each(["houses/home", "houses/home/0", ".gifts", ".erasing"])(
    "refuses a linked %s before opening or creating anything for agents",
    async (part) => {
        const dir = scratch(),
            outside = scratch(),
            models = fauxModels().models;
        const home = await testHome(dir, { models });
        await home.close(context);
        const path = join(dir, part);
        await rename(path, `${path}-old`);
        await symlink(outside, path);
        await writeFile(join(outside, "secret"), "chief-credentials");
        await expect(testHome(dir, { models })).rejects.toThrow(
            /root|directory/i,
        );
        expect(await readdir(outside)).toEqual(["secret"]);
        expect(await readFile(join(outside, "secret"), "utf8")).toBe(
            "chief-credentials",
        );
    },
);

it("refuses a linked root before starting an environment or any tree operation", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR),
        outside = scratch(),
        stage = scratch();
    await writeFile(join(outside, "canary"), "private");
    await rm(root, { recursive: true });
    await symlink(outside, root);
    expect(
        () => new SandboxEnv({ cwd: root, policy: closedPolicy(root) }),
    ).toThrow(/root|directory/i);
    await expect(listTree(home, root, ".", context)).resolves.toMatchObject({
        ok: false,
    });
    await expect(
        stageTree(
            home,
            { root, rel: "." },
            stage,
            { entries: 10, bytes: 100 },
            context,
        ),
    ).resolves.toMatchObject({ ok: false });
    await expect(
        removeTree(home, root, ["canary"], context),
    ).resolves.toMatchObject({ ok: false });
    await writeFile(join(stage, "new"), "new");
    await expect(
        deliverTree(home, { root, into: "." }, stage, ["new"], context),
    ).resolves.toMatchObject({ ok: false });
    expect(await readFile(join(outside, "canary"), "utf8")).toBe("private");
});
