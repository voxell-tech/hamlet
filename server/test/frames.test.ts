import { randomUUID } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { type Connection, connect } from "../src/connection.ts";
import type { HomeOptions } from "../src/home.ts";
import { houseOf } from "../src/home.ts";
import { type Script, scriptedModels } from "../src/models.ts";
import {
    applyOp,
    type ClientMessage,
    type Item,
    parseClient,
    parseServer,
    type ServerMessage,
    type Transcript,
} from "../src/protocol.ts";
import { safeValue } from "../src/scrub.ts";
import { agents as createSociety, tool, yes } from "./acceptance-support.ts";
import {
    agents,
    changes,
    chat,
    mayor,
    received,
    request,
    say,
    served,
} from "./connection-support.ts";
import { openaiFake } from "./openai-fake.ts";
import { closeAfter, fauxAgents, fauxModels, scratch } from "./support.ts";

const scenarios = [
    "society",
    "leaving",
    "vote",
    "realm",
    "library",
    "hello",
    "turn",
    "tool",
    "approval",
    "fault",
    "stop",
    "rewind",
    "new-and-resume",
    "paging",
    "reads-and-edits",
    "login-key",
    "budget-stop",
] as const;

type Scenario = (typeof scenarios)[number];
type Frame =
    | { from: "client"; message: ClientMessage }
    | { from: "server"; message: ServerMessage };
type Session = Awaited<ReturnType<typeof recording>>;
type WithoutId<M> = M extends unknown ? Omit<M, "id"> : never;
type Asked = WithoutId<Extract<ClientMessage, { id: string }>>;

for (const name of scenarios)
    it(`keeps a real ${name} session in protocol v0`, async () => {
        const file = new URL(`./frames/${name}.jsonl`, import.meta.url);
        if (process.env.HAMLET_RECORD_FRAMES === "1") {
            const run = await record(name);
            await run.connection.close();
            await run.listening;
            expect(run.errors).toHaveLength(1);
            expect(run.errors[0]).toMatchObject({
                message: "The connection to the server closed (1005).",
            });
            const frames = safeValue(run.home, run.frames);
            checkScenario(name, frames);
            await mkdir(new URL("./frames/", import.meta.url), {
                recursive: true,
            });
            await writeFile(
                file,
                `${frames.map((frame) => JSON.stringify(frame)).join("\n")}\n`,
            );
        }
        const text = await readFile(file, "utf8");
        expect(text.endsWith("\n")).toBe(true);
        const frames = text
            .trimEnd()
            .split("\n")
            .map((line): Frame => {
                const frame = JSON.parse(line);
                expect(["client", "server"]).toContain(frame.from);
                const parsed = (
                    frame.from === "client" ? parseClient : parseServer
                )(JSON.stringify(frame.message));
                expect(parsed).toHaveProperty("value", frame.message);
                return frame;
            });
        expect(frames[0]).toEqual({
            from: "client",
            message: { type: "hello", v: 0, token: "[REDACTED]" },
        });
        expect(frames[1]).toMatchObject({
            from: "server",
            message: { type: "state" },
        });
        checkScenario(name, frames);
    }, 30_000);

async function recording(script: Script | null, options: HomeOptions = {}) {
    const models =
        options.models ??
        (script === null ? undefined : scriptedModels(script));
    const run = await served({
        ...options,
        ...(models === undefined ? {} : { models }),
        env: { HAMLET_REFLECT_EVERY: "0", ...options.env },
    });
    await run.connection.close();
    const wire = await connect(run.server.url);
    closeAfter(() => wire.close());
    const frames: Frame[] = [];
    const connection: Connection = {
        ...wire,
        send(message) {
            frames.push({ from: "client", message: structuredClone(message) });
            wire.send(message);
        },
        async next() {
            const message = await wire.next();
            frames.push({ from: "server", message: structuredClone(message) });
            return message;
        },
    };
    connection.send({ type: "hello", v: 0, token: run.home.credentials.token });
    const state = await connection.next();
    if (state.type !== "state") throw new Error("No state after hello");
    const messages: ServerMessage[] = [state];
    const errors: unknown[] = [];
    const listening = (async () => {
        try {
            for (;;) messages.push(await connection.next());
        } catch (error) {
            errors.push(error);
        }
    })();
    return {
        home: run.home,
        connection,
        state,
        messages,
        frames,
        listening,
        errors,
    };
}

function ask(run: Session, message: Asked) {
    return request(run.connection, run.messages, {
        ...message,
        id: randomUUID(),
    });
}

