/**
 * `node admit.ts <store> first|again`: submits the input `r1` to a conversation through pi-durable alone. `first`
 * makes the conversation, prints what it admitted and then waits to be killed; `again` reopens the store, submits the
 * same input again and prints what that gave.
 */
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { createModels } from "@earendil-works/pi-ai/models";
import {
    type ConversationId,
    createRegistry,
    defineDoc,
    Harness,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";

const context = BACKGROUND_CONTEXT;
const [store, phase] = process.argv.slice(2);
if (store === undefined || (phase !== "first" && phase !== "again"))
    throw new Error("usage: admit.ts <store> first|again");

const Made = defineDoc<{ conversation: ConversationId | null }>({
    kind: "test.made",
    version: 1,
    scope: "session",
    initial: () => ({ conversation: null }),
});

const harness = await Harness.open(
    await openNodeSqliteStorage(store),
    { models: createModels(), registry: createRegistry(), onReport: () => {} },
    context,
);
let made = (await harness.snapshot(Made, context))?.conversation ?? null;
if (made === null) {
    made = await harness.commit(async (tx) => {
        const record = await tx.createConversation({
            ownership: { kind: "ownerless" },
        });
        (await tx.doc(Made)).conversation = record.id;
        return record.id;
    }, context);
}
const conversation = await harness.conversation(made, context);
if (conversation === undefined) throw new Error("the conversation is gone");
const submission = await conversation.submit(
    {
        type: "input",
        content: "Hello.",
        requestId: "r1",
        whenBusy: "followUp",
    },
    context,
);
process.stdout.write(
    `${JSON.stringify({ submission: String(submission.id) })}\n`,
);
if (phase === "first") setInterval(() => {}, 60_000);
else await harness.close(context);
