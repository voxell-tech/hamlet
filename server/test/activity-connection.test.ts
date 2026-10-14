import { expect, it, vi } from "vitest";
import { scriptedModels } from "../src/faux.ts";
import { agents, say, served } from "./connection-support.ts";

it("tells reading and writing apart and keeps only the last 600 UTF-8 bytes of speech", async () => {
    const speech = `${"\u00e9".repeat(400)}done`;
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({
            responses: [
                { calls: [{ name: "read", arguments: { path: "NOTES.md" } }] },
                {
                    calls: [
                        { name: "search", arguments: { pattern: "Notes" } },
                    ],
                },
                { calls: [{ name: "find", arguments: { path: "." } }] },
                {
                    calls: [
                        {
                            name: "write",
                            arguments: {
                                path: "NOTES.md",
                                content: "remember",
                            },
                        },
                    ],
                },
                {
                    calls: [
                        {
                            name: "read",
                            arguments: { path: "skills/a/SKILL.md" },
                        },
                    ],
                },
                speech,
            ],
        }),
    });
    say(run.connection, "Look and remember.");
    await vi.waitFor(
        () =>
            expect(agents(run.messages).at(-1)).toMatchObject({
                busy: false,
                saying: { text: `${"\u00e9".repeat(298)}done`, thought: false },
            }),
        { timeout: 10_000 },
    );
    const steps = agents(run.messages).map((agent) => agent.activity);
    expect(steps).toContainEqual({
        doing: "read-notes",
        doingWords: "Reading its notes",
        step: 2,
        failures: 0,
    });
    expect(steps).toContainEqual({
        doing: "read-file",
        doingWords: "Reading a file",
        step: 4,
        failures: 0,
    });
    expect(steps).toContainEqual({
        doing: "read-file",
        doingWords: "Reading a file",
        step: 6,
        failures: 0,
    });
    expect(steps).toContainEqual({
        doing: "write-notes",
        doingWords: "Writing its notes",
        step: 8,
        failures: 0,
    });
    expect(steps).toContainEqual({
        doing: "read-skill",
        doingWords: "Reading a skill",
        step: 10,
        failures: 0,
    });
});
