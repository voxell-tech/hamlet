/** The society tools: their names, descriptions, parameters and words. */

import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { ToolExecutionApi } from "@earendil-works/pi-durable";
import { inParts } from "./branch.ts";
import { money } from "./chat-items.ts";
import { LOOKED } from "./filesystem.ts";
import type { OfferAnswer } from "./messages/library.ts";
import type { Piece, Revision } from "./messages/profile.ts";
import type {
    ChiefQuestion,
    Named,
    SocietyActs,
    SocietyTool,
    Spend,
    Tier,
    ToolName,
} from "./messages/realm.ts";
import { type ProfileId, type Refused, U32_MAX } from "./messages/shared.ts";
import { type Folder, reached } from "./own-files.ts";
import {
    type ActOf,
    callOf,
    type Granted,
    libraryCalls,
    type Peers,
} from "./turn.ts";

export type Invalid = { readonly invalid: string };

/** The context an act reads beside the model's arguments. */
interface Calling {
    readonly peers: Peers;
    readonly api: Pick<ToolExecutionApi, "taskId">;
    readonly folder: Folder;
}

export interface Definition<N extends SocietyTool> {
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
    /** The words for a refusal given as a plain result. */
    readonly plain?: (why: Refused["why"]) => string | undefined;
}

const ADDRESS =
    "An agent's address: `hand`, `mayor`, `mayor@<hamlet id>`, `hire#<id>` or `worker#<id>`.";
const PATHS = {
    type: "array",
    items: { type: "string" },
    description:
        "Files or folders to send with it, in your lodge or your hamlet's folder. They are copied into the receiver's inbox, and the message says where. Links are not followed. A chief approves a send of more than 32 MiB or 1000 files.",
};
/** The tier of a hamlet an agent founds. */
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

/** The words for a call a chief must allow first. */
export const WAITING =
    "This call has ended; the chief's answer comes to you as a new input.";

export const OBJECTING = "The chief is considering an objection.";

/** The words of a call an answer makes beside its `object` call, which does not run. */
export const OBJECTED =
    "The chief is considering an objection, so this call did not run. Make it again if the chief says to go on.";

/** The limits of an `ask_chief` question. */
const MOST_QUESTIONS = 10;
const MOST_OPTIONS = 6;

/** The model's `reason`, as an act's `why`. */
function why(reason: string | undefined): { why?: string } {
    const given = present(reason);
    return given === undefined ? {} : { why: given };
}

/** Read as an `i64`, or `undefined` when the act's `u32` cannot carry it. */
function numberOf(
    kind: "hire" | "worker" | "profile",
    text: string,
): number | undefined {
    const found = new RegExp(`^${kind}#([+-]?)([0-9]+)$`).exec(text);
    const digits = found?.[2];
    if (digits === undefined) return undefined;
    if (found?.[1] === "-" && /[1-9]/.test(digits)) return undefined;
    const number = Number(digits);
    return number <= U32_MAX ? number : undefined;
}

/** An empty parameter reads as one left out. */
function present(text: string | undefined): string | undefined {
    const trimmed = text?.trim();
    return trimmed === "" ? undefined : trimmed;
}

function named(given: string): Named | Invalid {
    const address = given.trim();
    if (address === "hand" || address === "mayor") return { type: address };
    if (address.startsWith("mayor@")) {
        const hamlet = address.slice("mayor@".length).trim();
        return hamlet === ""
            ? { invalid: `\`${address}\` is not an address` }
            : { type: "mayor_of", hamlet };
    }
    const agent = numberOf("hire", address);
    if (agent !== undefined) return { type: "agent", agent };
    const worker = numberOf("worker", address);
    if (worker !== undefined) return { type: "worker", worker };
    return { invalid: `\`${address}\` is not an address` };
}

function profileNamed(text: string): ProfileId | Invalid {
    const given = text.trim();
    return (
        numberOf("profile", given) ?? {
            invalid: `\`${given}\` is not an address`,
        }
    );
}

