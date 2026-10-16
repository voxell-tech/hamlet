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
            },
            providers: await home.modelHome.providers(),
            saved: settings,
        };
    },
};
