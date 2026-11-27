/** The one turn a worker or an errand runs, and how it ended for the realm. */
import type { Context } from "@earendil-works/chord/context";
import type {
    Conversation,
    Cursor,
    EntryId,
    SettledSubmissionRecord,
} from "@earendil-works/pi-durable";
import { type PiOptions, withHarness } from "../pi/runtime.ts";
import type { AgentContext } from "./actor.ts";
import type { Brief } from "./messages/agent.ts";
import type { Ended } from "./messages/realm.ts";
import type { RealmTask } from "./messages/shared.ts";
import { draftOf, type Peers } from "./turn.ts";

/** pi-durable admits it once: the brief is the root conversation's one input. */
const BRIEF_ID = "brief";

/**
 * Each wake admits a worker's or an errand's one turn again under `BRIEF_ID`,
 * so a call a kill cut off is made again and the turn never runs twice.
 */
export async function briefTurn(
    c: AgentContext,
    options: PiOptions,
    peers: Peers,
    kind: "worker" | "errand",
    brief: Brief,
    task: RealmTask,
): Promise<void> {
    const settled = await withHarness(
        c,
        options,
        async (harness, context, waiting) => {
            const root = await harness.root(context);
            const submission = await root.submit(
                draftOf({ kind, id: BRIEF_ID, content: brief.text, task }),
                context,
            );
            const settled = await waiting(() => submission.wait(context));
            // A short cut-off is asked again once its input settled (C28).
            await waiting(() => root.waitForIdle(context));
            return settled;
        },
    );
    // A call of its own, so the agent does not sleep before the realm is told.
    await withHarness(c, options, async (harness, context) => {
        const last = await lastOf(
            await harness.root(context),
            settled.entry,
            context,
        );
        const ended = endedOf(kind, settled, last);
        await (kind === "worker"
            ? peers.reach.worker_done({ ended })
            : peers.reach.errand_done({ ended }));
    });
}

/** How an errand's and a worker's turn ends. */
function endedOf(
    kind: "worker" | "errand",
    settled: SettledSubmissionRecord,
    last: Last,
): Ended {
    const failed = (why: string): Ended => ({ type: "failed", why });
    if (settled.status === "done") {
        if (last.words !== undefined)
            return { type: "answered", text: last.words };
        return failed(
            last.notice ??
                (kind === "errand"
                    ? "the errand gave no answer"
                    : "its turn failed"),
        );
    }
    if (settled.reason === "aborted") return failed("its turn was stopped");
    return failed(
        typeof settled.detail === "string" ? settled.detail : settled.reason,
    );
}

interface Last {
    /** The text of the last answer that has any, its blocks joined by lines and trimmed. */
    readonly words?: string;
    readonly notice?: string;
}

async function lastOf(
    conversation: Conversation,
    since: EntryId | undefined,
    context: Context,
): Promise<Last> {
    let notice: string | undefined;
    let cursor: Cursor | undefined;
    do {
        const page = await conversation.entries(
            {
                order: "descending",
                ...(since === undefined ? {} : { minEntryId: since }),
            },
            100,
            cursor,
            context,
        );
        for (const entry of page.items) {
            if (entry.kind === "hamlet.notice") {
                notice ??= (entry.data as { text?: string } | undefined)?.text;
                continue;
            }
            const message = entry.model?.[0];
            if (message?.role !== "assistant") continue;
            const text = message.content
                .flatMap((part) => (part.type === "text" ? [part.text] : []))
                .join("\n")
                .trim();
            if (text !== "") return { words: text };
        }
        cursor = page.next;
    } while (cursor !== undefined);
    return notice === undefined ? {} : { notice };
}
