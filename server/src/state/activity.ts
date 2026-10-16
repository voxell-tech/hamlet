import type { AgentEvent, SnapshotEvent } from "@earendil-works/pi-durable";
import type { Agent, Doing } from "../protocol.ts";

const WORDS: Record<Doing, string> = {
    rest: "At rest",
    think: "Thinking",
    "read-file": "Reading a file",
    "read-notes": "Reading its notes",
    "read-skill": "Reading a skill",
    "write-file": "Writing a file",
    "write-notes": "Writing its notes",
    "write-skill": "Writing a skill",
    run: "Running a command",
    fetch: "Fetching from the network",
    custom: "Using a tool added by hand",
    await: "Waiting for an agent's answer",
};

/** Committed work retained for one conversation when a state refresh is queued. */
export type Work = Pick<
    Activity,
    "activity" | "saying" | "working" | "pending" | "faulted" | "consulting"
>;

/** A zero-step activity before the first committed work event. */
export function activityOf(
    doing: "rest" | "think" = "rest",
): Agent["activity"] {
    return { doing, doingWords: WORDS[doing], step: 0, failures: 0 };
}

/** The work and words of one conversation, folded only from committed events. */
export class Activity {
    /** The current work step and its accumulated tool failures. */
    activity: Agent["activity"] = activityOf();
    /** The current visible text or reasoning fragment. */
    saying: Agent["saying"] = { text: "", thought: false };
    /** An active generation run in the committed conversation. */
    working = false;
    /** Queued inputs that can start model work. */
    pending = 0;
    /** The last observed answer ended in failure or abort. */
    faulted = false;
    /** An active librarian consultation or skill submission. */
    consulting = false;
    readonly #blocks = new Map<number, string>();
    readonly #calls = new Set<string>();

    /** Seeds the fold from one committed conversation snapshot. */
    constructor(snapshot: SnapshotEvent) {
        this.note(snapshot);
    }

    /** Advances the fold with the next committed conversation event. */
    note(event: AgentEvent): void {
        switch (event.type) {
            case "snapshot":
                this.#blocks.clear();
                this.#calls.clear();
                {
                    this.working = event.run !== undefined;
                    this.pending = event.inbox.filter(
                        (item) => item.mode !== "write",
                    ).length;
                    this.rest();
                    if (!this.working) {
                        const last = event.entries.findLast(
                            (entry) => entry.model?.[0]?.role === "assistant",
                        )?.model?.[0];
                        this.faulted =
                            last?.role === "assistant" &&
                            (last.stopReason === "error" ||
                                last.stopReason === "aborted");
                        if (last?.role === "assistant")
                            for (const block of last.content) {
                                if (block.type === "text")
                                    this.speak(block.text, false);
                                else if (block.type === "thinking")
                                    this.speak(block.thinking, true);
                            }
                        return;
                    }
                    this.faulted = false;
                    this.think();
                    // The active run's input heads its work, even after a tool-round handover.
                    const start = event.entries.findLastIndex(
                        (entry) => entry.model?.[0]?.role === "user",
                    );
                    for (const entry of event.entries.slice(
                        Math.max(0, start + 1),
                    )) {
                        const message = entry.model?.[0];
                        if (message?.role === "assistant") {
                            for (const block of message.content) {
                                if (block.type === "text")
                                    this.speak(block.text, false);
                                else if (block.type === "thinking")
                                    this.speak(block.thinking, true);
                                else {
                                    this.#calls.add(block.id);
                                    this.tool(block.name, block.arguments);
                                }
                            }
                        } else if (message?.role === "toolResult")
                            this.ended(message.isError);
                    }
                    for (const block of event.generation?.message?.content ??
                        []) {
                        if (block.type === "text")
                            this.speak(block.text, false);
                        else if (block.type === "thinking")
                            this.speak(block.thinking, true);
                    }
                    return;
                }
            case "run_start":
                this.#calls.clear();
                this.#blocks.clear();
                this.rest();
                this.working = true;
                this.faulted = false;
                this.saying = { text: "", thought: false };
                return;
            case "turn_start":
                this.think();
                return;
            case "run_end":
                this.working = false;
                this.rest();
                return;
            case "inbox_update":
                this.pending = event.items.filter(
                    (item) => item.mode !== "write",
                ).length;
                return;
            case "message_start":
                if (event.message.role !== "assistant") return;
                this.#blocks.clear();
                for (const [index, block] of event.message.content.entries())
                    if (block.type !== "toolCall") this.block(block, index);
                return;
            case "message_update":
                for (const change of event.changes) {
                    if (
                        change.type === "text_delta" ||
                        change.type === "thinking_delta"
                    ) {
                        this.think();
                        this.#blocks.set(
                            change.contentIndex,
                            (this.#blocks.get(change.contentIndex) ?? "") +
                                change.delta,
                        );
                        this.speak(
                            change.delta,
                            change.type === "thinking_delta",
                        );
                    } else if ("block" in change) {
                        if (change.block.type !== "toolCall")
                            this.block(change.block, change.contentIndex);
                    } else if (change.type === "message") {
                        for (const [
                            index,
                            block,
                        ] of change.message.content.entries())
                            if (block.type !== "toolCall")
                                this.block(block, index);
                    }
                }
                return;
            case "message_end": {
                const message = event.entry.model?.[0];
                if (message?.role === "assistant") {
                    for (const [index, block] of message.content.entries())
                        this.block(block, index);
                    this.faulted =
                        message.stopReason === "error" ||
                        message.stopReason === "aborted";
                }
                return;
            }
            case "tool_execution_start":
                if (!this.#calls.has(event.toolCallId)) {
                    this.#calls.add(event.toolCallId);
                    this.tool(event.toolName, event.args);
                }
                return;
            case "tool_execution_end": {
                const message = event.entry?.model?.[0];
                const result =
                    message?.role === "toolResult" ? message : event.result;
                this.ended(result?.isError ?? true);
                return;
            }
        }
    }

