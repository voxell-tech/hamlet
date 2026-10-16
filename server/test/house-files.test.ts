import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import * as fs from "node:fs/promises";
import {
    chmod,
    lstat,
    mkdir,
    open,
    readdir,
    readFile,
    symlink,
    writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { ExecutionError, err } from "@earendil-works/pi-durable/env";
import { expect, it, vi } from "vitest";
import { houseOf } from "../src/home/layout.ts";
import {
    deliverTree,
    listTree,
    removeHouse,
    removeTree,
    stageTree,
} from "../src/house.ts";
import { closedPolicy, SandboxEnv } from "../src/sandbox.ts";
import {
    context,
    fauxModels,
    MAYOR,
    scratch,
    sh,
    testHome,
} from "./support.ts";

vi.mock("node:fs/promises", async (original) => ({
    ...(await original<typeof import("node:fs/promises")>()),
}));

const caps = { entries: 4096, bytes: 32 * 1024 * 1024 };

it.skipIf(process.platform !== "darwin")(
    "revokes a lingering process's writes before traversing an erased house",
    async () => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        const house = houseOf(home.layout, MAYOR);
        const outside = scratch();
        await mkdir(join(house, "work"));
        await writeFile(join(house, "work/canary"), "mine");
        await writeFile(join(outside, "canary"), "private");
        const env = new SandboxEnv({ cwd: house, policy: closedPolicy(house) });
        const tmp = (await sh(env, 'printf %s "$TMPDIR"')).output;
        await sh(
            env,
            `(cd '${house}'; (while [ ! -e '${tmp}/release' ]; do sleep 0.01; done; mv work moved && ln -s '${outside}' work; echo $? > '${tmp}/done') >/dev/null 2>&1 &)`,
        );
        await env.cleanup(context);
        const original = fs.readdir;
        let attempted = false;
        const walking = vi
            .spyOn(fs, "readdir")
            .mockImplementation(
                async (...args: Parameters<typeof fs.readdir>) => {
                    if (String(args[0]).endsWith("/work") && !attempted) {
                        attempted = true;
                        await writeFile(join(tmp, "release"), "go");
                        await vi.waitFor(async () =>
                            expect(
                                await readFile(join(tmp, "done"), "utf8"),
                            ).toBeTruthy(),
                        );
                    }
                    return original(...args);
                },
            );
        try {
            await removeHouse(home, house);
            expect(attempted).toBe(true);
            expect(await readFile(join(tmp, "done"), "utf8")).not.toBe("0\n");
            expect(await readFile(join(outside, "canary"), "utf8")).toBe(
                "private",
            );
        } finally {
            walking.mockRestore();
            await writeFile(join(tmp, "release"), "go");
        }
    },
);

it("stages regular files without following links or copying special files, naming every refused entry", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const stage = scratch();
    const outside = scratch();
    await writeFile(join(outside, "canary"), "Not for an agent");
    await mkdir(join(root, "tree"));
    await writeFile(join(root, "tree/regular"), "Allowed");
    await symlink(join(outside, "canary"), join(root, "tree/link"));
    execFileSync("mkfifo", [join(root, "tree/pipe")]);
    let words = "";
    try {
        await stageTree(home, { root, rel: "tree" }, stage, caps, context);
    } catch (error) {
        words = String(error);
    }
    expect(words).toContain("link");
    expect(words).toContain("pipe");
    expect(words).not.toContain(root);
    expect(words).not.toContain(outside);
    expect(await readdir(join(stage, "tree"))).toEqual(["regular"]);
    expect(await readFile(join(stage, "tree/regular"), "utf8")).toBe("Allowed");
    expect(await readFile(join(outside, "canary"), "utf8")).toBe(
        "Not for an agent",
    );
});

