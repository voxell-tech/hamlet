import { readFileSync } from "node:fs";
import { Compile } from "typebox/compile";
import Schema, { type XSchema } from "typebox/schema";
import { describe, expect, it } from "vitest";
import {
    type Agent,
    ClientMessage,
    type Hamlet,
    type Item,
    type Limits,
    type Say,
    ServerMessage,
    type UsageView,
    type Wait,
} from "../src/protocol.ts";
import { protocolSchema } from "../src/schema.ts";

/** Examples of each message, by its type, covering each of its forms. */
type Examples<M extends { type: string }> = {
    [T in M["type"]]: Extract<M, { type: T }>[];
};

const ID = "6f1c2a7e-3b9d-4e8a-9c51-0d2f4b6a8e13";
const mayor = { hamlet: "home", agent: "mayor" };
const chat = { with: mayor };
const ref = { hamlet: "home", number: 0 };

/** The server's default limits (assets/limits.schema.json). */
const limits: Limits = {
    votes: {
        passYes: 2,
        passOf: 3,
        leastVoters: 3,
        sealed: true,
        groundedOnly: false,
        ballotMinutes: 5,
        openMinutes: 60,
        retryMinutes: 60,
    },
    hamlets: { most: 6, roadsBetween: true, removalMayors: 3 },
    budget: { period: "day", tokens: null, cost: 5, minutes: null },
    task: { messages: 12, tokens: null, cost: 1, minutes: null },
    road: { tokens: 0, cost: 0.01, minutes: 0, growsOverDays: 7 },
    mostAgents: 12,
    mostRoads: 24,
    models: [],
    model: null,
    pinned: {},
    mayor: {
        createAgents: true,
        buildRoads: true,
        removeAgents: true,
        mergeAgents: true,
        toldDays: 7,
    },
    leaving: { protectedMinutes: 0, mergedNotes: 6000 },
    asking: { mayorReview: "remote", refusals: 3, questions: 3 },
    permissions: { network: [], unconfined: [] },
};

const hamlet: Hamlet = {
    roads: [],
    votes: [],
    at: null,
    id: "home",
    name: "home",
    folder: "/Users/chief/code/project",
    before: [],
    mode: "manual",
    modeWords: "Agents ask you before each edit and command.",
    limits,
    budget: {
        period: "day",
        periodWords: "today",
        gone: 0.5,
        spent: { tokens: 18200, micros: 420000, ms: 42000 },
        caps: { micros: 5000000, ms: null },
        summary: "Hamlet today: $0.42 of $5.00",
        lines: ["- founder: local::qwen3.6:35b-a3b, $0.42"],
        left: 0.916,
        remains: "$4.58",
    },
};

const agent: Agent = {
    consulting: false,
    conversations: [],
    ref,
    name: "founder",
    purpose: "Keeps the hamlet",
    since: 1760000000000,
    mayor: true,
    model: "local::qwen3.6:35b-a3b",
    busy: true,
    faulted: false,
    activity: {
        doing: "fetch",
        doingWords: "Fetching from the network",
        step: 2,
        failures: 0,
    },
    saying: { text: "Looking at the tests.", thought: false },
    spent: { tokens: 18200, micros: 420000, ms: 42000, cost: "$0.42" },
    skills: [],
};

const wait: Wait = {
    id: "41",
    by: "founder",
    agent: ref,
    about: "src/main.rs",
    subject: "src/main.rs",
    change: {
        path: "src/main.rs",
        created: false,
        lines: [
            { mark: " ", text: "fn main() {" },
            { mark: "-", text: '    println!("hi");' },
            { mark: "+", text: '    println!("hello");' },
        ],
        total: 3,
        added: 1,
        removed: 1,
        count: "1 lines added, 1 removed",
    },
    answers: ["allow", "always", "deny"],
    since: 1760000000000,
    asked: 1,
    chat,
    detail: {
        kind: "approval",
        danger: "edit",
        says: "wants to edit",
        always: "session",
    },
};

const usage: UsageView = {
    calls: 3,
    unreported: 0,
    input: 12000,
    cached: 4000,
    output: 2200,
    reasoning: 0,
    ms: 14000,
    micros: 420000,
    lastInput: 4100,
    summary: "18.2k tok, $0.42",
};

