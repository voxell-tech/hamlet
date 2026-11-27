import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { AgentInput } from "../src/agent/messages/agent.ts";
import type { SocietyActs, SocietyTool } from "../src/agent/messages/realm.ts";
import {
    fakeCalls,
    from,
    line,
    named,
    shown,
    toolCall,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context, fixture } from "./testing.ts";

export async function schemas(
    agent: Woken,
): Promise<{ name: string; description: string; parameters: unknown }[]> {
    const harness = await agent.harness();
    const root = await harness.root(context);
    return (await root.agent(context)).tools.map((tool) => ({
        name: tool.name,
        description: tool.description,
        parameters: JSON.parse(JSON.stringify(tool.parameters)),
    }));
}

export async function offered(agent: Woken): Promise<string[]> {
    return (await schemas(agent)).map((tool) => tool.name);
}

export function acts(
    sent: Parameters<typeof named>[0],
    name: string,
): unknown[] {
    return named(sent, name).map(
        (call) => (call.message as { act: unknown }).act,
    );
}

const ADDRESS =
    "An agent's address: `hand`, `mayor`, `mayor@<hamlet id>`, `hire#<id>` or `worker#<id>`.";
const REASON = {
    type: "string",
    description: "Why, in a sentence, if it helps; the record keeps it.",
};
const PATHS = {
    type: "array",
    items: { type: "string" },
    description:
        "Files or folders to send with it, in your lodge or your hamlet's folder. They are copied into the receiver's inbox, and the message says where. Links are not followed. A chief approves a send of more than 32 MiB or 1000 files.",
};
export const address = { type: "string", description: ADDRESS };
export const text = (description: string) => ({ type: "string", description });
const PROFILE_ADDRESS = text("The profile's address, `profile#<id>`.");

