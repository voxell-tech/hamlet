import type { Context } from "@earendil-works/chord";
import {
    defineDoc,
    defineExtension,
    hook,
    type SubmissionId,
    type ToolHookCall,
    ToolTask,
    type Tx,
} from "@earendil-works/pi-durable";
import { AskedDoc, refuse, refused, TrustedDoc, trustKey } from "./asked.ts";
import { CHANGE, changeOf } from "./change.ts";
import { ReflectionEntry, type With, WithDoc } from "./chats.ts";
import { type ChiefResult, chiefAccess } from "./chief-access.ts";
import { judge, type Reach, type Standing } from "./danger.ts";
import { envFor, LEAVE, type Leave, placeOf } from "./environment.ts";
import { footing } from "./footing.ts";
import { GrantsDoc, give, grantsOf, holds } from "./grants.ts";
import { WorkDoc } from "./hamlet.ts";
import type { Home } from "./home.ts";
import { houseOf } from "./layout.ts";
import { LimitsDoc } from "./limits.ts";
import { type Manifest, ManifestsDoc } from "./manifests.ts";
import type { Part } from "./parts.ts";
import { safeValue, safeWords } from "./plainly.ts";
import { addressOf, RecordDoc } from "./record.ts";
import { canon, within } from "./sandbox/policy.ts";
import { SettingsDoc, settingsOf } from "./settings.ts";
import { holdFor } from "./society/objections.ts";
import { turnOf } from "./turn.ts";
import {
    type Choice,
    type Danger,
    OversightDoc,
    type Ruling,
    RulingDoc,
    raise,
    ruling,
    settle,
    type Wait,
} from "./waits.ts";
import { line, strip } from "./words.ts";

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

/** Characters of either side of an edit that a chief reads. */
const EDIT_SHOWN = 2000;
const closing = new WeakSet<Home>();

/** Refusal of new reflection approvals while the home closes. */
export function closeGate(home: Home): void {
    closing.add(home);
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
        const wait = (await tx.doc(OversightDoc)).waits[given.wait];
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
): Promise<ChiefResult<string>> {
    const wait = (await home.harness.snapshot(OversightDoc, context))?.waits[
        given.wait
    ];
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
        throw new Error(`That cannot be answered that way:\n${strip(wait)}`);
    const settled = await answer(home, given, context);
    return {
        ok: true,
        value:
            settled === undefined
                ? "Nothing waits for you."
                : `Answered /${given.choice}: ${line(settled, 80)}`,
    };
}

