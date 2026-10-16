import { createHash } from "node:crypto";
import { expect, it } from "vitest";
import { accept, broughtBytes, planMerge } from "../src/agents/merge-plan.ts";
import type { AgentRecord, Archived } from "../src/record.ts";

const gone: AgentRecord = {
    number: 2,
    name: "scout",
    purpose: "Looks things up.",
    since: 0,
    model: null,
};
const digest = (text: string) =>
    createHash("sha256").update(text).digest("hex");

it("plans joined notes, identical files and whole-skill clashes without changing either tree", () => {
    const source = new Map([
        ["NOTES.md", Buffer.from("# Notes\nThe wiki is slow.\n")],
        [
            "skills/sort/SKILL.md",
            Buffer.from(
                "---\nname: sort\ndescription: x\n---\nRun skills/sort/run.sh",
            ),
        ],
        ["skills/sort/run.sh", Buffer.from("sort")],
        ["skills/lookup/SKILL.md", Buffer.from("lookup")],
        ["list.txt", Buffer.from("theirs")],
        ["same.txt", Buffer.from("same")],
        ["found.txt", Buffer.from("Dune")],
    ]);
    const target = new Map([
        ["skills/sort/SKILL.md", digest("mine")],
        ["list.txt", digest("mine")],
        ["same.txt", digest("same")],
    ]);
    const before = Buffer.from("# Notes\nAlice likes tea.\n");
    const planned = planMerge({ hamlet: "home", gone, before, source, target });
    if (!planned.ok) throw new Error("expected a merge plan");
    const plan = planned.value;
    expect(plan.notes).toBe("joined");
    expect(plan.after).toContain("## From scout\nThe wiki is slow.");
    expect(plan.files.get("merged/scout/NOTES.before.md")).toEqual(before);
    expect(plan.files.get("merged/scout/NOTES.md")).toEqual(
        source.get("NOTES.md"),
    );
    expect(
        plan.files.get("skills/sort-from-scout/SKILL.md")?.toString(),
    ).toContain("name: sort-from-scout");
    expect(
        plan.files.get("skills/sort-from-scout/SKILL.md")?.toString(),
    ).toContain("skills/sort-from-scout/run.sh");
    expect(plan.files.get("skills/sort-from-scout/run.sh")?.toString()).toBe(
        "sort",
    );
    expect(plan.files.get("skills/lookup/SKILL.md")?.toString()).toContain(
        "lookup",
    );
    expect(plan.files.get("merged/scout/list.txt")?.toString()).toBe("theirs");
    expect(plan.files.get("found.txt")?.toString()).toBe("Dune");
    expect(plan.files.has("same.txt")).toBe(false);
    expect(plan.clashes).toEqual([
        "skill `sort` (the absorbed one is `sort-from-scout`)",
        "file `list.txt` (the absorbed one is `merged/scout/list.txt`)",
    ]);
    expect(source.get("skills/sort/SKILL.md")?.toString()).toContain(
        "name: sort\n",
    );
    expect(target.get("list.txt")).toBe(digest("mine"));
});

it.each([
    ["```markdown\n# Notes\nBoth facts.\n```", "# Notes\nBoth facts."],
    ["<think>hm</think>\nHere:\n# Notes\nBoth facts.", "# Notes\nBoth facts."],
    ["# Notes\n", undefined],
    ["x".repeat(101), undefined],
])("accepts usable rewritten notes only: %s", (reply, expected) => {
    expect(accept(reply, 100)).toBe(expected);
});

it("reconstructs renamed skills when the absorbed name itself contains -from-", () => {
    const absorbed = { ...gone, name: "notes-from-field" };
    const source = new Map([
        [
            "skills/sort/SKILL.md",
            Buffer.from("---\nname: sort\n---\nRun skills/sort/run.sh"),
        ],
        ["skills/sort/image.bin", Buffer.from([0xff, 0xfe])],
    ]);
    const planned = planMerge({
        hamlet: "home",
        gone: absorbed,
        before: Buffer.from("# Notes\nMine"),
        source,
        target: new Map([["skills/sort/SKILL.md", digest("mine")]]),
    });
    if (!planned.ok) throw new Error("expected a merge plan");
    const plan = planned.value;
    const entry: Archived = {
        id: "a1",
        agent: absorbed,
        how: "merged",
        into: 1,
        at: 7,
        by: "chief:chief",
        reason: "Overlap",
        roads: [],
        back: null,
        merge: {
            kept: { ...gone, number: 1, name: "librarian" },
            notesBefore: plan.before,
            notesAfter: plan.after,
            brought: [],
            clashes: plan.clashes,
            inherited: [],
            applied: false,
        },
    };
    const path = "skills/sort-from-notes-from-field/SKILL.md";
    const file = {
        path,
        sha256: digest(
            "---\nname: sort-from-notes-from-field\n---\nRun skills/sort-from-notes-from-field/run.sh",
        ),
    };
    expect(broughtBytes(source, entry, file)?.toString()).toContain(
        "skills/sort-from-notes-from-field/run.sh",
    );
    const image = {
        path: "skills/sort-from-notes-from-field/image.bin",
        sha256: createHash("sha256")
            .update(Buffer.from([0xff, 0xfe]))
            .digest("hex"),
    };
    expect(broughtBytes(source, entry, image)).toEqual(
        Buffer.from([0xff, 0xfe]),
    );
    source.set("skills/sort/SKILL.md", Buffer.from("changed after planning"));
    expect(broughtBytes(source, entry, file)).toBeUndefined();
});

it("skips identical whole skills but refuses a renamed destination already in use", () => {
    const source = new Map([["skills/sort/SKILL.md", Buffer.from("same")]]);
    const input = {
        hamlet: "home",
        gone,
        before: Buffer.from("# Notes\n"),
        source,
        target: new Map([["skills/sort/SKILL.md", digest("same")]]),
    };
    const same = planMerge(input);
    if (!same.ok) throw new Error("expected a merge plan");
    expect([...same.value.files.keys()]).toEqual([
        "merged/scout/NOTES.before.md",
        "merged/scout/NOTES.md",
    ]);
    input.target.set("skills/sort/run.sh", digest("kept sidecar"));
    input.target.set("skills/sort-from-scout/SKILL.md", digest("already here"));
    expect(planMerge(input)).toMatchObject({
        ok: false,
        error: {
            kind: "merge-path-taken",
            path: "skills/sort-from-scout/SKILL.md",
        },
    });
});
