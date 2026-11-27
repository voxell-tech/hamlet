import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { inFolder } from "../native/support.ts";
import { startDemo } from "./support.ts";

/** The demo opens no sandbox on Windows, so its workspace cannot start. */
const SKIP = { skip: process.platform === "win32" };

/** The commit a `tag` line names, as hexadecimal. */
function taggedCommit(line: string): string {
    const bytes = /commit: CommitId\(Sha1\(\[([\d, ]+)\]\)\)/.exec(line)?.[1];
    assert.ok(bytes, line);
    return bytes
        .split(", ")
        .map((byte) => Number(byte).toString(16).padStart(2, "0"))
        .join("");
}

test(
    "a hire made from a profile's version fills from it and saves to its branch",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const demo = startDemo(dir, ["--realm-actor"]);
            const made = await demo.ask(
                "profile make 1 c1 Ada A careful reader.",
            );
            const id = /^made\/c1 (\d+)$/.exec(made)?.[1];
            assert.ok(id, made);
            const v1 = taggedCommit(
                await demo.ask(`profile ${id} tag v1 first`),
            );
            assert.equal(
                await demo.ask(`profile ${id} edit CHARACTER.md Changed.`),
                "edit Done",
            );
            assert.equal(await demo.ask("realm found h1"), "founded/h1 2");
            assert.equal(await demo.ask("realm live 2"), "live/2 1");
            assert.equal(
                await demo.ask(`realm hire 2 c2 profile#${id}@v1 helper`),
                "hired/c2 3",
            );
            assert.equal(await demo.ask("realm live 3"), "live/3 1");
            const folder = join(dir, "filled");
            assert.match(
                await demo.ask(`profile ${id} fill 3 ${folder}`),
                new RegExp(`^fill parts=1 files=0 largest=\\d+ tip=${v1}$`),
            );
            assert.match(
                await demo.ask(`profile ${id} save 3 notes.md kept`),
                /^save Saved /,
            );
            assert.match(
                await demo.ask(`profile ${id} fill 3 ${folder}`),
                /^fill parts=1 files=1 /,
            );
            assert.equal(
                readFileSync(join(folder, "notes.md"), "utf8"),
                "kept",
            );
            assert.match(
                await demo.ask("library shelves 3"),
                /^shelves ShelfList /,
            );
            assert.equal(await demo.stop(), 0);
        });
    },
);
