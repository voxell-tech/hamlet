import { rename, rm, writeFile } from "node:fs/promises";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { FAUX_MODEL, readScript, scriptedModels } from "./faux.ts";
import { openHome } from "./home.ts";
import { probeSandbox } from "./sandbox.ts";
import { serveHome } from "./server.ts";

/** A home served until closed in order or stopped by a signal (S1, S3). */
export async function serve(
    dir: string,
    options: {
        port: number;
        faux: string | undefined;
        beforeClose?: () => void | Promise<void>;
    },
) {
    probeSandbox();
    const context = BACKGROUND_CONTEXT;
    const faux =
        options.faux === undefined
            ? undefined
            : scriptedModels(await readScript(options.faux));
    const home = await openHome(
        dir,
        faux === undefined
            ? {}
            : {
                  models: faux,
                  env: { ...process.env, HAMLET_MODEL: FAUX_MODEL },
              },
        context,
    );
    const server = await serveHome(home, options.port, context).catch(
        async (error: unknown) => {
            await home.close(context);
            throw error;
        },
    );
    const written = `${home.layout.server}.${process.pid}.tmp`;
    try {
        // The address lands whole; the token stays in credentials.json.
        await writeFile(
            written,
            `${JSON.stringify({ url: server.url, pid: process.pid })}\n`,
        );
        await rename(written, home.layout.server);
    } catch (error) {
        await rm(written, { force: true });
        await server.close();
        await home.close(context);
        throw error;
    }
    process.stderr.write(
        `hamlet: serving ${home.dir} at ${server.url}; Ctrl-C stops\n`,
    );
    void home.harness.closed.then((end) => {
        if (end.reason !== "failed") return;
        process.stderr.write(
            `Error: the home's storage failed: ${end.error}\n`,
        );
        process.exit(1);
    });
    let closing: Promise<void> | undefined;
    const close = () => {
        closing ??= (async () => {
            await options.beforeClose?.();
            await rm(home.layout.server, { force: true });
            await server.close();
            await home.close(context);
            process.off("SIGINT", stop);
            process.off("SIGTERM", stop);
        })();
        return closing;
    };
    const stop = () => {
        if (closing !== undefined) process.exit(130);
        process.stderr.write("hamlet: stopping; Ctrl-C again stops at once\n");
        void close().then(
            () => process.exit(0),
            (error: unknown) => {
                process.stderr.write(`Error: ${String(error)}\n`);
                process.exit(1);
            },
        );
    };
    process.on("SIGINT", stop);
    process.on("SIGTERM", stop);
    return {
        url: server.url,
        token: home.credentials.token,
        home: home.dir,
        close,
    };
}
