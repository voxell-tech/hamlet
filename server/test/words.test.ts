import { describe, expect, it } from "vitest";
import type { Danger, Wait } from "../src/oversight.ts";
import { clip, line, queue, says, strip, waited } from "../src/oversight.ts";

const wait = (
    danger: Danger | null = "network",
    about = "curl https://example.org",
): Wait => ({
    id: "1000-1",
    kind: "approval",
    danger,
    by: "mayor",
    agent: { hamlet: "home", number: 0 },
    about,
    change: null,
    answers: ["allow", "always", "deny"],
    since: 1000,
    asked: 1,
    chat: null,
    calls: [],
});

describe("the chief's words for a wait", () => {
    it("flattens whitespace, counts characters, and adds dots only when cut (words.rs:5-11)", () => {
        expect(clip("  Tea\n  with\t milk.  ", 14)).toBe("Tea with milk.");
        expect(clip("  Tea\n  with\t milk.  ", 8)).toBe("Tea with...");
        expect(clip("🍵🍵🍵", 2)).toBe("🍵🍵...");
    });
    it.each([
        ["network", "wants the network for"],
        ["unconfined", "wants to run, with no sandbox,"],
        ["destructive", "wants to run, with no sandbox, the destructive"],
        ["reserved", "wants to do what you reserved:"],
        ["edit", "wants to edit"],
        ["command", "wants to run"],
    ] as const)("names %s after the agent", (danger, words) => {
        expect(says(danger)).toBe(words);
    });
    it("quotes only an edit's path, and quotes a command on one line (queue.rs:60-89)", () => {
        expect(line(wait("edit", "a.txt\nA new file:\none"), 60)).toBe(
            "mayor wants to edit `a.txt`",
        );
        expect(line(wait(), 10)).toBe(
            "mayor wants the network for `curl https...`",
        );
        const question = {
            ...wait(null, "Is this right?"),
            kind: "question",
        } as const;
        expect(line(question, 60)).toBe("mayor asks: Is this right?");
        expect(line({ ...question, kind: "fault" }, 60)).toBe(
            "mayor failed: Is this right?",
        );
        expect(
            line(
                {
                    ...question,
                    kind: "objection",
                    about: "alice told mayor: do it\nmayor objects: no",
                },
                7,
            ),
        ).toBe("mayor object...");
    });
    it("prints a strip with each line indented and only its offered answers (typed.rs:22-36)", () => {
        expect(strip(wait("edit", "a.txt\nA new file:\none"))).toBe(
            "[approval] mayor wants to edit `a.txt`\n    a.txt\n    A new file:\n    one\n    answer with: /allow /always /deny",
        );
        expect(
            strip({
                ...wait(null, "boom"),
                kind: "fault",
                answers: ["dismiss"],
            }),
        ).toBe(
            "[fault] mayor failed: boom\n    boom\n    answer with: /dismiss",
        );
    });
    it.each([
        [0, "under a minute"],
        [59_999, "under a minute"],
        [60_000, "1 min"],
        [3_599_000, "59 min"],
        [3_600_000, "1 h 00 min"],
        [7_620_000, "2 h 07 min"],
    ])("names %s ms waited as %s", (ms, words) =>
        expect(waited(Number(ms))).toBe(words),
    );
    it("reads oldest first even at the same millisecond or after the clock went back", () => {
        const first = wait();
        const second = { ...first, id: "900-2", since: 900 };
        const tenth = { ...first, id: "900-10", since: 900 };
        expect(
            queue({
                waits: {
                    [tenth.id]: tenth,
                    [second.id]: second,
                    [first.id]: first,
                },
            }).map((wait) => wait.id),
        ).toEqual(["1000-1", "900-2", "900-10"]);
    });
});
