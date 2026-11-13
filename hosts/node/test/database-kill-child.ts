// Runs one `tell` on an agent at the file named first, and kills itself
// with SIGKILL at the point named second. It prints the point first, so the
// parent knows the kill was not some other death.
import { writeSync } from "node:fs";
import { fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import type { SqliteDatabase } from "@earendil-works/pi-durable/storage/sqlite";
import { context, openAgent } from "./testing.ts";

const [file, point] = process.argv.slice(2);
if (file === undefined || point === undefined) {
    throw new Error("usage: database-kill-child.ts <file> <point>");
}

const reach = (name: string) => {
    if (name === point) {
        writeSync(1, `reached ${name}\n`);
        process.kill(process.pid, "SIGKILL");
    }
};

/** The database under the layer, with the kill points around its letters. */
const killing = (database: SqliteDatabase): SqliteDatabase => ({
    ...database,
    transaction: async (callback) => {
        let wrote = false;
        const result = await database.transaction(async (tx) => {
            const value = await callback({
                ...tx,
                run: async (sql, ...params) => {
                    const letter = sql.startsWith("INSERT INTO stand_in_sends");
                    if (letter) {
                        reach("after-result-insert");
                    }
                    await tx.run(sql, ...params);
                    if (letter) {
                        wrote = true;
                        reach("after-send-insert");
                    }
                },
            });
            if (wrote) {
                reach("before-commit");
            }
            return value;
        });
        if (wrote) {
            reach("after-commit");
        }
        return result;
    },
});

const agent = await openAgent(file, {
    base: killing,
    afterBuffer: () => reach("after-buffer"),
});
agent.faux.setResponses([
    fauxAssistantMessage(
        [fauxToolCall("tell", { to: "bob", body: "hi" }, { id: "letter" })],
        { stopReason: "toolUse" },
    ),
    fauxAssistantMessage("done"),
]);
const submission = await agent.root.submit(
    { type: "input", content: "write" },
    context,
);
await submission.wait(context);
await agent.close();
writeSync(1, "never killed\n");
