import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { openAgentFilesystem } from "../../src/agent/native-filesystem.ts";
import {
    fakeCalls,
    from,
    toolCall,
    toolResults,
    wakeAgent,
} from "../agent-testing.ts";
import { context, scratchFolder } from "../testing.ts";

/** Each own search's whole result, as the model reads it. */
async function searches(
    t: TestContext,
    files: Record<string, string>,
    calls: Record<string, string>[],
) {
    const filesystem = openAgentFilesystem(scratchFolder(t), 1);
    for (const [path, content] of Object.entries(files)) {
        assert.ok((await filesystem.write(path, content, context)).ok, path);
    }
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files: filesystem,
        responses: [
            ...calls.map((args, at) => toolCall("search", args, `c${at}`)),
            fauxAssistantMessage("Searched."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: { type: "line", text: "Search." },
    });
    await agent.idle();
    return toolResults(agent);
}

const answered = (output: string) => ({
    output,
    isError: false,
    diagnostics: [],
});
const refused = (output: string) => ({
    output,
    isError: true,
    diagnostics: [],
});

test("an own search refuses an empty or a bad pattern in today's words", async (t) => {
    assert.deepEqual(
        await searches(t, { "memory/notes.md": "notes" }, [
            { pattern: "" },
            { pattern: "", path: "missing" },
            { pattern: "(?=a)" },
            { pattern: "(a)\\1" },
            { pattern: "(a" },
            { pattern: "a{", path: "missing" },
            { pattern: "a{99999}{99999}" },
        ]),
        [
            refused("`pattern` is required. Supply a regular expression."),
            refused("`pattern` is required. Supply a regular expression."),
            refused(
                "`pattern` holds a backreference, a lookaround or another part a search in linear time cannot take. Use a linear-time pattern.",
            ),
            refused(
                "`pattern` holds a backreference, a lookaround or another part a search in linear time cannot take. Use a linear-time pattern.",
            ),
            refused(
                "`pattern` is not a regular expression: unclosed group. Correct the pattern.",
            ),
            refused(
                "`pattern` is not a regular expression: unclosed counted repetition. Correct the pattern.",
            ),
            refused(
                "`pattern` is not a regular expression: Compiled regex exceeds size limit of 10485760 bytes.. Correct the pattern.",
            ),
        ],
    );
});

test("an own search shows each path from where a relative path starts", async (t) => {
    assert.deepEqual(
        await searches(
            t,
            { "memory/notes.md": "notes", "skills/a/SKILL.md": "more notes" },
            [
                { pattern: "notes", path: "memory" },
                { pattern: "notes" },
                { pattern: "notes", path: "memory/notes.md" },
            ],
        ),
        [
            answered("memory/notes.md:1: notes"),
            answered(
                "memory/notes.md:1: notes\nskills/a/SKILL.md:1: more notes",
            ),
            answered("memory/notes.md:1: notes"),
        ],
    );
});

test("an own search trims pi's spaces from a line's end, then clips it to 300 characters", async (t) => {
    const lines = [
        `a${"x".repeat(299)}`,
        `b${"x".repeat(300)}`,
        `c${"é".repeat(400)}`,
        `d${"x".repeat(299)} \u00a0\u3000\ufeff\t`,
        "e\u0085",
    ];
    assert.deepEqual(
        await searches(t, { "memory/long.md": lines.join("\n") }, [
            { pattern: "^[a-e]" },
        ]),
        [
            answered(
                [
                    `memory/long.md:1: a${"x".repeat(299)}`,
                    `memory/long.md:2: b${"x".repeat(299)}...`,
                    `memory/long.md:3: c${"é".repeat(299)}...`,
                    `memory/long.md:4: d${"x".repeat(299)}`,
                    "memory/long.md:5: e\u0085",
                ].join("\n"),
            ),
        ],
    );
});
