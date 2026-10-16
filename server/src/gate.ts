import type { Context } from "@earendil-works/chord";
import {
    defineDoc,
    defineExtension,
    hook,
    type SubmissionId,
    ToolTask,
} from "@earendil-works/pi-durable";
import { CHANGE, changeBetween } from "./change.ts";
import { ReflectionEntry, type With, WithDoc } from "./chats/conversations.ts";
import { turnOf } from "./chats/turn.ts";
import {
    agentEnv,
    footing,
    insideHouse,
    LEAVE,
    type Leave,
    placeOf,
    WorkDoc,
} from "./environment.ts";
import { type Call, callOf, callText } from "./gate/call.ts";
import { give } from "./gate/grants.ts";
import { rule } from "./gate/rule.ts";
import { standingOf } from "./gate/standing.ts";
import { ManifestsDoc, reservedNames } from "./hand/manifests.ts";
import { houseOf } from "./home/layout.ts";
import type { Home, Part } from "./home.ts";
import { LimitsDoc } from "./limits/rules.ts";
import { AskedDoc, refuse, TrustedDoc, trustKey } from "./oversight/asked.ts";
import {
    type Choice,
    type Danger,
    open,
    type Ruling,
    RulingDoc,
    raise,
    ruling,
    settle,
    type Wait,
} from "./oversight/waits.ts";
import { line, strip } from "./oversight/words.ts";
import { holdFor } from "./oversight.ts";
import { type ChiefRefusal, chiefAccess } from "./realm/access.ts";
import { addressOf, RecordDoc } from "./record.ts";
import { err, type Result } from "./result.ts";

import { safeValue, safeWords } from "./scrub.ts";

/** The turn and decision of a call, kept until its tool task ends. */
const CallDoc = defineDoc<{
    input: SubmissionId | null;
    leave: Leave | null;
    block: string | null;
}>({
    kind: "hamlet.gate-call",
    version: 1,
    scope: "task",
    initial: () => ({ input: null, leave: null, block: null }),
});

type Decided = { run: Leave } | { block: string };
type Decision = Decided | { wait: Danger };

/** Refusal of new reflection approvals while the home closes. */
export function closeGate(home: Home): void {
    home.gate?.close();
}

/** A chief's answer, with its reason. */
export type Answer = {
    wait: string;
    choice: Choice;
    text?: string;
    chief: string;
};

/** The settlement of a wait and every permission its answer gives, in one commit (H11, H34). */
export function answer(
    home: Home,
    given: Answer,
    context: Context,
): Promise<Wait | undefined> {
    return home.harness.commit(async (tx) => {
        const wait = (await open(tx, context)).find(
            (wait) => wait.id === given.wait,
        );
        if (wait === undefined || !wait.answers.includes(given.choice))
            return undefined;
        const text = safeWords(home, given.text ?? "").trim();
        if (
            given.choice === "always" &&
            (wait.danger === "network" || wait.danger === "unconfined")
        ) {
            if (wait.agent === null) return undefined;
            await give(
                tx,
                wait.agent,
                wait.by,
                wait.danger,
                given.chief,
                home.now(),
            );
        }
        const counted = new Set<number>();
        for (const call of wait.calls) {
            const status = (await tx.task(call.task))?.state.status;
            if (
                status === undefined ||
                status === "terminal" ||
                status === "completing"
            )
                continue;
            const state = await tx.doc(CallDoc, call.task);
            if (given.choice === "allow" || given.choice === "always") {
                const place = await tx.doc(WithDoc, call.conversation);
                const standing = await standingOf(home, tx, place);
                state.leave = {
                    net:
                        wait.danger === "network" ||
                        ((wait.danger === "edit" ||
                            wait.danger === "command") &&
                            standing.net),
                };
                if (
                    given.choice === "always" &&
                    (wait.danger === "edit" || wait.danger === "command")
                ) {
                    const key = trustKey(place);
                    (await tx.doc(TrustedDoc, key, key))[wait.danger] = true;
                }
            }
            if (
                given.choice === "deny" &&
                wait.danger !== null &&
                !counted.has(call.conversation)
            ) {
                const asked = await tx.doc(AskedDoc, call.conversation);
                if (asked.input === state.input)
                    refuse(
                        asked,
                        wait.by,
                        wait.danger,
                        wait.about,
                        text || "no reason given",
                    );
                counted.add(call.conversation);
            }
        }
        return settle(
            tx,
            given.wait,
            { how: given.choice, text, chief: given.chief },
            home.now(),
        );
    }, context);
}

