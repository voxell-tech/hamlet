// Plays an agent's command that swaps a folder of its filesystem for a
// link to a folder outside, and back, as fast as it can, until the file
// named third exists. It keeps the folder it swaps out inside the agent's
// filesystem, so a tool that holds it still writes inside. It prints
// "ready", then how many times the link was in place.
import {
    existsSync,
    renameSync,
    rmSync,
    symlinkSync,
    unlinkSync,
    writeSync,
} from "node:fs";

const [folder, outside, stop] = process.argv.slice(2);
if (folder === undefined || outside === undefined || stop === undefined) {
    throw new Error(
        "usage: filesystem-swap-child.ts <folder> <outside> <stop>",
    );
}
const parked = `${folder}-parked`;

/** Runs `step`; false when the file system refused it. */
const tried = (step: () => void) => {
    try {
        step();
        return true;
    } catch {
        return false;
    }
};

writeSync(1, "ready\n");
let swaps = 0;
for (let turn = 0; ; turn++) {
    if (turn % 64 === 0 && existsSync(stop)) break;
    if (!tried(() => renameSync(folder, parked))) continue;
    if (tried(() => symlinkSync(outside, folder))) {
        swaps++;
        unlinkSync(folder);
    }
    // A tool made the folder again while it was away: keep the tool's.
    if (!tried(() => renameSync(parked, folder))) {
        rmSync(parked, { recursive: true, force: true });
    }
}
writeSync(1, `${swaps}\n`);
