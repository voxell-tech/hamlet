//! The Realm page: the limits that hold for every hamlet, each
//! written to the home's limits as it is changed.

use hamlet_engine_sdk::api::agents::RealmLimits;
use hamlet_engine_sdk::ui::{
    AnyView, FrameProps, Size, Space, Tone, UiExt, ViewExt, column,
    field, keyed, label, resource, row, switch,
};

use crate::parts::{note, part, problem, setting};
use crate::{Known, Page, SettingsWorldExt};

pub(crate) fn page() -> AnyView {
    let hamlets = column((
        part("signpost", "HAMLETS AND THE ROADS BETWEEN"),
        whole("Most hamlets", ("hamlets", "most"), 1000, |l| {
            l.most_hamlets
        }),
        yes(
            "Agents of different hamlets may have roads",
            ("hamlets", "roads_between"),
            |l| l.roads_between,
        ),
        whole(
            "Mayors it takes to ask for a hamlet's removal",
            ("hamlets", "removal_mayors"),
            100,
            |l| l.removal_mayors,
        ),
        whole(
            "Days until an unused road grows over",
            ("road", "grows_over_days"),
            3650,
            |l| l.grows_over_days,
        ),
    ));
    let votes = column((
        part("ballot", "VOTES"),
        note(
            "Agents may vote to replace their mayor or found a \
             hamlet. You can overturn any vote.",
        ),
        setting("It passes with yes from", pass(), None),
        whole(
            "Fewest agents to hold one",
            ("votes", "least_voters"),
            100,
            |l| l.least_voters,
        ),
        yes(
            "Ballots hidden until it closes",
            ("votes", "sealed"),
            |l| l.sealed,
        ),
        yes(
            "A yes needs grounds in its house",
            ("votes", "grounded_only"),
            |l| l.grounded_only,
        ),
        whole(
            "Minutes to answer a ballot",
            ("votes", "ballot_minutes"),
            600,
            |l| l.ballot_minutes,
        ),
        whole(
            "Minutes a vote stays open",
            ("votes", "open_minutes"),
            100_000,
            |l| l.open_minutes,
        ),
        whole(
            "Minutes before calling again",
            ("votes", "retry_minutes"),
            100_000,
            |l| l.retry_minutes,
        ),
    ));
    let unreadable = keyed(
        resource::<Known, _>(|known| {
            known.limits.as_ref().and_then(|l| l.problem.clone())
        }),
        |problem| match problem {
            Some(why) => label(format!(
                "The limits cannot be read, so these are the \
                 defaults: {why}"
            ))
            .size(Size::Small)
            .tone(Tone::Critical)
            .boxed(),
            None => column(()).boxed(),
        },
    );
    column((
        note("These apply to every hamlet."),
        unreadable,
        problem(Page::Realm),
        hamlets.space(Space::Gap),
        votes.space(Space::Gap),
    ))
    .space(Space::Double)
    .boxed()
}

/// A whole number of the limits, at `key` of its section, from 0 to
/// `most`.
fn whole(
    what: &'static str,
    (section, key): (&'static str, &'static str),
    most: u64,
    get: fn(&RealmLimits) -> u64,
) -> AnyView {
    agents_ui::whole::<Known>(
        what,
        most,
        move |known| known.limits.as_ref().map(get),
        move |world, n| world.set_limit(section, key, n.to_string()),
        |world, why| world.refuse(Page::Realm, why),
    )
}

/// A yes or a no of the limits, at `key` of its section.
fn yes(
    what: &'static str,
    (section, key): (&'static str, &'static str),
    get: fn(&RealmLimits) -> bool,
) -> AnyView {
    switch(
        what,
        resource::<Known, _>(move |known| {
            known.limits.as_ref().is_some_and(get)
        }),
    )
    .on_toggle(move |world, on| {
        world.set_limit(section, key, on.to_string())
    })
    .boxed()
}

/// The share of yes a vote passes with: yes of so many, each from 1
/// to 100.
fn pass() -> AnyView {
    let part = |yes: bool| {
        field(resource::<Known, _>(move |known| {
            known
                .limits
                .as_ref()
                .map(|limits| {
                    let (of_yes, of) = limits.pass;
                    if yes { of_yes } else { of }.to_string()
                })
                .unwrap_or_default()
        }))
        .on_commit(move |world, typed| {
            let Some(n) = typed
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=100).contains(n))
            else {
                world.refuse(
                    Page::Realm,
                    "It passes with yes from: each a whole number \
                     from 1 to 100."
                        .into(),
                );
                return;
            };
            let Some((was_yes, was_of)) = world
                .resource::<Known>()
                .limits
                .as_ref()
                .map(|limits| limits.pass)
            else {
                return;
            };
            let (yes, of) =
                if yes { (n, was_of) } else { (was_yes, n) };
            world.set_limit(
                "votes",
                "pass",
                format!("[{yes}, {of}]"),
            );
        })
    };
    row((part(true), note("in"), part(false))).boxed()
}