/** The answer deed's words for typed commands and protocol messages. */
export async function answerWords(
    home: Home,
    given: Answer,
    context: Context,
): Promise<Result<string, ChiefRefusal["error"] | AnswerFailure>> {
    const wait = (await open(home.harness, context)).find(
        (wait) => wait.id === given.wait,
    );
    if (wait === undefined)
        return { ok: true, value: "Nothing waits for you." };
    const hamlet = wait.agent?.hamlet ?? wait.chat?.hamlet;
    if (hamlet) {
        const access = await chiefAccess(
            home,
            hamlet,
            "answer wait",
            context,
            given.chief,
        );
        if (!access.ok) return access;
    }
    if (!wait.answers.includes(given.choice))
        return err({
            kind: "answer-not-offered",
            operation: "answer wait",
            wait: wait.id,
            retrySafe: true,
            offered: strip(wait),
        });
    const settled = await answer(home, given, context);
    return {
        ok: true,
        value:
            settled === undefined
                ? "Nothing waits for you."
                : `Answered /${given.choice}: ${line(settled, 80)}`,
    };
}

/** An answer outside the choices offered by this wait. */
export type AnswerFailure = {
    kind: "answer-not-offered";
    operation: "answer wait";
    wait: string;
    retrySafe: true;
    offered: string;
};

/** The words for an unavailable answer. */
export function answerFailureText(failure: AnswerFailure): string {
    switch (failure.kind) {
        case "answer-not-offered":
            return `That cannot be answered that way:\n${failure.offered}`;
    }
    const unhandled: never = failure.kind;
    throw new Error(`Unclassified answer failure: ${unhandled}`);
}

