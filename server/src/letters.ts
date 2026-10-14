import { Type } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    type ConversationId,
    defineDoc,
    defineExtension,
    defineTool,
} from "@earendil-works/pi-durable";
import { startConversation, WithDoc } from "./chats.ts";
import { HOME } from "./hamlet.ts";
import type { Part } from "./parts.ts";
import { RecordDoc } from "./record.ts";

/** Each agent's conversation with each agent that asks it something, keyed `asked:asker` by number (C22). */
export const QuestionsDoc = defineDoc<{
    conversations: { [key: string]: ConversationId };
}>({
    kind: "hamlet.questions",
    version: 1,
    scope: "session",
    initial: () => ({ conversations: {} }),
});

/** The messages every agent sends along its roads (H30). */
export const Letters: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.letters",
        tools: [
            defineTool({
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
                        (
                            await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            )
                        )?.number ?? 0;
                    const asked = await api.commit(async (tx) => {
                        const record = await tx.doc(RecordDoc, HOME, HOME);
                        const found = record.agents.find(
                            (each) => each.name === agent,
                        );
                        if (found === undefined) {
                            throw new Error(
                                `No agent of the hamlet is called ${agent}.`,
                            );
                        }
                        const questions = (await tx.doc(QuestionsDoc))
                            .conversations;
                        const key = `${found.number}:${asker}`;
                        const existing = questions[key];
                        if (existing !== undefined) return existing;
                        const created = await startConversation(home, tx, {
                            hamlet: HOME,
                            number: found.number,
                            chief: null,
                            chat: 1,
                        });
                        questions[key] = created;
                        return created;
                    }, context);
                    const conversation = await api.conversation(asked, context);
                    if (conversation === undefined) {
                        throw new Error(
                            `The conversation with ${agent} is gone.`,
                        );
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
                        throw new Error(
                            `${agent} gave no answer (${settled.reason}).`,
                        );
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
                    return {
                        output: [{ type: "text", text: `${agent}: ${text}` }],
                    };
                },
            }),
        ],
    }),
});