const item: Item = {
    id: "12:1",
    at: 1760000000000,
    kind: "tool",
    text: "",
    tool: {
        name: "edit",
        summary: '{"path":"src/main.rs"}',
        exit: null,
        ms: null,
        failure: null,
        change: wait.change,
    },
};

const said: Say = {
    type: "say",
    id: ID,
    to: mayor,
    text: "Look at the tests.",
};

const fromClient: Examples<ClientMessage> = {
    hello: [{ type: "hello", v: 0, token: "t0ken" }],
    say: [said],
    give: [
        {
            type: "give",
            id: ID,
            to: mayor,
            files: [{ path: "folder/a.txt", size: 1 }],
        },
        { type: "give", id: ID, to: mayor, skill: { starter: "web-search" } },
        {
            type: "give",
            id: ID,
            to: mayor,
            skill: { from: { hamlet: "home", agent: "scout" }, name: "maps" },
        },
    ],
    "give-part": [
        {
            type: "give-part",
            id: ID,
            upload: "upload-1",
            path: "folder/a.txt",
            offset: 0,
            data: "QQ==",
        },
    ],
    answer: [
        { type: "answer", id: ID, wait: "41", choice: "allow" },
        { type: "answer", id: ID, wait: "41", choice: "reply", text: "No." },
    ],
    stop: [
        { type: "stop", id: ID },
        { type: "stop", id: ID, agent: mayor },
    ],
    rewind: [{ type: "rewind", id: ID, chat, item: "12:0" }],
    new: [{ type: "new", id: ID, to: mayor }],
    resume: [{ type: "resume", id: ID, to: mayor, number: 2 }],
    retry: [{ type: "retry", id: ID, to: mayor }],
    edit: [
        { type: "edit", id: ID, doc: "charter", text: null },
        { type: "edit", id: ID, doc: "character", agent: mayor, text: "Calm." },
        { type: "edit", id: ID, doc: "notes", agent: mayor, text: "# Notes\n" },
        {
            type: "edit",
            id: ID,
            doc: "limits",
            hamlet: "home",
            set: { section: "budget", key: "cost", value: 2.5 },
        },
        {
            type: "edit",
            id: ID,
            doc: "limits",
            hamlet: "home",
            set: { section: "budget", key: "minutes", value: null },
        },
        { type: "edit", id: ID, doc: "limits", hamlet: "home", whole: limits },
        {
            type: "edit",
            id: ID,
            doc: "settings",
            set: {
                model: "local::qwen3.6:35b-a3b",
                reflectEvery: 10,
                allowNet: false,
                endpoints: [
                    { name: "local", baseUrl: "http://localhost:11434/v1" },
                ],
            },
        },
        {
            type: "edit",
            id: ID,
            doc: "catalogue",
            model: "local::qwen3.6:35b-a3b",
            set: { allowed: true, price: { input: 0.1, output: 0.1 } },
        },
        {
            type: "edit",
            id: ID,
            doc: "catalogue",
            model: "local::qwen3.6:35b-a3b",
            set: { price: null, note: "Fast enough." },
        },
    ],
    open: [
        { type: "open", id: ID, chat },
        {
            type: "open",
            id: ID,
            chat: { with: mayor, past: 1 },
            before: "12:0",
        },
    ],
    close: [{ type: "close", chat }],
    login: [
        {
            type: "login",
            id: ID,
            start: { provider: "chatgpt", method: "subscription" },
        },
        { type: "login", id: ID, reply: { login: "l1", value: "sk-x" } },
        { type: "login", id: ID, cancel: "l1" },
    ],
    check: [
        { type: "check", id: ID, what: "model" },
        { type: "check", id: ID, what: "model", model: "openai::gpt-5" },
        { type: "check", id: ID, what: "connection" },
    ],
    read: [
        { type: "read", id: ID, what: "prompt", agent: mayor },
        { type: "read", id: ID, what: "history", agent: mayor },
        { type: "read", id: ID, what: "models" },
        { type: "read", id: ID, what: "models", refresh: true },
        { type: "read", id: ID, what: "papers", agent: mayor },
        { type: "read", id: ID, what: "charter" },
    ],
    "move-houses": [{ type: "move-houses", id: ID }],
};