async function open(run: Session, before?: string): Promise<Transcript> {
    const message = await ask(run, {
        type: "open",
        chat,
        ...(before === undefined ? {} : { before }),
    });
    if (message.type !== "transcript")
        throw new Error(`No transcript: ${JSON.stringify(message)}`);
    return message;
}

async function speak(run: Session, text: string, answer: string) {
    say(run.connection, text);
    const sent = run.frames.at(-1)?.message;
    if (sent?.type !== "say") throw new Error("No chief line sent");
    await received(
        run.messages,
        (message): message is ServerMessage =>
            "to" in message && message.to === sent.id,
    );
    await vi.waitFor(
        () => {
            expect(live(run.messages).at(-1)?.text).toBe(answer);
            expect(agents(run.messages).at(-1)?.busy).toBe(false);
        },
        { timeout: 10_000 },
    );
}

function live(messages: ServerMessage[]): Item[] {
    const items: Item[] = [];
    const status = { working: false, pending: 0 };
    for (const message of messages) {
        if (message.type === "transcript")
            items.splice(0, items.length, ...structuredClone(message.items));
        if (message.type !== "turn") continue;
        for (const op of message.ops) applyOp(items, status, op);
    }
    return items;
}

async function record(name: Scenario): Promise<Session> {
    switch (name) {
        case "society": {
            const { models, faux } = fauxModels();
            const run = await recording(null, { models });
            await open(run);
            await createSociety({ ...run, faux });
            fauxAgents(faux, {
                "home:0": [
                    tool("tell_agent", {
                        name: "smith",
                        message: "Make a hinge",
                    }),
                    fauxAssistantMessage("Sent"),
                    fauxAssistantMessage("Letter received"),
                ],
                "home:2": [fauxAssistantMessage("Hinge ready")],
            });
            await ask(run, { type: "say", to: mayor, text: "Tell smith" });
            await vi.waitFor(
                () =>
                    expect(
                        live(run.messages).some(
                            (i) => i.text === "Letter received",
                        ),
                    ).toBe(true),
                { timeout: 10000 },
            );
            await ask(run, {
                type: "open",
                chat: {
                    with: { hamlet: "home", agent: "smith" },
                    from: { hamlet: "home", number: 0 },
                    kind: "letters",
                },
            });
            fauxAgents(faux, {
                "home:0": [
                    tool("ask_agent", {
                        name: "scout",
                        message: "Where is the iron?",
                    }),
                    fauxAssistantMessage("Iron found"),
                ],
                "home:1": [fauxAssistantMessage("In the northern hill")],
            });
            await ask(run, { type: "say", to: mayor, text: "Ask scout" });
            await ask(run, {
                type: "open",
                chat: {
                    with: { hamlet: "home", agent: "scout" },
                    from: { hamlet: "home", number: 0 },
                    kind: "questions",
                },
            });
            await open(run);
            return run;
        }
        case "leaving": {
            const { models, faux } = fauxModels();
            const run = await recording(null, { models });
            await open(run);
            await createSociety({ ...run, faux });
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/remove scout Finished",
            });
            await ask(run, { type: "say", to: mayor, text: "/restore scout" });
            fauxAgents(faux, {
                errands: [fauxAssistantMessage("# Notes\nFind and forge.")],
            });
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/merge scout into smith Together",
            });
            await open(run);
            return run;
        }
        case "vote": {
            const { models, faux } = fauxModels();
            const run = await recording(null, { models });
            await open(run);
            await createSociety({ ...run, faux });
            fauxAgents(faux, {
                errands: [
                    fauxAssistantMessage("Let them decide"),
                    yes(),
                    yes(),
                    yes(),
                ],
            });
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/vote smith New direction",
            });
            await vi.waitFor(() =>
                expect(
                    changes(run.messages).some(
                        (u) =>
                            u.type === "hamlet" &&
                            u.hamlet.votes?.some(
                                (v) => !v.open && v.report.includes("Passed:"),
                            ),
                    ),
                ).toBe(true),
            );
            await open(run);
            return run;
        }
        case "realm": {
            const { models, faux } = fauxModels();
            const run = await recording(null, { models });
            await open(run);
            await createSociety({ ...run, faux });
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/found mill miller Grind flour",
            });
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/move-hamlet mill 60 80",
            });
            fauxAgents(faux, {
                "mill:0": [
                    tool("create_agent", {
                        name: "mason",
                        purpose: "Stonework",
                        reason: "Build",
                    }),
                    fauxAssistantMessage("Mason ready"),
                ],
            });
            const mill = { hamlet: "mill", agent: "mayor" };
            await ask(run, { type: "say", to: mill, text: "Make a mason" });
            fauxAgents(faux, {
                "home:0": [
                    tool("agree_road", {
                        a: "scout",
                        b: "mason",
                        reason: "Trade",
                    }),
                    fauxAssistantMessage("Road proposed"),
                ],
                "mill:0": [
                    fauxAssistantMessage("Considering"),
                    tool("agree_road", {
                        a: "mason",
                        b: "scout",
                        reason: "Trade",
                    }),
                    fauxAssistantMessage("Road agreed"),
                ],
            });
            await ask(run, { type: "say", to: mayor, text: "Propose a road" });
            await ask(run, { type: "say", to: mill, text: "Agree the road" });
            await vi.waitFor(() =>
                expect(
                    changes(run.messages).some(
                        (u) =>
                            u.type === "hamlet" &&
                            u.hamlet.roads?.some(
                                (r) =>
                                    r.a.hamlet !== r.b.hamlet &&
                                    (r.a.number === 1 || r.b.number === 1),
                            ),
                    ),
                ).toBe(true),
            );
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/remove-hamlet mill Finished",
            });
            await vi.waitFor(() =>
                expect(
                    run.messages.some(
                        (m) =>
                            m.type === "state" &&
                            m.closed?.some((h) => h.id === "mill"),
                    ),
                ).toBe(true),
            );
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/erase-hamlet mill",
            });
            await vi.waitFor(() =>
                expect(
                    run.messages.filter((m) => m.type === "state").at(-1),
                ).toMatchObject({ closed: [] }),
            );
            return run;
        }
        case "library": {
            const { models, faux } = fauxModels();
            const run = await recording(null, { models });
            await open(run);
            await createSociety({ ...run, faux });
            const skill = join(
                houseOf(run.home.layout, { hamlet: "home", number: 1 }),
                "skills",
                "map-iron",
            );
            await mkdir(skill, { recursive: true });
            await writeFile(
                join(skill, "SKILL.md"),
                "---\nname: map-iron\ndescription: Maps iron deposits.\n---\nRead the map.\n",
            );
            fauxAgents(faux, {
                "home:1": [
                    tool("submit_skill", {
                        skill: "map-iron",
                        reason: "Useful maps",
                    }),
                    fauxAssistantMessage("Shared"),
                ],
                errands: [fauxAssistantMessage("ACCEPT Useful maps")],
            });
            await ask(run, {
                type: "say",
                to: { hamlet: "home", agent: "scout" },
                text: "Share the map",
            });
            await vi.waitFor(() =>
                expect(
                    run.messages.some(
                        (m) =>
                            m.type === "state" &&
                            m.library?.some((s) => s.name === "map-iron"),
                    ),
                ).toBe(true),
            );
            await ask(run, {
                type: "say",
                to: mayor,
                text: "/unshelve map-iron Retire",
            });
            await vi.waitFor(() =>
                expect(
                    run.messages.filter((m) => m.type === "state").at(-1),
                ).toMatchObject({
                    library: expect.not.arrayContaining([
                        expect.objectContaining({ name: "map-iron" }),
                    ]),
                }),
            );
            return run;
        }
        case "hello":
            return recording({ responses: [] });
        case "turn": {
            const greeting =
                "Hello chief. The mayor is here and ready to work with you. "
                    .repeat(6)
                    .trim();
            const run = await recording({
                tokensPerSecond: 100,
                responses: [
                    {
                        thinking: "I will greet the chief.",
                        calls: [
                            {
                                name: "bash",
                                arguments: { command: "echo hello" },
                            },
                        ],
                    },
                    greeting,
                ],
            });
            await open(run);
            await speak(run, "Hello, mayor.", greeting);
            await open(run);
            return run;
        }
        case "tool": {
            const run = await recording({
                responses: [
                    {
                        calls: [
                            {
                                name: "bash",
                                arguments: { command: "echo success" },
                            },
                            {
                                name: "bash",
                                arguments: { command: "echo broken; exit 7" },
                            },
                        ],
                    },
                    "One command succeeded and one failed.",
                ],
            });
            await open(run);
            await speak(
                run,
                "Run both commands.",
                "One command succeeded and one failed.",
            );
            return run;
        }
        case "approval": {
            const run = await recording({
                responses: [
                    {
                        calls: [
                            {
                                name: "bash",
                                arguments: {
                                    command:
                                        "printf 'heading\\napproved\\n' > result.txt",
                                },
                            },
                            {
                                name: "write",
                                arguments: {
                                    path: "result.txt",
                                    content: "heading\nreviewed\n",
                                },
                            },
                        ],
                    },
                    "The approved command finished.",
                ],
            });
            await open(run);
            expect(
                await ask(run, {
                    type: "say",
                    to: mayor,
                    text: `/folder ${scratch()}`,
                }),
            ).toMatchObject({ kind: "notice" });
            say(run.connection, "Run the command and write in manual mode.");
            const waits: string[] = [];
            for (const danger of ["command", "edit"]) {
                await vi.waitFor(
                    () =>
                        expect(changes(run.messages)).toContainEqual(
                            expect.objectContaining({
                                type: "wait",
                                wait: expect.objectContaining({ danger }),
                            }),
                        ),
                    { timeout: 10_000 },
                );
                const wait = changes(run.messages).find(
                    (change) =>
                        change.type === "wait" && change.wait.danger === danger,
                );
                if (wait?.type !== "wait") throw new Error("No approval");
                expect(wait.wait.kind).toBe("approval");
                waits.push(wait.wait.id);
                expect(
                    await ask(run, {
                        type: "answer",
                        wait: wait.wait.id,
                        choice: "allow",
                    }),
                ).toMatchObject({ kind: "notice" });
            }
            await vi.waitFor(
                () => {
                    expect(live(run.messages).at(-1)?.text).toBe(
                        "The approved command finished.",
                    );
                    for (const id of waits)
                        expect(changes(run.messages)).toContainEqual({
                            type: "wait-closed",
                            id,
                            how: "allow",
                        });
                    expect(agents(run.messages).at(-1)?.busy).toBe(false);
                },
                { timeout: 10_000 },
            );
            expect(
                await ask(run, { type: "say", to: mayor, text: "/mode auto" }),
            ).toMatchObject({ kind: "notice" });
            return run;
        }
        case "fault": {
            const { models, faux } = fauxModels();
            faux.setResponses([
                fauxAssistantMessage("", {
                    stopReason: "error",
                    errorMessage: "Faux model unavailable",
                }),
            ]);
            const run = await recording({ responses: [] }, { models });
            await open(run);
            say(run.connection, "Try the model.");
            await vi.waitFor(
                () => {
                    expect(agents(run.messages).at(-1)).toMatchObject({
                        busy: false,
                        faulted: true,
                    });
                    expect(changes(run.messages)).toContainEqual(
                        expect.objectContaining({
                            type: "wait",
                            wait: expect.objectContaining({ kind: "fault" }),
                        }),
                    );
                },
                { timeout: 10_000 },
            );
            const wait = changes(run.messages).find(
                (change) => change.type === "wait",
            );
            if (wait?.type !== "wait") throw new Error("No fault wait");
            expect(
                await ask(run, {
                    type: "answer",
                    wait: wait.wait.id,
                    choice: "dismiss",
                }),
            ).toMatchObject({ kind: "notice" });
            return run;
        }
        case "stop": {
            const run = await recording({
                responses: [
                    {
                        calls: [
                            {
                                name: "bash",
                                arguments: {
                                    command:
                                        "echo started > NOTES.md; sleep 30",
                                },
                            },
                        ],
                    },
                    "This answer must never arrive.",
                ],
            });
            await open(run);
            say(run.connection, "Run until I stop you.");
            await vi.waitFor(
                async () =>
                    expect(
                        await ask(run, {
                            type: "read",
                            what: "papers",
                            agent: mayor,
                        }),
                    ).toMatchObject({ papers: { notes: "started" } }),
                { timeout: 10_000 },
            );
            expect(
                await ask(run, { type: "stop", agent: mayor }),
            ).toMatchObject({ kind: "notice" });
            await vi.waitFor(
                () => {
                    expect(live(run.messages).at(-1)).toMatchObject({
                        kind: "notice",
                        text: expect.stringContaining("Stopped:"),
                    });
                    expect(agents(run.messages).at(-1)?.busy).toBe(false);
                },
                { timeout: 10_000 },
            );
            return run;
        }
        case "rewind": {
            const run = await recording({
                responses: ["First answer.", "Second answer."],
            });
            await open(run);
            await speak(run, "Keep this line.", "First answer.");
            await speak(run, "Take back this line.", "Second answer.");
            const target = live(run.messages).find(
                (item) => item.text === "Take back this line.",
            );
            if (target === undefined) throw new Error("No second chief line");
            expect(
                await ask(run, { type: "rewind", chat, item: target.id }),
            ).toMatchObject({ kind: "rewound", text: "Take back this line." });
            await vi.waitFor(
                () =>
                    expect(live(run.messages).map((item) => item.text)).toEqual(
                        ["Keep this line.", "First answer."],
                    ),
                { timeout: 10_000 },
            );
            return run;
        }
        case "new-and-resume": {
            const run = await recording({
                responses: ["The first conversation."],
            });
            await open(run);
            await speak(
                run,
                "Remember our first conversation.",
                "The first conversation.",
            );
            expect(
                await ask(run, { type: "say", to: mayor, text: "/new" }),
            ).toMatchObject({ kind: "notice" });
            expect(
                await ask(run, { type: "read", what: "history", agent: mayor }),
            ).toMatchObject({
                history: expect.arrayContaining([
                    expect.objectContaining({
                        number: 1,
                        current: false,
                        messages: 2,
                    }),
                ]),
            });
            expect(
                await ask(run, { type: "say", to: mayor, text: "/resume 1" }),
            ).toMatchObject({ kind: "notice" });
            await vi.waitFor(
                () =>
                    expect(live(run.messages).at(-1)?.text).toBe(
                        "Went back to an earlier session.",
                    ),
                { timeout: 10_000 },
            );
            return run;
        }
        case "paging": {
            const run = await recording({
                responses: Array.from(
                    { length: 23 },
                    (_, i) => `Answer ${i + 1}.`,
                ),
            });
            for (let i = 1; i <= 23; i++)
                expect(
                    await ask(run, {
                        type: "say",
                        to: mayor,
                        text: `Line ${i}.`,
                    }),
                ).toMatchObject({ kind: "notice", text: "" });
            const latest = await open(run);
            expect(latest.items).toHaveLength(40);
            expect(latest.earlier).toBe(latest.items[0]?.id);
            if (latest.earlier === null) throw new Error("No older page");
            expect(await open(run, latest.earlier)).toMatchObject({
                earlier: null,
                items: Array.from({ length: 6 }, (_, i) =>
                    expect.objectContaining({
                        text:
                            i % 2 === 0
                                ? `Line ${i / 2 + 1}.`
                                : `Answer ${(i + 1) / 2}.`,
                    }),
                ),
            });
            return run;
        }
        case "reads-and-edits": {
            const run = await recording({ responses: [] });
            for (const what of ["prompt", "papers"] as const)
                expect(
                    await ask(run, { type: "read", what, agent: mayor }),
                ).toHaveProperty(what);
            for (const what of ["charter", "models"] as const)
                expect(await ask(run, { type: "read", what })).toHaveProperty(
                    what,
                );
            expect(
                await ask(run, {
                    type: "edit",
                    doc: "limits",
                    hamlet: "home",
                    set: { section: "budget", key: "cost", value: 2 },
                }),
            ).toMatchObject({ kind: "notice" });
            const limits = run.state.hamlets[0]?.limits;
            if (limits === undefined) throw new Error("No limits");
            expect(
                await ask(run, {
                    type: "edit",
                    doc: "limits",
                    hamlet: "home",
                    whole: {
                        ...limits,
                        mostAgents: 4,
                        budget: { ...limits.budget, cost: 3 },
                    },
                }),
            ).toMatchObject({ kind: "notice" });
            expect(
                await ask(run, {
                    type: "edit",
                    doc: "limits",
                    hamlet: "home",
                    set: { section: "budget", key: "cost", value: -1 },
                }),
            ).toMatchObject({ kind: "error" });
            expect(
                await ask(run, {
                    type: "edit",
                    doc: "settings",
                    set: { reflectEvery: 0, allowNet: true },
                }),
            ).toMatchObject({ kind: "notice" });
            expect(
                await ask(run, {
                    type: "edit",
                    doc: "charter",
                    text: "Serve the chief with care.",
                }),
            ).toMatchObject({ kind: "notice" });
            expect(
                await ask(run, { type: "read", what: "charter" }),
            ).toMatchObject({
                charter: { own: true, text: "Serve the chief with care." },
            });
            return run;
        }
        case "login-key": {
            const endpoint = await openaiFake();
            const run = await recording(null, {
                env: {
                    HAMLET_MODEL: "local::org/large",
                    HAMLET_BASE_URL: endpoint.url,
                },
            });
            const prompt = await ask(run, {
                type: "login",
                start: { provider: "openai", method: "key" },
            });
            expect(prompt).toMatchObject({
                type: "login",
                step: { prompt: "Enter OpenAI API key", secret: true },
            });
            if (prompt.type !== "login") throw new Error("No login prompt");
            expect(
                await ask(run, {
                    type: "login",
                    reply: {
                        login: prompt.login,
                        value: "sk-fake-frames-not-a-real-key",
                    },
                }),
            ).toMatchObject({ kind: "notice" });
            await vi.waitFor(
                () => {
                    expect(run.messages).toContainEqual(
                        expect.objectContaining({
                            type: "login",
                            step: { done: "openai" },
                        }),
                    );
                    expect(changes(run.messages)).toContainEqual(
                        expect.objectContaining({
                            type: "providers",
                            providers: expect.arrayContaining([
                                expect.objectContaining({
                                    id: "openai",
                                    configured: true,
                                }),
                            ]),
                        }),
                    );
                },
                { timeout: 10_000 },
            );
            return run;
        }
        case "budget-stop": {
            const { models, faux } = fauxModels(["faux-1"], 1_000_000);
            faux.setResponses([
                fauxAssistantMessage(
                    fauxToolCall("read", { path: "NOTES.md" }),
                    { stopReason: "toolUse" },
                ),
                fauxAssistantMessage("This answer must never arrive."),
            ]);
            const run = await recording({ responses: [] }, { models });
            await open(run);
            expect(
                await ask(run, {
                    type: "edit",
                    doc: "limits",
                    hamlet: "home",
                    set: { section: "budget", key: "cost", value: 0.01 },
                }),
            ).toMatchObject({ kind: "notice" });
            expect(
                await ask(run, {
                    type: "say",
                    to: mayor,
                    text: "Read your notes until the budget stops you.",
                }),
            ).toMatchObject({ kind: "notice" });
            await vi.waitFor(
                () => {
                    expect(live(run.messages).at(-1)).toMatchObject({
                        kind: "notice",
                        text: expect.stringMatching(/budget.*\$0\.01.*chief/),
                    });
                    expect(agents(run.messages).at(-1)?.busy).toBe(false);
                },
                { timeout: 10_000 },
            );
            expect(faux.state.callCount).toBe(1);
            return run;
        }
    }
}

