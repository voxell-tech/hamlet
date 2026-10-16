import { randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import { houseOf } from "../src/home/layout.ts";
import { uploads } from "../src/house.ts";
import {
    context,
    fauxModels,
    MAYOR,
    mayorChat,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

function gate() {
    let release = () => {};
    const ready = new Promise<void>((resolve) => {
        release = resolve;
    });
    return { ready, release };
}

async function gifts() {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const gifts = uploads(home, context);
    return {
        home,
        faux,
        gifts,
        owner: new AbortController(),
        to: { hamlet: "home", agent: "mayor" },
    };
}

it("discards a disconnected manifest after its in-flight staging step, leaving no recreated stage", async () => {
    const run = await gifts();
    const entered = gate();
    const resume = gate();
    const originalMkdir = fs.mkdir;
    const originalRm = fs.rm;
    let removal: Promise<void> | undefined;
    let paused = false;
    const making = vi
        .spyOn(fs, "mkdir")
        .mockImplementation(async (...args: Parameters<typeof fs.mkdir>) => {
            if (String(args[0]).includes("/.gifts/gift-") && !paused) {
                paused = true;
                entered.release();
                await resume.ready;
            }
            return Reflect.apply(originalMkdir, fs, args);
        });
    const removing = vi
        .spyOn(fs, "rm")
        .mockImplementation((...args: Parameters<typeof fs.rm>) => {
            const work = Reflect.apply(originalRm, fs, args);
            if (String(args[0]).includes("/.gifts/gift-")) removal = work;
            return work;
        });
    syncBuiltinESMExports();
    try {
        const begin = run.gifts.begin(
            {
                type: "give",
                id: randomUUID(),
                to: run.to,
                files: [
                    { path: "folder/a", size: 1 },
                    { path: "folder/b", size: 1 },
                ],
            },
            run.owner.signal,
        );
        const refused = expect(begin).resolves.toMatchObject({
            ok: false,
            error: { kind: "connection-closed" },
        });
        await entered.ready;
        run.owner.abort();
        await removal;
        resume.release();
        await refused;
        await run.gifts.stop();
        expect(await fs.readdir(join(run.home.layout.dir, ".gifts"))).toEqual(
            [],
        );
        expect(
            await fs.readdir(join(houseOf(run.home.layout, MAYOR), "inbox")),
        ).toEqual([]);
        expect(run.faux.state.callCount).toBe(0);
    } finally {
        resume.release();
        making.mockRestore();
        removing.mockRestore();
        syncBuiltinESMExports();
        await run.gifts.stop();
    }
});

it("finishes the whole gift and its letter when the connection closes after every byte is in", async () => {
    const run = await gifts();
    const begin = await run.gifts.begin(
        {
            type: "give",
            id: randomUUID(),
            to: run.to,
            files: [
                { path: "folder/empty", size: 0 },
                { path: "folder/last", size: 1 },
            ],
        },
        run.owner.signal,
    );
    if (!begin.ok || !begin.value.upload) throw new Error("No upload");
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    const entered = gate();
    const resume = gate();
    const originalStat = fs.lstat;
    const originalRm = fs.rm;
    let removal: Promise<void> | undefined;
    const stating = vi
        .spyOn(fs, "lstat")
        .mockImplementation(async (...args: Parameters<typeof fs.lstat>) => {
            if (String(args[0]) === inbox) {
                entered.release();
                await resume.ready;
            }
            return Reflect.apply(originalStat, fs, args);
        });
    const removing = vi
        .spyOn(fs, "rm")
        .mockImplementation((...args: Parameters<typeof fs.rm>) => {
            const work = Reflect.apply(originalRm, fs, args);
            if (String(args[0]).includes("/.gifts/gift-")) removal = work;
            return work;
        });
    syncBuiltinESMExports();
    try {
        const part = run.gifts.part(
            {
                type: "give-part",
                id: randomUUID(),
                upload: begin.value.upload,
                path: "folder/last",
                offset: 0,
                data: "QQ==",
            },
            run.owner.signal,
        );
        const delivered = expect(part).resolves.toEqual({
            ok: true,
            value: "Handed over `inbox/folder`.",
        });
        await entered.ready;
        run.owner.abort();
        await removal;
        resume.release();
        await delivered;
        await run.gifts.stop();
        expect(await fs.readFile(join(inbox, "folder/last"), "utf8")).toBe("A");
        expect(await fs.readFile(join(inbox, "folder/empty"))).toHaveLength(0);
        expect(await fs.readdir(join(run.home.layout.dir, ".gifts"))).toEqual(
            [],
        );
        expect(await transcript(await mayorChat(run.home))).toContainEqual(
            expect.objectContaining({
                kind: "hamlet.chief",
                data: expect.objectContaining({
                    text: expect.stringContaining("inbox/folder"),
                }),
            }),
        );
    } finally {
        resume.release();
        stating.mockRestore();
        removing.mockRestore();
        syncBuiltinESMExports();
        await run.gifts.stop();
    }
});

it("confines the write even when the inbox becomes a link after its filesystem check", async () => {
    const run = await gifts();
    const begin = await run.gifts.begin(
        {
            type: "give",
            id: randomUUID(),
            to: run.to,
            files: [{ path: "folder/last", size: 1 }],
        },
        run.owner.signal,
    );
    if (!begin.ok || !begin.value.upload) throw new Error("No upload");
    const inbox = join(houseOf(run.home.layout, MAYOR), "inbox");
    const outside = scratch();
    const original = fs.lstat;
    let swapped = false;
    const stating = vi
        .spyOn(fs, "lstat")
        .mockImplementation(async (...args: Parameters<typeof fs.lstat>) => {
            const info = await Reflect.apply(original, fs, args);
            if (!swapped && String(args[0]) === inbox) {
                swapped = true;
                await fs.rename(inbox, `${inbox}-kept`);
                await fs.symlink(outside, inbox);
            }
            return info;
        });
    syncBuiltinESMExports();
    try {
        await expect(
            run.gifts.part(
                {
                    type: "give-part",
                    id: randomUUID(),
                    upload: begin.value.upload,
                    path: "folder/last",
                    offset: 0,
                    data: "QQ==",
                },
                run.owner.signal,
            ),
        ).resolves.toMatchObject({ ok: false });
        expect(await fs.readdir(outside)).toEqual([]);
        expect(await fs.readdir(`${inbox}-kept`)).toEqual([]);
        expect(await fs.readdir(join(run.home.layout.dir, ".gifts"))).toEqual(
            [],
        );
        expect(run.faux.state.callCount).toBe(0);
    } finally {
        stating.mockRestore();
        syncBuiltinESMExports();
        await run.gifts.stop();
    }
});
