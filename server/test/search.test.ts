import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";
import { openPolicy } from "../src/sandbox/reach.ts";
import { SandboxEnv } from "../src/sandbox.ts";
import { find } from "../src/tools/find.ts";
import { search } from "../src/tools/search.ts";
import { globMatches, wanted } from "../src/tools/walk.ts";
import { context, scratch } from "./support.ts";

/** A project folder beside a home, seen from the folder as an agent of the home sees it. */
function project() {
    const folder = scratch();
    const outer = scratch();
    const home = join(outer, "home");
    const house = join(home, "houses", "home", "0");
    const put = (path: string, text: string | Buffer) => {
        mkdirSync(dirname(path), { recursive: true });
        writeFileSync(path, text);
    };
    put(join(folder, "src/main.rs"), "fn main() {\n    todo!()\n}\n");
    put(join(folder, "src/deep/mod.rs"), "// TODO later\n");
    put(join(folder, "target/debug/main.rs"), "fn main() {}\n");
    put(join(folder, "node_modules/x/main.js"), "fn main\n");
    put(join(folder, ".git/config"), "fn main\n");
    put(join(folder, "blob.bin"), "\0fn main");
    put(
        join(folder, "raw.txt"),
        Buffer.from([0xff, 0x20, 0x66, 0x6e, 0x20, 0x6d, 0x61, 0x69, 0x6e]),
    );
    put(join(folder, "big.txt"), `fn main\n${"x".repeat(1024 * 1024)}`);
    put(join(home, "api_key.x"), "fn main");
    put(join(house, "NOTES.md"), "mine");
    const at = (cwd: string) =>
        new SandboxEnv({
            cwd,
            policy: openPolicy({ folder, house, home, shared: [] }),
        });
    return { folder, outer, home, house, env: at(folder), at, put };
}

describe("search (reading.rs:389)", () => {
    it("finds a pattern in the files under a folder, and only in text files of reasonable size outside build folders", async () => {
        const { env } = project();

        expect(await search(env, { pattern: "fn\\s+main|todo" }, context)).toBe(
            "src/main.rs:1: fn main() {\nsrc/main.rs:2:     todo!()",
        );
        expect(
            await search(
                env,
                { pattern: "todo", ignore_case: true, name: "mod.*" },
                context,
            ),
        ).toBe("src/deep/mod.rs:1: // TODO later");
        expect(await search(env, { pattern: "nowhere" }, context)).toBe(
            "no match",
        );
    });

    it("names a pattern it cannot take, and refuses a backreference in words", async () => {
        const { env } = project();

        await expect(search(env, { pattern: "(" }, context)).rejects.toThrow(
            "`pattern` is not a regular expression: ",
        );
        await expect(
            search(env, { pattern: "(a)\\1" }, context),
        ).rejects.toThrow(
            "`pattern` holds a backreference, a lookaround or another part a search in linear time cannot take",
        );
        await expect(search(env, { pattern: "" }, context)).rejects.toThrow(
            "`pattern` is required",
        );
    });

    it("answers a pattern that backtracks without end at once", async () => {
        const { env, folder, put } = project();
        put(join(folder, "long.txt"), `${"a".repeat(100_000)}b\n`);

        const started = Date.now();
        expect(
            await search(env, { pattern: "(a+)+$", path: "long.txt" }, context),
        ).toBe("no match");
        expect(Date.now() - started).toBeLessThan(1000);
    });

    it("stops at 100 hits, saying so", async () => {
        const { env, folder, put } = project();
        put(join(folder, "many.txt"), "hit\n".repeat(150));

        const hits = (
            await search(env, { pattern: "hit", path: "many.txt" }, context)
        ).split("\n");
        expect(hits).toHaveLength(101);
        expect(hits[99]).toBe("many.txt:100: hit");
        expect(hits[100]).toBe(
            "[more than 100. Narrow it with `path` or `name`]",
        );
    });

    it("goes around Hamlet's home when it searches from above it, and is refused within it", async () => {
        const { outer, home, house, at } = project();
        const env = at(outer);

        expect(
            await search(
                env,
                { pattern: "fn main", name: "api_key.*" },
                context,
            ),
        ).toBe("no match");
        await expect(
            search(env, { pattern: "fn main", path: home }, context),
        ).rejects.toThrow();
        expect(
            await search(env, { pattern: "mine", path: house }, context),
        ).toBe("home/houses/home/0/NOTES.md:1: mine");
    });
});

describe("find (reading.rs:389)", () => {
    it("lists a folder, sorted with a slash after each folder, or says it is empty", async () => {
        const { env, folder } = project();
        mkdirSync(join(folder, "empty"));

        expect(await find(env, { path: "src" }, context)).toBe(
            "deep/\nmain.rs",
        );
        expect(await find(env, { path: "empty" }, context)).toBe("(empty)");
        await expect(
            find(env, { path: "src/main.rs" }, context),
        ).rejects.toThrow("src/main.rs is not a folder");
    });

    it("finds names under a folder in walk order, its files before its folders", async () => {
        const { env } = project();

        expect(await find(env, { name: "*.rs" }, context)).toBe(
            "src/main.rs\nsrc/deep/mod.rs",
        );
        expect(await find(env, { name: "nothing" }, context)).toBe(
            "no file by that name",
        );
    });
});

describe("a name a glob asks for (reading.rs:378)", () => {
    it("takes * for any run and ? for one character, and a glob with / against the path under the folder", () => {
        expect(globMatches("*.rs", "main.rs")).toBe(true);
        expect(globMatches("ma?n.*", "main.rs")).toBe(true);
        expect(globMatches("*", "")).toBe(true);
        expect(globMatches("*.rs", "main.rs.bak")).toBe(false);
        expect(globMatches("main", "main.rs")).toBe(false);
        expect(wanted("src/*/mod.rs", "src/a/mod.rs")).toBe(true);
        expect(wanted("mod.rs", "src/a/lib.rs")).toBe(false);
        expect(wanted("mod.rs", "src/a/mod.rs")).toBe(true);
    });
});