it("stages a whole house or a single tree, preserving bytes and empty folders", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    await mkdir(join(root, "tree/empty"), { recursive: true });
    await writeFile(join(root, "tree/bytes"), Buffer.from([0, 255, 1, 2]));
    const stage = scratch();
    await stageTree(
        home,
        { root: `${root}/`, rel: "." },
        `${stage}/`,
        { entries: 20000, bytes: 512 * 1024 * 1024 },
        context,
    );
    expect(await readFile(join(stage, "tree/bytes"))).toEqual(
        Buffer.from([0, 255, 1, 2]),
    );
    expect((await lstat(join(stage, "tree/empty"))).isDirectory()).toBe(true);
    const one = scratch();
    await stageTree(home, { root, rel: "tree/bytes" }, one, caps, context);
    expect(await readFile(join(one, "bytes"))).toEqual(
        Buffer.from([0, 255, 1, 2]),
    );
});

it("keeps ordinary modes through staging and delivery, clearing setuid and setgid", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    await mkdir(join(root, "tool/scripts"), { recursive: true });
    await writeFile(join(root, "tool/scripts/run"), "#!/bin/sh\necho ready\n");
    await chmod(join(root, "tool/scripts/run"), 0o6751);
    await chmod(join(root, "tool/scripts"), 0o2750);
    await chmod(join(root, "tool"), 0o750);
    const stage = scratch();
    await stageTree(home, { root, rel: "tool" }, stage, caps, context);
    for (const [path, mode] of [
        ["tool", 0o750],
        ["tool/scripts", 0o750],
        ["tool/scripts/run", 0o751],
    ] as const)
        expect((await lstat(join(stage, path))).mode & 0o7777).toBe(mode);
    await deliverTree(home, { root, into: "inbox" }, stage, ["tool"], context);
    for (const [path, mode] of [
        ["tool", 0o750],
        ["tool/scripts", 0o750],
        ["tool/scripts/run", 0o751],
    ] as const)
        expect((await lstat(join(root, "inbox", path))).mode & 0o7777).toBe(
            mode,
        );
});

it.each(["entries", "bytes"] as const)(
    "refuses a tree over the caller's %s cap before delivery",
    async (meter) => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        const root = houseOf(home.layout, MAYOR);
        await mkdir(join(root, "tree"));
        await writeFile(join(root, "tree/file"), "four");
        const stage = scratch();
        await expect(
            stageTree(
                home,
                { root, rel: "tree" },
                stage,
                {
                    entries: meter === "entries" ? 1 : 10,
                    bytes: meter === "bytes" ? 3 : 100,
                },
                context,
            ),
        ).rejects.toThrow(meter);
        expect(await readdir(join(root, "inbox"))).toEqual([]);
    },
);

it("delivers without clobbering files, directories or links and rolls back only its claims", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const stage = scratch();
    await writeFile(join(stage, "first"), "new");
    await mkdir(join(stage, "parcel"));
    await writeFile(join(stage, "parcel/contents"), "new");
    await mkdir(join(root, "inbox/parcel"));
    await writeFile(join(root, "inbox/parcel/mine"), "Keep");
    await expect(
        deliverTree(
            home,
            { root, into: "inbox" },
            stage,
            ["first", "parcel"],
            context,
        ),
    ).rejects.toThrow();
    expect(await readdir(join(root, "inbox"))).toEqual(["parcel"]);
    expect(await readdir(join(root, "inbox/parcel"))).toEqual(["mine"]);
    const outside = scratch();
    await writeFile(join(stage, "linked"), "new");
    await symlink(outside, join(root, "inbox/linked"));
    await expect(
        deliverTree(home, { root, into: "inbox" }, stage, ["linked"], context),
    ).rejects.toThrow();
    expect(await readdir(outside)).toEqual([]);
    await writeFile(join(root, "inbox/first"), "Keep file");
    await expect(
        deliverTree(home, { root, into: "inbox" }, stage, ["first"], context),
    ).rejects.toThrow();
    expect(await readFile(join(root, "inbox/first"), "utf8")).toBe("Keep file");
    await deliverTree(
        home,
        { root, into: "brought" },
        stage,
        ["parcel"],
        context,
    );
    expect(await readFile(join(root, "brought/parcel/contents"), "utf8")).toBe(
        "new",
    );
});

