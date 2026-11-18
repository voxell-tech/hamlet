//! The Hand's acts on the realm.

use hamlet_protocol::client::EditSettingsSet;
use hamlet_store::ids::Actor;
use serde_json::{Map, Value};

use super::args::{Called, EditCharter, EditSettings, Unshelve};
use super::{Call, cut, unsupported};
use crate::conversations::prompt;
use crate::core::Core;
use crate::realm::change::{Done, refuse};
use crate::society::authority;
use crate::society::refusal::Refusal;

impl Core<'_, '_> {
    pub(super) fn realm_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        let by = Actor::Agent(call.conversation.agent);
        let db = self.realm.store().connection();
        match called {
            Called::ReadCharter(_) => Ok(Ok(prompt::charter(db)?)),
            Called::ReadModels(_) => {
                let models = self.models_listed()?;
                Ok(Ok(cut(serde_json::to_string_pretty(&models)?)))
            }
            Called::Unshelve(Unshelve { skill, reason }) => {
                self.begin(call)?;
                self.unshelve_skill(by, &skill, &reason)
            }
            Called::EditCharter(EditCharter { text }) => {
                if !authority::may_rule(db, by)? {
                    refuse!(Refusal::NotRealmChief);
                }
                self.begin(call)?;
                self.edit_charter(Some(text))
            }
            Called::EditSettings(args) => {
                if !authority::may_rule(db, by)? {
                    refuse!(Refusal::NotRealmChief);
                }
                let Ok(set) = settings_set(args) else {
                    refuse!(Refusal::SettingsUnfit);
                };
                self.begin(call)?;
                self.edit_settings(set)
            }
            _ => refuse!(unsupported(call)),
        }
    }
}

fn settings_set(
    args: EditSettings,
) -> serde_json::Result<EditSettingsSet> {
    let EditSettings {
        model,
        effort,
        reflect_every,
        newcomers,
        character,
    } = args;
    let mut set = Map::new();
    if let Some(model) = model {
        set.insert("model".into(), model.into());
    }
    if let Some(effort) = effort {
        // `none` lets agents think as their model does.
        let effort = match effort.as_str() {
            "none" => Value::Null,
            _ => effort.into(),
        };
        set.insert("effort".into(), effort);
    }
    if let Some(every) = reflect_every {
        set.insert("reflectEvery".into(), every.into());
    }
    if let Some(newcomers) = newcomers {
        set.insert("newcomers".into(), newcomers.into());
    }
    if let Some(character) = character {
        set.insert("character".into(), character.into());
    }
    serde_json::from_value(Value::Object(set))
}
