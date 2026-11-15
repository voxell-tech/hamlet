/**
 * The realm keeps a marker per sender, id, action and digest, so a rerun
 * sends the same call and the realm answers what it recorded. All are
 * `replay: "safe"`.
 */
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    defineExtension,
    defineTool,
    type Extension,
    GenerationTask,
    hook,
    type ToolRegistration,
} from "@earendil-works/pi-durable";
import {
    type Named,
    type ProfileId,
    REALM,
    type SocietyActs,
    type SocietyTool,
    type Tier,
    U32_MAX,
} from "./messages.ts";
import {
    type ActOf,
    callOut,
    failed,
    type Granted,
    type Peers,
    words,
} from "./turn.ts";

type Invalid = { readonly invalid: string };

interface Definition<N extends SocietyTool> {
    readonly name: string;
    readonly description: string;
    /**
     * pi-durable checks a call against it first. A parameter no field of the
     * act carries is offered and never read.
     */
    readonly parameters: object;
    readonly act: (args: never) => ActOf<N> | Invalid;
    readonly said: (answer: Granted<SocietyActs[N]["answer"]>) => string;
}

/** Today's words for an address (`crates/hamlet_server/src/calls/society.rs`). */
const ADDRESS =
    "An agent's address: `hand`, `mayor`, `mayor@<hamlet id>`, `hire#<id>` or `worker#<id>`.";
const PATHS = {
    type: "array",
    items: { type: "string" },
    description:
        "Files or folders to send with it, in your lodge or your hamlet's folder. They are copied into the receiver's inbox, and the message says where. Links are not followed. A chief approves a send of more than 32 MiB or 1000 files.",
};
/** The tier today's server gives a hamlet an agent founds: the column's default (`crates/hamlet_store/src/schema.sql`). */
const FOUNDED_TIER: Tier = "fifty";
const string = (description: string) => ({ type: "string", description });
const address = string(ADDRESS);
const reason = string("Why, in a sentence, if it helps; the record keeps it.");
const object = (properties: object, required: readonly string[]) => ({
    type: "object",
    properties,
    required,
});
const names = { type: "array", items: { type: "string" } };

/**
 * Read as today's server reads it (an `i64`), or `undefined` when the act's
 * `u32` cannot carry it.
 */
function numberOf(
    kind: "hire" | "worker" | "scholar",
    text: string,
): number | undefined {
    const found = new RegExp(`^${kind}#([+-]?)([0-9]+)$`).exec(text);
    const digits = found?.[2];
    if (digits === undefined) return undefined;
    if (found?.[1] === "-" && /[1-9]/.test(digits)) return undefined;
    const number = Number(digits);
    return number <= U32_MAX ? number : undefined;
}

/** Today's server reads an empty parameter as one left out. */
function present(text: string | undefined): string | undefined {
    const trimmed = text?.trim();
    return trimmed === "" ? undefined : trimmed;
}

/**
 * Read as today's server reads it
 * (`crates/hamlet_server/src/society/addresses.rs`, `parse`).
 */
function named(given: string): Named | Invalid {
    const address = given.trim();
    if (address === "hand" || address === "mayor") return { type: address };
    if (address.startsWith("mayor@")) {
        const hamlet = address.slice("mayor@".length).trim();
        return hamlet === ""
            ? { invalid: `\`${address}\` is not an address` }
            : { type: "mayor_of", hamlet };
    }
    const agent = numberOf("hire", address) ?? numberOf("worker", address);
    if (agent !== undefined) return { type: "agent", agent };
    return { invalid: `\`${address}\` is not an address` };
}

/**
 * As `parse_scholar` of `crates/hamlet_server/src/society/addresses.rs`. The
 * act has no field for a pinned `@<version>`, so a pin is refused, not
 * hired at `main`.
 */
function profileOf(text: string | undefined): ProfileId | Invalid | undefined {
    const given = present(text);
    if (given === undefined) return undefined;
    return (
        numberOf("scholar", given) ?? {
            invalid: `\`${given}\` is not an address`,
        }
    );
}

function withNamed<K extends string, Act>(
    addresses: Record<K, string>,
    build: (named: Record<K, Named>) => Act,
): Act | Invalid {
    const out = {} as Record<K, Named>;
    for (const key of Object.keys(addresses) as K[]) {
        const one = named(addresses[key]);
        if ("invalid" in one) return one;
        out[key] = one;
    }
    return build(out);
}