it("does not write through a linked destination parent, even when it is swapped at copy time", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const stage = scratch();
    const outside = scratch();
    await writeFile(join(stage, "file"), "new");
    const original = SandboxEnv.prototype.exec;
    let swapped = false;
    const copying = vi
        .spyOn(SandboxEnv.prototype, "exec")
        .mockImplementation(async function (
            this: SandboxEnv,
            command,
            options,
            c,
        ) {
            if (
                Array.isArray(command) &&
                command.includes(join(root, "brought")) &&
                !swapped
            ) {
                swapped = true;
                await symlink(outside, join(root, "brought"));
            }
            return original.call(this, command, options, c);
        });
    try {
        await expect(
            deliverTree(
                home,
                { root, into: "brought" },
                stage,
                ["file"],
                context,
            ),
        ).rejects.toThrow();
    } finally {
        copying.mockRestore();
    }
    expect(swapped).toBe(true);
    expect(await readdir(outside)).toEqual([]);
});

it("removes only claimed targets when the sandbox reports failure after copying", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const stage = scratch();
    await writeFile(join(stage, "new"), "new");
    await writeFile(join(root, "inbox/mine"), "keep");
    const original = SandboxEnv.prototype.exec;
    let failed = false;
    const copying = vi
        .spyOn(SandboxEnv.prototype, "exec")
        .mockImplementation(async function (
            this: SandboxEnv,
            command,
            options,
            c,
        ) {
            const result = await original.call(this, command, options, c);
            if (
                Array.isArray(command) &&
                command.includes(join(root, "inbox")) &&
                !failed
            ) {
                failed = true;
                return err(new ExecutionError("timeout", "timed out"));
            }
            return result;
        });
    try {
        await expect(
            deliverTree(home, { root, into: "inbox" }, stage, ["new"], context),
        ).rejects.toThrow();
    } finally {
        copying.mockRestore();
    }
    expect(failed).toBe(true);
    expect(await readdir(join(root, "inbox"))).toEqual(["mine"]);
});

it("lists regular files with their size and SHA-256, reporting links and special files", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const outside = scratch();
    await mkdir(join(root, "tree"));
    await writeFile(join(root, "tree/file"), "hello");
    await writeFile(join(outside, "canary"), "private");
    await symlink(outside, join(root, "tree/link"));
    execFileSync("mkfifo", [join(root, "tree/pipe")]);
    expect(await listTree(home, root, "tree", context)).toEqual({
        files: [
            {
                path: "tree/file",
                size: 5,
                sha256: createHash("sha256").update("hello").digest("hex"),
            },
        ],
        skipped: [
            { path: "tree/link", kind: "link" },
            { path: "tree/pipe", kind: "special" },
        ],
    });
    expect(await listTree(home, root, "tree/link", context)).toEqual({
        files: [],
        skipped: [{ path: "tree/link", kind: "link" }],
    });
});

it("removes a link itself without touching the folder outside and refuses unsafe paths", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const outside = scratch();
    await writeFile(join(outside, "canary"), "keep");
    await symlink(outside, join(root, "linked"));
    await removeTree(home, root, ["linked"], context);
    expect(
        await lstat(join(root, "linked")).catch(() => undefined),
    ).toBeUndefined();
    expect(await readFile(join(outside, "canary"), "utf8")).toBe("keep");
    for (const rel of [".", "../canary", "/tmp/canary", "inbox/../../canary"])
        await expect(removeTree(home, root, [rel], context)).rejects.toThrow();
    for (const dir of [
        home.dir,
        home.layout.houses,
        home.layout.library,
        `${home.layout.houses}/home/../home/0`,
        outside,
    ])
        await expect(removeHouse(home, dir)).rejects.toThrow();
    await symlink(outside, join(home.layout.houses, "linked"));
    await expect(
        removeHouse(home, join(home.layout.houses, "linked")),
    ).rejects.toThrow();
    await mkdir(join(home.layout.houses, "gone"));
    await writeFile(join(home.layout.houses, "gone/notes"), "four");
    expect(await removeHouse(home, join(home.layout.houses, "gone"))).toBe(4);
    expect(await readdir(outside)).toEqual(["canary"]);
});

