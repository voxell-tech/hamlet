import type { Context } from "@earendil-works/chord";
import {
    AgentDoc,
    type ConversationId,
    configure,
    DEFAULT_COMPACTION_POLICY,
    type Tx,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats.ts";
import type { Home, Part } from "../home.ts";
import { type Limits, LimitsDoc, limitsOf } from "../limits.ts";
import { liveHamlets, RealmDoc } from "../realm.ts";
import { type AgentRef, agentKey, HOME, MAYOR, RecordDoc } from "../record.ts";
import { configureRole } from "../roles.ts";
import {
    modelName,
    modelRef,
    type Settings,
    SettingsDoc,
    settingsOf,
} from "../settings.ts";
import { AgentConversationsDoc } from "../society/docs.ts";

/** The share of the window of the mayor's model kept free for an answer (C28). */
const RESERVE = 0.2;

/** The reason a choice of model is recorded with when its edit gives none. */
const CHOSEN = "The chief chose it.";

/** The model an agent runs: the chief's pin, an allowed mayor choice, the hamlet's model or the home's default (H29). */
export function choose(
    limits: Readonly<Limits>,
    settings: Readonly<Settings>,
    agent: { name: string; mayor: boolean; model?: string | null },
): string {
    const name = agent.mayor ? MAYOR : agent.name;
    const pinned = Object.hasOwn(limits.pinned, name)
        ? limits.pinned[name]
        : undefined;
    return (
        pinned ??
        (agent.model && limits.models.includes(agent.model)
            ? agent.model
            : undefined) ??
        limits.model ??
        settings.model
    );
}

/** A chief's edit of the documents a choice of model reads, made by `change`. */
export type Edit = {
    /** The chief who makes it. */
    by: string;
    /** Why, for the record; a chief's choice by default. */
    reason?: string;
    change(tx: Tx): void | Promise<void>;
};

/**
 * Makes `edit` in `tx`. In the same commit it configures the conversations of each agent whose model the edit changes,
 * so the change reaches the agent's next request, and records the agent's new model (H29).
 */
export async function commitEdit(
    home: Home,
    tx: Tx,
    edit: Edit,
): Promise<void> {
    const agents = await agentsOf(tx);
    const before = await Promise.all(
        agents.map((agent) => choiceOf(home, tx, agent)),
    );
    await edit.change(tx);
    for (const [index, agent] of agents.entries()) {
        const model = await choiceOf(home, tx, agent);
        if (model === before[index]) continue;
        for (const conversation of await chatsOf(tx, agent)) {
            await run(tx, conversation, model);
        }
        (await tx.doc(RecordDoc, agent.hamlet, agent.hamlet)).decisions.push({
            kind: "model",
            at: home.now(),
            by: edit.by,
            agent: agent.number,
            model,
            reason: edit.reason ?? CHOSEN,
        });
    }
}

/** Configures `conversation` with the choices of the agent it is with. */
export async function configureAgent(
    home: Home,
    tx: Tx,
    conversation: ConversationId,
): Promise<void> {
    const agent = await tx.doc(WithDoc, conversation);
    await run(tx, conversation, await choiceOf(home, tx, agent));
    await configureRole(home, tx, conversation);
}

/**
 * Keeps the choices of models in force. At start each chat runs its agent's model, which an override may have changed
 * since the last start, and the room kept for an answer follows the window of the mayor's model.
 */
export const Choice: Part = (home) => {
    const stops: (() => Promise<unknown>)[] = [];
    const follow = async (context: Context) => {
        const settings = settingsOf(
            await home.harness.snapshot(SettingsDoc, context),
            home.overrides,
        );
        const limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, HOME, context),
        );
        const model = modelRef(
            choose(limits, settings, { name: "", mayor: true }),
        );
        const window =
            model &&
            home.models.getModel(model.provider, model.modelId)?.contextWindow;
        home.compaction.reserveTokens =
            window === undefined
                ? DEFAULT_COMPACTION_POLICY.reserveTokens
                : Math.round(window * RESERVE);
    };
    return {
        async start(context) {
            await home.harness.commit(async (tx) => {
                for (const agent of await agentsOf(tx)) {
                    const model = await choiceOf(home, tx, agent);
                    for (const conversation of await chatsOf(tx, agent)) {
                        await configureRole(home, tx, conversation);
                        const stored = (await tx.doc(AgentDoc, conversation))
                            .model;
                        if (
                            stored === undefined ||
                            modelName(stored) !== model
                        ) {
                            await run(tx, conversation, model);
                        }
                    }
                }
            }, context);
            await follow(context);
            for (const watch of [
                await home.harness.watchDoc(SettingsDoc, context),
                await home.harness.watchDoc(LimitsDoc, HOME, context),
            ]) {
                if (watch === undefined) continue;
                watch.start((_value, _ops, context) => follow(context));
                stops.push(() => watch.stop());
            }
        },
        async stop() {
            for (const stop of stops.splice(0)) await stop();
        },
    };
};

/** The model `agent` runs now, read in `tx`. */
async function choiceOf(home: Home, tx: Tx, agent: AgentRef): Promise<string> {
    const record = await tx.doc(RecordDoc, agent.hamlet, agent.hamlet);
    const limits = await tx.doc(LimitsDoc, agent.hamlet, agent.hamlet);
    const settings = settingsOf(await tx.doc(SettingsDoc), home.overrides);
    const resident = record.agents.find((each) => each.number === agent.number);
    const name = resident?.name ?? "";
    return choose(limits, settings, {
        name,
        mayor: record.mayor === agent.number,
        model: resident?.model ?? null,
    });
}

/** Every living agent of the realm. */
async function agentsOf(tx: Tx): Promise<AgentRef[]> {
    const agents: AgentRef[] = [];
    for (const { id } of liveHamlets(await tx.doc(RealmDoc))) {
        for (const a of (await tx.doc(RecordDoc, id, id)).agents)
            agents.push({ hamlet: id, number: a.number });
    }
    return agents;
}

/** Every conversation belonging to an agent. */
async function chatsOf(tx: Tx, agent: AgentRef): Promise<ConversationId[]> {
    const key = agentKey(agent);
    return (await tx.doc(AgentConversationsDoc, key, key)).ids;
}

/** Configures `conversation` to run `model`, a name `endpoint::id`. */
async function run(
    tx: Tx,
    conversation: ConversationId,
    model: string,
): Promise<void> {
    await configure(tx, conversation, { model: modelRef(model) ?? null });
}

/** Current roles and choices applied to every living agent's conversations. */
export async function reconfigure(home: Home, tx: Tx): Promise<void> {
    for (const agent of await agentsOf(tx))
        for (const conversation of await chatsOf(tx, agent))
            await configureAgent(home, tx, conversation);
}