/** The gate every tool call passes before it runs (C7, H11). */
export const Gate: Part = (home) => ({
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
                    let before: string | undefined;
                    if (
                        call.name === "write" &&
                        typeof call.arguments.path === "string"
                    ) {
                        const location = await placeOf(
                            home,
                            api,
                            api.conversationId,
                            context,
                        );
                        const env = await envFor(
                            home,
                            { conversationId: api.conversationId, read: api },
                            { net: false },
                            context,
                        );
                        if (location !== undefined && env !== undefined) {
                            const path = footing(call.arguments.path, location);
                            const read = await env.readTextFile(path, context);
                            if (read.ok) before = safeWords(home, read.value);
                            else if (read.error.code !== "not_found") {
                                const info = await env.fileInfo(path, context);
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
                    const preview = safeValue(
                        home,
                        changeOf(call.name, call.arguments, before),
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
                            const ruled = await tx.doc(RulingDoc, api.taskId);
                            if (ruled.how !== null)
                                return fromRuling(
                                    ruled,
                                    state.leave ?? { net: false },
                                );
                            const oversight = await tx.doc(OversightDoc);
                            const held = Object.values(oversight.waits).find(
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
                            const standing = await standingOf(home, tx, place);
                            const path = call.arguments.path;
                            if (
                                (call.name === "write" ||
                                    call.name === "edit") &&
                                typeof path === "string"
                            ) {
                                const house = houseOf(home.layout, place);
                                const work = await tx.doc(
                                    WorkDoc,
                                    place.hamlet,
                                    place.hamlet,
                                );
                                if (
                                    within(
                                        canon(
                                            footing(path, {
                                                house,
                                                folder: work.folder,
                                            }),
                                        ),
                                        canon(house),
                                    )
                                )
                                    standing.asksEdits = false;
                            }
                            const manifests = (await tx.doc(ManifestsDoc))
                                .tools;
                            let verdict = judge(
                                reachOf(call, manifests),
                                standing,
                            );
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
                            if (
                                record.mayor === place.number &&
                                reserved(call, limits)
                            )
                                verdict = { ask: "reserved" };
                            if ("run" in verdict) {
                                state.leave = verdict.run;
                                return verdict;
                            }
                            if (
                                closing.has(home) &&
                                turn?.kind === ReflectionEntry.kind
                            ) {
                                state.block =
                                    "nobody is here to approve it; it did not run";
                                return { block: state.block };
                            }
                            const asked = await tx.doc(
                                AskedDoc,
                                api.conversationId,
                            );
                            if (asked.input !== state.input)
                                Object.assign(
                                    asked,
                                    AskedDoc.definition.initial(),
                                    { input: state.input },
                                );
                            const about = safeWords(
                                home,
                                aboutCall(call, before),
                            );
                            const block = refused(
                                asked,
                                by,
                                verdict.ask,
                                about,
                                limits.asking.refusals,
                            );
                            if (block !== undefined) {
                                state.block = block;
                                return { block };
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
                                    chat: place.chief === null ? null : place,
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
});

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

function reachOf(call: ToolHookCall, manifests: readonly Manifest[]): Reach {
    if (call.name === "bash")
        return {
            kind: "shell",
            command: String(call.arguments.command),
            net: call.arguments.network === true,
        };
    if (call.name === "write" || call.name === "edit") return { kind: "edit" };
    if (["read", "search", "find"].includes(call.name)) return { kind: "read" };
    const manifest = manifests.find((tool) => tool.name === call.name);
    return manifest === undefined
        ? { kind: "other" }
        : { kind: "custom", net: manifest.net };
}

function aboutCall(call: ToolHookCall, before?: string): string {
    if (call.name === "bash") return String(call.arguments.command);
    if (call.name === "write" || call.name === "edit") {
        const path = cut(String(call.arguments.path));
        const edits = call.arguments.edits as
            | { oldText: string; newText: string }[]
            | undefined;
        const old =
            call.name === "write"
                ? (before ?? "")
                : (edits ?? []).map((edit) => edit.oldText).join("\n");
        const now =
            call.name === "write"
                ? String(call.arguments.content)
                : (edits ?? []).map((edit) => edit.newText).join("\n");
        const created =
            call.name === "write" ? before === undefined : old === "";
        return created
            ? `${path}\nA new file:\n${cut(now)}`
            : `${path}\nReplace:\n${cut(old)}\nWith:\n${cut(now)}`;
    }
    return `${call.name} ${JSON.stringify(call.arguments)}`;
}

function cut(text: string): string {
    const chars = [...text];
    return chars.length > EDIT_SHOWN
        ? `${chars.slice(0, EDIT_SHOWN).join("")}\n[${chars.length - EDIT_SHOWN} more characters]`
        : text;
}

async function standingOf(home: Home, tx: Tx, place: With): Promise<Standing> {
    const work = await tx.doc(WorkDoc, place.hamlet, place.hamlet);
    const record = await tx.doc(RecordDoc, place.hamlet, place.hamlet);
    const by = addressOf(record, place.number);
    const limits = await tx.doc(LimitsDoc, place.hamlet, place.hamlet);
    const settings = settingsOf(await tx.doc(SettingsDoc), home.overrides);
    const grants = grantsOf(await tx.doc(GrantsDoc));
    const key = trustKey(place);
    const trust = await tx.doc(TrustedDoc, key, key);
    const manual = work.folder !== null && work.mode === "manual";
    return {
        confined: true,
        net:
            settings.allowNet ||
            holds(limits.permissions, grants, by, place, "network"),
        unconfined: holds(limits.permissions, grants, by, place, "unconfined"),
        asksEdits: manual && !trust.edit,
        asksCommands: manual && !trust.command,
    };
}

/** The limits that reserve a mayor's society act (H22). */
function reserved(
    call: ToolHookCall,
    limits: import("./limits.ts").Limits,
): boolean {
    switch (call.name) {
        case "create_agent":
            return (
                !limits.mayor.createAgents ||
                (!limits.mayor.buildRoads &&
                    Array.isArray(call.arguments.roads) &&
                    call.arguments.roads.length > 0)
            );
        case "build_road":
            return !limits.mayor.buildRoads;
        case "remove_agent":
            return !limits.mayor.removeAgents;
        case "merge_agents":
            return !limits.mayor.mergeAgents;
        default:
            return false;
    }
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
