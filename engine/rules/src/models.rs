//! The model each agent of a hamlet runs: the chief's choice, else
//! the mayor's, else the hamlet's own.

use anyhow::{Result, bail};
use hamlet_home::limits::Limits;
use hamlet_model::catalogue::{self, Catalogue};
use hamlet_model::log;
use serde_json::json;

use crate::hamlet::{Census, Hamlet};

impl Hamlet {
    /// The mayor's choice of model for `name`. An empty `model` is
    /// the hamlet's own. The `reason` is kept in the record (H17).
    pub fn choose_model(
        &self,
        name: &str,
        model: &str,
        reason: &str,
    ) -> Result<()> {
        let limits = self.limits()?;
        if !self.residents().iter().any(|r| r.name == name) {
            bail!("no agent named `{name}`");
        }
        if let Some(pinned) = limits.pinned.get(name) {
            bail!(
                "the chief has set the model of `{name}` to \
                 `{pinned}`; only the chief can change it"
            );
        }
        let runs = self.models(&limits);
        if !model.is_empty()
            && !runs.iter().any(|m| m.name() == model)
        {
            bail!(
                "this hamlet may not run `{model}`. You may choose: \
                 {}",
                allowed_words(&runs)
            );
        }
        self.append(json!({
            "v": 1,
            "type": "model_chosen",
            "ts": log::now_millis(),
            "name": name,
            "model": model,
            "reason": reason.trim(),
        }))
    }

    /// The models this hamlet may run besides its own: those on its
    /// own list in the limits that the home's catalogue also allows.
    pub fn models(&self, limits: &Limits) -> Vec<catalogue::Model> {
        Catalogue::load(self.home())
            .allowed()
            .filter(|model| limits.allows(&model.name()))
            .cloned()
            .collect()
    }

    /// The model `name` runs: the chief's choice for it, else the
    /// mayor's while the hamlet may run it, else the hamlet's own
    /// default. `None` is the home's default.
    pub fn model_of(&self, name: &str) -> Option<String> {
        let limits = self.limits().ok()?;
        if let Some(pinned) = limits.pinned.get(name) {
            return Some(pinned.clone());
        }
        let runs = self.models(&limits);
        Hamlet::model_in(&self.census(), &limits, &runs, name)
    }

    /// `model_of` from a census and the list of `models` already
    /// read.
    pub(crate) fn model_in(
        census: &Census,
        limits: &Limits,
        runs: &[catalogue::Model],
        name: &str,
    ) -> Option<String> {
        if let Some(pinned) = limits.pinned.get(name) {
            return Some(pinned.clone());
        }
        census
            .model(name)
            .filter(|chosen| runs.iter().any(|m| m.name() == *chosen))
            .or(limits
                .model
                .clone()
                .filter(|model| !model.is_empty()))
    }
}

/// The models the mayor may choose among `runs`, each with what it
/// costs.
pub(crate) fn allowed_words(runs: &[catalogue::Model]) -> String {
    if runs.is_empty() {
        return "only the hamlet's own model (leave `model` empty)."
            .into();
    }
    let cards = runs
        .iter()
        .map(|model| format!("\n- {}", model.card()))
        .collect::<String>();
    format!("{cards}\n- the hamlet's own (leave `model` empty)")
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::MAYOR;

    fn hamlet(tag: &str, limits: &str) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("models-{tag}"));
        set_limits(&dir, limits);
        let hamlet = Hamlet::new(dir.to_path_buf());
        for name in ["librarian", "scout", "clerk"] {
            hamlet.create(name, "Works.", "test").unwrap();
        }
        (dir, hamlet)
    }

    fn set_limits(home: &std::path::Path, text: &str) {
        std::fs::write(hamlet_home::limits::path(home), text)
            .unwrap();
    }

    #[test]
    fn the_chief_s_choice_of_model_outranks_the_mayor_s() {
        let (dir, hamlet) =
            hamlet("models", "models = [\"small\", \"huge\"]\n");
        assert_eq!(hamlet.model_of("scout"), None);
        // On the hamlet's list, but the home has not allowed it.
        assert!(hamlet.choose_model("scout", "small", "r").is_err());
        let allow = |ids: &[&str]| {
            let models = ids
                .iter()
                .map(|id| catalogue::Model {
                    id: id.to_string(),
                    allowed: true,
                    ..catalogue::Model::default()
                })
                .collect();
            Catalogue { models }.save(hamlet.home()).unwrap();
        };
        // Allowed by the home, but not on the hamlet's list.
        allow(&["small", "other"]);
        assert!(hamlet.choose_model("scout", "other", "r").is_err());
        assert!(hamlet.choose_model("scout", "huge", "r").is_err());
        assert!(hamlet.choose_model("nobody", "small", "r").is_err());
        hamlet.choose_model("scout", "small", "r").unwrap();
        assert_eq!(
            hamlet.model_of("scout").as_deref(),
            Some("small")
        );

        set_limits(
            &dir,
            "models = [\"small\"]\n[pinned]\nscout = \"big\"\nmayor \
             = \"big\"\n",
        );
        assert_eq!(hamlet.model_of("scout").as_deref(), Some("big"));
        assert_eq!(hamlet.model_of(MAYOR).as_deref(), Some("big"));
        let said = hamlet
            .choose_model("scout", "small", "r")
            .unwrap_err()
            .to_string();
        assert!(said.contains("only the chief can change it"));

        // An agent nobody chose for runs the hamlet's default.
        set_limits(&dir, "models = [\"small\"]\nmodel = \"plain\"\n");
        assert_eq!(
            hamlet.model_of("librarian").as_deref(),
            Some("plain")
        );
        assert_eq!(
            hamlet.model_of("scout").as_deref(),
            Some("small")
        );
        // Unpinned, the mayor's choice holds again, and the mayor
        // can give it back to the hamlet's own.
        set_limits(&dir, "models = [\"small\"]\n");
        assert_eq!(
            hamlet.model_of("scout").as_deref(),
            Some("small")
        );
        hamlet.choose_model("scout", "", "r").unwrap();
        assert_eq!(hamlet.model_of("scout"), None);
    }
}
