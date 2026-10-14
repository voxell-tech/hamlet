import type { Context } from "@earendil-works/chord";
import { getSystemMessageText, type Message } from "@earendil-works/pi-ai";
import type { Conversation, PromptInput } from "@earendil-works/pi-durable";
import { envFor } from "./environment.ts";
import type { Home } from "./home.ts";

/**
 * The prompt `conversation` sends at its next request, whole, as the model reads it (C14). A section that keeps its
 * shown words through a run, as the budget does, gives them here too.
 */
export async function told(
    home: Home,
    conversation: Conversation,
    context: Context,
): Promise<string> {
    const agent = await conversation.agent(context);
    const shown = sectionsShown((await conversation.context(context)).messages);
    const input: PromptInput = {
        conversationId: conversation.id,
        agent,
        env: await envFor(
            home,
            { conversationId: conversation.id, read: home.harness },
            { net: false },
            context,
        ),
        shown: Object.fromEntries(shown),
        read: home.harness,
    };
    // pi-durable's renderSections, which it does not export: a section
    // that fails keeps the words it showed.
    const sections = new Map<string, string>();
    for (const section of agent.sections) {
        let text: string | undefined;
        try {
            text = await section.render(input, context);
        } catch {
            text = shown.get(section.key);
            if (text !== undefined) sections.set(section.key, text);
            continue;
        }
        if (text === undefined) continue;
        sections.set(
            section.key,
            section.tag === false
                ? text
                : `<${section.key}>\n${text}\n</${section.key}>`,
        );
    }
    return getSystemMessageText({
        role: "system",
        content: "",
        sections: Object.fromEntries(sections),
        timestamp: home.now(),
    });
}

/** The sections the system messages of a context leave in effect, as pi-durable's replaySections finds them. */
function sectionsShown(messages: readonly Message[]): Map<string, string> {
    const shown = new Map<string, string>();
    for (const message of messages) {
        if (message.role !== "system") continue;
        for (const [key, text] of Object.entries(message.sections ?? {})) {
            if (text === null) shown.delete(key);
            else shown.set(key, text);
        }
    }
    return shown;
}
