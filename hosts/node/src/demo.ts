/**
 * The demo's TypeScript kinds, which the Node host of `demo --link` runs.
 */

import { setTimeout as sleep } from "node:timers/promises";
import {
    type ActorSchedule,
    Address,
    actor,
    type Caller,
    type CallOptions,
    event,
    isActorErrorCode,
    UserError,
} from "./actor/actor/mod.ts";
import { db } from "./actor/db/mod.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };

const REALM = Address.of("realm", "local");

const FIRED_POLL_MS = 25;

/**
 * The errors of a call that `ask` answers as what the agent read, each as
 * its group and code. A call that fails with any other fails `ask`.
 */
const READ = [
    ["message", "wrong_caller"],
    ["actor", "refused"],
] as const;

function shown(caller: Caller | null): string {
    if (caller === null) return "no caller";
    if (caller === "client") return "client";
    return `${caller.actor.kind}/${caller.actor.key}`;
}

/** Arms a one-shot of `step`, `everyMs` ahead, unless one is pending. */
async function armStep(schedule: ActorSchedule, everyMs: number) {
    const pending = await schedule.list();
    if (!pending.some((one) => one.action === "step")) {
        await schedule.after(everyMs, "step");
    }
}

/**
 * A stand-in for an agent, `agent/<n>`: it keeps a note for each call of
 * `note`, calls other actors as itself, arms one-shots, keeps a count on a
 * chain of one-shots, and can end its own process at a step of an action.
 */
const agent = actor({
    db: db({
        onMigrate: async (raw) => {
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, caller TEXT NOT NULL)",
            );
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS fired (id INTEGER PRIMARY KEY, at INTEGER NOT NULL)",
            );
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS jobs (id INTEGER PRIMARY KEY)",
            );
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS ticks (id INTEGER PRIMARY KEY CHECK (id = 0), count INTEGER NOT NULL, every_ms INTEGER NOT NULL)",
            );
        },
    }),
    events: {
        /** The count of `step`, once its commit returned. */
        ticked: event<number>(),
        /** The id of `note`'s note, refused to a client. */
        noted: event<number>({
            canSubscribe: (c) => c.caller !== "client",
        }),
    },
    /**
     * Finishes each job a kill cut off, with a note naming `resumed`, and
     * tells the realm how many.
     */
    onWake: async (c) => {
        const finished = await c.db.transaction(async (tx) => {
            const { changes } = await tx.run(
                "INSERT INTO notes (caller) SELECT 'resumed' FROM jobs",
            );
            await tx.run("DELETE FROM jobs");
            return changes;
        });
        if (finished > 0) {
            await c.call(REALM, "resumed", [finished], CALL);
        }
    },
    /** Says on standard error that the agent goes to sleep. */
    onSleep: (c) => {
        console.error(`${c.address.kind}/${c.address.key} sleeps`);
    },
    actions: {
        /**
         * Keeps a note that names the caller, in one statement, says who
         * called on standard error, broadcasts the note's id as `noted`, and
         * answers how many notes there are.
         */
        note: async (c) => {
            const { lastInsertRowId } = await c.db.run(
                "INSERT INTO notes (caller) VALUES (?)",
                shown(c.caller),
            );
            console.error(
                `${c.address.kind}/${c.address.key} called by ${shown(c.caller)}`,
            );
            c.broadcast("noted", lastInsertRowId);
            return lastInsertRowId;
        },
        count: async (c) =>
            (await c.db.get<{ n: number }>("SELECT count(*) AS n FROM notes"))
                ?.n ?? 0,
        /**
         * Keeps a job, in one statement, then ends this process with SIGKILL
         * before it finishes the job or answers.
         */
        begin: async (c) => {
            await c.db.run("INSERT INTO jobs DEFAULT VALUES");
            process.kill(process.pid, "SIGKILL");
            // The signal may arrive after `kill` returns.
            await sleep(CALL.deadlineMs);
            throw new Error("the Node host outlived its own SIGKILL");
        },
        /** Fails with a coded error that carries metadata. */
        refuse: () => {
            throw new UserError("refused by the demo", {
                code: "demo_refused",
                metadata: { n: 2 ** 32, list: [1, "two"] },
            });
        },
        /** Arms a one-shot of `fire`, `ms` ahead. Answers how many it holds. */
        arm: async (c, ms: number) => {
            await c.schedule.at(Date.now() + ms, "fire");
            return (await c.schedule.list()).length;
        },
        fire: async (c) => {
            await c.db.run("INSERT INTO fired (at) VALUES (?)", Date.now());
        },
        /**
         * How many one-shots have fired, read once `atLeast` have or
         * `withinMs` have passed, whichever is first.
         */
        fired: async (c, atLeast: number, withinMs: number) => {
            const until = Date.now() + withinMs;
            for (;;) {
                const fired =
                    (
                        await c.db.get<{ n: number }>(
                            "SELECT count(*) AS n FROM fired",
                        )
                    )?.n ?? 0;
                if (fired >= atLeast || Date.now() >= until) return fired;
                await sleep(FIRED_POLL_MS);
            }
        },
        /**
         * Starts the chain of `step`, one each `everyMs`, unless it runs.
         * Answers the count so far.
         */
        tick: async (c, everyMs: number) => {
            const row = await c.db.get<{ count: number }>(
                "INSERT INTO ticks (id, count, every_ms) VALUES (0, 0, ?) ON CONFLICT (id) DO UPDATE SET every_ms = excluded.every_ms RETURNING count",
                everyMs,
            );
            await armStep(c.schedule, everyMs);
            return row?.count;
        },
        /**
         * Adds one to the count, broadcasts it as `ticked` once committed,
         * then arms the next `step` unless another is pending.
         */
        step: async (c) => {
            const { count, everyMs } = await c.db.transaction(async (tx) => {
                await tx.run("UPDATE ticks SET count = count + 1");
                const row = await tx.get<{ count: number; every_ms: number }>(
                    "SELECT count, every_ms FROM ticks",
                );
                if (row === undefined) {
                    throw new Error("step runs only after tick");
                }
                return { count: row.count, everyMs: row.every_ms };
            });
            c.broadcast("ticked", count);
            await armStep(c.schedule, everyMs);
        },
        ticks: async (c) =>
            (await c.db.get<{ count: number }>("SELECT count FROM ticks"))
                ?.count ?? 0,
        pid: () => process.pid,
        /** The Node host's resident memory, in kB of 1,000 bytes. */
        rss: () => Math.round(process.memoryUsage.rss() / 1_000),
        /**
         * Calls `action` of the actor `kind`/`key` as this agent, with
         * `args`. Answers what the agent read: the answer and its JavaScript
         * type, or an error of `READ` with its message and metadata.
         */
        ask: async (
            c,
            kind: string,
            key: string | number,
            action: string,
            ...args: unknown[]
        ) => {
            try {
                const answer = await c.call(
                    Address.of(kind, String(key)),
                    action,
                    args,
                    CALL,
                );
                return { answer, type: typeof answer };
            } catch (error) {
                for (const [group, code] of READ) {
                    if (isActorErrorCode(error, group, code)) {
                        return {
                            error: {
                                group,
                                code,
                                message: error.message,
                                metadata: error.metadata ?? null,
                            },
                        };
                    }
                }
                throw error;
            }
        },
    },
});

/** Every kind the Node host can run, by kind. */
export const kinds = { agent } as const;
