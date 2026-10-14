import { Type } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    type ConversationId,
    configure,
    defineDoc,
    defineExtension,
    defineTool,
} from "@earendil-works/pi-durable";
import { RecordDoc } from "./record.ts";

/** The agent a conversation is with (C22); a conversation not made for another agent is the mayor's. */
export const WithDoc = defineDoc<{ agent: string }>({
    kind: "hamlet.with",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "current",
    initial: () => ({ agent: "mayor" }),
});

/** Each agent's conversation with each agent that asks it something, keyed `asked:asker` (C22). */
export const QuestionsDoc = defineDoc<{
    conversations: Record<string, ConversationId>;
}>({
    kind: "hamlet.questions",
    version: 1,
    scope: "session",
    initial: () => ({ conversations: {} }),
});

const askAgent = defineTool({
    name: "ask_agent",
    description:
        "Ask another agent of your hamlet a question, and wait for its answer.",
    parameters: Type.Object({
        agent: Type.String(),
        question: Type.String(),
    }),
    // A rerun finds the same conversation and the same submission.
    replay: "safe",
    execute: async ({ agent, question }, api, context) => {
        const asker =
            (await api.snapshot(WithDoc, api.conversationId, context))?.agent ??
            "mayor";
        const { model } = await api.agent(context);
        const asked = await api.commit(async (tx) => {
            const record = await tx.doc(RecordDoc);
            if (!record.agents.some((each) => each.name === agent)) {
                throw new Error(`No agent of the hamlet is called ${agent}.`);
            }
            const questions = (await tx.doc(QuestionsDoc)).conversations;
            const key = `${agent}:${asker}`;
            const existing = questions[key];
            if (existing !== undefined) return existing;
            const created = await tx.createConversation({
                ownership: { kind: "ownerless" },
            });
            await configure(
                tx,
                created.id,
                model === undefined ? {} : { model },
            );
            (await tx.doc(WithDoc, created.id)).agent = agent;
            questions[key] = created.id;
            return created.id;
        }, context);
        const conversation = await api.conversation(asked, context);
        if (conversation === undefined) {
            throw new Error(`The conversation with ${agent} is gone.`);
        }
        const submission = await conversation.submit(
            {
                type: "input",
                content: question,
                requestId: `ask:${api.taskId}`,
            },
            context,
        );
        const settled = await submission.wait(context);
        if (settled.type !== "input" || settled.status !== "done") {
            throw new Error(`${agent} gave no answer (${settled.reason}).`);
        }
        const answer = await api.commit(
            (tx) => tx.entry(AssistantEntry, settled.answer),
            context,
        );
        const message = answer?.model?.[0];
        const text =
            message?.role === "assistant"
                ? message.content
                      .flatMap((part) =>
                          part.type === "text" ? [part.text] : [],
                      )
                      .join("\n")
                : "";
        return { output: [{ type: "text", text: `${agent}: ${text}` }] };
    },
});

/** What every agent sends along its roads (H30). */
export const Letters = defineExtension({
    name: "hamlet.letters",
    tools: [askAgent],
});
