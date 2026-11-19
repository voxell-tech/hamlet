/**
 * The realm keeps a marker per sender, id, action and digest, so a rerun
 * sends the same call and the realm answers what it recorded. All are
 * `replay: "safe"`.
 */

import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    defineExtension,
    defineTool,
    type Extension,
    GenerationTask,
    hook,
    type ToolExecutionApi,
    type ToolRegistration,
} from "@earendil-works/pi-durable";
import { inParts } from "./branch.ts";
import { hasWordsOrCall } from "./compaction.ts";
import { LOOKED } from "./filesystem.ts";
import {
    type Named,
    type OfferAnswer,
    type Piece,
    type ProfileId,
    REALM,
    type Revision,
    type SocietyActs,
    type SocietyTool,
    type Tier,
    type ToolName,
    U32_MAX,
} from "./messages.ts";
import {
    type ActOf,
    callOf,
    callOut,
    failed,
    type Granted,
    libraryCalls,
    type Peers,
    words,
} from "./turn.ts";

type Invalid = { readonly invalid: string };

/** The context an act reads beside the model's arguments. */
interface Calling {
    readonly peers: Peers;
    readonly api: Pick<ToolExecutionApi, "taskId">;
}

interface Definition<N extends SocietyTool> {
    readonly name: string;
    readonly description: string;
    /** pi-durable checks a call against it first. */
    readonly parameters: object;
    readonly act: (
        args: never,
        calling: Calling,
    ) => ActOf<N> | Invalid | Promise<ActOf<N> | Invalid>;
    /**
     * Absent for an act the realm keeps for a chief: its answer is always
     * a refusal, and what the chief rules arrives later as a notice.
     */
    readonly said?: (
        answer: Granted<SocietyActs[N]["answer"]>,
        args: never,
    ) => string;
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

/** As `parse_scholar` of `crates/hamlet_server/src/society/addresses.rs`. */
function profileNamed(text: string): ProfileId | Invalid {
    const given = text.trim();
    return (
        numberOf("scholar", given) ?? {
            invalid: `\`${given}\` is not an address`,
        }
    );
}

function profileOf(text: string | undefined): ProfileId | Invalid | undefined {
    const given = present(text);
    return given === undefined ? undefined : profileNamed(given);
}

/**
 * A version or a commit as today's `version_of` names one
 * (`crates/hamlet_server/src/calls/society/scholars.rs:55-71`): `main`, a
 * commit's 40 hex digits, or a version's name. Today resolves any name git
 * does, a branch's or a short commit's too; a revision names neither.
 */
function revisionOf(text: string): Revision {
    if (text === "main") return { type: "main" };
    if (/^[0-9a-f]{40}$/.test(text)) return { type: "commit", commit: text };
    return { type: "version", version: text };
}

/** As `crates/hamlet_server/src/calls/society/library.rs:131-139`. */
const NOT_A_SKILL =
    "`skill` is its folder under skills/, in letters, digits, hyphens and underscores.";

/**
 * Offers the files of `skills/<skill>/` to the library under the tool's call,
 * so a rerun offers the same parts.
 */
async function offered(
    { peers, api }: Calling,
    skill: string,
    reason: string,
): Promise<OfferAnswer | { invalid: string }> {
    const folder = `skills/${skill}`;
    const walked = await peers.files.walk(folder, BACKGROUND_CONTEXT);
    // As `skill_files` (`crates/hamlet_server/src/calls/society/library.rs:252-257`).
    if (!walked.ok || walked.value.files.length === 0) {
        return { invalid: `no skill is named \`${skill}\`` };
    }
    if (walked.value.capped) {
        return {
            invalid: `\`${skill}\` holds more files than the agent can list, ${LOOKED}, so it was not offered.`,
        };
    }
    const files = [];
    for (const path of walked.value.files.sort()) {
        const read = await peers.files.read(path, BACKGROUND_CONTEXT);
        if (!read.ok) throw read.error;
        files.push({ path: path.slice(folder.length + 1), bytes: read.value });
    }
    const parts = inParts(files);
    const call = callOf(api);
    let answer: OfferAnswer = { type: "more" };
    for (const [part, changes] of parts.entries()) {
        answer = await libraryCalls(peers).offer({
            call,
            skill,
            reason,
            part,
            last: part === parts.length - 1,
            files: changes.flatMap(({ type, ...piece }) =>
                type === "write" ? [piece as Piece] : [],
            ),
        });
        if (answer.type === "refused") return answer;
    }
    return answer;
}

/** `text` before and after the first `separator`, if it holds one. */
function splitOnce(
    text: string | undefined,
    separator: string,
): [string | undefined, string | undefined] {
    if (text === undefined) return [undefined, undefined];
    const at = text.indexOf(separator);
    return at < 0
        ? [text, undefined]
        : [text.slice(0, at), text.slice(at + separator.length)];
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

/** A society tool by its act, as `ToolName` names it, and any other as given. */
function toolNamesOf(given: readonly string[] | undefined): ToolName[] {
    return (given ?? []).map((name) => ({ name: ACT_OF.get(name) ?? name }));
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
        act: (args: { to: string; reason?: string }) => {
            const why = present(args.reason);
            return withNamed({ to: args.to }, ({ to }) => ({
                to,
                ...(why === undefined ? {} : { why }),
            }));
        },
        said: () => "Your lead has the request.",
    },
    create_profile: {
        name: "create_scholar",
        description:
            "Make a new scholar of the city, to hire next. Without a character it starts from the realm's own. A scholar you make may wait at the harbor until it is let into the city; its hires work all the same.",
        parameters: object(
            {
                name: string(
                    "A short name, one line with no colon. It is a label and may repeat.",
                ),
                character: string("Who the scholar is and how it works."),
            },
            ["name"],
        ),
        act: (args: { name: string; character?: string }) => {
            const character = present(args.character);
            return {
                name: args.name.trim(),
                ...(character === undefined ? {} : { character }),
            };
        },
        // The realm keeps no harbor: a scholar it makes lives in the city.
        said: (answer) =>
            `The scholar ${answer.label} lives in the city; hire it next.`,
    },
    read_profile: {
        name: "read_scholar",
        description:
            "List a scholar's files, or read one, at its `main` or a version.",
        parameters: object(
            {
                scholar: string("The scholar's address, `scholar#<id>`."),
                path: string("A file to read."),
                version: string("A version or commit."),
            },
            ["scholar"],
        ),
        act: (args: { scholar: string; path?: string; version?: string }) => {
            const profile = profileNamed(args.scholar);
            if (typeof profile === "object") return profile;
            const path = present(args.path);
            const version = present(args.version);
            return {
                profile,
                ...(version === undefined ? {} : { at: revisionOf(version) }),
                ...(path === undefined ? {} : { path }),
            };
        },
        // As `read_scholar` (`crates/hamlet_server/src/calls/society/scholars.rs:94-113`).
        said: (answer, args: { scholar: string }) => {
            if (answer.type === "text") {
                return answer.cut ? `${answer.text}\n[cut here]` : answer.text;
            }
            return answer.paths.length === 0
                ? `${args.scholar.trim()} has no files.`
                : answer.paths.map(({ path }) => `- ${path}`).join("\n");
        },
    },
    edit_profile: {
        name: "edit_scholar",
        description:
            "Write a file of a scholar's `main`; a chief approves it first.",
        parameters: object(
            {
                scholar: string("The scholar's address, `scholar#<id>`."),
                path: string("The file."),
                content: string("Its new text; empty removes it."),
                message: string("What the change does."),
            },
            ["scholar", "path", "content", "message"],
        ),
        act: (args: {
            scholar: string;
            path: string;
            content: string;
            message?: string;
        }) => {
            const profile = profileNamed(args.scholar);
            if (typeof profile === "object") return profile;
            const message = present(args.message);
            return {
                profile,
                path: args.path.trim(),
                text: args.content,
                ...(message === undefined ? {} : { message }),
            };
        },
    },
    tag: {
        name: "tag",
        description:
            "Name the scholar's `main` as a version; a chief approves it first.",
        parameters: object(
            {
                scholar: string("The scholar's address, `scholar#<id>`."),
                tag: string("The version's name."),
                note: string("What it holds."),
            },
            ["scholar", "tag"],
        ),
        act: (args: { scholar: string; tag: string; note?: string }) => {
            const profile = profileNamed(args.scholar);
            if (typeof profile === "object") return profile;
            const note = present(args.note);
            return {
                profile,
                version: args.tag.trim(),
                ...(note === undefined ? {} : { note }),
            };
        },
    },
    revert: {
        name: "revert",
        description:
            "Undo a commit of a scholar's `main`; a chief approves it first.",
        parameters: object(
            {
                scholar: string("The scholar's address, `scholar#<id>`."),
                commit: string("The commit or version to undo."),
                reason,
            },
            ["scholar", "commit"],
        ),
        act: (args: { scholar: string; commit: string; reason?: string }) => {
            const profile = profileNamed(args.scholar);
            if (typeof profile === "object") return profile;
            const why = present(args.reason);
            return {
                profile,
                what: revisionOf(present(args.commit) ?? "main"),
                ...(why === undefined ? {} : { reason: why }),
            };
        },
    },
    ask_librarian: {
        name: "ask_librarian",
        description:
            "Ask the librarian which skills in the library help with something.",
        parameters: object({ question: string("What you need a skill for.") }, [
            "question",
        ]),
        act: (args: { question: string }) => {
            const question = present(args.question);
            // As `crates/hamlet_server/src/calls/society/library.rs:81-84`.
            return question === undefined
                ? { invalid: "Say what you need a skill for." }
                : { question };
        },
        said: () => "The librarian is looking.",
    },
    submit_skill: {
        name: "submit_skill",
        description:
            "Offer a skill from your skills/ folder to the library, where every agent can read it. The librarian judges it.",
        parameters: object(
            {
                skill: string("The skill's folder name under skills/."),
                reason,
            },
            ["skill"],
        ),
        act: async (
            args: { skill: string; reason?: string },
            calling: Calling,
        ) => {
            const skill = args.skill.trim();
            if (!/^[A-Za-z0-9_-]+$/.test(skill))
                return { invalid: NOT_A_SKILL };
            const reason = present(args.reason);
            const answer = await offered(calling, skill, reason ?? "");
            if ("invalid" in answer) return answer;
            if (answer.type === "refused") return { invalid: answer.words };
            if (answer.type === "more") {
                throw new Error(
                    "the library kept no offer after the last part",
                );
            }
            return {
                offer: answer.offer,
                skill,
                ...(reason === undefined ? {} : { reason }),
            };
        },
        said: () => "The librarian is judging it.",
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
            // As `scholar_pinned` (`crates/hamlet_server/src/commands.rs:1484-1497`).
            const [pinned, version] = splitOnce(present(args.scholar), "@");
            const profile = profileOf(pinned);
            if (typeof profile === "object") return profile;
            const hamlet = present(args.hamlet);
            return {
                purpose: args.purpose,
                ...(hamlet === undefined ? {} : { hamlet }),
                ...(profile === undefined ? {} : { profile }),
                ...(version === undefined ? {} : { version }),
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
            const off = toolNamesOf(args.off);
            const on = toolNamesOf(args.on);
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

/** The name the model calls a tool by, for a `ToolName`. */
export function calledAs({ name }: ToolName): string {
    return Object.hasOwn(TOOLS, name) ? TOOLS[name as SocietyTool].name : name;
}

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
            const made = await definition.act(args as never, { peers, api });
            if ("invalid" in made) return failed(made.invalid);
            return callOut(
                peers,
                api,
                context,
                REALM,
                act as SocietyTool,
                // A name that stands for any act loses the row's pairing, so
                // these casts restate what `TOOLS` already checked.
                made as never,
                (answer) => {
                    if (definition.said === undefined) {
                        throw new Error(
                            `the realm granted ${definition.name}, which it keeps for a chief`,
                        );
                    }
                    return words(
                        definition.said(answer as never, args as never),
                    );
                },
                definition.name,
            );
        },
    });
}

/** As today, a society tool is offered whatever a lead turned off. */
export function society(peers: Peers): Extension {
    return defineExtension({
        name: "hamlet.society",
        tools: (Object.keys(TOOLS) as SocietyTool[]).map((act) =>
            societyTool(peers, act),
        ),
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
        if (input.question === undefined || !hasWordsOrCall(answer))
            return undefined;
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
