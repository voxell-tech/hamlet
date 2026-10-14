import { defineDoc, type Tx } from "@earendil-works/pi-durable";
import { type AgentRef, agentKey } from "./hamlet.ts";
import type { Limits } from "./limits.ts";

export type Permission = "network" | "unconfined";

/** A lasting permission kept by the agent's number (C9, H11). */
export type Grant = {
    agent: AgentRef;
    name: string;
    permission: Permission;
    by: string;
};

/** A line of the append-only record of permissions. */
export type GrantEvent =
    | (Grant & { type: "granted"; at: number })
    | {
          type: "revoked";
          agent: AgentRef;
          permission: Permission;
          by: string;
          at: number;
      };

/** The home's record of permissions given and taken back. */
export const GrantsDoc = defineDoc<{ events: GrantEvent[] }>({
    kind: "hamlet.grants",
    version: 1,
    scope: "session",
    initial: () => ({ events: [] }),
});

/** The grants still held, after the latest event for each agent and permission. */
export function grantsOf(
    stored: Readonly<{ events: GrantEvent[] }> | undefined,
): Grant[] {
    const held = new Map<string, Grant>();
    for (const event of stored?.events ?? []) {
        const key = `${agentKey(event.agent)}:${event.permission}`;
        held.delete(key);
        if (event.type === "granted")
            held.set(key, {
                agent: { ...event.agent },
                name: event.name,
                permission: event.permission,
                by: event.by,
            });
    }
    return [...held.values()];
}

/** The permission an agent has by office or name in the limits, or by its number in the grants. */
export function holds(
    permissions: Limits["permissions"],
    grants: readonly Grant[],
    name: string,
    agent: AgentRef,
    permission: Permission,
): boolean {
    return (
        permissions[permission].includes(name) ||
        grants.some(
            (grant) =>
                agentKey(grant.agent) === agentKey(agent) &&
                grant.permission === permission,
        )
    );
}

/** A grant to `agent`, given in `name` by the chief `by`. */
export async function give(
    tx: Tx,
    agent: AgentRef,
    name: string,
    permission: Permission,
    by: string,
    at: number,
): Promise<void> {
    (await tx.doc(GrantsDoc)).events.push({
        type: "granted",
        at,
        agent,
        name,
        permission,
        by,
    });
}

/** A revocation recorded only when `agent` holds `permission`. */
export async function revoke(
    tx: Tx,
    agent: AgentRef,
    permission: Permission,
    by: string,
    at: number,
): Promise<boolean> {
    const record = await tx.doc(GrantsDoc);
    if (
        !grantsOf(record).some(
            (grant) =>
                agentKey(grant.agent) === agentKey(agent) &&
                grant.permission === permission,
        )
    )
        return false;
    record.events.push({ type: "revoked", at, agent, permission, by });
    return true;
}