function profileOf(text: string | undefined): ProfileId | Invalid | undefined {
    const given = present(text);
    return given === undefined ? undefined : profileNamed(given);
}

/**
 * A version or a commit as a call names one: `main`, a commit's 40 hex
 * digits, or a branch's or a version's name.
 */
function revisionOf(text: string): Revision {
    if (text === "main") return { type: "main" };
    if (/^[0-9a-f]{40}$/.test(text)) return { type: "commit", commit: text };
    return { type: "version", version: text };
}

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

/** The tools' names and words for the model. */
export const TOOLS: { readonly [N in SocietyTool]: Definition<N> } = {
    tell: {
        name: "tell",
        description:
            "Send a letter to an agent you have a road to. It is queued for them; you do not wait for an answer.",
        parameters: object(
            { to: address, text: string("The letter."), paths: PATHS },
            ["to", "text"],
        ),
        act: async (
            args: { to: string; text: string; paths?: string[] },
            calling,
        ) => {
            const paths = await enclosed(args.paths, calling);
            return withNamed({ to: args.to }, ({ to }) => ({
                to,
                text: args.text,
                ...paths,
            }));
        },
        said: (answer) => `Sent to ${answer.to.name}.`,
    },
    ask: {
        name: "ask",
        description:
            "Ask an agent you have a road to a question. The call ends at once, and the answer comes to you as a new input. Ask only what you need; a question to someone waiting on you is refused.",
        parameters: object(
            { to: address, question: string("The question."), paths: PATHS },
            ["to", "question"],
        ),
        act: async (
            args: { to: string; question: string; paths?: string[] },
            calling,
        ) => {
            const paths = await enclosed(args.paths, calling);
            return withNamed({ to: args.to }, ({ to }) => ({
                to,
                text: args.question,
                ...paths,
            }));
        },
        said: (answer) => `Waiting for ${answer.on.name}'s answer.`,
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
        name: "create_profile",
        description:
            "Make a new profile of the city, to hire next. Without a character it starts from the realm's own. A profile you make may wait at the harbor until it is let into the city; its hires work all the same.",
        parameters: object(
            {
                name: string(
                    "A short name, one line with no colon. It is a label and may repeat.",
                ),
                character: string("Who the profile is and how it works."),
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
        // The realm keeps no harbor: a profile it makes lives in the city.
        said: (answer) =>
            `The profile ${answer.label} lives in the city; hire it next.`,
    },
    read_profile: {
        name: "read_profile",
        description:
            "List a profile's files, or read one, at its `main` or a version.",
        parameters: object(
            {
                profile: string("The profile's address, `profile#<id>`."),
                path: string("A file to read."),
                version: string("A version or commit."),
            },
            ["profile"],
        ),
        act: (args: { profile: string; path?: string; version?: string }) => {
            const profile = profileNamed(args.profile);
            if (typeof profile === "object") return profile;
            const path = present(args.path);
            const version = present(args.version);
            return {
                profile,
                ...(version === undefined ? {} : { at: revisionOf(version) }),
                ...(path === undefined ? {} : { path }),
            };
        },
        said: (answer, args: { profile: string }) => {
            if (answer.type === "text") {
                return answer.cut ? `${answer.text}\n[cut here]` : answer.text;
            }
            return answer.paths.length === 0
                ? `${args.profile.trim()} has no files.`
                : answer.paths.map(({ path }) => `- ${path}`).join("\n");
        },
    },
    edit_profile: {
        name: "edit_profile",
        description:
            "Write a file of a profile's `main`; a chief approves it first.",
        parameters: object(
            {
                profile: string("The profile's address, `profile#<id>`."),
                path: string("The file."),
                content: string("Its new text; empty removes it."),
                message: string("What the change does."),
            },
            ["profile", "path", "content", "message"],
        ),
        act: (args: {
            profile: string;
            path: string;
            content: string;
            message?: string;
        }) => {
            const profile = profileNamed(args.profile);
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
            "Name the profile's `main` as a version; a chief approves it first.",
        parameters: object(
            {
                profile: string("The profile's address, `profile#<id>`."),
                tag: string("The version's name."),
                note: string("What it holds."),
            },
            ["profile", "tag"],
        ),
        act: (args: { profile: string; tag: string; note?: string }) => {
            const profile = profileNamed(args.profile);
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
            "Undo a commit of a profile's `main`; a chief approves it first.",
        parameters: object(
            {
                profile: string("The profile's address, `profile#<id>`."),
                commit: string("The commit or version to undo."),
                reason,
            },
            ["profile", "commit"],
        ),
        act: (args: { profile: string; commit: string; reason?: string }) => {
            const profile = profileNamed(args.profile);
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
            "Found a hamlet with a profile as its mayor, or a blank agent when `profile` is left out.",
        parameters: object(
            {
                name: string(
                    "The name it is shown by: one line, up to 40 characters, no colon. Its id is made for it.",
                ),
                profile: string(
                    "The address, `profile#<id>`, of the profile its mayor is a hire of.",
                ),
                purpose: string("What the hamlet is for."),
            },
            ["name", "purpose"],
        ),
        act: (args: { name: string; profile?: string; purpose: string }) => {
            const mayor = profileOf(args.profile);
            if (typeof mayor === "object") return mayor;
            return {
                name: args.name,
                tier: FOUNDED_TIER,
                purpose: args.purpose,
                ...(mayor === undefined ? {} : { mayor }),
            };
        },
        said: (answer, args: { name: string }) =>
            `Founded ${args.name.trim()} (\`${answer.hamlet}\`).`,
    },
    seat: {
        name: "seat_mayor",
        description: "Seat a hire as its hamlet's mayor.",
        parameters: object({ hire: address, reason }, ["hire"]),
        act: (args: { hire: string; reason?: string }) =>
            withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...why(args.reason),
            })),
        said: (answer) =>
            `${answer.agent.name} is the mayor of ${answer.hamlet} now.`,
    },
    hire: {
        name: "hire",
        description:
            "Hire a profile into your hamlet under you, or under a hire of your tree. Leave out `profile` to hire a blank agent instead, which starts from the realm's character and the library's skills and is not a profile of the city.",
        parameters: object(
            {
                profile: string(
                    "The profile's address, `profile#<id>`, with `@<version>` to pin a version.",
                ),
                name: string("A blank agent's name, in place of `profile`."),
                purpose: string("What the hire is for."),
                under: string("The hire it answers to; you when left out."),
                hamlet: string("The hamlet, for the Hand."),
            },
            ["purpose"],
        ),
        act: (args: {
            profile?: string;
            name?: string;
            purpose: string;
            under?: string;
            hamlet?: string;
        }) => {
            // The realm finds the hamlet and `under` after this checks
            // `profile` and `name`.
            const given = present(args.under);
            const under = given === undefined ? undefined : named(given);
            if (under !== undefined && "invalid" in under) return under;
            if (
                present(args.profile) !== undefined &&
                present(args.name) !== undefined
            ) {
                return {
                    invalid:
                        "Give `profile` to hire one of the city, or `name` for a blank agent, not both.",
                };
            }
            const [pinned, version] = splitOnce(present(args.profile), "@");
            const profile = profileOf(pinned);
            if (typeof profile === "object") return profile;
            const hamlet = present(args.hamlet);
            const name = present(args.name);
            return {
                purpose: args.purpose,
                ...(hamlet === undefined ? {} : { hamlet }),
                ...(name === undefined ? {} : { name }),
                ...(profile === undefined ? {} : { profile }),
                ...(version === undefined ? {} : { version }),
                ...(under === undefined ? {} : { under }),
            };
        },
        said: (answer) => `Hired ${answer.agent.name}.`,
    },
    end: {
        name: "end_hire",
        description: "End a hire under you. Its work stays in its profile.",
        parameters: object({ hire: address, reason }, ["hire"]),
        act: (args: { hire: string; reason?: string }) =>
            withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...why(args.reason),
            })),
        said: (answer) => `Ended ${answer.agent.name}.`,
    },
    merge_hires: {
        name: "merge_hires",
        description:
            "Merge one hire under you into another: the kept hire takes what the other learned, its roads and its hires.",
        parameters: object({ absorbed: address, into: address, reason }, [
            "absorbed",
            "into",
        ]),
        act: (args: { absorbed: string; into: string; reason?: string }) =>
            withNamed({ absorbed: args.absorbed, into: args.into }, (both) => ({
                ...both,
                ...why(args.reason),
            })),
        said: (answer) =>
            `Merged ${answer.absorbed.name} into ${answer.into.name}.`,
    },
    remove_hamlet: {
        name: "remove_hamlet",
        description: "Remove a hamlet; a chief approves it first.",
        parameters: object({ hamlet: string("The hamlet's id."), reason }, [
            "hamlet",
        ]),
        act: (args: { hamlet: string; reason?: string }) => ({
            hamlet: args.hamlet.trim(),
            ...why(args.reason),
        }),
    },
    lay_road: {
        name: "lay_road",
        description:
            "Lay a road between two hires under you, so they can write and ask each other.",
        parameters: object({ a: address, b: address, reason }, ["a", "b"]),
        act: (args: { a: string; b: string; reason?: string }) =>
            withNamed({ a: args.a, b: args.b }, (road) => ({
                ...road,
                ...why(args.reason),
            })),
        said: (answer) =>
            `Laid a road between ${answer.a.name} and ${answer.b.name}.`,
    },
    take_up_road: {
        name: "take_up_road",
        description: "Take up a road you laid between two hires under you.",
        parameters: object({ a: address, b: address, reason }, ["a", "b"]),
        act: (args: { a: string; b: string; reason?: string }) =>
            withNamed({ a: args.a, b: args.b }, (road) => ({
                ...road,
                ...why(args.reason),
            })),
        said: (answer) =>
            `Took up the road between ${answer.a.name} and ${answer.b.name}.`,
    },
    set_tools: {
        name: "set_tools",
        description: "Turn built-in tools off or back on for a hire under you.",
        parameters: object({ hire: address, off: names, on: names, reason }, [
            "hire",
        ]),
        act: (args: {
            hire: string;
            off?: string[];
            on?: string[];
            reason?: string;
        }) => {
            const off = toolNamesOf(args.off);
            const on = toolNamesOf(args.on);
            return withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...(off.length === 0 ? {} : { off }),
                ...(on.length === 0 ? {} : { on }),
                ...why(args.reason),
            }));
        },
        said: (answer) => `Changed the tools of ${answer.agent.name}.`,
    },
    ask_chief: {
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
                    maxItems: MOST_QUESTIONS,
                    items: {
                        type: "object",
                        properties: {
                            question: { type: "string" },
                            description: { type: "string" },
                            options: {
                                type: "array",
                                maxItems: MOST_OPTIONS,
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
        act: (args: { message?: string; questions: AskedQuestion[] }) => {
            const message = present(args.message);
            return {
                ...(message === undefined ? {} : { message }),
                questions: args.questions.map(questionOf),
            };
        },
        said: () => WAITING,
        plain: (why) => {
            if (why.type === "no_chief") {
                return "No chief oversees this turn; go on with what you know.";
            }
            if (
                why.type !== "too_many" ||
                !("most" in why) ||
                typeof why.most !== "number"
            ) {
                return undefined;
            }
            const { most } = why;
            const times = most === 1 ? "once" : `${most} times`;
            return `You have asked the chief ${times} this turn; go on with what you know.`;
        },
    },
    object: {
        name: "object",
        description:
            "Object to the chief's instruction you are working on, with your reason. The chief rules: go on, or withdraw it.",
        parameters: object(
            { reason: string("Why the instruction should not be done.") },
            ["reason"],
        ),
        act: (args: { reason: string }) => ({ reason: args.reason }),
        said: () => OBJECTING,
    },
    start_worker: {
        name: "start_worker",
        description:
            "Start a worker on a task: a helper with your tools that works in a scratch folder and where you work, then answers once. The call ends at once, and the worker's answer comes to you as a new input.",
        parameters: object(
            {
                task: string("What the worker is to do, and what to answer."),
                tools: {
                    type: "array",
                    items: { type: "string" },
                    description:
                        "Your built-in tools it may use; all of them when left out.",
                },
            },
            ["task"],
        ),
        act: (args: { task: string; tools?: string[] }) => {
            const tools = (args.tools ?? []).map((name) => ({ name }));
            return {
                brief: args.task.trim(),
                ...(tools.length === 0 ? {} : { tools }),
            };
        },
        said: (answer) =>
            `Started ${answer.worker.name}; its answer comes to you as a reply.`,
    },
    set_allowance: {
        name: "set_allowance",
        description:
            "Give a hire under you an allowance in dollars for the budget's period, or none.",
        parameters: object(
            { hire: address, dollars: { type: "number" }, reason },
            ["hire"],
        ),
        act: (args: { hire: string; dollars?: number; reason?: string }) =>
            withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...(args.dollars === undefined
                    ? {}
                    : { micros: microsOf(args.dollars) }),
                ...why(args.reason),
            })),
        said: (answer) => `Set the allowance of ${answer.agent.name}.`,
    },
    read_budget: {
        name: "read_budget",
        description:
            "Read what you may spend: your hamlet's or the realm's spend this period against its budget, your own, your allowance and your task's cap. A call over any of them is refused.",
        parameters: object({}, []),
        act: () => ({}),
        said: budgetWords,
    },
    agree_road: {
        name: "agree_road",
        description:
            "As a mayor, agree a road between a hire of your hamlet and one of another. It opens once both mayors agree.",
        parameters: object({ mine: address, theirs: address, reason }, [
            "mine",
            "theirs",
        ]),
        act: (args: { mine: string; theirs: string; reason?: string }) =>
            withNamed({ mine: args.mine, theirs: args.theirs }, (road) => ({
                ...road,
                ...why(args.reason),
            })),
        said: ({ type, mine, theirs }) =>
            type === "laid"
                ? `The road between ${mine.name} and ${theirs.name} is open.`
                : `You agree a road between ${mine.name} and ${theirs.name}; it opens once the other mayor agrees.`,
    },
    set_nickname: {
        name: "nickname",
        description:
            "Give a hire under you a short nickname it is shown by in the world; leave out `nickname` for its own name.",
        parameters: object(
            { hire: address, nickname: string("At most 24 letters."), reason },
            ["hire"],
        ),
        act: (args: { hire: string; nickname?: string; reason?: string }) => {
            const name = present(args.nickname);
            return withNamed({ hire: args.hire }, ({ hire }) => ({
                hire,
                ...(name === undefined ? {} : { name }),
                ...why(args.reason),
            }));
        },
        said: (answer, args: { nickname?: string }) => {
            const nickname = present(args.nickname);
            return nickname === undefined
                ? `${answer.agent.name} goes by its name again.`
                : `${answer.agent.name} is called ${nickname} now.`;
        },
    },
    set_model: {
        name: "set_model",
        description:
            "Choose the model a hire under you runs; leave out `model` for its hamlet's.",
        parameters: object(
            {
                hire: address,
                model: string("The model, as `provider::id`."),
                reason,
            },
            ["hire"],
        ),
        act: (args: { hire: string; model?: string; reason?: string }) => {
            const model = present(args.model);
            return withNamed({ hire: args.hire }, ({ hire }) => ({
                of: { type: "hire" as const, hire },
                ...(model === undefined ? {} : { model }),
                ...why(args.reason),
            }));
        },
        said: (answer, args: { model?: string }) =>
            `${answer.agent.name} runs ${present(args.model) ?? "its hamlet's model"} now.`,
    },
    read_record: {
        name: "read_record",
        description: "Read a hamlet's record: every decision with its reason.",
        parameters: object({ hamlet: string("The hamlet's id.") }, ["hamlet"]),
        act: (args: { hamlet: string }) => ({ hamlet: args.hamlet.trim() }),
        said: (answer) =>
            answer.lines.length === 0
                ? "Nothing is decided yet."
                : answer.lines.map(({ text }) => text).join("\n"),
    },
};