/** The gate every tool call passes before it runs (C7, H11). */
export const Gate: Part = (home) => {
    let closing = false;
    home.gate = {
        close: () => {
            closing = true;
        },
    };
    return {
        extension: defineExtension({
            name: "hamlet.gate",
            hooks: [
                hook(ToolTask, {
                    beforeTool: async (call, api, context) => {
                        await holdFor(home, api, context);
                        const place = await api.snapshot(
                            WithDoc,
                            api.conversationId,
                            context,
                        );
                        if (place === undefined) return undefined;
                        const turn = await turnOf(home.harness, api, context);
                        let parsed: Call = callOf(
                            call,
                            [],
                            reservedNames(home.registry),
                        );
                        let before: string | undefined;
                        if (
                            parsed.kind === "edit" &&
                            parsed.source === "write"
                        ) {
                            const location = await placeOf(
                                home,
                                api,
                                api.conversationId,
                                context,
                            );
                            const env = await agentEnv(
                                home,
                                {
                                    conversationId: api.conversationId,
                                    read: api,
                                },
                                { net: false },
                                context,
                            );
                            if (location !== undefined && env !== undefined) {
                                const path = footing(parsed.path, location);
                                const read = await env.readTextFile(
                                    path,
                                    context,
                                );
                                if (read.ok)
                                    before = safeWords(home, read.value);
                                else if (read.error.code !== "not_found") {
                                    const info = await env.fileInfo(
                                        path,
                                        context,
                                    );
                                    const size = info.ok
                                        ? `, ${info.value.size} bytes`
                                        : "";
                                    const reason =
                                        read.error.code === "invalid"
                                            ? "is not UTF-8 text"
                                            : "could not be read as text";
                                    before = `[a file that ${reason}${size}]`;
                                }
                            }
                        }
                        if (parsed.kind === "edit" && parsed.source === "write")
                            parsed = {
                                ...parsed,
                                old: before ?? "",
                                created: before === undefined,
                            };
                        const typed = parsed;
                        const preview = safeValue(
                            home,
                            typed.kind === "edit"
                                ? changeBetween(
                                      typed.path,
                                      typed.old,
                                      typed.now,
                                      typed.source === "write" && typed.created,
                                  )
                                : undefined,
                        );
                        const change =
                            preview === undefined
                                ? null
                                : await api.memo(CHANGE, preview, context);
                        const decision = await home.harness.commit<Decision>(
                            async (tx) => {
                                const state = await tx.doc(CallDoc, api.taskId);
                                if (state.block !== null)
                                    return { block: state.block };
                                const ruled = await tx.doc(
                                    RulingDoc,
                                    api.taskId,
                                );
                                if (ruled.how !== null)
                                    return fromRuling(
                                        ruled,
                                        state.leave ?? { net: false },
                                    );
                                const held = (await open(tx, context)).find(
                                    (wait) =>
                                        wait.calls.some(
                                            (held) => held.task === api.taskId,
                                        ),
                                );
                                if (held?.danger != null)
                                    return { wait: held.danger };
                                if (state.leave !== null)
                                    return { run: { net: state.leave.net } };
                                state.input = turn?.input ?? null;
                                const standing = await standingOf(
                                    home,
                                    tx,
                                    place,
                                );
                                const path =
                                    typed.kind === "edit"
                                        ? typed.path
                                        : undefined;
                                if (path !== undefined) {
                                    const house = houseOf(home.layout, place);
                                    const work = await tx.doc(
                                        WorkDoc,
                                        place.hamlet,
                                        place.hamlet,
                                    );
                                    if (
                                        insideHouse(
                                            footing(path, {
                                                house,
                                                folder: work.folder,
                                            }),
                                            house,
                                        )
                                    )
                                        standing.houseEdit = true;
                                }
                                const record = await tx.doc(
                                    RecordDoc,
                                    place.hamlet,
                                    place.hamlet,
                                );
                                const by = waitBy(record, place);
                                const limits = await tx.doc(
                                    LimitsDoc,
                                    place.hamlet,
                                    place.hamlet,
                                );
                                const asked = await tx.doc(
                                    AskedDoc,
                                    api.conversationId,
                                );
                                const effectiveAsked =
                                    asked.input === state.input
                                        ? asked
                                        : {
                                              ...AskedDoc.definition.initial(),
                                              input: state.input,
                                          };
                                const about = safeWords(home, callText(typed));
                                const manifests = (await tx.doc(ManifestsDoc))
                                    .tools;
                                const currentCall =
                                    typed.kind === "society" ||
                                    typed.kind === "other"
                                        ? callOf(
                                              call,
                                              manifests,
                                              reservedNames(home.registry),
                                          )
                                        : typed;
                                const verdict = rule(
                                    currentCall,
                                    standing,
                                    { ...effectiveAsked, by, about },
                                    limits,
                                    record.mayor === place.number,
                                );
                                if ("run" in verdict) {
                                    state.leave = verdict.run;
                                    return verdict;
                                }
                                Object.assign(asked, effectiveAsked);
                                if (
                                    closing &&
                                    turn?.kind === ReflectionEntry.kind
                                ) {
                                    state.block =
                                        "nobody is here to approve it; it did not run";
                                    return { block: state.block };
                                }
                                if ("block" in verdict) {
                                    state.block = verdict.block;
                                    return verdict;
                                }
                                await raise(
                                    tx,
                                    {
                                        kind: "approval",
                                        danger: verdict.ask,
                                        by,
                                        agent: {
                                            hamlet: place.hamlet,
                                            number: place.number,
                                        },
                                        about,
                                        change,
                                        answers:
                                            verdict.ask === "destructive" ||
                                            verdict.ask === "reserved"
                                                ? ["allow", "deny"]
                                                : ["allow", "always", "deny"],
                                        chat:
                                            place.chief === null ? null : place,
                                        call: {
                                            task: api.taskId,
                                            conversation: api.conversationId,
                                        },
                                    },
                                    home.now(),
                                );
                                return { wait: verdict.ask };
                            },
                            context,
                        );
                        let final: Decided;
                        if ("wait" in decision) {
                            const ruled = await ruling(
                                home.harness,
                                api.taskId,
                                context,
                            );
                            const leave = (
                                await api.snapshot(CallDoc, api.taskId, context)
                            )?.leave ?? { net: decision.wait === "network" };
                            final = fromRuling(ruled, leave);
                        } else {
                            final = decision;
                        }
                        if ("block" in final) return final;
                        await api.memo(LEAVE, final.run, context);
                        return undefined;
                    },
                }),
            ],
        }),
    };
};

function fromRuling(ruled: Readonly<Ruling>, leave: Leave): Decided {
    if (ruled.how === "allow" || ruled.how === "always")
        return { run: { net: leave.net } };
    return {
        block:
            ruled.how === "deny"
                ? `the chief refused it: ${ruled.text || "no reason given"}`
                : "nobody is here to approve it; it did not run",
    };
}

function waitBy(
    record: import("./record.ts").HamletRecord,
    place: With,
): string {
    const name = addressOf(record, place.number);
    return name === "mayor" && place.hamlet !== "home"
        ? `mayor@${place.hamlet}`
        : name;
}

export {
    type Grant,
    type GrantEvent,
    GrantsDoc,
    give,
    grantsOf,
    holds,
    type Permission,
    revoke,
} from "./gate/grants.ts";
