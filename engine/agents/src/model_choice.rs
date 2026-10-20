//! The model an agent runs, chosen by the chief from those the home
//! allows, and written where its hamlet keeps it.

use bevy::ecs::world::World;
use hamlet_core::agents::{self, Mayor, Settlement};
use hamlet_core::core::Settings;
use hamlet_engine_api::Failure;
use hamlet_home::limits::{self, Limits};
use hamlet_rules::hamlet::{Hamlet, MAYOR};

/// The model choice's methods on [`World`].
pub(crate) trait ModelChoiceWorldExt {
    /// Has the living agent `name` run `model` from now on: its
    /// hamlet is let run it, a choice the chief wrote into the
    /// limits by hand is taken out, and the choice is written
    /// where the mayor's are. The mayor's own is pinned in the
    /// limits.
    fn run_model(
        &mut self,
        name: &str,
        model: &str,
    ) -> Result<(), Failure>;
}

impl ModelChoiceWorldExt for World {
    fn run_model(
        &mut self,
        name: &str,
        model: &str,
    ) -> Result<(), Failure> {
        let failed =
            |why: anyhow::Error| Failure::Failed(format!("{why:#}"));
        let agent =
            agents::named(self, name).ok_or(Failure::NotFound)?;
        let hamlet = agents::hamlet_of(self, agent)
            .and_then(|at| {
                self.try_query::<&Settlement>().and_then(|mut all| {
                    all.iter(self).find(|one| one.name == at).cloned()
                })
            })
            .ok_or(Failure::NotFound)?
            .record();
        let home_default =
            self.resource::<Settings>().config.model.clone();
        if self.get::<Mayor>(agent).is_some() {
            // The mayor's own is the chief's alone to say.
            let model = (model != home_default).then_some(model);
            return set(&hamlet, ("pinned", MAYOR), model)
                .map_err(failed);
        }
        let kept = Limits::load(hamlet.root()).unwrap_or_default();
        if kept.pinned.contains_key(name) {
            set(&hamlet, ("pinned", name), None).map_err(failed)?;
        }
        // With none of its own it runs the hamlet's, or the home's.
        let default = kept.model.clone().unwrap_or(home_default);
        let chosen = if model == default { "" } else { model };
        if !chosen.is_empty() && !kept.allows(chosen) {
            let mut list = kept.models.clone();
            list.retain(|known| known != chosen);
            list.push(chosen.to_string());
            let written = list
                .iter()
                .map(|name| format!("{name:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            limits::set(
                hamlet.root(),
                "",
                "models",
                Some(&format!("[{written}]")),
            )
            .map_err(failed)?;
        }
        hamlet
            .choose_model(name, chosen, "The chief chose it.")
            .map_err(failed)?;
        agents::sync(self);
        Ok(())
    }
}

/// Sets `key` of `section` in the limits of `hamlet` to `model`, or
/// takes it out.
fn set(
    hamlet: &Hamlet,
    (section, key): (&str, &str),
    model: Option<&str>,
) -> anyhow::Result<()> {
    let written = model.map(|model| format!("{model:?}"));
    limits::set(hamlet.root(), section, key, written.as_deref())
}