function actsOf(given: readonly string[] | undefined): SocietyTool[] | Invalid {
    const acts: SocietyTool[] = [];
    for (const name of given ?? []) {
        const act = ACT_OF.get(name);
        if (act === undefined)
            return { invalid: `\`${name}\` is not a society tool.` };
        acts.push(act);
    }
    return acts;
}

/** Today's names and words for the model, word for word. */
const TOOLS: { readonly [N in SocietyTool]: Definition<N> } = {
    tell: {
        name: "tell",
        description:
            "Send a letter to an agent you have a road to. It is queued for them; you do not wait for an answer.",
        parameters: object(
            { to: address, text: string("The letter."), paths: PATHS },
            ["to", "text"],
        ),
        act: (args: { to: string; text: string }) =>
            withNamed({ to: args.to }, ({ to }) => ({ to, text: args.text })),
        said: () => "Sent.",
    },
    ask: {
        name: "ask",
        description:
            "Ask an agent you have a road to a question and wait for its answer. Ask only what you need; a question to someone waiting on you is refused.",
        parameters: object(
            { to: address, question: string("The question."), paths: PATHS },
            ["to", "question"],
        ),
        act: (args: { to: string; question: string }) =>
            withNamed({ to: args.to }, ({ to }) => ({
                to,
                text: args.question,
            })),
        said: () => "Asked. The answer arrives as a new input.",
    },
    request_road: {
        name: "request_road",
        description:
            "Ask your lead for a road to an agent of your hamlet you need to reach.",
        parameters: object({ to: address, reason }, ["to"]),
        act: (args: { to: string; reason?: string }) =>
            withNamed({ to: args.to }, ({ to }) => ({
                to,
                why: present(args.reason) ?? "",
            })),
        said: () => "Your lead has the request.",
    },
    found: {
        name: "found_hamlet",
        description:
            "Found a hamlet with a scholar as its mayor, or a blank agent when `scholar` is left out.",
        parameters: object(
            {
                name: string(
                    "The name it is shown by: one line, up to 40 characters, no colon. Its id is made for it.",
                ),
                scholar: string(
                    "The address, `scholar#<id>`, of the scholar its mayor is a hire of.",
                ),
                purpose: string("What the hamlet is for."),
            },
            ["name", "purpose"],
        ),
        act: (args: { name: string; scholar?: string; purpose: string }) => {
            const mayor = profileOf(args.scholar);
            if (typeof mayor === "object") return mayor;
            return {
                name: args.name,
                tier: FOUNDED_TIER,
                purpose: args.purpose,
                ...(mayor === undefined ? {} : { mayor }),
            };
        },
        said: (answer) =>
            `Founded the hamlet \`${answer.hamlet}\`. Its mayor, \`hire#${answer.mayor}\`, is still being hired.`,
    },
    seat: {
        name: "seat_mayor",
        description: "Seat a hire as its hamlet's mayor.",
        parameters: object({ hire: address, reason }, ["hire"]),
        act: (args: { hire: string }) =>
            withNamed({ hire: args.hire }, ({ hire }) => ({ hire })),
        said: () => "Seated.",
    },
    hire: {
        name: "hire",
        description:
            "Hire a scholar into your hamlet under you, or under a hire of your tree. Leave out `scholar` to hire a blank agent instead, which starts from the realm's character and the library's skills and is not a scholar of the city.",
        parameters: object(
            {
                scholar: string(
                    "The scholar's address, `scholar#<id>`, with `@<version>` to pin a version.",
                ),
                name: string("A blank agent's name, in place of `scholar`."),
                purpose: string("What the hire is for."),
                under: string("The hire it answers to; you when left out."),
                hamlet: string("The hamlet, for the Hand."),
            },
            ["purpose"],
        ),
        act: (args: {
            scholar?: string;
            name?: string;
            purpose: string;
            under?: string;
            hamlet?: string;
        }) => {
            // Today (`calls/society.rs`, `hire_tool`) finds the hamlet and
            // `under` before it checks `scholar` and `name`; here the realm
            // finds them after.
            const given = present(args.under);
            const under = given === undefined ? undefined : named(given);
            if (under !== undefined && "invalid" in under) return under;
            if (
                present(args.scholar) !== undefined &&
                present(args.name) !== undefined
            ) {
                return {
                    invalid:
                        "Give `scholar` to hire one of the city, or `name` for a blank agent, not both.",
                };
            }
            const profile = profileOf(args.scholar);
            if (typeof profile === "object") return profile;
            const hamlet = present(args.hamlet);
            return {
                purpose: args.purpose,
                ...(hamlet === undefined ? {} : { hamlet }),
                ...(profile === undefined ? {} : { profile }),
                ...(under === undefined ? {} : { under }),
            };
        },
        said: (answer) =>
            `Hired \`hire#${answer.agent}\`. It is still being hired.`,
    },
    end: {
        name: "end_hire",
        description: "End a hire under you. Its work stays in its scholar.",
        parameters: object({ hire: address, reason }, ["hire"]),
        act: (args: { hire: string; reason?: string }) => {
            const why = present(args.reason);
            return withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...(why === undefined ? {} : { why }),
            }));
        },
        said: () => "Ended.",
    },
    lay_road: {
        name: "lay_road",
        description:
            "Lay a road between two hires under you, so they can write and ask each other.",
        parameters: object({ a: address, b: address, reason }, ["a", "b"]),
        act: (args: { a: string; b: string }) =>
            withNamed({ a: args.a, b: args.b }, (road) => road),
        said: () => "The road is laid.",
    },
    take_up_road: {
        name: "take_up_road",
        description: "Take up a road you laid between two hires under you.",
        parameters: object({ a: address, b: address, reason }, ["a", "b"]),
        act: (args: { a: string; b: string }) =>
            withNamed({ a: args.a, b: args.b }, (road) => road),
        said: () => "The road is taken up.",
    },
    set_tools: {
        name: "set_tools",
        description: "Turn built-in tools off or back on for a hire under you.",
        parameters: object({ hire: address, off: names, on: names, reason }, [
            "hire",
        ]),
        act: (args: { hire: string; off?: string[]; on?: string[] }) => {
            const off = actsOf(args.off);
            if ("invalid" in off) return off;
            const on = actsOf(args.on);
            if ("invalid" in on) return on;
            return withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...(off.length === 0 ? {} : { off }),
                ...(on.length === 0 ? {} : { on }),
            }));
        },
        said: () => "Set.",
    },
};