const fromServer: Examples<ServerMessage> = {
    state: [
        {
            houses: "/Users/chief/.hamlet/houses",
            library: [],
            closed: [],
            type: "state",
            epoch: 1,
            revision: 7,
            chief: "chief",
            sandbox: "Seatbelt",
            hamlets: [hamlet],
            agents: [agent],
            waits: [wait],
            grants: [
                {
                    agent: ref,
                    name: "founder",
                    permission: "network",
                    by: "chief",
                    source: "grant",
                },
                {
                    agent: null,
                    name: "scout",
                    permission: "unconfined",
                    by: null,
                    source: "limits",
                },
            ],
            settings: {
                model: "local::qwen3.6:35b-a3b",
                reflectEvery: 10,
                allowNet: false,
                endpoints: [
                    {
                        name: "local",
                        baseUrl: "http://localhost:11434/v1",
                        key: false,
                    },
                ],
                unset: false,
                link: { state: "connected", models: 3, words: "Connected" },
                workspace: { url: null, running: null },
            },
            providers: [
                {
                    id: "chatgpt",
                    name: "ChatGPT",
                    configured: false,
                    methods: ["subscription"],
                    label: "Continue with ChatGPT",
                },
            ],
            deeds: [
                {
                    name: "mode",
                    usage: "<manual|auto>",
                    description:
                        "Whether agents ask before they change the folder",
                },
            ],
            starters: [],
        },
    ],
    change: [
        {
            type: "change",
            epoch: 1,
            revision: 8,
            changes: [
                { type: "hamlet", hamlet },
                { type: "agent", agent },
                { type: "agent-gone", ref },
                { type: "wait", wait: { ...wait, change: null, chat: null } },
                { type: "wait-closed", id: "41", how: "allowed" },
                { type: "grants", grants: [] },
                {
                    type: "settings",
                    settings: {
                        model: "openai::gpt-5",
                        reflectEvery: 10,
                        allowNet: true,
                        endpoints: [],
                        unset: true,
                        link: { state: "untested", models: null, words: null },
                        workspace: {
                            url: "https://workspace.example",
                            running: null,
                            move: {
                                state: "moving",
                                done: 1,
                                of: 3,
                                text: "",
                            },
                        },
                    },
                },
                { type: "providers", providers: [] },
            ],
        },
    ],
    turn: [
        {
            type: "turn",
            chat,
            ops: [
                { op: "add", item },
                { op: "append", item: "12:0", text: "more" },
                {
                    op: "close",
                    item: "12:1",
                    exit: 7,
                    ms: 1200,
                    failure: "curl: (7) Failed to connect",
                },
                {
                    op: "status",
                    working: true,
                    pending: 1,
                    last: { what: "tool bash", at: 1760000000000 },
                },
                { op: "status", working: false, pending: 0, last: null },
                { op: "usage", usage },
                { op: "reset" },
            ],
        },
    ],
    transcript: [
        {
            type: "transcript",
            to: ID,
            chat,
            items: [
                {
                    id: "11:0",
                    at: 1760000000000,
                    kind: "user",
                    text: "Look at the tests.",
                    chief: "chief",
                    rewindable: true,
                },
                item,
            ],
            earlier: null,
            last: { what: "tool bash", at: 1760000000000 },
            working: true,
            pending: 0,
            usage,
        },
        {
            type: "transcript",
            chat: { with: mayor, past: 1 },
            items: [],
            earlier: "3:0",
            last: null,
            working: false,
            pending: 0,
            usage,
        },
    ],
    reply: [
        {
            type: "reply",
            to: ID,
            kind: "notice",
            text: "Nothing waits for you.",
        },
        {
            type: "reply",
            to: ID,
            kind: "error",
            text: "usage: /mode <manual|auto>",
        },
        { type: "reply", to: ID, kind: "rewound", text: "Look at the tests." },
    ],
    login: [
        {
            type: "login",
            login: "l1",
            to: ID,
            step: {
                url: "https://auth.openai.com/x",
                instructions: "Sign in.",
            },
        },
        { type: "login", login: "l1", step: { code: "ABCD-1234" } },
        {
            type: "login",
            login: "l1",
            step: { prompt: "Paste your key", secret: true },
        },
        { type: "login", login: "l1", step: { progress: "Waiting" } },
        { type: "login", login: "l1", step: { done: "chatgpt" } },
        {
            type: "login",
            login: "l1",
            step: { failed: "The key was refused." },
        },
    ],
    read: [
        { type: "read", to: ID, prompt: "# Charter\n" },
        {
            type: "read",
            to: ID,
            history: [
                {
                    number: 1,
                    current: false,
                    title: "Look at the tests.",
                    by: "chief",
                    began: 1760000000000,
                    modified: 1760000100000,
                    messages: 4,
                    tokens: 18200,
                },
            ],
        },
        {
            type: "read",
            to: ID,
            models: [
                {
                    name: "local::qwen3.6:35b-a3b",
                    price: { input: 0.1, output: 0.1 },
                    priced: "assigned",
                    context: 32768,
                    allowed: true,
                    note: "",
                },
                {
                    name: "openai::gpt-5",
                    price: null,
                    priced: "unknown",
                    context: null,
                    allowed: false,
                    note: "",
                },
            ],
        },
        {
            type: "read",
            to: ID,
            papers: {
                notes: "# Notes\n",
                character: "Calm.",
                skills: [{ name: "release", description: "Cuts a release" }],
            },
        },
        { type: "read", to: ID, charter: { text: "# Charter\n", own: false } },
    ],
    fault: [
        { type: "fault", text: "The token is wrong." },
        { type: "fault", text: "No such agent.", to: ID },
    ],
};

