import { randomUUID } from "node:crypto";
import { realpathSync } from "node:fs";
import { mkdir, symlink } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import type { Home } from "../src/home.ts";
import { HOME, RecordDoc } from "../src/record.ts";
import { say } from "../src/say.ts";
import { WorkDoc } from "../src/work.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

it("keeps the folder, its earlier folders and mode through a restart without a model call", async () => {
    const { models, faux } = fauxModels();
    const dir = scratch();
    let home = await testHome(dir, { models });
    const first = realpathSync(scratch());
    const second = realpathSync(scratch());
    expect(await typed(home, "/folder")).toMatchObject({
        text: "This hamlet works on no folder. `/folder <folder>` points it at one.",
    });
    expect(await typed(home, `/folder ${first}`)).toMatchObject({
        text: `This hamlet works on \`${first}\` now. Agents ask you before each edit and command.`,
    });
    expect(await typed(home, "/mode auto")).toMatchObject({
        text: "Agents edit and run commands in the folder without asking.",
    });
    expect(await typed(home, `/folder ${second}`)).toMatchObject({
        kind: "notice",
    });
    expect(await work(home)).toEqual({
        folder: second,
        mode: "auto",
        before: [first],
    });
    expect(await typed(home, "/folder none")).toMatchObject({
        text: "This hamlet works on no folder now. Its agents are back in their own workspaces.",
    });
    expect(await work(home)).toEqual({
        folder: null,
        mode: "auto",
        before: [second, first],
    });
    expect(await typed(home, `/folder ${first}`)).toMatchObject({
        kind: "notice",
    });
    expect(await work(home)).toEqual({
        folder: first,
        mode: "auto",
        before: [second],
    });
    await typed(home, `/folder ${first}`);
    expect(await work(home)).toEqual({
        folder: first,
        mode: "auto",
        before: [second],
    });
    expect(await typed(home, "/mode fast")).toMatchObject({
        kind: "error",
        text: "usage: /mode <manual|auto>",
    });
    expect(await work(home)).toEqual({
        folder: first,
        mode: "auto",
        before: [second],
    });
    await home.close(context);
    home = await testHome(dir, { models });
    expect(await typed(home, "/folder")).toMatchObject({
        text: `This hamlet works on \`${first}\`. Agents edit and run commands in the folder without asking. \`/folder none\` stops that.`,
    });
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.decisions,
    ).toContainEqual(
        expect.objectContaining({
            kind: "folder_set",
            folder: first,
            by: "chief",
        }),
    );
    expect(
        (await home.harness.snapshot(RecordDoc, HOME, context))?.decisions,
    ).toContainEqual(
        expect.objectContaining({
            kind: "mode_set",
            mode: "auto",
            by: "chief",
        }),
    );
    expect(faux.state.callCount).toBe(0);
});

it("refuses Hamlet's home, its children and parents, links to it and relative or missing folders", async () => {
    const { models } = fauxModels();
    const outer = realpathSync(scratch());
    const home = await testHome(join(outer, "home"), { models });
    await mkdir(join(home.dir, "inside"));
    const alias = join(outer, "alias");
    await symlink(home.dir, alias);
    for (const folder of [home.dir, join(home.dir, "inside"), outer, alias]) {
        expect(await typed(home, `/folder ${folder}`)).toMatchObject({
            kind: "error",
            text: `a hamlet cannot work on Hamlet's own folder (${home.dir}), or on one that holds it`,
        });
        expect((await work(home))?.folder).toBeNull();
    }
    expect(await typed(home, "/folder relative")).toMatchObject({
        kind: "error",
        text: "give the folder's full path, such as /Users/you/code/project",
    });
    expect(await typed(home, `/folder ${outer}/missing`)).toMatchObject({
        kind: "error",
        text: `\`${outer}/missing\` is not a folder`,
    });
    expect((await work(home))?.folder).toBeNull();
    expect(await typed(home, "/folder ~")).toMatchObject({ kind: "notice" });
    expect((await work(home))?.folder).toBe(realpathSync(homedir()));
});

function typed(home: Home, text: string) {
    return say(
        home,
        {
            type: "say",
            id: randomUUID(),
            to: { hamlet: HOME, agent: "mayor" },
            text,
        },
        context,
    );
}
function work(home: Home) {
    return home.harness.snapshot(WorkDoc, HOME, context);
}
