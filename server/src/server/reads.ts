import type { Context } from "@earendil-works/chord";
import { chatOf } from "../chats/conversations.ts";
import { historyOf } from "../chats.ts";
import type { Home } from "../home.ts";
import { catalogueOf, refreshModels } from "../models.ts";
import { CharacterDoc, CharterDoc, charterOf, told } from "../prompt.ts";
import type { Address, Listed, Read, ReadResult } from "../protocol.ts";
import { type ChiefResult, chiefAccess } from "../realm/access.ts";
import { resolve } from "../realm/roads.ts";
import { type AgentRef, agentKey } from "../record.ts";
import { ok } from "../result.ts";

/** A chief's view of the prompt, history, catalogue and papers (C14, C24). */
export async function read(
    home: Home,
    message: Read,
    context: Context,
): Promise<ChiefResult<ReadResult>> {
    const reply = { type: "read", to: message.id } as const;
    if (message.what === "charter")
        return ok({
            ...reply,
            charter: {
                text: await charterOf(home.harness, context),
                own:
                    (await home.harness.snapshot(CharterDoc, context))?.own !=
                    null,
            },
        });
    if (message.what === "models") {
        if (message.refresh) await refreshModels(home, context);
        const cards = new Map(
            (await catalogueOf(home, context)).map((card) => [card.name, card]),
        );
        for (const model of home.models.getModels()) {
            const name = `${model.provider}::${model.id}`;
            if (cards.has(name)) continue;
            cards.set(name, {
                name,
                price: { input: model.cost.input, output: model.cost.output },
                priced: "listed",
                context: model.contextWindow,
                allowed: false,
                note: "",
            });
        }
        const models: Listed[] = [...cards.values()].sort((a, b) =>
            a.name.localeCompare(b.name),
        );
        return ok({ ...reply, models });
    }
    const resolved = await readableAgent(home, message.agent, context);
    if (!resolved.ok) return resolved;
    const agent = resolved.value;
    const chat = { agent, chief: home.chief };
    if (message.what === "history")
        return ok({ ...reply, history: await historyOf(home, chat, context) });
    const conversation = await chatOf(home, chat, context);
    if (message.what === "prompt")
        return ok({
            ...reply,
            prompt: await told(home, conversation, context),
        });
    const notes = await home.houses.of(agent).notes(context);
    const character = await home.harness.snapshot(
        CharacterDoc,
        agentKey(agent),
        context,
    );
    const cut = (text: string) =>
        message.full ? text : [...text.trim()].slice(0, 6000).join("");
    return ok({
        ...reply,
        papers: {
            notes: cut(notes.ok ? notes.value.text : ""),
            character: cut(character?.text ?? ""),
            skills: (await home.houses.of(agent).skills(context)).map(
                ({ name, description }) => ({ name, description }),
            ),
        },
    });
}

/** A living agent whose papers the chief may read or edit. */
export async function readableAgent(
    home: Home,
    address: Address,
    context: Context,
): Promise<ChiefResult<AgentRef>> {
    const access = await chiefAccess(
        home,
        address.hamlet,
        "read or edit papers",
        context,
    );
    if (!access.ok) return access;
    const resolved = await resolve(
        home.harness,
        address.hamlet,
        `${address.agent}@${address.hamlet}`,
        context,
    );
    if (!resolved) throw new Error(`no agent named \`${address.agent}\``);
    if ("gone" in resolved) throw new Error(resolved.gone);
    return { ok: true, value: resolved.ref };
}
