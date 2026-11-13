// This file was changed for Hamlet. See NOTICE in this package.
import { z } from "zod/v4";
import {
    getActorEnvoyVersion,
    getActorPool,
    getActorTotalSlots,
} from "@/utils/env-vars";

export const EnvoyConfigSchema = z.object({
    poolName: z.string().default(() => getActorPool() ?? "default"),
    version: z.number().default(() => getActorEnvoyVersion() ?? 1),

    // Deprecated.
    totalSlots: z.number().default(() => getActorTotalSlots() ?? 100000),
    nodeKey: z.string().optional(),
});
export type EnvoyConfigInput = z.input<typeof EnvoyConfigSchema>;
export type EnvoyConfig = z.infer<typeof EnvoyConfigSchema>;