/** The JSON Schema kept in the repository. */
const exported: XSchema = JSON.parse(
    readFileSync(new URL("../../schema.json", import.meta.url), "utf8"),
);

/** Whether a value is a client's or the server's message. */
const client = Compile(ClientMessage);
const server = Compile(ServerMessage);

describe("protocol v0", () => {
    it("is the JSON Schema kept in the repository", () => {
        expect(exported).toEqual(protocolSchema());
    });

    it("takes the example of each message, by its definitions and by the exported schema", () => {
        for (const message of Object.values(fromClient).flat()) {
            expect(client.Check(message)).toBe(true);
            expect(Schema.Check(exported, message)).toBe(true);
        }
        for (const message of Object.values(fromServer).flat()) {
            expect(server.Check(message)).toBe(true);
            expect(Schema.Check(exported, message)).toBe(true);
        }
    });

    it("refuses unknown types, missing and extra fields, and ids that are not UUIDs", () => {
        for (const message of [
            { type: "shout", id: ID },
            { type: "say", id: ID, to: mayor },
            { ...said, loud: true },
            { ...said, id: "1" },
        ]) {
            expect(client.Check(message)).toBe(false);
            expect(Schema.Check(exported, message)).toBe(false);
        }
        expect(server.Check({ type: "say" })).toBe(false);
    });

    it("takes a whole limits document as the chief wrote it, for the server to check (H22)", () => {
        // Each field left out takes its default, and the server refuses a
        // wrong one in Rust's words (agents/tests.rs:329).
        for (const whole of [
            { budget: { cost: 7 } },
            { mostAgents: "12" },
            { most_agents: 12 },
            { budget: { period: "week" } },
        ]) {
            const message = {
                type: "edit",
                id: ID,
                doc: "limits",
                hamlet: "home",
                whole,
            };
            expect(client.Check(message)).toBe(true);
            expect(Schema.Check(exported, message)).toBe(true);
        }
    });
});

