/**
 * The realm's messages: what an agent asks of the realm, and the
 * answers.
 */
import { Address } from "../../actor/actor/mod.ts";
import type { Copies } from "./agent.ts";
import type { OfferId } from "./library.ts";
import type { CommitId, Path, Revision } from "./profile.ts";
import type {
    AgentNumber,
    CallId,
    ConversationRef,
    HamletId,
    LetterId,
    ProfileId,
    RealmTask,
    Refused,
} from "./shared.ts";

/** `mayor` is the office of the caller's hamlet. */
export type Named =
    | { readonly type: "hand" }
    | { readonly type: "mayor" }
    | { readonly type: "mayor_of"; readonly hamlet: HamletId }
    | { readonly type: "agent"; readonly agent: AgentNumber };

export type Tier = "fifty" | "hundred" | "two_hundred" | "five_hundred";

export interface Tool<Act> {
    readonly call: CallId;
    /** The realm's task of the calling turn. */
    readonly task?: RealmTask;
    /** The calling turn's conversation. */
    readonly conversation?: ConversationRef;
    readonly act: Act;
}

export type Done = { readonly type: "done" } | Refused;

/** A society tool's act, or a built-in or hand tool's name. */
export interface ToolName {
    readonly name: string;
}

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
            /** The profile's version the hire starts from; `main` when absent. */
            readonly version?: string;
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
        act: { readonly to: Named; readonly text: string };
        answer: Done;
    };
    ask: {
        act: {
            readonly to: Named;
            readonly text: string;
            readonly conversation: ConversationRef;
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
            readonly why?: string;
        };
        answer: Done;
    };
    set_tools: {
        act: {
            readonly hire: Named;
            readonly off?: readonly ToolName[];
            readonly on?: readonly ToolName[];
        };
        answer: Done;
    };
    create_profile: {
        /** Without a character, the profile starts from the realm's. */
        act: { readonly name: string; readonly character?: string };
        answer:
            | {
                  readonly type: "made";
                  readonly profile: ProfileId;
                  /** `scholar#<id> (<name>)`. */
                  readonly label: string;
              }
            | Refused;
    };
    read_profile: {
        /** Without `path`, the files are listed; without `at`, at `main`. */
        act: {
            readonly profile: ProfileId;
            readonly at?: Revision;
            readonly path?: string;
        };
        answer:
            | { readonly type: "listed"; readonly paths: readonly Path[] }
            | {
                  readonly type: "text";
                  readonly text: string;
                  /** Cut after the profile's most bytes a read gives. */
                  readonly cut: boolean;
              }
            | Refused;
    };
    edit_profile: {
        /** An empty `text` removes the file. */
        act: {
            readonly profile: ProfileId;
            readonly path: string;
            readonly text: string;
            readonly message?: string;
        };
        answer: Done;
    };
    tag: {
        act: {
            readonly profile: ProfileId;
            readonly version: string;
            readonly note?: string;
        };
        answer:
            | {
                  readonly type: "tagged";
                  readonly version: string;
                  readonly commit: CommitId;
              }
            | Refused;
    };
    revert: {
        act: {
            readonly profile: ProfileId;
            readonly what: Revision;
            readonly reason?: string;
        };
        answer:
            | {
                  readonly type: "reverted";
                  readonly commit: CommitId;
                  /** Files that changed again since and kept their later lines. */
                  readonly clashes: readonly Path[];
              }
            | { readonly type: "unchanged" }
            | Refused;
    };
    /** The librarian's answer arrives later, as an input. */
    ask_librarian: { act: { readonly question: string }; answer: Done };
    /** Asks the librarian to judge an offer the library kept; its verdict arrives later, as an input. */
    submit_skill: {
        act: {
            readonly offer: OfferId;
            readonly skill: string;
            readonly reason?: string;
        };
        answer: Done;
    };
}

export type SocietyTool = keyof SocietyActs;

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

export interface OauthLogin {
    readonly access: string;
    readonly refresh: string;
    /** Whole seconds since the epoch. */
    readonly expires: number;
    readonly extra?: string;
}

export type Kept =
    | {
          readonly type: "key";
          readonly key: string;
          /** The key's settings, left out when it has none; not secrets. */
          readonly env?: Readonly<Record<string, string>>;
      }
    | { readonly type: "oauth"; readonly login: OauthLogin };

export type CredentialAnswer =
    | Kept
    | Refused<{ readonly type: "not_signed_in"; readonly provider: string }>;

export interface KeepCredential {
    readonly provider: string;
    readonly value: Kept;
}

export interface TurnFailed {
    readonly task: RealmTask;
    readonly conversation: ConversationRef;
    readonly words: string;
    readonly cause: "model" | "budget";
}

export type LapseReason = "stopped" | "rewound" | "ended" | "resumed";

export interface Lapse {
    /** Absent: every conversation of the agent. */
    readonly conversation?: ConversationRef;
    readonly why: LapseReason;
}

export interface OpenTask {
    /** A repeat carries the same id. */
    readonly request: string;
    readonly turn: "retry" | "reflection";
}

export type TaskOpened =
    | { readonly type: "opened"; readonly task: RealmTask }
    | Refused;

/** The versions the agent holds; the realm answers the parts newer than these. */
export interface ReadCopies {
    readonly charter?: number;
    readonly model?: number;
    readonly settings?: number;
}

/** The realm's secrets in force, which only a scrub reads. */
export interface SecretList {
    readonly version: number;
    readonly values: readonly string[];
}

export type Ended =
    | { readonly type: "answered"; readonly text: string }
    | { readonly type: "failed"; readonly why: string };

export interface TurnEnded {
    readonly ended: Ended;
}

export type RealmActions = {
    readonly [N in SocietyTool]: (
        message: Tool<SocietyActs[N]["act"]>,
    ) => SocietyActs[N]["answer"];
} & {
    reply(message: Reply): Done;
    may_spend(message: MaySpend): Done;
    charge(message: Charge): null;
    credential(message: { readonly provider: string }): CredentialAnswer;
    keep_credential(message: KeepCredential): null;
    turn_failed(message: TurnFailed): null;
    lapse(message: Lapse): null;
    open_task(message: OpenTask): TaskOpened;
    read_copies(message: ReadCopies): Copies;
    secrets(): SecretList;
    worker_done(message: TurnEnded): null;
    errand_done(message: TurnEnded): null;
};

export const REALM: Address<RealmActions> = Address.of("realm", "local");
