import { spawn } from "node:child_process";
import { once } from "node:events";
import { closeAfter } from "./support.ts";

/** A parent directory swapped for an outside link while the caller uses it. */
export async function swappingDirectory(
    path: string,
    outside: string,
): Promise<void> {
    const swapper = spawn(
        process.execPath,
        [
            "--input-type=module",
            "-e",
            `
        import { symlinkSync, renameSync, unlinkSync } from 'node:fs';
        const [path, outside] = process.argv.slice(1);
        process.stdout.write('ready');
        const pause = () => new Promise(resolve => setTimeout(resolve, 1));
        for (;;) {
            await pause();
            renameSync(path, path + '-kept');
            symlinkSync(outside, path);
            await pause();
            unlinkSync(path);
            renameSync(path + '-kept', path);
        }
    `,
            path,
            outside,
        ],
        { stdio: ["ignore", "pipe", "inherit"] },
    );
    closeAfter(async () => {
        swapper.kill();
        await once(swapper, "close");
    });
    await once(swapper.stdout, "data");
}
