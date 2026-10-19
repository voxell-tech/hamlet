//! The panes of the home's models, and narrowing a list of them.

use bevy::ecs::entity::Entity;
use hamlet_sdk::AnyView;

use crate::shown;

/// The most models a list shows at once.
const SHOWN: usize = 40;

/// The endpoints of the home and the models each serves.
pub fn endpoints() -> AnyView {
    shown(|panes| (panes.endpoints)())
}

/// The catalogue: every model the endpoints serve, for the chief to
/// allow, price and describe.
pub fn shelf() -> AnyView {
    shown(|panes| (panes.shelf)())
}

/// The model `agent` runs, chosen from those its hamlet allows.
pub fn of_agent(agent: Entity) -> AnyView {
    shown(move |panes| (panes.of_agent)(agent))
}

/// The `names` that `narrow` leaves, and the count of those past what
/// is shown.
pub fn narrowed<N: AsRef<str>>(
    names: impl Iterator<Item = N>,
    narrow: &str,
) -> (Vec<String>, usize) {
    let words = narrow.to_lowercase();
    let mut names = names
        .filter(|name| {
            let name = name.as_ref().to_lowercase();
            words.split_whitespace().all(|word| name.contains(word))
        })
        .map(|name| name.as_ref().to_string())
        .collect::<Vec<_>>();
    let more = names.len().saturating_sub(SHOWN);
    names.truncate(SHOWN);
    (names, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_narrows_the_catalogue_and_the_rest_is_counted() {
        let all = (0..SHOWN + 5)
            .map(|n| format!("vendor/large-{n}"))
            .chain(["Other/Small".to_string()])
            .collect::<Vec<_>>();
        let (shown, more) = narrowed(all.iter(), "");
        assert_eq!((shown.len(), more), (SHOWN, 6));
        assert_eq!(
            narrowed(all.iter(), "small other"),
            (vec!["Other/Small".into()], 0)
        );
    }
}