/** The society tools as a model is offered them, typed out by hand. */
export const OFFERED = [
    {
        name: "tell",
        description:
            "Send a letter to an agent you have a road to. It is queued for them; you do not wait for an answer.",
        parameters: {
            type: "object",
            properties: {
                to: address,
                text: text("The letter."),
                paths: PATHS,
            },
            required: ["to", "text"],
        },
    },
    {
        name: "ask",
        description:
            "Ask an agent you have a road to a question. The call ends at once, and the answer comes to you as a new input. Ask only what you need; a question to someone waiting on you is refused.",
        parameters: {
            type: "object",
            properties: {
                to: address,
                question: text("The question."),
                paths: PATHS,
            },
            required: ["to", "question"],
        },
    },
    {
        name: "request_road",
        description:
            "Ask your lead for a road to an agent of your hamlet you need to reach.",
        parameters: {
            type: "object",
            properties: { to: address, reason: REASON },
            required: ["to"],
        },
    },
    {
        name: "found_hamlet",
        description:
            "Found a hamlet with a profile as its mayor, or a blank agent when `profile` is left out.",
        parameters: {
            type: "object",
            properties: {
                name: text(
                    "The name it is shown by: one line, up to 40 characters, no colon. Its id is made for it.",
                ),
                profile: text(
                    "The address, `profile#<id>`, of the profile its mayor is a hire of.",
                ),
                purpose: text("What the hamlet is for."),
            },
            required: ["name", "purpose"],
        },
    },
    {
        name: "seat_mayor",
        description: "Seat a hire as its hamlet's mayor.",
        parameters: {
            type: "object",
            properties: { hire: address, reason: REASON },
            required: ["hire"],
        },
    },
    {
        name: "hire",
        description:
            "Hire a profile into your hamlet under you, or under a hire of your tree. Leave out `profile` to hire a blank agent instead, which starts from the realm's character and the library's skills and is not a profile of the city.",
        parameters: {
            type: "object",
            properties: {
                profile: text(
                    "The profile's address, `profile#<id>`, with `@<version>` to pin a version.",
                ),
                name: text("A blank agent's name, in place of `profile`."),
                purpose: text("What the hire is for."),
                under: text("The hire it answers to; you when left out."),
                hamlet: text("The hamlet, for the Hand."),
            },
            required: ["purpose"],
        },
    },
    {
        name: "end_hire",
        description: "End a hire under you. Its work stays in its profile.",
        parameters: {
            type: "object",
            properties: { hire: address, reason: REASON },
            required: ["hire"],
        },
    },
    {
        name: "merge_hires",
        description:
            "Merge one hire under you into another: the kept hire takes what the other learned, its roads and its hires.",
        parameters: {
            type: "object",
            properties: { absorbed: address, into: address, reason: REASON },
            required: ["absorbed", "into"],
        },
    },
    {
        name: "remove_hamlet",
        description: "Remove a hamlet; a chief approves it first.",
        parameters: {
            type: "object",
            properties: { hamlet: text("The hamlet's id."), reason: REASON },
            required: ["hamlet"],
        },
    },
    {
        name: "lay_road",
        description:
            "Lay a road between two hires under you, so they can write and ask each other.",
        parameters: {
            type: "object",
            properties: { a: address, b: address, reason: REASON },
            required: ["a", "b"],
        },
    },
    {
        name: "take_up_road",
        description: "Take up a road you laid between two hires under you.",
        parameters: {
            type: "object",
            properties: { a: address, b: address, reason: REASON },
            required: ["a", "b"],
        },
    },
    {
        name: "set_tools",
        description: "Turn built-in tools off or back on for a hire under you.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                off: { type: "array", items: { type: "string" } },
                on: { type: "array", items: { type: "string" } },
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "submit_skill",
        description:
            "Offer a skill from your skills/ folder to the library, where every agent can read it. The librarian judges it.",
        parameters: {
            type: "object",
            properties: {
                skill: text("The skill's folder name under skills/."),
                reason: REASON,
            },
            required: ["skill"],
        },
    },
    {
        name: "ask_librarian",
        description:
            "Ask the librarian which skills in the library help with something.",
        parameters: {
            type: "object",
            properties: { question: text("What you need a skill for.") },
            required: ["question"],
        },
    },
    {
        name: "create_profile",
        description:
            "Make a new profile of the city, to hire next. Without a character it starts from the realm's own. A profile you make may wait at the harbor until it is let into the city; its hires work all the same.",
        parameters: {
            type: "object",
            properties: {
                name: text(
                    "A short name, one line with no colon. It is a label and may repeat.",
                ),
                character: text("Who the profile is and how it works."),
            },
            required: ["name"],
        },
    },
    {
        name: "read_profile",
        description:
            "List a profile's files, or read one, at its `main` or a version.",
        parameters: {
            type: "object",
            properties: {
                profile: PROFILE_ADDRESS,
                path: text("A file to read."),
                version: text("A version or commit."),
            },
            required: ["profile"],
        },
    },
    {
        name: "edit_profile",
        description:
            "Write a file of a profile's `main`; a chief approves it first.",
        parameters: {
            type: "object",
            properties: {
                profile: PROFILE_ADDRESS,
                path: text("The file."),
                content: text("Its new text; empty removes it."),
                message: text("What the change does."),
            },
            required: ["profile", "path", "content", "message"],
        },
    },
    {
        name: "tag",
        description:
            "Name the profile's `main` as a version; a chief approves it first.",
        parameters: {
            type: "object",
            properties: {
                profile: PROFILE_ADDRESS,
                tag: text("The version's name."),
                note: text("What it holds."),
            },
            required: ["profile", "tag"],
        },
    },
    {
        name: "revert",
        description:
            "Undo a commit of a profile's `main`; a chief approves it first.",
        parameters: {
            type: "object",
            properties: {
                profile: PROFILE_ADDRESS,
                commit: text("The commit or version to undo."),
                reason: REASON,
            },
            required: ["profile", "commit"],
        },
    },
    {
        name: "ask_chief",
        description:
            "Ask the chief a question you cannot go on without. The call ends at once, and the chief's answer comes to you as a new input. Ask only when no agent and no file can tell you. Use object to argue against an instruction.",
        parameters: {
            type: "object",
            properties: {
                message: { type: "string" },
                questions: {
                    type: "array",
                    minItems: 1,
                    maxItems: 10,
                    items: {
                        type: "object",
                        properties: {
                            question: { type: "string" },
                            description: { type: "string" },
                            options: {
                                type: "array",
                                maxItems: 6,
                                items: {
                                    anyOf: [
                                        { type: "string" },
                                        {
                                            type: "object",
                                            properties: {
                                                title: { type: "string" },
                                                description: { type: "string" },
                                            },
                                            required: ["title"],
                                        },
                                    ],
                                },
                            },
                            text_input: {
                                type: "boolean",
                                description:
                                    "Allow custom words. Defaults to true.",
                            },
                        },
                        required: ["question"],
                    },
                },
            },
            required: ["questions"],
        },
    },
    {
        name: "object",
        description:
            "Object to the chief's instruction you are working on, with your reason. The chief rules: go on, or withdraw it.",
        parameters: {
            type: "object",
            properties: {
                reason: text("Why the instruction should not be done."),
            },
            required: ["reason"],
        },
    },
    {
        name: "start_worker",
        description:
            "Start a worker on a task: a helper with your tools that works in a scratch folder and where you work, then answers once. The call ends at once, and the worker's answer comes to you as a new input.",
        parameters: {
            type: "object",
            properties: {
                task: text("What the worker is to do, and what to answer."),
                tools: {
                    type: "array",
                    items: { type: "string" },
                    description:
                        "Your built-in tools it may use; all of them when left out.",
                },
            },
            required: ["task"],
        },
    },
    {
        name: "read_budget",
        description:
            "Read what you may spend: your hamlet's or the realm's spend this period against its budget, your own, your allowance and your task's cap. A call over any of them is refused.",
        parameters: { type: "object", properties: {}, required: [] },
    },
    {
        name: "set_model",
        description:
            "Choose the model a hire under you runs; leave out `model` for its hamlet's.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                model: text("The model, as `provider::id`."),
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "set_allowance",
        description:
            "Give a hire under you an allowance in dollars for the budget's period, or none.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                dollars: { type: "number" },
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "nickname",
        description:
            "Give a hire under you a short nickname it is shown by in the world; leave out `nickname` for its own name.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                nickname: text("At most 24 letters."),
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "agree_road",
        description:
            "As a mayor, agree a road between a hire of your hamlet and one of another. It opens once both mayors agree.",
        parameters: {
            type: "object",
            properties: { mine: address, theirs: address, reason: REASON },
            required: ["mine", "theirs"],
        },
    },
    {
        name: "read_record",
        description: "Read a hamlet's record: every decision with its reason.",
        parameters: {
            type: "object",
            properties: { hamlet: text("The hamlet's id.") },
            required: ["hamlet"],
        },
    },
];

