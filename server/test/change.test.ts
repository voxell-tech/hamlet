import { describe, expect, it } from "vitest";
import { changeBetween, changeOf } from "../src/change.ts";

/** The lines of the change from `old` to `new`, marked as a diff marks them. */
function marks(old: string, now: string): string {
    return changeBetween("a.rs", old, now, false)
        .lines.map((line) => `${line.mark}${line.text}`)
        .join("|");
}

describe("the change of an edit or a write (change.rs:223, :242)", () => {
    it("is the lines that differ among those that stay", () => {
        expect(marks("a\nb\nc", "a\nB\nc")).toBe(" a|-b|+B| c");
        expect(marks("a\nc", "a\nb\nc")).toBe(" a|+b| c");
        expect(marks("a\nb\nc", "a\nc")).toBe(" a|-b| c");
        expect(marks("a\nb\n", "a\nb")).toBe("");
        expect(marks("", "a\nb")).toBe("+a|+b");
        const change = changeBetween("a.rs", "a\nb\nc\nd", "a\nx\ny\nd", false);
        expect([change.added, change.removed]).toEqual([2, 2]);
        expect(change.count).toBe("2 lines added, 2 removed");
        expect(change.created).toBe(false);
        expect(changeBetween("a.rs", "", "a", true).count).toBe(
            "New file, 1 line",
        );
        expect(changeBetween("a.rs", "a", "a", false).count).toBe(
            "No line changed",
        );
        expect(changeBetween("a.rs", "a", "a\nb", false).count).toBe(
            "1 line added",
        );
        expect(changeBetween("a.rs", "a\nb\nc", "a", false).count).toBe(
            "2 lines removed",
        );
    });

    it("keeps its counts and its first 200 lines, also past a million pairs of lines", () => {
        const added = Array.from({ length: 50_000 }, (_, n) => `new ${n}`);
        const change = changeBetween(
            "a.rs",
            "1\n2\n3\n4\n5\nold\n7",
            `1\n2\n3\n4\n5\n${added.join("\n")}\n7`,
            false,
        );
        expect([change.added, change.removed]).toEqual([50_000, 1]);
        // Three lines that stay before what differs, and the one after.
        expect(change.total).toBe(50_005);
        expect(change.lines).toHaveLength(200);
        expect(change.lines[0]?.text).toBe("3");
        expect(change.lines[3]?.mark).toBe("-");

        const old = Array.from({ length: 1500 }, (_, n) => `old ${n}`);
        const now = Array.from({ length: 1500 }, (_, n) => `now ${n}`);
        const wide = changeBetween(
            "a.rs",
            old.join("\n"),
            now.join("\n"),
            false,
        );
        expect([wide.added, wide.removed, wide.total]).toEqual([
            1500, 1500, 3000,
        ]);
        expect(wide.lines[0]).toEqual({ mark: "-", text: "old 0" });
        expect(wide.lines[199]).toEqual({ mark: "-", text: "old 199" });
    });

    it("cuts each line and the path at 240 characters", () => {
        const long = "x".repeat(300);
        const change = changeBetween(long, "", long, true);
        expect(change.path).toHaveLength(240);
        expect(change.lines[0]?.text).toHaveLength(240);
    });

    it("comes from the arguments of an edit or a write, and from nothing else", () => {
        expect(
            changeOf("edit", {
                path: "a.txt",
                edits: [
                    { oldText: "one", newText: "One" },
                    { oldText: "three", newText: "Three" },
                ],
            }),
        ).toMatchObject({
            path: "a.txt",
            created: false,
            lines: [
                { mark: "-", text: "one" },
                { mark: "-", text: "three" },
                { mark: "+", text: "One" },
                { mark: "+", text: "Three" },
            ],
        });
        expect(
            changeOf("write", { path: "a.txt", content: "one\n" }),
        ).toMatchObject({
            path: "a.txt",
            created: true,
            count: "New file, 1 line",
        });
        expect(
            changeOf("write", { path: "a.txt", content: "one\ntwo" }, "one"),
        ).toMatchObject({ created: false, count: "1 line added" });
        expect(changeOf("bash", { path: "a", content: "b" })).toBeUndefined();
        expect(changeOf("edit", { path: "a", edits: "b" })).toBeUndefined();
        expect(changeOf("write", { path: 1, content: "b" })).toBeUndefined();
        expect(changeOf("edit", "not an object")).toBeUndefined();
    });
});
