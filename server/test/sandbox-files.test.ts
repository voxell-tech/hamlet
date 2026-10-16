import { spawn } from "node:child_process";
import { once } from "node:events";
import {
    mkdirSync,
    readFileSync,
    realpathSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { readdir } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { expect, it } from "vitest";
import { closedPolicy } from "../src/sandbox/reach.ts";
import { SandboxEnv } from "../src/sandbox.ts";
import { closeAfter, context, scratch } from "./support.ts";

async function racing() {
    const house = scratch();
    const inside = join(house, "inside");
    const outside = scratch();
    mkdirSync(inside);
    writeFileSync(join(inside, "file"), "inside");
    writeFileSync(join(outside, "file"), "outside-secret");
    const link = join(house, "link");
    symlinkSync(inside, link);
    const swapper = spawn(
        process.execPath,
        [
            "--input-type=module",
            "-e",
            `
        import { symlinkSync, renameSync } from 'node:fs';
        const [link, inside, outside] = process.argv.slice(1);
        process.stdout.write('ready');
        for (;;) for (const target of [inside, outside]) {
            symlinkSync(target, link + '-next');
            renameSync(link + '-next', link);
            await new Promise(resolve => setTimeout(resolve, 1));
        }
    `,
            link,
            inside,
            outside,
        ],
        { stdio: ["ignore", "pipe", "inherit"] },
    );
    closeAfter(async () => {
        swapper.kill();
        await once(swapper, "close");
    });
    await once(swapper.stdout, "data");
    const env = new SandboxEnv({ cwd: house, policy: closedPolicy(house) });
    closeAfter(() => env.cleanup(context));
    expect(await env.readTextFile(join(inside, "file"), context)).toEqual({
        ok: true,
        value: "inside",
    });
    return { house, inside, outside, env, path: join(link, "file") };
}

it("never reads an outside file when a parent link swaps during readTextFile (C18, S12)", async () => {
    const { env, path } = await racing();
    for (let n = 0; n < 600; n++) {
        const result = await env.readTextFile(path, context);
        if (result.ok) expect(result.value).toBe("inside");
    }
});

it.each([
    "readBinaryFile",
    "readTextLines",
    "openTextLineReader",
    "openBinaryReader",
    "fileInfo",
    "canonicalPath",
] as const)(
    "keeps %s inside the house while a parent link swaps",
    async (method) => {
        const { env, path, inside } = await racing();
        for (let n = 0; n < 250; n++) {
            const result =
                method === "openBinaryReader"
                    ? await env.openBinaryReader(path, undefined, context)
                    : method === "readTextLines"
                      ? await env.readTextLines(path, undefined, context)
                      : await env[method](path, context);
            if (!result.ok) continue;
            if (method === "openBinaryReader") {
                const reader =
                    result.value as import("@earendil-works/pi-durable/env").BinaryReader;
                const bytes = await reader.read(0, 100, context);
                if (bytes.ok)
                    expect(new TextDecoder().decode(bytes.value)).toBe(
                        "inside",
                    );
                await reader.close(context);
            } else if (method === "openTextLineReader") {
                const reader =
                    result.value as import("@earendil-works/pi-durable/env").TextLineReader;
                const line = await reader.readLine(context);
                if (line.ok) expect(line.value?.text).toBe("inside");
                await reader.close(context);
            } else if (method === "readBinaryFile")
                expect(
                    new TextDecoder().decode(result.value as Uint8Array),
                ).toBe("inside");
            else if (method === "readTextLines")
                expect(result.value).toEqual(["inside"]);
            else if (method === "fileInfo")
                expect(result.value).toMatchObject({ size: 6 });
            else if (method === "canonicalPath")
                expect(result.value).toBe(realpathSync(join(inside, "file")));
        }
    },
);

it.each(["listDir", "openDirReader"] as const)(
    "keeps %s names private under a swapping link",
    async (method) => {
        const { env, path, outside } = await racing();
        writeFileSync(join(outside, "outside-secret-name"), "secret");
        for (let n = 0; n < 250; n++) {
            const result = await env[method](join(path, ".."), context);
            if (!result.ok) continue;
            if (method === "listDir")
                expect(JSON.stringify(result.value)).not.toContain(
                    "outside-secret-name",
                );
            else {
                const reader =
                    result.value as import("@earendil-works/pi-durable/env").DirReader;
                expect(
                    JSON.stringify(await reader.next(100, context)),
                ).not.toContain("outside-secret-name");
                await reader.close(context);
            }
        }
    },
);

it.each([
    "writeFile",
    "appendFile",
    "truncateFile",
    "flushFile",
    "renameFile",
    "remove",
    "createDir",
] as const)(
    "keeps %s changes inside the house under a swapping link",
    async (method) => {
        const { env, path, outside, inside } = await racing();
        for (let n = 0; n < 250; n++) {
            writeFileSync(join(inside, "file"), "inside");
            if (method === "writeFile" || method === "appendFile")
                await env[method](path, "WRITTEN", context);
            else if (method === "truncateFile")
                await env.truncateFile(path, 1, context);
            else if (method === "flushFile") await env.flushFile(path, context);
            else if (method === "renameFile") {
                await env.renameFile(path, join(inside, "moved"), context);
                await env.renameFile(
                    join(inside, "file"),
                    join(path, "..", "destination"),
                    context,
                );
            } else if (method === "remove")
                await env.remove(path, undefined, context);
            else
                await env.createDir(
                    join(path, "..", "new-dir"),
                    undefined,
                    context,
                );
            expect(readFileSync(join(outside, "file"), "utf8")).toBe(
                "outside-secret",
            );
        }
        await expect(readdir(outside)).resolves.toEqual(["file"]);
    },
);

it("keeps exists from disclosing an outside name under a swapping link", async () => {
    const { env, path, outside } = await racing();
    writeFileSync(join(outside, "outside-only"), "secret");
    for (let n = 0; n < 250; n++) {
        const found = await env.exists(
            join(path, "..", "outside-only"),
            context,
        );
        if (found.ok) expect(found.value).toBe(false);
    }
});

it.each(["createTempDir", "createTempFile"] as const)(
    "confines %s even when a prefix goes through a swapping link",
    async (method) => {
        const { env, house, outside } = await racing();
        const tmp = await env.createTempDir(undefined, context);
        if (!tmp.ok) throw tmp.error;
        const prefix = `${relative(method === "createTempDir" ? dirname(tmp.value) : tmp.value, join(house, "link"))}/temp-`;
        for (let n = 0; n < 150; n++) {
            if (method === "createTempDir")
                await env.createTempDir(prefix, context);
            else await env.createTempFile({ prefix }, context);
        }
        await expect(readdir(outside)).resolves.toEqual(["file"]);
    },
);

it("keeps a watch from reporting names outside a swapping parent link", async () => {
    const { env, path, outside } = await racing();
    const changes: unknown[] = [];
    for (let n = 0; n < 20; n++) {
        const watch = await env.watch(
            [{ path: join(path, ".."), recursive: true }],
            (change) => changes.push(change),
            context,
        );
        if (!watch.ok) continue;
        writeFileSync(join(outside, `outside-name-${n}`), "secret");
        await new Promise((resolve) => setTimeout(resolve, 60));
        await watch.value.close(context);
    }
    expect(JSON.stringify(changes)).not.toContain("outside-name");
});
