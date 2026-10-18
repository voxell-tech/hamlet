import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Points where a test stops this runtime, so it can be killed there. */
export type FaultPoint =
    | "before_admit"
    | "after_admit"
    | "before_cancelled"
    | "before_operation"
    | "after_receive";

/**
 * Stops for good at `point` when `HAMLET_HARNESS_FAULT` names it, once per `HAMLET_FAULT_DIR`: the stop writes
 * `runtime-paused` there, and a runtime started after it passes the point. Does nothing in other runs.
 */
export async function fault(point: FaultPoint): Promise<void> {
    const dir = process.env.HAMLET_FAULT_DIR;
    if (process.env.HAMLET_HARNESS_FAULT !== point || dir === undefined) return;
    const paused = join(dir, "runtime-paused");
    if (existsSync(paused)) return;
    writeFileSync(paused, point);
    await new Promise<never>(() => {});
}
