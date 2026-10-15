import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { catalogueOf } from "./catalogue.ts";
import { CharacterDoc } from "./character.ts";
import { CharterDoc, charterOf } from "./charter.ts";
import { chatOf } from "./chats.ts";
import { envFor } from "./environment.ts";
import { agentKey, HOME } from "./hamlet.ts";
import { historyOf } from "./history.ts";
import type { Home } from "./home.ts";
import { houseOf } from "./layout.ts";
import { refreshModels } from "./models.ts";
import type { Listed, Read, ReadResult } from "./protocol.ts";
import { addressed } from "./record.ts";
import { houseSkills } from "./skills.ts";
import { told } from "./told.ts";

/** A chief's view of the prompt, history, catalogue and papers (C14, C24). */
export async function read(
    home: Home,
    message: Read,
    context: Context,
): Promise<ReadResult> {
    const reply = { type: "read", to: message.id } as const;
    if (message.what === "charter")
        return {
            ...reply,
            charter: {
                text: await charterOf(home.harness, context),
                own:
                    (await home.harness.snapshot(CharterDoc, context))?.own !=
                    null,
            },
        };
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
        return { ...reply, models };
    }
    if (message.agent.hamlet !== HOME)
        throw new Error(`no hamlet named \`${message.agent.hamlet}\``);
    const agent = await addressed(home.harness, message.agent, context);
    if (agent === undefined)
        throw new Error(`no agent named \`${message.agent.agent}\``);
    const chat = { agent, chief: home.chief };
    if (message.what === "history")
        return { ...reply, history: await historyOf(home, chat, context) };
    const conversation = await chatOf(home, chat, context);
    if (message.what === "prompt")
        return { ...reply, prompt: await told(home, conversation, context) };
    const env = await envFor(
        home,
        { conversationId: conversation.id, read: home.harness },
        { net: false },
        context,
    );
    const house = houseOf(home.layout, agent);
    const notes = await env?.readTextFile(join(house, "NOTES.md"), context);
    const character = await home.harness.snapshot(
        CharacterDoc,
        agentKey(agent),
        context,
    );
    const cut = (text: string) =>
        message.full ? text : [...text.trim()].slice(0, 6000).join("");
    return {
        ...reply,
        papers: {
            notes: cut(notes?.ok ? notes.value : ""),
            character: cut(character?.text ?? ""),
            skills: (await houseSkills(house, home.sandbox, context)).map(
                ({ name, description }) => ({ name, description }),
            ),
        },
    };
}
