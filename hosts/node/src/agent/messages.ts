/**
 * The messages of `crates/hamlet_messages` that an agent takes or sends. A
 * field that is not set is absent, never `null`. An integer the agent
 * writes is a `u32`.
 */
import {
    arr,
    bigint,
    compile,
    int,
    lit,
    obj,
    oneOf,
    opt,
    type Schema,
    type Static,
    str,
    union,
    unknown,
} from "@earendil-works/pi-schema";
import { ActorError, Address } from "../actor/actor/mod.ts";

declare const minted: unique symbol;

type Minted<Name extends string> = string & { readonly [minted]: Name };

/** The id a call out carries, which its taker finds a repeat by. */
export type CallId = Minted<"call">;

export type ConversationRef = Minted<"conversation">;

export type RealmTask = string;

export type AgentNumber = number;
export type LetterId = number;
export type ProfileId = number;
export type HamletId = string;
export type CommandId = number;

export const U32_MAX = 2 ** 32 - 1;

export function u32(value: number): number {
    if (Number.isNaN(value) || value <= 0) return 0;
    return Math.min(Math.round(value), U32_MAX);
}

/** The taker kept nothing of the request. `words` are for the model. */
export interface Refused {
    readonly type: "refused";
    readonly why: { readonly type: string };
    readonly words: string;
}

export interface Stamped {
    readonly call: CallId;
    readonly conversation: ConversationRef;
    readonly realm_task?: RealmTask;
}

export type Program =
    | {
          readonly type: "script";
          readonly script: string;
          readonly network: boolean;
          readonly timeout_ms?: number;
      }
    | {
          readonly type: "tool";
          readonly name: string;
          /** A JSON object, as text. */
          readonly arguments: string;
      };

export interface Run extends Stamped {
    readonly program: Program;
}

export type Need = "network" | "command" | "edit";

export type Exit =
    | { readonly type: "code"; readonly code: number }
    | { readonly type: "timed_out" }
    | { readonly type: "failed"; readonly words: string }
    | { readonly type: "cut_off" }
    | { readonly type: "cancelled" };

export interface Cut {
    readonly total_bytes: number;
    readonly total_lines: number;
    readonly full_output: string;
}

export interface CommandEnd {
    readonly command: CommandId;
    readonly exit: Exit;
    readonly output: string;
    readonly cut?: Cut;
    readonly policy: string;
    readonly took_ms: number;
}

export type RunEnd =
    | ({ readonly type: "ran" } & CommandEnd)
    | {
          readonly type: "not_run";
          readonly why: { readonly type: string };
          readonly words: string;
      };

export type RunAnswer =
    | { readonly type: "ended"; readonly end: RunEnd }
    | { readonly type: "running"; readonly command: CommandId }
    | { readonly type: "waits_for_chief"; readonly needs: readonly Need[] }
    | Refused;

export interface ReadFile {
    readonly path: string;
    readonly offset?: number;
    readonly limit?: number;
}

export interface Search {
    readonly under: string;
    readonly pattern: string;
    readonly name?: string;
    readonly ignore_case: boolean;
}

export interface Find {
    readonly under: string;
    readonly name?: string;
}

export interface Note {
    readonly severity: "info" | "warn";
    readonly code: string;
    readonly message: string;
}

export interface Text {
    readonly text: string;
    readonly notes: readonly Note[];
    readonly truncation?: {
        readonly by?: "lines" | "bytes";
        readonly total_lines: number;
        readonly total_bytes: number;
        readonly output_lines: number;
        readonly output_bytes: number;
        readonly first_line_exceeds_limit: boolean;
    };
}

export type ReadAnswer =
    | ({ readonly type: "text" } & Text)
    | { readonly type: "image"; readonly mime: string; readonly data: string }
    | { readonly type: "failed"; readonly words: string }
    | Refused;

export type SearchAnswer =
    | ({ readonly type: "text" } & Text)
    | { readonly type: "failed"; readonly words: string }
    | Refused;

export interface WriteFile extends Stamped {
    readonly path: string;
    readonly content: string;
}

export interface Replace {
    readonly old: string;
    readonly new: string;
}

export interface EditFile extends Stamped {
    readonly path: string;
    readonly edits: readonly Replace[];
}

export interface Change {
    readonly path: string;
    readonly created: boolean;
    readonly hash: string;
    readonly diff?: {
        readonly diff: string;
        readonly patch: string;
        readonly first_changed_line?: number;
    };
}

export type ChangeEnd =
    | ({ readonly type: "changed" } & Change)
    | { readonly type: "failed"; readonly words: string }
    | {
          readonly type: "not_run";
          readonly why: { readonly type: string };
          readonly words: string;
      };

export type ChangeAnswer =
    | { readonly type: "ended"; readonly end: ChangeEnd }
    | { readonly type: "waits_for_chief"; readonly needs: readonly Need[] }
    | Refused;

