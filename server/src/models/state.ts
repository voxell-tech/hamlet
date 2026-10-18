import type { HomeBase } from "../home.ts";
import type { State } from "../protocol.ts";
import { type Settings, SettingsDoc, settingsOf } from "../settings.ts";
import type { View } from "../state.ts";
import { CatalogueDoc } from "./catalogue.ts";

/** Settings and provider state, including the typed settings used by realm choices. */
export const modelView: View<
    Required<Pick<State, "settings" | "providers">> & { saved: Settings }
> = {
    docs: [SettingsDoc.definition, CatalogueDoc.definition],
    async build(home, context) {
        const settings = settingsOf(
            await home.harness.snapshot(SettingsDoc, context),
            home.overrides,
        );
        return {
            settings: {
                model: settings.model,
                reflectEvery: settings.reflectEvery,
                allowNet: settings.allowNet,
                endpoints: await Promise.all(
                    settings.endpoints.map(async (endpoint) => ({
                        ...endpoint,
                        key:
                            (await home.credentials.read(endpoint.name)) !==
                            undefined,
                    })),
                ),
                unset: !settings.saved && home.overrides.model === undefined,
                link: settings.link,
                workspace: {
                    url: home.credentials.workspace?.url ?? null,
                    running: home.workspace?.url ?? null,
                    ...movedTo(home, home.credentials.workspace?.url ?? null),
                },
            },
            providers: await home.modelHome.providers(),
            saved: settings,
        };
    },
};

/** The latest move of the houses, while it is about the workspace now chosen. */
function movedTo(home: Pick<HomeBase, "moving">, chosen: string | null) {
    const status = home.moving.status;
    if (status === undefined || status.to !== chosen) return {};
    const { to: _, ...move } = status;
    return { move };
}
