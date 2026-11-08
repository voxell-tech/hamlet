// pi's history as a session of Claude Code's own, which a query resumes.
import type { Message } from "@earendil-works/pi-ai";
import {
    type ContentBlock,
    createSession,
    type Message as SessionMessage,
    type ToolResultBlock,
    type UserContentBlock,
} from "cc-session-io";
import { type Folders, PROVIDER } from "./claude.ts";

/** How Claude Code names a tool of the MCP server `hamlet`. */
export const TOOL_PREFIX = "mcp__hamlet__";

/** A new Claude Code session holding `messages`, for a query to resume; its id. */
export function importSession(
    messages: readonly Message[],
    at: Folders,
    model: string,
): string {
    const session = createSession({
        projectPath: at.work,
        claudeDir: at.config,
        model,
    });
    session.importMessages(anthropicMessages(messages));
    session.save();
    return session.sessionId;
}

/** `messages` as Anthropic messages, each turn's tool results in one user message after its call. */
function anthropicMessages(messages: readonly Message[]): SessionMessage[] {
    const converted: SessionMessage[] = [];
    const ids = new Map<string, string>();
    let results: { role: "user"; content: ToolResultBlock[] } | undefined;
    for (const message of messages) {
        if (message.role === "user") {
            converted.push({ role: "user", content: userContent(message) });
        } else if (message.role === "assistant") {
            // A turn stopped before anything was written answered nothing.
            if (message.content.length === 0) continue;
            const blocks = message.content.flatMap((block): ContentBlock[] => {
                if (block.type === "text")
                    return block.text === ""
                        ? []
                        : [{ type: "text" as const, text: block.text }];
                if (block.type === "thinking")
                    // Anthropic takes back only the signatures it gave Claude Code.
                    return message.provider === PROVIDER &&
                        block.thinkingSignature
                        ? [
                              {
                                  type: "thinking" as const,
                                  thinking: block.thinking,
                                  signature: block.thinkingSignature,
                              },
                          ]
                        : [];
                return [
                    {
                        type: "tool_use" as const,
                        id: toolId(block.id, ids),
                        name: `${TOOL_PREFIX}${block.name}`,
                        input: block.arguments,
                    },
                ];
            });
            converted.push({
                role: "assistant",
                content:
                    blocks.length > 0
                        ? blocks
                        : [
                              {
                                  type: "text",
                                  text: "[incompatible content omitted]",
                              },
                          ],
            });
            results = undefined;
        } else if (message.role === "toolResult") {
            const block: ToolResultBlock = {
                type: "tool_result",
                tool_use_id: toolId(message.toolCallId, ids),
                content: resultContent(message),
                ...(message.isError ? { is_error: true } : {}),
            };
            if (results === undefined) {
                results = { role: "user", content: [block] };
                converted.push(results);
            } else results.content.push(block);
        }
    }
    return converted;
}

/** A user message as Anthropic content. */
export function userContent(
    message: Extract<Message, { role: "user" }>,
): string | UserContentBlock[] {
    if (typeof message.content === "string")
        return message.content || "[empty]";
    const blocks = message.content.flatMap((block): UserContentBlock[] =>
        block.type === "text"
            ? block.text === ""
                ? []
                : [{ type: "text" as const, text: block.text }]
            : [
                  {
                      type: "image" as const,
                      source: {
                          type: "base64" as const,
                          media_type: block.mimeType,
                          data: block.data,
                      },
                  },
              ],
    );
    return blocks.length > 0 ? blocks : "[empty]";
}

/** A result as text, which Anthropic rejects empty. */
function resultContent(message: Extract<Message, { role: "toolResult" }>) {
    const text = message.content
        .flatMap((block) => (block.type === "text" ? [block.text] : []))
        .join("\n");
    if (text.trim() !== "") return text;
    return message.isError
        ? "The tool failed with no output."
        : `(${message.toolName} gave no output)`;
}

/** A call id as Anthropic takes it; another provider's may hold other characters. */
function toolId(id: string, ids: Map<string, string>): string {
    let clean = ids.get(id);
    if (clean === undefined) {
        clean = id.replace(/[^a-zA-Z0-9_-]/g, "_");
        ids.set(id, clean);
    }
    return clean;
}
