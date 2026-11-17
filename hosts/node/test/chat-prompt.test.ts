import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type AssistantMessage,
    type FauxResponseFactory,
    fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import { getCurrentSystemPrompt } from "@earendil-works/pi-ai/utils/transcript";
import { FileError, ok } from "@earendil-works/pi-durable/env";
import type { AgentFilesystem, FileEntry } from "../src/agent/filesystem.ts";
import type { AgentInput, Copies, ReadAnswer } from "../src/agent/messages.ts";
import { renderPrompt } from "../src/agent/prompt.ts";
import {
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    named,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

function listedFiles(files: Record<string, string>): AgentFilesystem {
    const base = fakeFiles(files);
    const inside = (path: string) =>
        path.startsWith(`${FAKE_ROOT}/`)
            ? path.slice(FAKE_ROOT.length + 1)
            : path;
    return {
        ...base,
        read: (path, context) => base.read(inside(path), context),
        write: (path, content, context) =>
            base.write(inside(path), content, context),
        list: async (path) => {
            const prefix = `${inside(path)}/`;
            const names = new Map<string, FileEntry["kind"]>();
            for (const key of Object.keys(files)) {
                if (!key.startsWith(prefix)) continue;
                const [name, ...rest] = key.slice(prefix.length).split("/");
                names.set(name ?? "", rest.length === 0 ? "file" : "directory");
            }
            return ok(
                [...names]
                    .sort(([a], [b]) => (a < b ? -1 : 1))
                    .map(([name, kind]) => ({
                        name,
                        path: `${FAKE_ROOT}/${prefix}${name}`,
                        kind,
                        size: 0,
                    })),
            );
        },
    };
}

function recorder(): {
    readonly prompts: string[];
    step(answer: AssistantMessage): FauxResponseFactory;
} {
    const prompts: string[] = [];
    return {
        prompts,
        step: (answer) => (context) => {
            prompts.push(getCurrentSystemPrompt(context.messages));
            return answer;
        },
    };
}

function workspaceFiles(files: Record<string, string>) {
    return {
        read_file: ({ path }: { path: string }): ReadAnswer => {
            const text = files[path];
            return text === undefined
                ? { type: "failed", words: `${path} is not there` }
                : { type: "text", text, notes: [] };
        },
    };
}

const HIRE: AgentInput = {
    kind: "hire",
    task: "standing-7",
    place: {
        version: 1,
        lead: 1,
        hires: [3],
        roads: [5],
        tools_off: [{ name: "end_hire" }],
        folder: "/work",
    },
};

async function readPrompt(agent: Woken): Promise<string> {
    return (await agent.actions.read_prompt(agent.by(from.client))).text;
}

async function sendCopies(agent: Woken, copies: Copies): Promise<void> {
    await agent.actions.copies(agent.by(from.realm), copies);
}

async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `line-${text}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

test("a file changed in the filesystem shows in the next request's prompt", async (t) => {
    const { calls } = fakeCalls({});
    const record = recorder();
    const agent = await wakeAgent(t, {
        calls,
        files: listedFiles({ "memory/chief.md": "The chief likes tea." }),
        responses: [
            record.step(
                toolCall("write", {
                    path: `${FAKE_ROOT}/memory/chief.md`,
                    content: "The chief likes coffee.",
                }),
            ),
            record.step(fauxAssistantMessage("Noted.")),
        ],
    });

    await say(agent, "I prefer coffee now.");

    const [before = "", after = ""] = record.prompts;
    assert.equal(record.prompts.length, 2);
    assert.match(before, /## memory\/chief\.md\nThe chief likes tea\./);
    assert.match(after, /## memory\/chief\.md\nThe chief likes coffee\./);
    assert.doesNotMatch(after, /tea/);
    await agent.stop();
});

test("the prompt cuts each part at its limit", async (t) => {
    // An astral character at each cut, so a cut counts characters, not UTF-16 units.
    const character = `${"c".repeat(3999)}😀tail`;
    const memory = "m".repeat(9000);
    const project = `${"p".repeat(7999)}😀tail`;
    const { calls } = fakeCalls({
        workspace: workspaceFiles({ "/work/AGENTS.md": project }),
    });
    const agent = await wakeAgent(t, {
        calls,
        input: HIRE,
        files: listedFiles({ "memory/long.md": memory }),
    });
    await sendCopies(agent, { character: { version: 1, value: character } });

    const text = await readPrompt(agent);

    assert.ok(
        text.includes(
            `# Your character\n${"c".repeat(3999)}😀\n[The character is cut here.]`,
        ),
    );
    const shown = `\n## memory/long.md\n${memory}\n`;
    assert.ok(
        text.includes(
            `${[...shown].slice(0, 8000).join("")}\n[The memory is longer than is shown and is cut here. Shorten it so all of it is shown.]`,
        ),
    );
    assert.ok(
        text.includes(
            `# The project's AGENTS.md\n${"p".repeat(7999)}😀\n[AGENTS.md is cut here.]`,
        ),
    );

    const cutByTheWorkspace = await renderPrompt({
        copies: async () => ({}),
        files: listedFiles({}),
        where: async () => ({ kind: "hire", place: HIRE.place }),
        report: assert.fail,
        readFile: async () => ({
            type: "text",
            text: "Use tabs.",
            notes: [],
            truncation: {
                by: "lines",
                total_lines: 3000,
                total_bytes: 9000,
                output_lines: 2000,
                output_bytes: 6000,
                first_line_exceeds_limit: false,
            },
        }),
    });
    assert.ok(
        cutByTheWorkspace.includes(
            "# The project's AGENTS.md\nUse tabs.\n[AGENTS.md is cut here.]",
        ),
    );

    await sendCopies(agent, {
        character: { version: 2, value: "c".repeat(4000) },
    });
    assert.ok(
        (await readPrompt(agent)).includes(
            `# Your character\n${"c".repeat(4000)}\n\n# Your memory`,
        ),
    );
    await agent.stop();
});

