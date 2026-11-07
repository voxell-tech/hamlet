//! The first-run gallery scenes.

use hamlet_app::welcome;
use hamlet_protocol::client as protocol;

use crate::scenes::Scene;

/// The first-run screen as `scene` shows it: closed, or at a step
/// with what a provider or the server might have answered.
pub(crate) fn first_run(scene: Scene) -> welcome::Welcome {
    let Some(step) = scene.first_run() else {
        return welcome::Welcome::default();
    };
    let mut shown = welcome::Welcome::default();
    shown.open = true;
    shown.step = step;
    shown.from = "OpenRouter".into();
    shown.models =
        catalogue().into_iter().map(|model| model.name).collect();
    shown.problem = match step {
        welcome::Step::Key => Some(
            "That key was not accepted. Check it was copied whole \
             and is still active."
                .into(),
        ),
        welcome::Step::Local => Some(welcome::NOTHING_LOCAL.into()),
        _ => None,
    };
    shown
}

/// The models OpenRouter lists in the first-run scenes.
pub(crate) fn catalogue() -> Vec<protocol::Listed> {
    let card = |name: &str, (input, output): (f64, f64), context| {
        protocol::Listed {
            context: Some(context),
            default_effort: None,
            efforts: Vec::new(),
            fast: false,
            images: None,
            name: format!("openrouter::{name}"),
            note: String::new(),
            price: Some(protocol::Price { input, output }),
            priced: protocol::ListedPriced::Listed,
            title: String::new(),
        }
    };
    vec![
        card("maker/large", (3.0, 15.0), 200_000),
        card("maker/large-fast", (5.0, 25.0), 200_000),
        card("maker/small", (0.25, 1.25), 128_000),
        card("other/open-weights-70b", (0.6, 0.8), 128_000),
        card("other/open-weights-8b", (0.0, 0.0), 32_000),
    ]
}