it("keeps a danger in an approval's detail and in no other kind's", () => {
    const wire = (detail: unknown) => ({
        ...fromServer.state[0],
        waits: [{ ...wait, detail }],
    });
    const approval = { kind: "approval", says: "wants to run", always: null };
    expect(server.Check(wire(approval))).toBe(false);
    expect(server.Check(wire({ ...approval, danger: "command" }))).toBe(true);
    const questions = {
        message: "Which grain?",
        questions: [
            {
                question: "Which grain?",
                description: "",
                options: [{ title: "Wheat", description: "" }],
                text_input: true,
            },
        ],
    };
    expect(server.Check(wire({ kind: "question", questions }))).toBe(true);
    expect(server.Check(wire({ kind: "question" }))).toBe(false);
    for (const detail of [
        { kind: "fault" },
        { kind: "question", questions },
        { kind: "objection" },
    ])
        expect(server.Check(wire({ ...detail, danger: "command" }))).toBe(
            false,
        );
});

const societyAgent = {
    ...agent,
    left: {
        how: "merged",
        into: ref,
        at: 12,
        by: "the mayor",
        reason: "Shared work",
        words: "scout merged into founder: Shared work",
    },
    consulting: true,
    conversations: [
        { with: ref, kind: "letters", count: 2, last: 12 },
        { with: ref, kind: "questions", count: 1, last: null },
    ],
};
const societyHamlet = {
    ...hamlet,
    roads: [
        {
            a: ref,
            b: { hamlet: "mill", number: 1 },
            messages: 10,
            level: "rail",
            lastFrom: ref,
        },
    ],
    votes: [
        {
            id: 1,
            motion: "Seat scout",
            by: "the chief",
            open: true,
            overturnable: true,
            report: "Voting",
            voters: [null, "none", "yes", "no", "abstain"].map((cast) => ({
                agent: ref,
                cast,
            })),
        },
    ],
    at: { x: 2.5, z: -3.5 },
};

it("takes the society and realm fields in snapshots, changes and watched chats", () => {
    const messages = [
        {
            ...fromServer.state[0],
            hamlets: [societyHamlet],
            agents: [societyAgent],
            library: [{ name: "maps", description: "Draw maps", by: "scout" }],
            closed: [
                {
                    id: "mill",
                    name: "The Mill",
                    at: 13,
                    by: "the chief",
                    reason: "Finished",
                },
            ],
        },
        {
            type: "change",
            epoch: 1,
            revision: 9,
            changes: [
                { type: "hamlet", hamlet: societyHamlet },
                { type: "agent", agent: societyAgent },
            ],
        },
        {
            type: "change",
            epoch: 1,
            revision: 10,
            changes: [
                {
                    type: "hamlet",
                    hamlet: {
                        ...societyHamlet,
                        at: null,
                        roads: [{ ...societyHamlet.roads[0], lastFrom: null }],
                    },
                },
            ],
        },
    ];
    for (const message of messages) {
        expect(server.Check(message)).toBe(true);
        expect(Schema.Check(exported, message)).toBe(true);
    }
    const message = { type: "open", id: ID, chat: { with: mayor, from: ref } };
    expect(client.Check(message)).toBe(true);
    expect(Schema.Check(exported, message)).toBe(true);
});

it("refuses unknown road levels and ballot casts", () => {
    for (const changed of [
        {
            ...societyHamlet,
            roads: [{ ...societyHamlet.roads[0], level: "highway" }],
        },
        {
            ...societyHamlet,
            votes: [
                {
                    ...societyHamlet.votes[0],
                    voters: [{ agent: ref, cast: "maybe" }],
                },
            ],
        },
    ]) {
        expect(
            server.Check({ ...fromServer.state[0], hamlets: [changed] }),
        ).toBe(false);
    }
});

it("accepts typed picks and rejects ambiguous or negative pick shapes", () => {
    const answer = {
        type: "answer",
        id: ID,
        wait: "question",
        choice: "reply",
    };
    expect(
        client.Check({
            ...answer,
            text: "words for a question that takes words",
        }),
    ).toBe(true);
    expect(
        client.Check({
            ...answer,
            picks: [
                { kind: "offer", index: 1 },
                { kind: "words", text: "custom" },
            ],
        }),
    ).toBe(true);
    for (const picks of [
        [],
        [{ kind: "offer", index: -1 }],
        [{ kind: "offer", index: 0.5 }],
        [{ kind: "offer", index: 1, text: "extra" }],
        [{ kind: "words", text: 9 }],
    ]) {
        expect(client.Check({ ...answer, picks })).toBe(false);
    }
});
