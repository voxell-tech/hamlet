//! The Settings page of the chief's Hamlet account: who is signed in
//! and the ways to change their username or password or to sign out,
//! or the ways to sign in or make one; and the home's OpenRouter key,
//! connected or disconnected.

use bevy::ecs::world::World;
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_client::realm::Providers;
use hamlet_ui::styles::{
    action, bad_action, bare, buttons, description, good_action,
    group, parts, quiet, section,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View};

use crate::account::{self, Form, Page, Place};
use crate::bazaar::market::{
    Market, disconnect, home_has_key, sign_out,
};
use crate::roll::{either, moved};

/// What the page is drawn from.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    page: Page,
    /// The home has an OpenRouter key.
    openrouter: bool,
    /// The name and email of the one signed in, if anyone is.
    user: Option<(String, String)>,
    said: Option<String>,
}

/// The account's page of Settings.
pub(super) fn page() -> impl View {
    let now = derived(|world: &World| {
        let form = world.resource::<Form>();
        let user = world.resource::<Market>().user().map(|user| {
            (user.name().to_string(), user.email.clone())
        });
        Now {
            page: form.at(Place::Settings),
            openrouter: home_has_key(world),
            user,
            said: form
                .said
                .clone()
                .filter(|_| form.at == Place::Settings),
        }
    })
    .when(either(
        either(moved::<Form>(), moved::<Market>()),
        moved::<Providers>(),
    ));
    keyed(now, |now| match (now.page, &now.user) {
        (Page::Shut, Some((name, email))) => {
            let mut all = vec![
                description(format!(
                    "Signed in as {name}, with {email}."
                ))
                .boxed(),
            ];
            all.extend(
                now.said
                    .clone()
                    .map(|said| description(said).boxed()),
            );
            all.push(
                buttons(vec![
                    action(Glyph::Rename, "CHANGE USERNAME", rename),
                    action(
                        Glyph::Shield,
                        "CHANGE PASSWORD",
                        |world| {
                            open(world, Page::Password);
                        },
                    ),
                    bad_action(Glyph::Power, "SIGN OUT", sign_out),
                ])
                .boxed(),
            );
            all.push(
                description(
                    "Signing out keeps your OpenRouter key on this \
                     computer.",
                )
                .boxed(),
            );
            all.push(openrouter(now.openrouter));
            parts(all).boxed()
        }
        (Page::Shut, None) => parts((
            description(
                "A Hamlet account lets you buy and sell at the \
                 Agent Bazaar, and keeps your OpenRouter key on \
                 every computer you sign in on.",
            ),
            buttons(vec![
                good_action(Glyph::Signpost, "SIGN IN", |world| {
                    open(world, Page::SignIn);
                }),
                action(Glyph::Plus, "CREATE AN ACCOUNT", |world| {
                    open(world, Page::SignUp);
                }),
            ]),
            openrouter(now.openrouter),
        ))
        .boxed(),
        (page, _) => parts((
            group((
                section(page.title()),
                account::body(page, Place::Settings),
            )),
            quiet(Glyph::Back, "CANCEL", account::shut),
        ))
        .boxed(),
    })
    .within(bare())
}

/// The home's OpenRouter key: the way to connect it, with a spend
/// limit, or to disconnect it here and from the account.
fn openrouter(connected: bool) -> AnyView {
    let (button, words) = match connected {
        true => (
            bad_action(
                Glyph::Gone,
                "DISCONNECT OPENROUTER",
                disconnect,
            ),
            "OpenRouter is connected. Disconnecting forgets its key \
             on this home and in your Hamlet account, so your other \
             computers do not get it again. It works at OpenRouter \
             until you remove it at openrouter.ai/settings/keys.",
        ),
        false => (
            good_action(
                Glyph::Signal,
                "CONNECT OPENROUTER",
                crate::welcome::connect_openrouter,
            ),
            "Pay OpenRouter for what your hamlet uses, with a spend \
             limit you choose. Signed in, the key is kept, locked \
             with your password, for your other computers.",
        ),
    };
    group((
        section("OPENROUTER"),
        description(words),
        buttons(vec![button]),
    ))
    .boxed()
}

fn open(world: &mut World, page: Page) {
    account::open(world, Place::Settings, page);
}

/// Opens the change of username, with the one now.
fn rename(world: &mut World) {
    let name = world
        .resource::<Market>()
        .user()
        .and_then(|user| user.username.clone())
        .unwrap_or_default();
    open(world, Page::Rename);
    world.resource_mut::<Form>().username = name;
}