export interface AddedTool {
    readonly name: string;
    readonly description: string;
    /** The JSON Schema of its arguments, as text. */
    readonly parameters: string;
}

export interface ToolList {
    readonly tools: readonly AddedTool[];
    readonly faults: readonly {
        readonly folder: string;
        readonly words: string;
    }[];
}

export interface WorkspaceActions {
    run(message: Run): RunAnswer;
    read_file(message: ReadFile): ReadAnswer;
    search(message: Search): SearchAnswer;
    find(message: Find): SearchAnswer;
    write_file(message: WriteFile): ChangeAnswer;
    edit_file(message: EditFile): ChangeAnswer;
    list_tools(): ToolList;
}

/** `mayor` is the office of the caller's hamlet. */
export type Named =
    | { readonly type: "hand" }
    | { readonly type: "mayor" }
    | { readonly type: "mayor_of"; readonly hamlet: HamletId }
    | { readonly type: "agent"; readonly agent: AgentNumber };

export type Tier = "fifty" | "hundred" | "two_hundred" | "five_hundred";

export interface Tool<Act> {
    readonly call: CallId;
    readonly act: Act;
}

export type Done = { readonly type: "done" } | Refused;

export type FoundAnswer =
    | {
          readonly type: "founded";
          readonly hamlet: HamletId;
          readonly mayor: AgentNumber;
      }
    | Refused;

export type HireAnswer =
    | { readonly type: "hired"; readonly agent: AgentNumber }
    | Refused;

export interface SocietyActs {
    found: {
        act: {
            readonly name: string;
            readonly mayor?: ProfileId;
            readonly tier: Tier;
            readonly purpose?: string;
        };
        answer: FoundAnswer;
    };
    hire: {
        act: {
            readonly hamlet?: HamletId;
            readonly under?: Named;
            readonly profile?: ProfileId;
            readonly purpose?: string;
        };
        answer: HireAnswer;
    };
    end: {
        act: { readonly hire: Named; readonly why?: string };
        answer: Done;
    };
    seat: { act: { readonly hire: Named }; answer: Done };
    tell: {
        act: {
            readonly to: Named;
            readonly text: string;
            readonly task?: RealmTask;
        };
        answer: Done;
    };
    ask: {
        act: {
            readonly to: Named;
            readonly text: string;
            readonly conversation: ConversationRef;
            readonly task?: RealmTask;
        };
        answer: Done;
    };
    lay_road: {
        act: { readonly a: Named; readonly b: Named };
        answer: Done;
    };
    take_up_road: {
        act: { readonly a: Named; readonly b: Named };
        answer: Done;
    };
    request_road: {
        act: {
            readonly to: Named;
            readonly why: string;
            readonly task?: RealmTask;
        };
        answer: Done;
    };
    set_tools: {
        act: {
            readonly hire: Named;
            readonly off?: readonly SocietyTool[];
            readonly on?: readonly SocietyTool[];
        };
        answer: Done;
    };
}

export type SocietyTool = keyof SocietyActs;

/** A list the compiler holds to `SocietyActs`'s keys. */
export const SOCIETY_TOOLS = Object.keys({
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
} satisfies { readonly [N in SocietyTool]: true }) as readonly SocietyTool[];

export interface Reply {
    readonly question: LetterId;
    readonly text: string;
}

export interface MaySpend {
    readonly task: RealmTask;
}

export interface Charge {
    readonly call: CallId;
    readonly task: RealmTask;
    readonly micros: number;
    readonly took_ms: number;
}

export type RealmActions = {
    readonly [N in SocietyTool]: (
        message: Tool<SocietyActs[N]["act"]>,
    ) => SocietyActs[N]["answer"];
} & {
    reply(message: Reply): Done;
    may_spend(message: MaySpend): Done;
    charge(message: Charge): null;
};

export interface Author {
    readonly agent: AgentNumber;
    readonly name?: string;
}

export interface Attached {
    readonly id: string;
    readonly at?: number;
    readonly name?: string;
    readonly size?: number;
}

export type Input =
    | {
          readonly type: "line";
          readonly text: string;
          readonly images?: readonly Attached[];
          readonly files?: readonly Attached[];
      }
    | { readonly type: "letter"; readonly from: Author; readonly text: string }
    | {
          readonly type: "question";
          readonly letter: LetterId;
          readonly from: Author;
          readonly text: string;
      }
    | {
          readonly type: "answer";
          readonly question: LetterId;
          readonly from: Author;
          readonly text: string;
          readonly conversation: string;
      };

export interface Submit {
    /** A second `submit` with the same id is the first one. */
    readonly id: string;
    readonly task?: RealmTask;
    readonly input: Input;
}