const ACT_OF: ReadonlyMap<string, SocietyTool> = new Map(
    (Object.keys(TOOLS) as SocietyTool[]).map((act) => [TOOLS[act].name, act]),
);

function societyTool<N extends SocietyTool>(
    peers: Peers,
    act: N,
): ToolRegistration {
    const definition: Definition<N> = TOOLS[act];
    return defineTool({
        name: definition.name,
        description: definition.description,
        parameters: definition.parameters,
        replay: "safe",
        execute: async (args, api, context) => {
            const made = definition.act(args as never);
            if ("invalid" in made) return failed(made.invalid);
            return callOut(
                peers,
                api,
                context,
                REALM,
                act as SocietyTool,
                // A name that stands for any act loses the row's pairing, so
                // these two casts restate what `TOOLS` already checked.
                made as never,
                (answer) => words(definition.said(answer as never)),
                definition.name,
            );
        },
    });
}

export function society(peers: Peers, off: readonly SocietyTool[]): Extension {
    return defineExtension({
        name: "hamlet.society",
        tools: (Object.keys(TOOLS) as SocietyTool[])
            .filter((act) => !off.includes(act))
            .map((act) => societyTool(peers, act)),
        hooks: [
            hook(GenerationTask, {
                onYield: (answer, api) => reply(peers, answer, api),
            }),
        ],
    });
}

/**
 * It runs at least once: a rerun after a kill replies again, and the realm
 * keeps the first reply by the question.
 */
async function reply(
    peers: Peers,
    answer: AssistantMessage,
    api: { readonly conversationId: Parameters<Peers["turns"]["of"]>[0] },
): Promise<undefined> {
    try {
        const input = await peers.turns.of(api.conversationId);
        if (input.question === undefined) return undefined;
        const text = answer.content
            .map((block) => (block.type === "text" ? block.text : ""))
            .join("");
        const taken = await peers.reach.reply({
            question: input.question,
            text,
        });
        if (taken.type === "refused") {
            peers.report("a reply was refused", taken.words);
        }
    } catch (error) {
        peers.report("a reply was not taken", error);
    }
    return undefined;
}
