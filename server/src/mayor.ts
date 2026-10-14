import { Type } from "@earendil-works/pi-ai";
import { defineExtension } from "@earendil-works/pi-durable";
import { defineRecordTool, RecordDoc } from "./record.ts";

const createAgent = defineRecordTool({
    name: "create_agent",
    description:
        "Create an agent in your hamlet, with its purpose and your reason.",
    parameters: Type.Object({
        name: Type.String(),
        purpose: Type.String(),
        reason: Type.String(),
    }),
    change: async (tx, { name, purpose, reason }) => {
        const record = await tx.doc(RecordDoc);
        const number = record.agents.length + 1;
        record.agents.push({ number, name, purpose });
        record.decisions.push({
            what: `Created ${name}`,
            reason,
            at: Date.now(),
        });
        return `Created ${name}, agent #${number}.`;
    },
});

/** The mayor's tools (H35). */
export const Mayor = defineExtension({
    name: "hamlet.mayor",
    tools: [createAgent],
});
