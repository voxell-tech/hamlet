// This file was changed for Hamlet. See NOTICE in this package.
import { z } from "zod/v4";
import { getActorPublicEndpoint, getActorPublicToken } from "../../utils/env-vars.ts";

export const DEFAULT_SERVERLESS_MAX_START_PAYLOAD_BYTES = 16 * 1024 * 1024;

export const ConfigurePoolSchema = z
    .object({
        name: z.string().optional(),
        url: z.string(),
        headers: z.record(z.string(), z.string()).optional(),
        requestLifespan: z.number().optional(),
        drainGracePeriod: z.number().optional(),
        metadata: z.record(z.string(), z.unknown()).optional(),
        metadataPollInterval: z.number().optional(),
        drainOnVersionUpgrade: z.boolean().optional(),
    })
    .optional();

export const ServerlessConfigSchema = z.object({
    // MARK: Routing
    basePath: z.string().optional().default("/api/actor"),
    maxStartPayloadBytes: z
        .number()
        .optional()
        .default(DEFAULT_SERVERLESS_MAX_START_PAYLOAD_BYTES),

    // MARK: Public Endpoint Configuration
    /**
     * The endpoint that clients should connect to.
     *
     * This is useful if clients connect to serverless directly
     * (e.g. `http://localhost:3000/api/actor`), they will fetch
     * `http://localhost:3000/api/actor/metadata` and be redirected to
     * the public endpoint.
     *
     * Supports URL auth syntax for namespace and token:
     * - `https://namespace:token@api.example.com`
     * - `https://namespace@api.example.com`
     *
     * Auto-determined based on endpoint and NODE_ENV if not specified.
     *
     * Can also be set via HAMLET_ACTOR_PUBLIC_ENDPOINT environment variable.
     */
    publicEndpoint: z
        .string()
        .optional()
        .transform((val) => val ?? getActorPublicEndpoint()),

    /**
     * Token that clients should use when connecting via the public endpoint.
     *
     * Can also be set via HAMLET_ACTOR_PUBLIC_TOKEN environment variable.
     *
     * Can also be specified in the publicEndpoint URL as `https://namespace:token@host`.
     */
    publicToken: z
        .string()
        .optional()
        .transform((val) => val ?? getActorPublicToken()),

    // There is no publicNamespace config option because the frontend and backend
    // cannot use different namespaces. The namespace is extracted from the
    // publicEndpoint URL auth syntax if provided.
});
export type ServerlessConfigInput = z.input<typeof ServerlessConfigSchema>;
export type ServerlessConfig = z.infer<typeof ServerlessConfigSchema>;
