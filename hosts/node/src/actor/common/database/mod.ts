// This file was changed for Hamlet. See NOTICE in this package.
import { getLogger } from "../log.ts";
import type {
    DatabaseProvider,
    NativeDatabaseProvider,
    RawAccess,
    SqliteDatabase,
    SqliteProfilingOptions,
    SqliteTransactionDatabase,
    SqliteTransactionOptions,
} from "./config.ts";
import {
    isManualTransactionControl,
    isSqliteBindingObject,
    MIGRATION_TRANSACTION_TIMEOUT_MS,
    toSqliteBindings,
    validateTransactionName,
    validateTransactionTimeout,
} from "./shared.ts";

export type { RawAccess } from "./config.ts";

export interface DatabaseFactoryConfig {
    onMigrate?: (db: RawAccess) => Promise<void> | void;
    warnOnManualTransactions?: boolean;
    /**
     * SQLite profiling configuration.
     *
     * @experimental This entire configuration surface is experimental and
     * subject to change without notice.
     */
    profiling?: SqliteProfilingOptions;
}
const nativeStateTransactionOpeners = new WeakMap<
    NativeDatabaseProvider,
    (
        timeoutMs?: number,
        context?: unknown,
    ) => Promise<SqliteTransactionDatabase>
>();
type NativeStateTransactionContext = {
    enter(): Promise<unknown>;
    exit(scope: unknown): void;
};
const nativeStateTransactionClientBinders = new WeakMap<
    object,
    (context: NativeStateTransactionContext) => object
>();

/** @internal */
export function registerNativeStateTransactionOpener<
    T extends NativeDatabaseProvider,
>(
    provider: T,
    opener: (
        timeoutMs?: number,
        context?: unknown,
    ) => Promise<SqliteTransactionDatabase>,
): T {
    nativeStateTransactionOpeners.set(provider, opener);
    return provider;
}

/** @internal */
export function bindNativeStateTransactionContext<T>(
    client: T,
    context: NativeStateTransactionContext,
): T {
    if (
        (typeof client !== "object" || client === null) &&
        typeof client !== "function"
    ) {
        return client;
    }
    const bind = nativeStateTransactionClientBinders.get(client as object);
    return (bind?.(context) ?? client) as T;
}

function hasMultipleStatements(query: string): boolean {
    const trimmed = query.trim().replace(/;+$/, "").trimEnd();
    return trimmed.includes(";");
}