/** A question as the model asks it: an option is a title, or a title and a description. */
interface AskedQuestion {
    readonly question: string;
    readonly description?: string;
    readonly options?: readonly (
        | string
        | { readonly title: string; readonly description?: string }
    )[];
    readonly text_input?: boolean;
}

/** A question as asked: words are allowed unless turned off. */
function questionOf(asked: AskedQuestion): ChiefQuestion {
    const options = (asked.options ?? []).map((option) =>
        typeof option === "string" ? { title: option } : option,
    );
    return {
        question: asked.question,
        ...(asked.description === undefined
            ? {}
            : { description: asked.description }),
        ...(options.length === 0 ? {} : { options }),
        text_input: asked.text_input ?? true,
    };
}

/** Dollars to the nearest millionth. */
function microsOf(dollars: number): string {
    return `${Math.max(0, Math.round(dollars * 1_000_000))}`;
}

function duration(ms: number): string {
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    const minutes = `${Math.floor((seconds % 3600) / 60)}`.padStart(2, "0");
    return `${Math.floor(seconds / 3600)}h ${minutes}m`;
}

/**
 * A budget's words: the cap nearest its end, or the money spent when there
 * is none.
 */
function budgetWords(
    answer: Granted<SocietyActs["read_budget"]["answer"]>,
): string {
    const { spend } = answer;
    const whose = spend.payer.type === "hamlet" ? "Hamlet" : "Realm";
    const period = answer.period === "day" ? "today" : "this month";
    const lines = [`${whose} ${period}: ${nearest(spend)}`];
    lines.push(`Your spend this period: ${money(answer.own_micros)}.`);
    if (answer.allowance_micros !== undefined) {
        lines.push(
            `Your allowance, with what everything under you spends: ${money(answer.allowance_micros)}.`,
        );
    }
    return lines.join("\n");
}

function nearest(spend: Spend): string {
    const caps = [
        [spend.spent_micros, spend.budget_micros, money],
        [spend.spent_ms, spend.budget_ms, duration],
    ] as const;
    let shown: string | undefined;
    let most = -1;
    for (const [spent, cap, show] of caps) {
        if (cap === undefined) continue;
        // A later cap wins a tie, as Rust's `max_by` keeps the last.
        const used = spent / Math.max(cap, 1);
        if (used >= most) {
            most = used;
            shown = `${show(spent)} of ${show(cap)}`;
        }
    }
    return shown ?? money(spend.spent_micros);
}

/** The model's `paths`, resolved as the file tools resolve theirs, as the files a letter sends. */
async function enclosed(
    paths: readonly string[] | undefined,
    { peers, folder }: Calling,
): Promise<{ paths?: { path: string }[] }> {
    const given = [];
    for (const path of paths ?? []) {
        given.push({ path: (await reached(peers.files, folder, path)).path });
    }
    return given.length === 0 ? {} : { paths: given };
}

const ACT_OF: ReadonlyMap<string, SocietyTool> = new Map(
    (Object.keys(TOOLS) as SocietyTool[]).map((act) => [TOOLS[act].name, act]),
);