function checkScenario(name: Scenario, frames: Frame[]) {
    const sent = frames.flatMap((frame) =>
        frame.from === "client" ? [frame.message] : [],
    );
    const messages = frames.flatMap((frame) =>
        frame.from === "server" ? [frame.message] : [],
    );
    const ops = messages.flatMap((message) =>
        message.type === "turn" ? message.ops : [],
    );
    switch (name) {
        case "society": {
            expect(agents(messages).map((a) => a.name)).toEqual(
                expect.arrayContaining(["scout", "smith", "poet"]),
            );
            expect(changes(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "hamlet",
                        hamlet: expect.objectContaining({
                            roads: expect.arrayContaining([
                                expect.objectContaining({ messages: 2 }),
                            ]),
                        }),
                    }),
                ]),
            );
            const items = messages.flatMap((m) =>
                m.type === "transcript"
                    ? m.items
                    : m.type === "turn"
                      ? m.ops.flatMap((o) => (o.op === "add" ? [o.item] : []))
                      : [],
            );
            expect(items).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        author: "smith@home",
                        text: expect.stringContaining("Hinge ready"),
                    }),
                ]),
            );
            expect(sent).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "open",
                        chat: expect.objectContaining({ kind: "questions" }),
                    }),
                ]),
            );
            break;
        }
        case "leaving": {
            expect(agents(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        name: "scout",
                        left: expect.objectContaining({ how: "removed" }),
                    }),
                    expect.objectContaining({
                        name: "scout",
                        left: expect.objectContaining({
                            how: "merged",
                            into: { hamlet: "home", number: 2 },
                        }),
                    }),
                ]),
            );
            expect(sent).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "say",
                        text: "/restore scout",
                    }),
                ]),
            );
            break;
        }
        case "vote":
            expect(changes(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "hamlet",
                        hamlet: expect.objectContaining({
                            votes: expect.arrayContaining([
                                expect.objectContaining({
                                    open: false,
                                    report: expect.stringContaining("Passed:"),
                                }),
                            ]),
                        }),
                    }),
                ]),
            );
            expect(agents(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({ name: "smith", mayor: true }),
                ]),
            );
            break;
        case "realm":
            expect(changes(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "hamlet",
                        hamlet: expect.objectContaining({
                            id: "mill",
                            at: { x: 60, z: 80 },
                        }),
                    }),
                ]),
            );
            expect(messages).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "state",
                        closed: expect.arrayContaining([
                            expect.objectContaining({ id: "mill" }),
                        ]),
                    }),
                ]),
            );
            expect(
                messages.filter((m) => m.type === "state").at(-1),
            ).toMatchObject({
                closed: [],
                hamlets: [expect.objectContaining({ id: "home" })],
            });
            break;
        case "library":
            expect(messages).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "state",
                        library: expect.arrayContaining([
                            expect.objectContaining({ name: "map-iron" }),
                        ]),
                    }),
                ]),
            );
            expect(
                messages.filter((m) => m.type === "state").at(-1),
            ).toMatchObject({
                library: expect.not.arrayContaining([
                    expect.objectContaining({ name: "map-iron" }),
                ]),
            });
            expect(agents(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        name: "scout",
                        consulting: true,
                    }),
                ]),
            );
            break;
        case "hello":
            expect(frames).toHaveLength(2);
            break;
        case "turn": {
            expect(ops.some((op) => op.op === "append")).toBe(true);
            const last = frames.at(-1)?.message;
            expect(last?.type).toBe("transcript");
            if (last?.type !== "transcript")
                throw new Error("No final transcript");
            expect(last.items).toEqual(live(messages.slice(0, -1)));
            const activity = { what: "replied", at: last.items.at(-1)?.at };
            expect(last.last).toEqual(activity);
            expect(ops.filter((op) => op.op === "status").at(-1)).toMatchObject(
                { last: activity },
            );
            expect(last.items.map((item) => item.kind)).toEqual([
                "user",
                "reasoning",
                "tool",
                "assistant",
            ]);
            break;
        }
        case "tool":
            expect(ops.filter((op) => op.op === "close")).toMatchObject([
                { exit: 0 },
                { exit: 7, failure: "broken" },
            ]);
            break;
        case "approval": {
            const updates = changes(messages);
            const wait = updates.find((update) => update.type === "wait");
            expect(wait).toMatchObject({
                wait: {
                    kind: "approval",
                    danger: "command",
                    answers: ["allow", "always", "deny"],
                },
            });
            if (wait?.type !== "wait") throw new Error("No approval");
            expect(updates).toContainEqual({
                type: "wait-closed",
                id: wait.wait.id,
                how: "allow",
            });
            expect(ops).toContainEqual(
                expect.objectContaining({ op: "close", exit: 0 }),
            );
            const edit = updates.find(
                (update) =>
                    update.type === "wait" && update.wait.danger === "edit",
            );
            expect(edit).toMatchObject({
                wait: {
                    kind: "approval",
                    danger: "edit",
                    change: {
                        path: "result.txt",
                        created: false,
                        lines: [
                            { mark: " ", text: "heading" },
                            { mark: "-", text: "approved" },
                            { mark: "+", text: "reviewed" },
                        ],
                        total: 3,
                        added: 1,
                        removed: 1,
                        count: "1 line added, 1 removed",
                    },
                },
            });
            if (edit?.type !== "wait") throw new Error("No held edit");
            expect(updates).toContainEqual({
                type: "wait-closed",
                id: edit.wait.id,
                how: "allow",
            });
            const tool = ops.find(
                (op) => op.op === "add" && op.item.tool?.name === "write",
            );
            if (tool?.op !== "add") throw new Error("No write item");
            expect(ops).toContainEqual(
                expect.objectContaining({
                    op: "close",
                    item: tool.item.id,
                    exit: 0,
                    change: edit.wait.change,
                }),
            );
            expect(
                live(messages).find((item) => item.id === tool.item.id),
            ).toMatchObject({ tool: { change: edit.wait.change } });
            expect(updates).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({ mode: "auto" }),
                }),
            );
            break;
        }
        case "fault": {
            expect(agents(messages)).toContainEqual(
                expect.objectContaining({ busy: false, faulted: true }),
            );
            const wait = changes(messages).find(
                (update) =>
                    update.type === "wait" && update.wait.kind === "fault",
            );
            if (wait?.type !== "wait") throw new Error("No fault wait");
            expect(changes(messages)).toContainEqual({
                type: "wait-closed",
                id: wait.wait.id,
                how: "dismiss",
            });
            expect(ops).toContainEqual(
                expect.objectContaining({
                    op: "add",
                    item: expect.objectContaining({
                        kind: "error",
                        text: expect.stringContaining("Faux model unavailable"),
                    }),
                }),
            );
            break;
        }
        case "stop":
            expect(ops).toContainEqual(
                expect.objectContaining({
                    op: "add",
                    item: expect.objectContaining({
                        kind: "notice",
                        text: expect.stringContaining("Stopped:"),
                    }),
                }),
            );
            expect(
                live(messages).some(
                    (item) => item.text === "This answer must never arrive.",
                ),
            ).toBe(false);
            expect(agents(messages).at(-1)?.busy).toBe(false);
            break;
        case "rewind": {
            const reply = messages.findIndex(
                (message) =>
                    message.type === "reply" && message.kind === "rewound",
            );
            expect(reply).toBeGreaterThan(0);
            const reset = messages.findIndex(
                (message) =>
                    message.type === "turn" &&
                    message.ops.some((op) => op.op === "reset"),
            );
            expect(reply).toBeGreaterThanOrEqual(0);
            expect(reset).toBeGreaterThanOrEqual(0);
            expect(messages[reset + 1]).toMatchObject({
                type: "transcript",
                items: [
                    expect.objectContaining({ text: "Keep this line." }),
                    expect.objectContaining({ text: "First answer." }),
                ],
            });
            break;
        }
        case "new-and-resume":
            expect(sent).toContainEqual(
                expect.objectContaining({ type: "say", text: "/new" }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "read",
                    history: expect.arrayContaining([
                        expect.objectContaining({
                            number: 1,
                            current: false,
                            messages: 2,
                        }),
                    ]),
                }),
            );
            expect(sent).toContainEqual(
                expect.objectContaining({ type: "say", text: "/resume 1" }),
            );
            expect(live(messages).map((item) => item.text)).toEqual([
                "Remember our first conversation.",
                "The first conversation.",
                "Went back to an earlier session.",
            ]);
            break;
        case "paging": {
            const pages = messages.filter(
                (message) => message.type === "transcript",
            );
            expect(pages.map((page) => page.items.length)).toEqual([40, 6]);
            const [latest, older] = pages;
            expect(latest?.items[0]?.text).toBe("Line 4.");
            expect(latest?.earlier).toBe(latest?.items[0]?.id);
            expect(sent).toContainEqual(
                expect.objectContaining({
                    type: "open",
                    before: latest?.earlier,
                }),
            );
            expect(older?.earlier).toBeNull();
            const all = [...(older?.items ?? []), ...(latest?.items ?? [])];
            expect(new Set(all.map((item) => item.id)).size).toBe(46);
            expect(all.map((item) => item.text)).toEqual(
                Array.from({ length: 23 }, (_, i) => [
                    `Line ${i + 1}.`,
                    `Answer ${i + 1}.`,
                ]).flat(),
            );
            break;
        }
        case "reads-and-edits":
            for (const key of ["prompt", "papers", "charter", "models"])
                expect(
                    messages.some(
                        (message) => message.type === "read" && key in message,
                    ),
                ).toBe(true);
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({
                        limits: expect.objectContaining({
                            budget: expect.objectContaining({ cost: 2 }),
                        }),
                    }),
                }),
            );
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({
                        limits: expect.objectContaining({
                            mostAgents: 4,
                            budget: expect.objectContaining({ cost: 3 }),
                        }),
                    }),
                }),
            );
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "settings",
                    settings: expect.objectContaining({ allowNet: true }),
                }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "reply",
                    kind: "error",
                    text: "budget.cost: a cost is a number of zero or more",
                }),
            );
            expect(messages.at(-1)).toMatchObject({
                type: "read",
                charter: { own: true, text: "Serve the chief with care." },
            });
            break;
        case "login-key":
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "login",
                    step: { prompt: "Enter OpenAI API key", secret: true },
                }),
            );
            expect(sent).toContainEqual(
                expect.objectContaining({
                    type: "login",
                    reply: expect.objectContaining({ value: "[REDACTED]" }),
                }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "login",
                    step: { done: "openai" },
                }),
            );
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "providers",
                    providers: expect.arrayContaining([
                        expect.objectContaining({
                            id: "openai",
                            configured: true,
                        }),
                    ]),
                }),
            );
            expect(JSON.stringify(frames)).not.toContain(
                "sk-fake-frames-not-a-real-key",
            );
            break;
        case "budget-stop":
            expect(live(messages).at(-1)).toMatchObject({
                kind: "notice",
                text: expect.stringMatching(/budget.*\$0\.01.*chief/),
            });
            expect(agents(messages).at(-1)?.busy).toBe(false);
            expect(
                live(messages).some(
                    (item) => item.text === "This answer must never arrive.",
                ),
            ).toBe(false);
            break;
    }
}
