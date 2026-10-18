//! The plugins a plugin needs or would like, and the plugins that
//! need it, as lines under its name.

use bevy::ecs::system::Query;
use bevy_fynix::ViewExt;
use hamlet_sdk::plugin::{Manifest, Need, PluginInfo};

use crate::AnyView;
use crate::looks::note;

/// Every plugin and what its manifest says.
pub(super) type Everyone<'w, 's> =
    Query<'w, 's, (&'static PluginInfo, &'static Manifest)>;

/// How a plugin is tied to the others.
#[derive(Clone, PartialEq, Default, Debug)]
pub(super) struct Links {
    /// The plugins it needs, each with the versions it needs.
    pub needs: Vec<String>,
    /// The plugins it uses where they are.
    pub wants: Vec<String>,
    /// The plugins that need it.
    pub needed_by: Vec<String>,
}

/// The links of the plugin `id`.
pub(super) fn of(id: &str, everyone: &Everyone) -> Links {
    let name = |id: &str| {
        everyone.iter().find(|(info, _)| info.id == id).map_or_else(
            || id.to_string(),
            |(info, _)| info.name.clone(),
        )
    };
    let words = |needs: &[Need]| {
        needs
            .iter()
            .map(|need| format!("{} {}", name(&need.id), need.range))
            .collect::<Vec<_>>()
    };
    let mine = everyone
        .iter()
        .find(|(info, _)| info.id == id)
        .map(|(_, manifest)| manifest);
    let mut needed_by = everyone
        .iter()
        .filter(|(_, manifest)| {
            manifest.needs.iter().any(|need| need.id == id)
        })
        .map(|(info, _)| info.name.clone())
        .collect::<Vec<_>>();
    needed_by.sort();
    Links {
        needs: mine.map(|m| words(&m.needs)).unwrap_or_default(),
        wants: mine.map(|m| words(&m.wants)).unwrap_or_default(),
        needed_by,
    }
}

/// The lines for `links`, none where there are none.
pub(super) fn lines(links: &Links) -> Vec<AnyView> {
    [
        ("Needs", &links.needs),
        ("Uses where it is", &links.wants),
        ("Needed by", &links.needed_by),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .map(|(what, names)| {
        note(format!("{what}: {}", names.join(", "))).boxed()
    })
    .collect()
}