test("a copy older than the one held changes nothing", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls });

    await sendCopies(agent, { charter: { version: 2, value: "Be brief." } });
    await sendCopies(agent, { charter: { version: 1, value: "Be long." } });
    await sendCopies(agent, { charter: { version: 2, value: "Be loud." } });

    assert.match(await readPrompt(agent), /^Be brief\.\n\n# Environment/);
    await agent.stop();
});

test("refreshed keeps what read_copies answers", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            read_copies: (have) =>
                have.charter === undefined
                    ? { charter: { version: 3, value: "From the realm." } }
                    : {},
        },
    });
    const agent = await wakeAgent(t, { calls });

    await until(async () =>
        (await readPrompt(agent)).startsWith("From the realm."),
    );
    assert.deepEqual(named(sent, "read_copies")[0]?.message, {});

    await agent.stop();
    const again = await agent.again();
    await until(() => named(sent, "read_copies").length === 2);
    assert.deepEqual(named(sent, "read_copies")[1]?.message, { charter: 3 });
    assert.ok((await readPrompt(again)).startsWith("From the realm."));
    await again.stop();
});

test("read_prompt answers what the next request sends", async (t) => {
    const { calls } = fakeCalls({
        workspace: workspaceFiles({ "/work/AGENTS.md": "Use tabs." }),
    });
    const record = recorder();
    const agent = await wakeAgent(t, {
        calls,
        input: HIRE,
        files: listedFiles({
            "memory/chief.md": "The chief likes tea.",
            "memory/places/MEMORY.md":
                "---\ndescription: Where things are.\n---\n",
            "skills/deploy/SKILL.md":
                "---\nname: deploy\ndescription: Ships it.\nusage: ./run.sh\n---\n",
        }),
        responses: [record.step(fauxAssistantMessage("Hello."))],
    });
    await sendCopies(agent, {
        charter: { version: 1, value: "Be brief." },
        character: { version: 1, value: "Calm." },
    });

    const text = await readPrompt(agent);
    await say(agent, "Hi");

    assert.equal(record.prompts[0], text);
    assert.match(text, /^Be brief\.\n\n# Environment\n/);
    assert.ok(text.includes("# Your character\nCalm."));
    assert.ok(text.includes("# The project's AGENTS.md\nUse tabs."));
    assert.ok(text.includes("- memory/places/: Where things are."));
    assert.ok(
        text.includes(
            `- deploy: Ships it. Run: \`cd '${FAKE_ROOT}' && ./run.sh\` (skills/deploy/SKILL.md)`,
        ),
    );
    assert.ok(text.includes("# Your place\n"));
    await agent.stop();
});

test("an errand's prompt is its own instructions only", async () => {
    const refused = () => Promise.reject(new Error("an errand reads nothing"));
    const text = await renderPrompt({
        copies: refused,
        files: listedFiles({ "memory/chief.md": "The chief likes tea." }),
        where: refused,
        readFile: refused,
        report: assert.fail,
        errand: "Sort the library.",
    });
    assert.equal(text, "Sort the library.");
});

test("a workspace that fails leaves the project's file out, keeps its folder lines, and is reported", async (t) => {
    const { calls } = fakeCalls({
        workspace: {
            read_file: () => {
                throw new Error("the workspace is away");
            },
        },
    });
    const record = recorder();
    const agent = await wakeAgent(t, {
        calls,
        input: HIRE,
        responses: [record.step(fauxAssistantMessage("One."))],
    });
    await say(agent, "first");

    assert.ok(
        record.prompts[0]?.includes(
            "# The project\nYour hamlet works on the folder /work.",
        ),
    );
    assert.ok(
        (await readPrompt(agent)).includes("Your hamlet works on the folder"),
    );
    assert.ok(
        agent.reported.some((line) => line.includes("the workspace is away")),
    );
    await agent.stop();
});

test("a memory folder that cannot be listed leaves the memory out, as today, and is reported", async (t) => {
    const files = fakeFiles({});
    const failing: AgentFilesystem = {
        ...files,
        list: async (path, context) =>
            path === "memory"
                ? {
                      ok: false,
                      error: new FileError(
                          "permission_denied",
                          "EACCES",
                          "memory",
                      ),
                  }
                : files.list(path, context),
    };
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files: failing,
    });

    const prompt = await readPrompt(agent);
    assert.ok(!prompt.includes("# Your memory"));
    assert.ok(prompt.includes("# Your skills"));
    assert.ok(agent.reported.some((line) => line.includes("EACCES")));
    await agent.stop();
});
