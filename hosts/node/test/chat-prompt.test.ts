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
import type { AgentInput, Copies } from "../src/agent/messages/agent.ts";
import type { Character } from "../src/agent/messages/profile.ts";
import type { ReadAnswer } from "../src/agent/messages/workspace.ts";
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
import { context, fixture } from "./testing.ts";

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
        tools_off: [{ name: "end" }, { name: "bash" }],
        folder: "/work",
        manual: false,
        agrees_roads: false,
    },
    copies: {},
};

async function readPrompt(agent: Woken): Promise<string> {
    return (await agent.actions.read_prompt(agent.by(from.client))).text;
}

async function sendCopies(agent: Woken, copies: Copies): Promise<void> {
    await agent.actions.copies(agent.by(from.realm), copies);
}

const TIP = "ab".repeat(20);

/**
 * A profile with no files, which gives the agent the character last given
 * to `keep`: by the fill the first time, by a move after.
 */
function profileWithCharacter() {
    let character: Character | undefined;
    return {
        files: () => ({
            type: "part" as const,
            tip: TIP,
            files: [],
            removed: [],
            ...(character === undefined ? {} : { character }),
        }),
        save: () => ({ type: "saved" as const, tip: TIP }),
        keep: async (agent: Woken, version: number, text: string) => {
            const first = character === undefined;
            character = { version, text };
            if (first) {
                await agent.actions.fill(agent.by(from.realm), { profile: 2 });
            } else {
                await agent.actions.moved(agent.by(from.profile(2)), {
                    tip: TIP,
                    character,
                });
            }
        },
    };
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
    const profile = profileWithCharacter();
    const { calls } = fakeCalls({
        workspace: workspaceFiles({ "/work/AGENTS.md": project }),
        profile,
    });
    const agent = await wakeAgent(t, {
        calls,
        input: HIRE,
        files: listedFiles({ "memory/long.md": memory }),
    });
    await profile.keep(agent, 1, character);

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
        tools: async () => ({
            tools: [],
            faults: [],
            confinement: { type: "sandbox", name: "bwrap" },
        }),
        readPlace: async () => ({ place: "# Your place" }),
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

    await profile.keep(agent, 2, "c".repeat(4000));
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
    const profile = profileWithCharacter();
    const { calls } = fakeCalls({
        workspace: workspaceFiles({ "/work/AGENTS.md": "Use tabs." }),
        profile,
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
    await sendCopies(agent, { charter: { version: 1, value: "Be brief." } });
    await profile.keep(agent, 1, "Calm.");

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
    assert.ok(text.endsWith("\n\n# Your place\nYou are `hand`."));
    await agent.stop();
});

test("an errand's prompt is its own instructions only", async () => {
    const refused = () => Promise.reject(new Error("an errand reads nothing"));
    const text = await renderPrompt({
        copies: refused,
        files: listedFiles({ "memory/chief.md": "The chief likes tea." }),
        where: refused,
        readFile: refused,
        tools: refused,
        readPlace: refused,
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

/** The place, realm and scholars sections as `realm/read_place.answer` holds them. */
const PLACE_TEXT =
    "# Your place\nYou are `hire#12 (Ada)`.\n\n# The realm\nNo hamlet is founded yet.\n\n# The city's scholars\n- `scholar#2`";

test("the prompt ends with the realm's place, realm and scholars, read once for each request", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            read_place: () => fixture("realm/read_place.answer") as never,
        },
    });
    const record = recorder();
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            record.step(toolCall("tell", {})),
            record.step(fauxAssistantMessage("Done.")),
        ],
    });
    await say(agent, "Hi");
    await agent.stop();

    assert.equal(record.prompts.length, 2);
    for (const prompt of record.prompts) {
        assert.ok(prompt.endsWith(`\n\n${PLACE_TEXT}`), prompt);
    }
    assert.deepEqual(
        named(sent, "read_place").map((call) => call.message),
        [{ conversation: "chat" }, { conversation: "chat" }],
    );
});

/** As `crates/hamlet_actors/tests/realm/prompt.rs` holds today's words. */
const HAND_WORDS =
    "You are the Hand of this realm: the chiefs' own agent. You found hamlets (`found_hamlet`) and seat their mayors (`seat_mayor`), make scholars (`create_scholar`) and hire them into any hamlet, and lead every hire. You read any hamlet's record (`read_record`) and any scholar (`read_scholar`). What you do to what a chief owns, removing a hamlet or changing, tagging or reverting a scholar, waits for a chief first. Each mayor answers to you.";
const LEAD_WORDS =
    "You lead the hires under you. `hire` brings a scholar of the city in under you or under a hire of your tree, and `create_scholar` makes a new scholar to hire. Hire when a kind of work will come back and deserves its own memory; do small one-off things yourself, or with `start_worker`. Check the hires below for one that fits before hiring. `lay_road` lets two hires under you write to each other directly; `end_hire` ends one nobody needs and `merge_hires` makes one hire of two whose work overlaps: the kept hire takes what the other learned, its roads and its hires. `set_model`, `set_tools` and `set_allowance` shape a hire under you, and `nickname` names it in the world.";
const OBJECT_WORDS =
    "A chief is a human the realm belongs to. If you think a chief's instruction is a mistake, use `object` once and wait for the chief's ruling before doing anything. If the chief says go on, do it and do not object again. You may not refuse.";