export type Outcome =
    | { readonly type: "run"; readonly end: RunEnd }
    | { readonly type: "change"; readonly end: ChangeEnd };

export interface Finished {
    readonly call: string;
    readonly outcome: Outcome;
}

export interface Place {
    /** A `u64`: a number while it is a safe integer, else a bigint. */
    readonly version: number | bigint;
    readonly lead?: AgentNumber;
    readonly hires: readonly AgentNumber[];
    readonly roads: readonly AgentNumber[];
    readonly tools_off: readonly SocietyTool[];
}

export interface Fill {
    readonly profile: ProfileId;
    readonly branch: string;
}

export interface AgentInput {
    readonly kind: "hand" | "hire";
    /** The standing task: it pays for a turn whose input names none. */
    readonly task: RealmTask;
    readonly place: Place;
}

export interface AgentActions {
    submit(message: Submit): null;
    finished(message: Finished): null;
    place(message: Place): null;
    fill(message: Fill): null;
}

export const WORKSPACE: Address<WorkspaceActions> = Address.of(
    "workspace",
    "local",
);
export const REALM: Address<RealmActions> = Address.of("realm", "local");

/** As each message's `Message::From` says. */
export const SENDERS = {
    submit: REALM,
    finished: WORKSPACE,
    place: REALM,
    fill: REALM,
} as const satisfies { readonly [N in keyof AgentActions]: Address };

const OPEN = { open: true } as const;
const U32 = int({ minimum: 0, maximum: U32_MAX });
const AUTHOR = obj({ agent: U32, name: opt(str()) }, OPEN);
const PLACE = obj(
    {
        version: union([int(), bigint()]),
        lead: opt(U32),
        hires: arr(U32),
        roads: arr(U32),
        tools_off: arr(str()),
    },
    OPEN,
);
const INPUT = union([
    obj(
        {
            type: lit("line"),
            text: str(),
            images: opt(arr(unknown())),
            files: opt(arr(unknown())),
        },
        OPEN,
    ),
    obj({ type: lit("letter"), from: AUTHOR, text: str() }, OPEN),
    obj(
        { type: lit("question"), letter: U32, from: AUTHOR, text: str() },
        OPEN,
    ),
    obj(
        {
            type: lit("answer"),
            question: U32,
            from: AUTHOR,
            text: str(),
            conversation: str(),
        },
        OPEN,
    ),
]);

/** Each drops `null` fields, ignores unknown ones and checks the rest. */
export const read: {
    readonly [N in keyof AgentActions]: (
        raw: unknown,
    ) => Parameters<AgentActions[N]>[0];
} & { readonly agentInput: (raw: unknown) => AgentInput } = {
    agentInput: reader(
        obj({ kind: oneOf(["hand", "hire"]), task: str(), place: PLACE }, OPEN),
        ({ kind, task, place }) => ({ kind, task, place: placeOf(place) }),
    ),
    submit: reader(
        obj({ id: str(), task: opt(str()), input: INPUT }, OPEN),
        (submit) => submit as Submit,
    ),
    // The end's own fields are the workspace's; only its tag is checked here.
    finished: reader(
        obj(
            {
                call: str(),
                outcome: obj(
                    {
                        type: oneOf(["run", "change"]),
                        end: obj({ type: str() }, OPEN),
                    },
                    OPEN,
                ),
            },
            OPEN,
        ),
        (finished) => finished as unknown as Finished,
    ),
    place: reader(PLACE, placeOf),
    fill: reader(obj({ profile: U32, branch: str() }, OPEN), (fill) => fill),
};

function reader<S extends Schema, T>(
    schema: S,
    shape: (value: Static<S>) => T,
): (raw: unknown) => T {
    const validator = compile(schema);
    return (raw) => {
        const read = validator.validate(withoutNulls(raw));
        if (!read.ok) {
            throw new ActorError(
                "message",
                "unreadable",
                `the message cannot be read: ${read.errors[0]?.message ?? ""}`,
            );
        }
        return shape(read.value as Static<S>);
    };
}

function withoutNulls(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(withoutNulls);
    if (value === null || typeof value !== "object") return value;
    return Object.fromEntries(
        Object.entries(value)
            .filter(([, field]) => field !== null)
            .map(([key, field]) => [key, withoutNulls(field)]),
    );
}

/**
 * A newer realm may add fields, and turn off a tool this agent does not
 * have. Both are dropped: the row keeps the place, and a field it cannot
 * serialise, such as a bigint, would fail the write.
 */
function placeOf({
    version,
    lead,
    hires,
    roads,
    tools_off,
}: Static<typeof PLACE>): Place {
    return {
        version,
        ...(lead === undefined ? {} : { lead }),
        hires,
        roads,
        tools_off: tools_off.filter((name): name is SocietyTool =>
            (SOCIETY_TOOLS as readonly string[]).includes(name),
        ),
    };
}
