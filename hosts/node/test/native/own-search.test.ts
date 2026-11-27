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

/** Each own call's whole result, as the model reads it, and the root. */
async function called(
    t: TestContext,
    files: Record<string, string>,
    calls: Record<string, string | boolean>[],
    tool: "search" | "find" = "search",
) {
    const filesystem = openAgentFilesystem(scratchFolder(t), 1);
    for (const [path, content] of Object.entries(files)) {
        assert.ok((await filesystem.write(path, content, context)).ok, path);
    }
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files: filesystem,
        responses: [
            ...calls.map((args, at) => toolCall(tool, args, `c${at}`)),
            fauxAssistantMessage("Searched."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: { type: "line", text: "Search." },
    });
    await agent.idle();
    return { root: filesystem.root, results: await toolResults(agent) };
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

test("an own search refuses an empty or a bad pattern in its words", async (t) => {
    assert.deepEqual(
        (
            await called(t, { "memory/notes.md": "notes" }, [
                { pattern: "" },
                { pattern: "", path: "missing" },
                { pattern: "(?=a)" },
                { pattern: "(a)\\1" },
                { pattern: "(a" },
                { pattern: "a{", path: "missing" },
                { pattern: "a{99999}{99999}" },
            ])
        ).results,
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
        (
            await called(
                t,
                {
                    "memory/notes.md": "notes",
                    "skills/a/SKILL.md": "more notes",
                },
                [
                    { pattern: "notes", path: "memory" },
                    { pattern: "notes" },
                    { pattern: "notes", path: "memory/notes.md" },
                ],
            )
        ).results,
        [
            answered("memory/notes.md:1: notes"),
            answered(
                "memory/notes.md:1: notes\nskills/a/SKILL.md:1: more notes",
            ),
            answered("memory/notes.md:1: notes"),
        ],
    );
});

test("an own search trims a line's end, then clips it to 300 characters", async (t) => {
    const lines = [
        `a${"x".repeat(299)}`,
        `b${"x".repeat(300)}`,
        `c${"é".repeat(400)}`,
        `d${"x".repeat(299)} \u00a0\u3000\ufeff\t`,
        "e\u0085",
        `f${"x".repeat(299)}${" ".repeat(724)}g`,
    ];
    assert.deepEqual(
        (
            await called(t, { "memory/long.md": lines.join("\n") }, [
                { pattern: "^[a-f]" },
            ])
        ).results,
        [
            answered(
                [
                    `memory/long.md:1: a${"x".repeat(299)}`,
                    `memory/long.md:2: b${"x".repeat(299)}...`,
                    `memory/long.md:3: c${"é".repeat(299)}...`,
                    `memory/long.md:4: d${"x".repeat(299)}`,
                    "memory/long.md:5: e\u0085",
                    `memory/long.md:6: f${"x".repeat(299)}...`,
                ].join("\n"),
            ),
        ],
    );
});

test("an own search shows whole the path of a name that begins with `..`", async (t) => {
    const { root, results } = await called(
        t,
        { "..dots.md": "notes dots", "memory/notes.md": "notes" },
        [{ pattern: "notes" }],
    );
    assert.deepEqual(results, [
        answered(`${root}/..dots.md:1: notes dots\nmemory/notes.md:1: notes`),
    ]);
});

test("an own find shows each path from where a relative path starts", async (t) => {
    const { root, results } = await called(
        t,
        {
            "..dots.md": "dots",
            "memory/notes.md": "notes",
            "skills/a/SKILL.md": "skill",
        },
        [{ name: "*.md" }, { name: "*.md", path: "memory" }],
        "find",
    );
    assert.deepEqual(results, [
        answered(`${root}/..dots.md\nmemory/notes.md\nskills/a/SKILL.md`),
        answered("memory/notes.md"),
    ]);
});

const SKIPPED = [
    ".git",
    ".hamlet",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    ".next",
];

test("an own search and find pass over the folders a walk skips, but not the folder they start in", async (t) => {
    const files = Object.fromEntries([
        ["build", "needle"],
        ["memory/notes.md", "needle"],
        ...SKIPPED.map((folder) => [`memory/${folder}/notes.md`, "needle"]),
    ]);
    assert.deepEqual(
        (
            await called(t, files, [
                { pattern: "needle" },
                { pattern: "needle", path: "memory/node_modules" },
            ])
        ).results,
        [
            answered("build:1: needle\nmemory/notes.md:1: needle"),
            answered("memory/node_modules/notes.md:1: needle"),
        ],
    );
    assert.deepEqual(
        (
            await called(
                t,
                files,
                [
                    { name: "notes.md" },
                    { name: "notes.md", path: "memory/.git" },
                ],
                "find",
            )
        ).results,
        [answered("memory/notes.md"), answered("memory/.git/notes.md")],
    );
});

test("a case-insensitive own search lowercases the pattern and the line", async (t) => {
    assert.deepEqual(
        (
            await called(
                t,
                { "memory/notes.md": "notes Ab" },
                ["[a-Z]", "[Z-a]", "\\x41B", "[[:upper:]]b"].map((pattern) => ({
                    pattern,
                    ignore_case: true,
                })),
            )
        ).results,
        [
            answered("memory/notes.md:1: notes Ab"),
            refused(
                "`pattern` is not a regular expression: invalid character class range, the start must be <= the end. Correct the pattern.",
            ),
            answered("no match"),
            answered("no match"),
        ],
    );
});
