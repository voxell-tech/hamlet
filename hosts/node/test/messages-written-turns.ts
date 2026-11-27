import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { Submit } from "../src/agent/messages/agent.ts";
import {
    fakeFiles,
    from,
    named,
    priced,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import {
    answer,
    answering,
    HAND,
    HIRE,
    LAST_WORDS,
    submitted,
    type Written,
} from "./messages-testing.ts";
import { fixture } from "./testing.ts";

export const TURN_WRITES: readonly Written[] = [
    {
        fixture: "workspace/run",
        task: "task-1",
        tool: ["bash", { command: "echo hi", timeout: 30, network: false }],
        answers: { workspace: { run: () => answer("workspace/run.running") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/run.unset",
        tool: ["bash", { command: "echo hi", network: true }],
        answers: { workspace: { run: () => answer("workspace/run.running") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/run.tool",
        task: "task-1",
        tool: ["word_count", { path: "/project/a.md" }],
        answers: {
            workspace: {
                list_tools: () => answer("workspace/list_tools"),
                run: () => answer("workspace/run.running"),
            },
        },
        offered: "word_count",
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/write_file",
        tool: ["write", { path: "/work/project/notes.md", content: "hello\n" }],
        answers: {
            workspace: {
                write_file: () => answer("workspace/write_file.ended.changed"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/edit_file",
        task: "task-1",
        tool: [
            "edit",
            {
                path: "/work/project/notes.md",
                edits: [{ oldText: "hello", newText: "goodbye" }],
            },
        ],
        answers: {
            workspace: {
                edit_file: () => answer("workspace/edit_file.ended.changed"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/read_file",
        tool: [
            "read",
            { path: "/work/project/notes.md", offset: 10, limit: 20 },
        ],
        answers: {
            workspace: { read_file: () => answer("workspace/read_file.text") },
        },
    },
    {
        fixture: "workspace/read_file.whole",
        tool: ["read", { path: "/work/project/notes.md" }],
        answers: {
            workspace: { read_file: () => answer("workspace/read_file.text") },
        },
    },
    {
        fixture: "workspace/search",
        tool: [
            "search",
            {
                pattern: "fn main",
                path: "/work/project",
                name: "*.rs",
                ignore_case: true,
            },
        ],
        answers: {
            workspace: { search: () => answer("workspace/search.text") },
        },
    },
    {
        fixture: "workspace/find",
        tool: ["find", { path: "/work/project" }],
        answers: {
            workspace: { find: () => answer("workspace/search.text") },
        },
    },
    // Sent at each wake.
    { fixture: "workspace/list_tools" },
    // Sent before each model request; a turn whose input names no task asks for the standing one.
    { fixture: "realm/may_spend" },
    {
        fixture: "realm/charge.tokens",
        models: (models) =>
            priced(models, 0.0025, { input: 1200, output: 280 }),
        own: ["call", "took_ms"],
    },
    {
        fixture: "realm/reply",
        task: "task-1",
        input: {
            type: "question",
            letter: 5,
            from: { agent: 12, name: "Ada" },
            text: "How many fail?",
        },
        answers: { realm: { reply: () => answer("realm/reply.done") } },
    },
    {
        fixture: "realm/tell",
        task: "task-1",
        tool: ["tell", { to: "mayor", text: "The tests pass" }],
        answers: { realm: { tell: () => answer("realm/tell.sent") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/tell.no_task",
        tool: ["tell", { to: "hand", text: "The tests pass" }],
        answers: { realm: { tell: () => answer("realm/tell.sent") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/tell.paths",
        task: "task-1",
        tool: [
            "tell",
            {
                to: "mayor",
                text: "The report",
                paths: ["/work/report.md", "/work/figures"],
            },
        ],
        answers: { realm: { tell: () => answer("realm/tell.sent") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/ask.paths",
        task: "task-1",
        tool: [
            "ask",
            {
                to: "hire#12",
                question: "Is this plan right?",
                paths: ["/work/plan.md"],
            },
        ],
        answers: { realm: { ask: () => answer("realm/ask.waiting") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/ask",
        task: "task-1",
        tool: ["ask", { to: "hire#12", question: "Which tests fail?" }],
        answers: { realm: { ask: () => answer("realm/ask.waiting") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/request_road",
        agent: HIRE,
        task: "task-1",
        tool: [
            "request_road",
            { to: "hire#13", reason: "To ask about the tests" },
        ],
        answers: {
            realm: { request_road: () => answer("realm/request_road.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/request_road.no_why",
        agent: HIRE,
        tool: ["request_road", { to: "mayor" }],
        answers: {
            realm: { request_road: () => answer("realm/request_road.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/found",
        tool: ["found_hamlet", { name: "Harbor", purpose: "Run the release" }],
        answers: { realm: { found: () => answer("realm/found.founded") } },
        own: ["call", "conversation"],
        // The tool has no tier to choose: a hamlet an agent founds has the
        // default, 50. The fixture's 500 is a chief's choice.
        constant: { "act.tier": "fifty" },
    },
    {
        fixture: "realm/hire",
        tool: ["hire", { purpose: "Check the links", under: "hire#12" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/hire.in_a_turn",
        task: "standing-12",
        tool: ["hire", { purpose: "Check the links" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/hire.at_a_version",
        tool: ["hire", { profile: "profile#4@v1", purpose: "Check the links" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/create_profile",
        tool: [
            "create_profile",
            { name: "Ada", character: "You write careful tests." },
        ],
        answers: {
            realm: {
                create_profile: () => answer("realm/create_profile.made"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/create_profile.realms_character",
        tool: ["create_profile", { name: "Ada" }],
        answers: {
            realm: {
                create_profile: () => answer("realm/create_profile.made"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/read_profile",
        tool: ["read_profile", { profile: "profile#4" }],
        answers: {
            realm: { read_profile: () => answer("realm/read_profile.listed") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/read_profile.file",
        tool: [
            "read_profile",
            { profile: "profile#4", path: "memory/tests.md", version: "v1" },
        ],
        answers: {
            realm: { read_profile: () => answer("realm/read_profile.text") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/edit_profile",
        tool: [
            "edit_profile",
            {
                profile: "profile#4",
                path: "CHARACTER.md",
                content: "You write careful tests.",
                message: "Sharper",
            },
        ],
        answers: {
            realm: {
                edit_profile: () =>
                    answer("realm/edit_profile.refused.reserved"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/tag",
        tool: [
            "tag",
            { profile: "profile#4", tag: "v1", note: "Before the refactor" },
        ],
        answers: { realm: { tag: () => answer("realm/tag.refused.reserved") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/revert",
        tool: [
            "revert",
            {
                profile: "profile#4",
                commit: "ab".repeat(20),
                reason: "It broke the tests",
            },
        ],
        answers: {
            realm: { revert: () => answer("realm/revert.refused.reserved") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/ask_librarian",
        tool: ["ask_librarian", { question: "How do I lint the docs?" }],
        answers: {
            realm: { ask_librarian: () => answer("realm/ask_librarian.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/submit_skill",
        tool: [
            "submit_skill",
            { skill: "lint-docs", reason: "It checks every link" },
        ],
        files: { "skills/lint-docs/SKILL.md": "---\nname: lint-docs\n---\n" },
        answers: {
            library: { offer: () => ({ type: "kept", offer: 3 }) },
            realm: { submit_skill: () => answer("realm/submit_skill.done") },
        },
        own: ["call", "conversation"],
    },
    // The numbering of an offer's or a save's parts is `test/branch.test.ts`'s.
    ...["library/offer", "library/offer.last"].map(
        (fixture): Written => ({
            fixture,
            tool: [
                "submit_skill",
                { skill: "fixtures", reason: "Every hire writes fixtures" },
            ],
            files: { "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n" },
            answers: {
                library: { offer: () => answer("library/offer.kept") },
                realm: {
                    submit_skill: () => answer("realm/submit_skill.done"),
                },
            },
            own: ["call", "part", "last"],
        }),
    ),
    // Asked at each wake of a hand or a hire.
    {
        fixture: "library/shelves",
        answers: { library: { shelves: () => answer("library/shelves") } },
    },
    {
        fixture: "library/shelves.seen",
        drive: async (t, calls) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.shelves_changed(agent.by(from.library), {
                list: { version: 4, skills: [] },
            });
            await agent.stop();
            return agent.again();
        },
    },
    {
        fixture: "profile/files.tip",
        drive: async (t, calls) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.fill(agent.by(from.realm), { profile: 2 });
            return agent;
        },
        answers: {
            profile: { files: () => answer("profile/files.part.last") },
        },
    },
    {
        fixture: "profile/learned",
        drive: async (t, calls) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.fill(agent.by(from.realm), { profile: 2 });
            await agent.actions.read_papers(agent.by(from.client));
            return agent;
        },
        answers: {
            profile: {
                files: () => answer("profile/files.part.last"),
                learned: () => answer("profile/learned"),
            },
        },
    },
    {
        fixture: "workspace/fit_image",
        drive: async (t, calls) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            const [line] = fixture("agent/submit.line.attached.args") as [
                Submit,
            ];
            await agent.actions.submit(agent.by(from.realm), line);
            return agent;
        },
        answers: {
            realm: { fetch_part: () => answer("realm/fetch_part.bytes.last") },
            workspace: {
                fit_image: () => answer("workspace/fit_image.image"),
            },
        },
    },
    {
        fixture: "profile/files.since",
        drive: async (t, calls, sent) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.fill(agent.by(from.realm), { profile: 2 });
            await agent.actions.moved(agent.by(from.profile(2)), {
                tip: "cd".repeat(20),
            });
            await until(() => named(sent, "files").length === 3);
            return agent;
        },
        answers: {
            profile: {
                files: answering(
                    {
                        type: "part",
                        tip: "12".repeat(20),
                        files: [],
                        removed: [],
                    },
                    answer("profile/files.part"),
                    answer("profile/files.part.last"),
                ),
            },
        },
    },
    ...["profile/save", "profile/save.last"].map(
        (fixture): Written => ({
            fixture,
            drive: async (t, calls) => {
                const files: Record<string, string> = {};
                const agent = await wakeAgent(t, {
                    calls,
                    input: HAND,
                    files: fakeFiles(files),
                    responses: [fauxAssistantMessage(LAST_WORDS)],
                });
                await agent.actions.fill(agent.by(from.realm), { profile: 2 });
                files["memory/tests.md"] = "# Notes\n";
                delete files["memory/old.md"];
                await agent.actions.submit(
                    agent.by(from.realm),
                    submitted(undefined),
                );
                await agent.idle();
                return agent;
            },
            answers: {
                profile: {
                    files: answering(
                        {
                            type: "part",
                            tip: "ab".repeat(20),
                            files: ["memory/tests.md", "memory/old.md"].map(
                                (path) => ({
                                    path,
                                    offset: 0,
                                    bytes: Buffer.from("old\n").toString(
                                        "base64",
                                    ),
                                    last: true,
                                }),
                            ),
                            removed: [],
                        },
                        answer("profile/files.part.last"),
                    ),
                    save: () => answer("profile/save.saved"),
                },
            },
            own: ["call", "part", "last"],
        }),
    ),
];
