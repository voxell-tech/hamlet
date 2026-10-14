import type { Context } from "@earendil-works/chord";
import {
    AgentDoc,
    type ConversationId,
    configure,
    DEFAULT_COMPACTION_POLICY,
    type Tx,
} from "@earendil-works/pi-durable";
import { ChatDoc, conversationsOf, WithDoc } from "./chats.ts";
import { type AgentRef, agentKey, HOME, MAYOR } from "./hamlet.ts";
import type { Home } from "./home.ts";
import { type Limits, LimitsDoc, limitsOf } from "./limits.ts";
import type { Part } from "./parts.ts";
import { RecordDoc } from "./record.ts";
import {
    modelName,
    modelRef,
    type Settings,
    SettingsDoc,
    settingsOf,
} from "./settings.ts";

/** The share of the window of the mayor's model kept free for an answer (C28). */
const RESERVE = 0.2;

/** The reason a choice of model is recorded with when its edit gives none. */
const CHOSEN = "The chief chose it.";

/** The model an agent runs, as `endpoint::id`: the chief's pin, else the hamlet's model, else the home's default (H29). */
export function choose(
    limits: Readonly<Limits>,
    settings: Readonly<Settings>,
    agent: { name: string; mayor: boolean },
): string {
    const name = agent.mayor ? MAYOR : agent.name;
    const pinned = Object.hasOwn(limits.pinned, name)
        ? limits.pinned[name]
        : undefined;
    return pinned ?? limits.model ?? settings.model;
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
    const name =
        record.agents.find((each) => each.number === agent.number)?.name ?? "";
    return choose(limits, settings, {
        name,
        mayor: record.mayor === agent.number,
    });
}

/** Every agent of the first hamlet. */
async function agentsOf(tx: Tx): Promise<AgentRef[]> {
    return (await tx.doc(RecordDoc, HOME, HOME)).agents.map((agent) => ({
        hamlet: HOME,
        number: agent.number,
    }));
}

/** Every conversation of `agent`'s chats. */
async function chatsOf(tx: Tx, agent: AgentRef): Promise<ConversationId[]> {
    const key = agentKey(agent);
    return conversationsOf(await tx.doc(ChatDoc, key, key));
}

/** Configures `conversation` to run `model`, a name `endpoint::id`. */
async function run(
    tx: Tx,
    conversation: ConversationId,
    model: string,
): Promise<void> {
    await configure(tx, conversation, { model: modelRef(model) ?? null });
}
