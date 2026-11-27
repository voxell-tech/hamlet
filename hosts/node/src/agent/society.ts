/**
 * The realm keeps a marker per sender, id, action and digest, so a rerun
 * sends the same call and the realm answers what it recorded. All are
 * `replay: "safe"`.
 */

import type { Context } from "@earendil-works/chord/context";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    type Cursor,
    defineExtension,
    defineTool,
    type Extension,
    GenerationTask,
    type HookApi,
    hook,
    type ToolExecutionResult,
    type ToolRegistration,
    ToolTask,
} from "@earendil-works/pi-durable";
import { BLOCKED, textOf } from "./chat-items.ts";
import { hasWordsOrCall } from "./compaction.ts";
import type { Place } from "./messages/agent.ts";
import { REALM, type SocietyTool } from "./messages/realm.ts";
import type { Folder } from "./own-files.ts";
import {
    type Definition,
    OBJECTED,
    OBJECTING,
    TOOLS,
    WAITING,
} from "./society-tools.ts";
import { callOf, callOut, failed, type Peers, words } from "./turn.ts";

function societyTool<N extends SocietyTool>(
    peers: Peers,
    folder: Folder,
    act: N,
): ToolRegistration {
    const definition: Definition<N> = TOOLS[act];
    return defineTool({
        name: definition.name,
        description: definition.description,
        parameters: definition.parameters,
        replay: "safe",
        execute: async (args, api, context) => {
            const made = await definition.act(args as never, {
                peers,
                api,
                folder,
            });
            if ("invalid" in made) return failed(made.invalid);
            const said = (text: string): ToolExecutionResult =>
                text === WAITING || text === OBJECTING
                    ? { ...words(text), details: { waiting: callOf(api) } }
                    : words(text);
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
                    return said(
                        definition.said(answer as never, args as never),
                    );
                },
                definition.name,
                ({ why, words: refusal }) => {
                    // A letter whose files it may not send is blocked.
                    if (why.type === "files") return failed(BLOCKED + refusal);
                    // Such a call waits until a chief rules, and the ruling
                    // arrives as a notice.
                    const text =
                        why.type === "reserved"
                            ? WAITING
                            : definition.plain?.(why);
                    return text === undefined ? undefined : said(text);
                },
            );
        },
    });
}

/** Every agent's but a worker's. */
const EVERYONE: readonly SocietyTool[] = [
    "tell",
    "ask",
    "start_worker",
    "submit_skill",
    "ask_librarian",
    "ask_chief",
    "read_budget",
    "object",
];

/** A lead's over its own tree, which the Hand has over all. */
const LEADS: readonly SocietyTool[] = [
    "create_profile",
    "hire",
    "end",
    "merge_hires",
    "set_model",
    "set_tools",
    "set_allowance",
    "set_nickname",
    "lay_road",
    "take_up_road",
];

/**
 * The society tools offered to the Hand, or a hire at `place`, in a chief's
 * chat.
 */
export function offeredSociety(
    kind: "hand" | "hire",
    place: Pick<Place, "agrees_roads">,
): readonly SocietyTool[] {
    if (kind === "hand") {
        return [
            ...EVERYONE,
            ...LEADS,
            "found",
            "seat",
            "remove_hamlet",
            "read_record",
            "read_profile",
            "edit_profile",
            "tag",
            "revert",
        ];
    }
    return [
        ...EVERYONE,
        "request_road",
        ...LEADS,
        ...(place.agrees_roads ? (["agree_road"] as const) : []),
    ];
}

/** A society tool is offered whatever a lead turned off. */
export function society(
    peers: Peers,
    folder: Folder,
    offered: readonly SocietyTool[],
): Extension {
    return defineExtension({
        name: "hamlet.society",
        tools: offered.map((act) => societyTool(peers, folder, act)),
        hooks: [
            hook(GenerationTask, {
                onYield: (answer, api) => reply(peers, answer, api),
            }),
            hook(ToolTask, {
                beforeTool: async (call, api, context) =>
                    call.parent === undefined &&
                    (await objectedBefore(api, call.id, context))
                        ? { block: OBJECTED }
                        : undefined,
            }),
        ],
    });
}

/**
 * Whether an `object` call before the call `id` in the answer that made it
 * opened an objection. The round runs its calls one at a time, so each
 * earlier call's result is in the history first, newer than the answer.
 */
async function objectedBefore(
    api: HookApi,
    id: string,
    context: Context,
): Promise<boolean> {
    const opened = new Set<string>();
    let cursor: Cursor | undefined;
    do {
        const page = await api.entries({}, 16, cursor, context);
        for (const entry of page.items) {
            for (const message of entry.model ?? []) {
                if (message.role === "toolResult") {
                    if (!message.isError && textOf(entry) === OBJECTING) {
                        opened.add(message.toolCallId);
                    }
                    continue;
                }
                if (message.role !== "assistant") continue;
                const calls = message.content.filter(
                    (part) => part.type === "toolCall",
                );
                const at = calls.findIndex((part) => part.id === id);
                if (at !== -1) {
                    return calls
                        .slice(0, at)
                        .some(
                            (part) =>
                                part.name === "object" && opened.has(part.id),
                        );
                }
            }
        }
        cursor = page.next;
    } while (cursor !== undefined);
    return false;
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