/** `npm run check` fails here when an act gains a field. */
const ACT_FIELDS = {
    found: ["name", "mayor", "tier", "purpose"],
    hire: ["hamlet", "under", "profile", "version", "purpose", "name"],
    end: ["hire", "why"],
    seat: ["hire", "why"],
    tell: ["to", "text", "paths"],
    ask: ["to", "text", "conversation", "paths"],
    lay_road: ["a", "b", "why"],
    take_up_road: ["a", "b", "why"],
    request_road: ["to", "why"],
    set_tools: ["hire", "off", "on", "why"],
    ask_chief: ["task", "conversation", "message", "questions"],
    object: ["task", "conversation", "reason"],
    start_worker: ["brief", "tools", "task", "hamlet"],
    set_allowance: ["hire", "micros", "why"],
    read_budget: ["task"],
    agree_road: ["mine", "theirs", "why"],
    set_nickname: ["hire", "name", "why"],
    set_model: ["of", "model", "effort", "fast", "why"],
    read_record: ["hamlet"],
    create_profile: ["name", "character"],
    read_profile: ["profile", "at", "path"],
    edit_profile: ["profile", "path", "text", "message"],
    merge_hires: ["absorbed", "into", "why"],
    remove_hamlet: ["hamlet", "why"],
    tag: ["profile", "version", "note"],
    revert: ["profile", "what", "reason"],
    ask_librarian: ["question"],
    submit_skill: ["offer", "skill", "reason"],
} as const satisfies {
    readonly [N in SocietyTool]: readonly (keyof SocietyActs[N]["act"])[];
};
export type Same<A, B> = [A] extends [B]
    ? [B] extends [A]
        ? true
        : false
    : false;
