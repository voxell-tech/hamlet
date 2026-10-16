import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, it } from "vitest";
import { houseReader, treeWorker } from "../src/environment.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

it("owns house readers per home and keeps notes and skills within that house", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const agent = { hamlet: "home", number: 0 };
    const house = home.houses.of(agent);
    expect(home.houses.of(agent)).toBe(house);
    await mkdir(join(house.path, "skills", "one"), { recursive: true });
    await writeFile(
        join(house.path, "skills", "one", "SKILL.md"),
        "---\nname: one\n---\n",
    );
    expect(await house.skillNames(context)).toEqual(["one"]);
    expect(await house.notes(context)).toMatchObject({
        ok: true,
        value: { text: "# Notes\n" },
    });
    const reader = houseReader(home, agent, context);
    try {
        expect(
            (
                await reader.writeFile(
                    join(house.path, "NOTES.md"),
                    "changed",
                    context,
                )
            ).ok,
        ).toBe(false);
        expect(
            (await reader.readTextFile(home.layout.credentials, context)).ok,
        ).toBe(false);
    } finally {
        await reader.cleanup(context);
    }
});

it("tree workers read only their source and write only their stage", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const root = scratch(),
        stage = scratch(),
        outside = scratch();
    await writeFile(join(root, "source"), "source");
    await writeFile(join(outside, "secret"), "secret");
    const env = treeWorker(home, {
        root,
        stage,
        caps: { entries: 10, bytes: 100 },
    });
    try {
        expect((await env.readTextFile(join(root, "source"), context)).ok).toBe(
            true,
        );
        expect(
            (await env.readTextFile(join(outside, "secret"), context)).ok,
        ).toBe(false);
        expect(
            (await env.writeFile(join(root, "source"), "bad", context)).ok,
        ).toBe(false);
        expect(
            (await env.writeFile(join(stage, "copy"), "copy", context)).ok,
        ).toBe(true);
    } finally {
        await env.cleanup(context);
    }
});

it("returns a classified work-folder refusal without changing the current folder", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const { folder } = await import("../src/environment.ts");
    expect(await folder(home, "home", "relative", context)).toMatchObject({
        ok: false,
        error: { kind: "folder-not-absolute" },
    });
});

it("distinguishes missing notes from invalid UTF-8 without changing the original bytes", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const house = home.houses.of({ hamlet: "home", number: 0 });
    const path = join(house.path, "NOTES.md");
    await rm(path);
    expect(await house.notes(context)).toMatchObject({
        ok: false,
        error: { kind: "notes-missing" },
    });
    await writeFile(path, Buffer.from([0xff]));
    expect(await house.notes(context)).toMatchObject({
        ok: false,
        error: { kind: "notes-not-utf8" },
    });
    expect([...(await readFile(path))]).toEqual([0xff]);
});

it("keeps a house's identity after the caller reuses its reference", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const agent = { hamlet: "home", number: 7 };
    const house = home.houses.of(agent);
    await house.build();
    agent.number = 8;
    expect(await house.notes(context)).toMatchObject({
        ok: true,
        value: { text: "# Notes\n" },
    });
});
