import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { type Static, Type } from "typebox";
import { Value } from "typebox/value";
import { connect } from "../src/connection.ts";
import { layoutOf } from "../src/home/layout.ts";
import { err, ok, type Result } from "../src/result.ts";

/** The address and token of a chief's server. */
export type ServerAddress = { url: string; token: string };

const ServerRecordSchema = Type.Object({
    url: Type.String(),
    pid: Type.Integer({ minimum: 1 }),
    started: Type.Optional(Type.String()),
});

/** A local server's published identity; older servers omit the start time. */
export type ServerRecord = Static<typeof ServerRecordSchema>;

/** An unreadable or malformed server record, which is not safe to replace. */
export type DiscoveryFailure = { kind: "server-record"; home: string };

/** Reads the published identity without treating an unreadable file as an absent server. */
export async function readServer(
    dir: string,
): Promise<Result<ServerRecord | null, DiscoveryFailure>> {
    const layout = layoutOf(resolve(dir));
    let text: string;
    try {
        text = await readFile(layout.server, "utf8");
    } catch (error) {
        if (
            error instanceof Error &&
            "code" in error &&
            error.code === "ENOENT"
        )
            return ok(null);
        return err({ kind: "server-record", home: layout.dir });
    }
    try {
        const value: unknown = JSON.parse(text);
        return Value.Check(ServerRecordSchema, value)
            ? ok(value)
            : err({ kind: "server-record", home: layout.dir });
    } catch {
        return err({ kind: "server-record", home: layout.dir });
    }
}

/** The local server's address, or the server the environment names. */
export async function reachHome(
    dir: string,
    env: NodeJS.ProcessEnv,
    server?: ServerRecord,
): Promise<ServerAddress> {
    if (env.HAMLET_SERVER) {
        if (!env.HAMLET_TOKEN)
            throw new Error(
                "HAMLET_SERVER needs HAMLET_TOKEN, its home's token",
            );
        return { url: env.HAMLET_SERVER, token: env.HAMLET_TOKEN };
    }
    const layout = layoutOf(resolve(dir));
    const found = server === undefined ? await readServer(dir) : ok(server);
    if (!found.ok || found.value === null) {
        throw new Error(
            `no server runs for ${layout.dir}; start one with hamlet server, or use hamlet dev for a scratch server and window.`,
        );
    }
    const { url } = found.value;
    const { connection } = JSON.parse(
        await readFile(layout.credentials, "utf8"),
    ) as { connection: { token: string } };
    return { url, token: connection.token };
}

/** An authenticated connection and the realm's starting state. */
export async function connectChief(
    address: ServerAddress,
    signal?: AbortSignal,
) {
    const connection = await connect(address.url, signal);
    try {
        connection.send({ type: "hello", v: 0, token: address.token });
        const state = await connection.next();
        if (state.type === "fault") throw new Error(state.text);
        if (state.type !== "state")
            throw new Error("The server sent no state.");
        return { connection, state };
    } catch (error) {
        await connection.close();
        throw error;
    }
}