/** Tools of oversight and the city the prompt names. */
const NAMED = [
    "object",
    "start_worker",
    "merge_hires",
    "set_model",
    "set_allowance",
    "nickname",
    "read_record",
];

const NAMED_NOT_OFFERED = ["merge_hires"];

test("every tool the prompt names is one the agent offers, but merge_hires", async (t) => {
    const missing = new Set<string>();
    for (const [input, words] of [
        [undefined, [HAND_WORDS, LEAD_WORDS, OBJECT_WORDS]],
        [HIRE, [LEAD_WORDS, OBJECT_WORDS]],
    ] as const) {
        const agent = await wakeAgent(t, {
            calls: fakeCalls({
                realm: {
                    read_place: () => ({
                        place: `# Your place\n${words.join("\n\n")}`,
                    }),
                },
            }).calls,
            ...(input === undefined ? {} : { input }),
        });
        const prompt = await readPrompt(agent);
        const root = await (await agent.harness()).root(context);
        const offered = (await root.agent(context)).tools.map(
            (tool) => tool.name,
        );
        await agent.stop();
        for (const name of NAMED) {
            if (prompt.includes(`\`${name}\``) && !offered.includes(name)) {
                missing.add(name);
            }
        }
    }
    assert.deepEqual([...missing], NAMED_NOT_OFFERED);
});

test("a request whose place the realm does not give is not sent, and its turn fails as an unasked spend does", async (t) => {
    let reads = 0;
    const { calls, sent } = fakeCalls({
        realm: {
            read_place: () => {
                reads += 1;
                if (reads === 2) throw new Error("the realm is gone");
                return fixture("realm/read_place.answer") as never;
            },
        },
    });
    const record = recorder();
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            record.step(toolCall("tell", {})),
            record.step(fauxAssistantMessage("Not sent.")),
        ],
    });
    await say(agent, "Hi");
    await until(() => named(sent, "turn_failed").length === 1);
    await agent.stop();

    assert.equal(record.prompts.length, 1);
    const [failed] = named(sent, "turn_failed").map(
        (call) => call.message as { words: string },
    );
    assert.match(
        failed?.words ?? "",
        /: it could not be asked \(the realm is gone\)$/,
    );
});

test("the first request of a turn is not sent when the realm does not give its place", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            read_place: () => {
                throw new Error("the realm is gone");
            },
        },
    });
    const record = recorder();
    const agent = await wakeAgent(t, {
        calls,
        responses: [record.step(fauxAssistantMessage("Not sent."))],
    });
    await say(agent, "Hi");
    await until(() => named(sent, "turn_failed").length === 1);
    await agent.stop();

    assert.deepEqual(record.prompts, []);
});

test("the environment names the sandbox and the tools that failed to load, as today", async (t) => {
    const faults = [
        { folder: "/realm/tools/lint", words: "it has no manifest.json" },
        { folder: "/realm/tools/fmt", words: "`fmt` is taken" },
    ];
    for (const [confinement, sandbox] of [
        [{ type: "sandbox", name: "bwrap" }, "bwrap"],
        [
            { type: "insecure" },
            "none. Commands are not confined: they reach whatever the chief's account can",
        ],
        [{ type: "remote" }, "the remote workspace's"],
    ] as const) {
        const { calls } = fakeCalls({
            workspace: {
                list_tools: () => ({ tools: [], faults, confinement }),
            },
        });
        const agent = await wakeAgent(t, { calls });
        const prompt = await readPrompt(agent);
        await agent.stop();

        const environment = prompt.slice(prompt.indexOf("# Environment"));
        assert.ok(
            environment.startsWith(
                `# Environment\nSandbox: ${sandbox}. Network for tools: off unless a chief allows it; a command that needs it waits until the chief answers, so set \`network\` to true on it and carry on.\nTools that failed to load:\n/realm/tools/lint: it has no manifest.json\n/realm/tools/fmt: \`fmt\` is taken\nSystem: `,
            ),
            environment,
        );
    }
});

test("the project says it is a git repository, and that a chief approves each change in a manual hamlet, as today", async (t) => {
    const git =
        "- The project is a git repository. When two agents must work apart, one may use a git worktree inside the project folder, such as under `.hamlet/worktrees/<name>`.";
    const manual =
        "- A chief approves each edit and each command in the project before it runs, so each one waits. `read`, `search` and `find` never wait: learn what you need with them, then make few, well-aimed edits and commands.";
    const projectOf = async (
        files: Record<string, string>,
        isManual: boolean,
    ) => {
        const { calls } = fakeCalls({ workspace: workspaceFiles(files) });
        const agent = await wakeAgent(t, {
            calls,
            input: { ...HIRE, place: { ...HIRE.place, manual: isManual } },
        });
        const prompt = await readPrompt(agent);
        await agent.stop();
        return prompt.split("\n");
    };

    const repository = await projectOf(
        { "/work/.git/HEAD": "ref: refs/heads/main\n" },
        true,
    );
    assert.ok(repository.includes(git) && repository.includes(manual));
    const worktree = await projectOf(
        { "/work/.git": "gitdir: /src/.git/worktrees/work\n" },
        false,
    );
    assert.ok(worktree.includes(git) && !worktree.includes(manual));
    const plain = await projectOf({}, false);
    assert.ok(!plain.includes(git) && !plain.includes(manual));
});