export function db({
    onMigrate,
    warnOnManualTransactions = true,
    profiling,
}: DatabaseFactoryConfig = {}): DatabaseProvider<RawAccess> {
    const provider: DatabaseProvider<RawAccess> = {
        sqliteProfiling: profiling,
        createClient: async (ctx) => {
            const nativeDatabaseProvider = ctx.nativeDatabaseProvider;
            if (!nativeDatabaseProvider) {
                throw new Error(
                    "native SQLite is required, but the current runtime did not provide a native database provider",
                );
            }

            const db = await nativeDatabaseProvider.open(ctx.actorId);
            let closed = false;
            let manualTransactionWarned = false;
            const ensureOpen = () => {
                if (closed) {
                    throw new Error(
                        "Database is closed. This usually means a background timer (setInterval, setTimeout) or a stray promise is still running after the actor stopped. Use c.abortSignal to clean up timers before the actor shuts down.",
                    );
                }
            };

            const createClient = (
                target: SqliteDatabase | SqliteTransactionDatabase,
                transactionScoped = false,
                stateTransactionContext?: NativeStateTransactionContext,
            ): RawAccess => {
                // `execute`'s accounting of one SQL text: the open check,
                // the manual-transaction warning, metrics and the debug log.
                const tracked = async <T>(
                    query: string,
                    run: () => Promise<T>,
                ): Promise<T> => {
                    ensureOpen();
                    if (
                        !transactionScoped &&
                        warnOnManualTransactions &&
                        !manualTransactionWarned &&
                        !hasMultipleStatements(query) &&
                        isManualTransactionControl(query)
                    ) {
                        manualTransactionWarned = true;
                        getLogger("database").warn(
                            { actorId: ctx.actorId },
                            "Manual cross-call SQLite transactions can interleave with other actor work. Use db.transaction() for coordinated transactions. Set warnOnManualTransactions: false in your db(...) configuration to disable this warning.",
                        );
                    }

                    const kvReadsBefore = ctx.metrics?.totalKvReads ?? 0;
                    const kvWritesBefore = ctx.metrics?.totalKvWrites ?? 0;
                    const start = performance.now();

                    try {
                        return await run();
                    } finally {
                        const durationMs = performance.now() - start;
                        ctx.metrics?.trackSql(query, durationMs);
                        if (ctx.metrics) {
                            const kvReads =
                                ctx.metrics.totalKvReads - kvReadsBefore;
                            const kvWrites =
                                ctx.metrics.totalKvWrites - kvWritesBefore;
                            ctx.log?.debug({
                                msg: "sql query",
                                query: query.slice(0, 120),
                                durationMs,
                                kvReads,
                                kvWrites,
                            });
                        }
                    }
                };
                const statement = (query: string, args: unknown[]) =>
                    tracked(query, () =>
                        target.execute(
                            query,
                            args.length === 0
                                ? undefined
                                : args.length === 1 &&
                                    isSqliteBindingObject(args[0])
                                  ? toSqliteBindings(args[0])
                                  : toSqliteBindings(args),
                        ),
                    );
                const rows = async <TRow extends Record<string, unknown>>(
                    query: string,
                    args: unknown[],
                ): Promise<TRow[]> => {
                    const { rows, columns } = await statement(query, args);
                    return rows.map((row) => rowToObject<TRow>(row, columns));
                };

                const client: RawAccess = {
                    execute: <
                        TRow extends Record<string, unknown> = Record<
                            string,
                            unknown
                        >,
                    >(
                        query: string,
                        ...args: unknown[]
                    ): Promise<TRow[]> =>
                        args.length === 0 && hasMultipleStatements(query)
                            ? tracked(query, () =>
                                  execMultiStatement<TRow>(target, query),
                              )
                            : rows<TRow>(query, args),
                    get: async <TRow extends Record<string, unknown>>(
                        query: string,
                        ...args: unknown[]
                    ) => (await rows<TRow>(query, args))[0],
                    all: <TRow extends Record<string, unknown>>(
                        query: string,
                        ...args: unknown[]
                    ) => rows<TRow>(query, args),
                    run: async (query, ...args) => {
                        const { changes, lastInsertRowId } = await statement(
                            query,
                            args,
                        );
                        return {
                            changes,
                            lastInsertRowId: lastInsertRowId ?? null,
                        };
                    },
                    transaction: async <T>(
                        callback: (tx: RawAccess) => Promise<T> | T,
                        options?: SqliteTransactionOptions,
                    ): Promise<T> => {
                        validateTransactionTimeout(options?.timeout);
                        validateTransactionName(options?.name);
                        if (
                            transactionScoped &&
                            options?.experimental?.includeState
                        ) {
                            throw new Error(
                                "experimental.includeState is not supported for nested transactions",
                            );
                        }
                        const includeState =
                            options?.experimental?.includeState === true;
                        const stateScope =
                            includeState && stateTransactionContext
                                ? await stateTransactionContext.enter()
                                : undefined;
                        try {
                            const transaction = includeState
                                ? await (() => {
                                      const beginStateTransaction =
                                          ctx.nativeDatabaseProvider &&
                                          nativeStateTransactionOpeners.get(
                                              ctx.nativeDatabaseProvider,
                                          );
                                      if (!beginStateTransaction) {
                                          throw new Error(
                                              "experimental.includeState is only supported by the embedded database provider",
                                          );
                                      }
                                      return beginStateTransaction(
                                          options?.timeout,
                                          stateScope,
                                      );
                                  })()
                                : await db.beginTransaction(
                                      options?.timeout,
                                      options?.name,
                                  );
                            const tx = createClient(transaction, true);
                            try {
                                const result = await callback(tx);
                                await transaction.commit();
                                return result;
                            } catch (error) {
                                try {
                                    await transaction.rollback();
                                } catch {
                                    // Preserve the callback or commit error after expiry cleanup.
                                }
                                throw error;
                            }
                        } finally {
                            if (stateScope !== undefined) {
                                stateTransactionContext?.exit(stateScope);
                            }
                        }
                    },
                    close: async () => {
                        if (!closed) {
                            closed = true;
                            await db.close();
                        }
                    },
                    nativeMetrics: () => db.nativeMetrics?.() ?? null,
                };
                if (!transactionScoped) {
                    nativeStateTransactionClientBinders.set(client, (context) =>
                        createClient(target, false, context),
                    );
                }
                return client;
            };
            const client = createClient(db);
            return client;
        },
        onMigrate: async (client) => {
            if (onMigrate) {
                await withMigrationSavepoint(client, (leased) =>
                    onMigrate(leased),
                );
            }
        },
    };
    return provider;
}

function rowToObject<TRow extends Record<string, unknown>>(
    row: unknown[],
    columns: string[],
): TRow {
    const rowObj: Record<string, unknown> = {};
    for (const [i, column] of columns.entries()) {
        rowObj[column] = row[i];
    }
    return rowObj as TRow;
}

async function execMultiStatement<TRow extends Record<string, unknown>>(
    db: Pick<SqliteDatabase, "exec">,
    query: string,
): Promise<TRow[]> {
    const results: Record<string, unknown>[] = [];
    let columnNames: string[] | null = null;
    await db.exec(query, (row: unknown[], columns: string[]) => {
        if (!columnNames) {
            columnNames = columns;
        }
        results.push(rowToObject(row, columnNames));
    });
    return results as TRow[];
}

async function withMigrationSavepoint<T>(
    client: RawAccess,
    callback: (leased: RawAccess) => Promise<T> | T,
): Promise<T> {
    return await client.transaction(
        async (transaction) => {
            await transaction.execute("SAVEPOINT __actor_on_migrate");
            try {
                const result = await callback(transaction);
                await transaction.execute(
                    "RELEASE SAVEPOINT __actor_on_migrate",
                );
                return result;
            } catch (error) {
                try {
                    await transaction.execute(
                        "ROLLBACK TO SAVEPOINT __actor_on_migrate",
                    );
                } finally {
                    await transaction.execute(
                        "RELEASE SAVEPOINT __actor_on_migrate",
                    );
                }
                throw error;
            }
        },
        {
            name: "actor-migration",
            timeout: MIGRATION_TRANSACTION_TIMEOUT_MS,
        },
    );
}