const _everyField: {
    readonly [N in SocietyTool]: Same<
        keyof SocietyActs[N]["act"],
        (typeof ACT_FIELDS)[N][number]
    >;
} = {
    found: true,
    hire: true,
    end: true,
    seat: true,
    tell: true,
    ask: true,
    lay_road: true,
    take_up_road: true,
    request_road: true,
    set_tools: true,
    ask_chief: true,
    object: true,
    start_worker: true,
    set_allowance: true,
    read_budget: true,
    agree_road: true,
    set_nickname: true,
    set_model: true,
    read_record: true,
    create_profile: true,
    read_profile: true,
    edit_profile: true,
    merge_hires: true,
    remove_hamlet: true,
    tag: true,
    revert: true,
    ask_librarian: true,
    submit_skill: true,
};

export const HIRE: AgentInput = {
    ...(fixture("agent/hire.input") as AgentInput),
    copies: {},
};

/** A hire with no lead, a mayor, at `agrees_roads`. */
export function mayor(agrees_roads: boolean): AgentInput {
    const { lead: _, ...place } = HIRE.place;
    return { ...HIRE, place: { ...place, agrees_roads } };
}

/** A mayor while another hamlet lives, offered every tool a hire is. */
export const MAYOR = mayor(true);

/**
 * How long the first `tell` waits for the second to reach the realm before
 * the test reads them as run in turn. Run at once, the second reaches it in
 * about 70 ms.
 */
export const OVERLAP_MS = 500;

/**
 * The society tools by kind: every agent's in a chief's chat, a lead's, and
 * the Hand's own; a hire's road tools.
 */
export const EVERYONE = [
    "tell",
    "ask",
    "start_worker",
    "submit_skill",
    "ask_librarian",
    "ask_chief",
    "read_budget",
    "object",
];
export const LEADS = [
    "create_profile",
    "hire",
    "end_hire",
    "merge_hires",
    "set_model",
    "set_tools",
    "set_allowance",
    "nickname",
    "lay_road",
    "take_up_road",
];
export const SOCIETY = new Set(OFFERED.map((tool) => tool.name));
export const OFFERED_TO_THE_HAND = new Set([
    ...EVERYONE,
    ...LEADS,
    "found_hamlet",
    "seat_mayor",
    "remove_hamlet",
    "read_record",
    "read_profile",
    "edit_profile",
    "tag",
    "revert",
]);

export function queue<T>(...answers: T[]): () => T {
    return () => answers.shift() as T;
}

export async function seatAndHire(
    t: Parameters<typeof wakeAgent>[0],
    hires: string[],
    profiles: string[] = [],
): Promise<{ seat: unknown[]; hire: unknown[]; results: string[] }> {
    const { calls, sent } = fakeCalls({
        realm: {
            seat: () => ({ type: "done", agent: shown(22), hamlet: "h" }),
            hire: () => ({ type: "hired", agent: shown(22) }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            ...hires.map((hire, n) =>
                toolCall("seat_mayor", { hire }, `seat${n}`),
            ),
            ...profiles.map((profile, n) =>
                toolCall("hire", { profile, purpose: "Dig" }, `hire${n}`),
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Seat."));
    await agent.idle();
    const results = (await agent.entries())
        .filter((entry) => entry.kind === "pi.tool-result")
        .map((entry) => entry.text);
    await agent.stop();
    return { seat: acts(sent, "seat"), hire: acts(sent, "hire"), results };
}

export const seated = (agent: number) => ({ hire: { type: "agent", agent } });

export const COMMIT = "ab".repeat(20);
