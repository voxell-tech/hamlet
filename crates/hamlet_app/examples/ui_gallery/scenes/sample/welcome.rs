//! The first-run gallery scenes.

use bevy::ecs::world::World;
use hamlet_app::account::{Form, Page, Place};
use hamlet_app::welcome;

use crate::scenes::Scene;

/// The first-run screen as `scene` shows it: closed, or at a step
/// with what a provider or the server might have answered.
pub(crate) fn first_run(scene: Scene) -> welcome::Welcome {
    let step = match scene {
        Scene::Welcome(step) => step,
        Scene::Account(_) => welcome::Step::Account,
        _ => return welcome::Welcome::default(),
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

/// The page of a Hamlet account the scene shows, with what might have
/// been typed in it.
pub(crate) fn setup(world: &mut World, scene: Scene) {
    let Scene::Account(page) = scene else {
        return;
    };
    let said = (page == Page::Code)
        .then(|| "We sent a code to mira@example.com.".to_string());
    let problem = (page == Page::Username).then(|| {
        "That username is taken. Choose another.".to_string()
    });
    world.insert_resource(Form {
        page,
        at: Place::Welcome,
        email: "mira@example.com".into(),
        password: "a long password".into(),
        code: if page == Page::Reset {
            "482913".into()
        } else {
            String::new()
        },
        username: "mira".into(),
        said,
        problem,
        ..Form::default()
    });
}