    private block(
        block: import("@earendil-works/pi-ai").AssistantMessage["content"][number],
        index: number,
    ): void {
        if (block.type === "toolCall") {
            if (!this.#calls.has(block.id)) {
                this.#calls.add(block.id);
                this.tool(block.name, block.arguments);
            }
            return;
        }
        const text = block.type === "text" ? block.text : block.thinking;
        const previous = this.#blocks.get(index) ?? "";
        this.#blocks.set(index, text);
        if (text === previous) return;
        this.think();
        this.speak(
            text.startsWith(previous) ? text.slice(previous.length) : text,
            block.type === "thinking",
        );
    }

    private rest(): void {
        this.consulting = false;
        this.activity = activityOf();
    }
    private think(): void {
        this.consulting = false;
        if (this.activity.doing === "think") return;
        this.activity.doing = "think";
        this.activity.doingWords = WORDS.think;
        this.activity.step++;
    }
    private tool(name: string, args: unknown): void {
        this.consulting = name === "ask_librarian" || name === "submit_skill";
        const doing = doingOf(name, args);
        this.activity.doing = doing;
        this.activity.doingWords =
            name === "request_road"
                ? "Asking the mayor for a road"
                : name === "agree_road"
                  ? "Waiting for the other mayor about a road"
                  : name === "agree_move"
                    ? "Waiting for the other mayor about a move"
                    : this.consulting
                      ? "Consulting the librarian"
                      : WORDS[doing];
        this.activity.step++;
        this.saying = { text: "", thought: true };
    }
    private ended(failed: boolean): void {
        this.consulting = false;
        if (failed) this.activity.failures++;
        this.activity.doing = "think";
        this.activity.doingWords = WORDS.think;
        this.activity.step++;
    }
    private speak(text: string, thought: boolean): void {
        if (thought !== this.saying.thought)
            this.saying = { text: "", thought };
        const chars = [...(this.saying.text + text)];
        let bytes = 0;
        const kept: string[] = [];
        for (const char of thought ? chars : chars.reverse()) {
            bytes += Buffer.byteLength(char);
            if (bytes > 600) break;
            kept.push(char);
        }
        this.saying.text = (thought ? kept : kept.reverse()).join("");
    }
}

function doingOf(name: string, args: unknown): Doing {
    const text = JSON.stringify(args).toLowerCase();
    const paper = text.includes("notes.md")
        ? "notes"
        : /skills\/|skill.md/.test(text)
          ? "skill"
          : "file";
    if (["read", "search", "find"].includes(name)) return `read-${paper}`;
    if (name === "edit" || name === "write") return `write-${paper}`;
    if (name === "bash" || name === "powershell") {
        if (text.includes("skill.md")) return "read-skill";
        if (
            [
                "curl ",
                "wget ",
                "http://",
                "https://",
                "web-search",
                "read-webpage",
                "weather",
                "call-api",
            ].some((word) => text.includes(word))
        )
            return "fetch";
        return text.includes("notes.md") ? "read-notes" : "run";
    }
    if (name === "create_agent") return "think";
    return [
        "ask_agent",
        "request_road",
        "agree_road",
        "agree_move",
        "ask_librarian",
        "submit_skill",
    ].includes(name)
        ? "await"
        : "custom";
}
