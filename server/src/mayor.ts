import { Type } from "@earendil-works/pi-ai";
import { defineExtension } from "@earendil-works/pi-durable";
import { HOME, MAYOR } from "./hamlet.ts";
import type { Part } from "./parts.ts";
import { defineRecordTool, RecordDoc } from "./record.ts";

/** The mayor's tools (H35). */
export const MayorTools: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.mayor",
        tools: [
            defineRecordTool({
                name: "create_agent",
                description:
                    "Create an agent in your hamlet, with its purpose and your reason.",
                parameters: Type.Object({
                    name: Type.String(),
                    purpose: Type.String(),
                    reason: Type.String(),
                }),
                change: async (tx, { name, purpose, reason }) => {
                    const record = await tx.doc(RecordDoc, HOME, HOME);
                    const number = record.next++;
                    const at = home.now();
                    record.agents.push({
                        number,
                        name,
                        purpose,
                        since: at,
                        model: null,
                    });
                    record.decisions.push({
                        kind: "created",
                        name,
                        purpose,
                        at,
                        by: MAYOR,
                        agent: number,
                        reason,
                    });
                    return `Created ${name}, agent #${number}.`;
                },
            }),
        ],
    }),
});
