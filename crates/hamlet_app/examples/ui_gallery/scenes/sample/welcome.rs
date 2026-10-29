//! The first-run gallery scenes.

use hamlet_app::welcome;

use crate::scenes::Scene;

/// The first-run screen as `scene` shows it: closed, or at a step
/// with what a provider or the server might have answered.
pub(crate) fn first_run(scene: Scene) -> welcome::Welcome {
    let Scene::Welcome(step) = scene else {
        return welcome::Welcome::default();
    };
    let models = [
        "openrouter::maker/large",
        "openrouter::maker/large-fast",
        "openrouter::maker/small",
        "openrouter::other/open-weights-70b",
        "openrouter::other/open-weights-8b",
    ];
    let mut shown = welcome::Welcome::default();
    shown.open = true;
    shown.step = step;
    shown.from = "OpenRouter".into();
    shown.models = models.map(String::from).to_vec();
    shown.problem = match step {
        welcome::Step::Key => Some(
            "That key was not accepted. Check that you copied all \
             of it and that it is still active, then try again."
                .into(),
        ),
        welcome::Step::Local => Some(welcome::NOTHING_LOCAL.into()),
        _ => None,
    };
    shown
}