it("keeps list operations read-only and remove operations confined to their root", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const outside = scratch();
    await writeFile(join(outside, "canary"), "private");
    await writeFile(join(root, "mine"), "keep");
    const original = SandboxEnv.prototype.exec;
    const observed: { mode: string; stdout: string; exit: number }[] = [];
    const running = vi
        .spyOn(SandboxEnv.prototype, "exec")
        .mockImplementation(async function (
            this: SandboxEnv,
            command,
            options,
            c,
        ) {
            if (
                Array.isArray(command) &&
                command.includes(root) &&
                command.includes("-e")
            ) {
                const mode = command.includes("remove") ? "remove" : "list";
                const probe =
                    mode === "list"
                        ? `const fs = require('node:fs'); let read = false, write = false; try { fs.readFileSync(process.argv[1]); read = true; } catch {} try { fs.writeFileSync(process.argv[2], 'changed'); write = true; } catch {} console.log(JSON.stringify({ read, write }));`
                        : `const fs = require('node:fs'); try { fs.unlinkSync(process.argv[1]); console.log('removed'); } catch { console.log('confined'); }`;
                let stdout = "";
                const result = await original.call(
                    this,
                    [
                        process.execPath,
                        "-e",
                        probe,
                        join(outside, "canary"),
                        join(root, "mine"),
                    ],
                    {
                        onOutput: (text) => {
                            stdout += text;
                        },
                    },
                    c,
                );
                observed.push({
                    mode,
                    stdout: stdout.trim(),
                    exit: result.ok ? result.value.exitCode : -1,
                });
            }
            return original.call(this, command, options, c);
        });
    try {
        await listTree(home, root, "mine", context);
        await removeTree(home, root, ["missing"], context);
    } finally {
        running.mockRestore();
    }
    expect(observed).toEqual([
        { mode: "list", stdout: '{"read":false,"write":false}', exit: 0 },
        { mode: "remove", stdout: "confined", exit: 0 },
    ]);
    expect(await readFile(join(root, "mine"), "utf8")).toBe("keep");
    expect(await readFile(join(outside, "canary"), "utf8")).toBe("private");
});

it("removes nested files, broken links and empty folders, and refuses a linked parent", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    await mkdir(join(root, "tree/empty"), { recursive: true });
    await writeFile(join(root, "tree/file"), "remove");
    await symlink(join(scratch(), "missing"), join(root, "tree/broken"));
    await removeTree(home, root, ["tree", "missing"], context);
    expect(
        await lstat(join(root, "tree")).catch(() => undefined),
    ).toBeUndefined();
    const outside = scratch();
    await writeFile(join(outside, "canary"), "keep");
    await symlink(outside, join(root, "linked"));
    await expect(
        removeTree(home, root, ["linked/canary"], context),
    ).rejects.toThrow();
    expect(await readFile(join(outside, "canary"), "utf8")).toBe("keep");
});

it("accepts a merge tree larger than a gift while still enforcing the gift cap", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = houseOf(home.layout, MAYOR);
    const file = await open(join(root, "large"), "w");
    try {
        await file.truncate(33 * 1024 * 1024);
    } finally {
        await file.close();
    }
    const stage = scratch();
    await stageTree(
        home,
        { root, rel: "large" },
        stage,
        { entries: 20000, bytes: 512 * 1024 * 1024 },
        context,
    );
    expect((await lstat(join(stage, "large"))).size).toBe(33 * 1024 * 1024);
    await deliverTree(
        home,
        { root, into: "merged" },
        stage,
        ["large"],
        context,
    );
    expect((await lstat(join(root, "merged/large"))).size).toBe(
        33 * 1024 * 1024,
    );
    await expect(
        stageTree(home, { root, rel: "large" }, scratch(), caps, context),
    ).rejects.toThrow("bytes");
});
