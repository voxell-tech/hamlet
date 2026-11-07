//! The pages of first-run setup.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, overlay, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::Settings;
use hamlet_ui::styles::{
    self, back, back_link, bare, beside, description, good_action,
    group, headed, marked, nav_card, note, parts, scroll_area,
    small_rules,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, View, built, with_cx};

use super::{Step, Welcome, close, go, look, pick};
use crate::models::{ModelRow, model_row, narrowed};
use crate::roll::{either, moved};

fn title(which: Glyph, text: &'static str) -> impl View {
    headed(which, styles::title(text))
}

pub(super) fn said(text: impl Into<String>) -> AnyView {
    description(text.into()).boxed()
}

/// The news of the last thing asked: that it is awaited, or the
/// reason it did not work.
pub(super) fn news(waiting: &'static str) -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.asking, welcome.problem.clone())
        }),
        move |(asking, problem)| match problem {
            _ if *asking => said(waiting).boxed(),
            Some(why) => label(why.clone())
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
            None => frame().boxed(),
        },
    )
    .within(bare())
    .boxed()
}

fn choice(adding: bool) -> AnyView {
    let (name, says, leave, after) = if adding {
        (
            "Add a place for models",
            "Where do the models come from? Your current model \
             stays as it is.",
            "Cancel",
            "Its models are listed under Models, for you to allow.",
        )
    } else {
        (
            "Welcome to Hamlet",
            "Your hamlet needs a model to think with. How would you \
             like to give it one?",
            "I'll set this up later",
            "You can come back to this from Settings.",
        )
    };
    parts((
        title(Glyph::Hearth, name),
        said(says),
        nav_card(
            Glyph::Signal,
            "Sign in with OpenRouter",
            "The quickest way. You sign in with your browser and \
             pay OpenRouter for what you use.",
            |world| {
                super::providers::start(world, "openrouter", false);
            },
        ),
        nav_card(
            Glyph::Signal,
            "Continue with ChatGPT",
            "Use your ChatGPT plan, with no key. You sign in with \
             your browser, and what your hamlet does counts toward \
             your plan's usage.",
            |world| super::providers::start(world, "chatgpt", false),
        ),
        nav_card(
            Glyph::Key,
            "I have a key",
            "Use a key from a provider such as OpenAI, DeepSeek or \
             OpenRouter. You pay them for what you use.",
            |world| go(world, Step::Key),
        ),
        nav_card(
            Glyph::Chip,
            "I run my own model",
            "Use a model served on the server's machine, with \
             Ollama or LM Studio.",
            look,
        ),
        beside((
            marked(Glyph::Clock, leave)
                .rules(back_link)
                .tagged(Variant::Default)
                .on_activate(close),
            note(after),
        ))
        .tagged(hamlet_ui::widgets::platform::Wraps),
    ))
    .boxed()
}

/// The head of a step: the way back to the first, and its name.
pub(super) fn head(which: Glyph, text: &'static str) -> impl View {
    beside((
        back("Back", |world| go(world, Step::Choice)),
        title(which, text),
    ))
}

/// The model a new hamlet is offered first: the realm's default
/// when it is listed, else the first listed.
fn recommended(models: &[String], default: &str) -> Option<String> {
    models
        .iter()
        .find(|name| *name == default)
        .or(models.first())
        .cloned()
}

fn models() -> AnyView {
    let list = keyed(
        derived(|world: &World| {
            let welcome = world.resource::<Welcome>();
            let default = world
                .get_resource::<Settings>()
                .map(|settings| settings.0.model.clone())
                .unwrap_or_default();
            let best = recommended(&welcome.models, &default);
            // The one recommended leads the list until a search.
            let mut order = welcome.models.clone();
            if welcome.narrow.trim().is_empty()
                && let Some(at) = order
                    .iter()
                    .position(|name| Some(name) == best.as_ref())
            {
                let first = order.remove(at);
                order.insert(0, first);
            }
            let (ids, _, pages) =
                narrowed(order.iter(), &welcome.narrow, 0);
            (ids, pages, best)
        })
        .when(either(moved::<Welcome>(), moved::<Settings>())),
        |(ids, pages, best)| {
            let mut rows = ids
                .iter()
                .map(|id| {
                    let chosen = id.clone();
                    model_row(
                        ModelRow {
                            name: id.clone(),
                            picked: false,
                            tag: (Some(id) == best.as_ref())
                                .then_some("Recommended"),
                            end: None,
                        },
                        move |world| pick(world, &chosen),
                    )
                })
                .collect::<Vec<_>>();
            if ids.is_empty() {
                rows.push(said("No models found. Try another name."));
            }
            if *pages > 1 {
                rows.push(said(
                    "There are more. Type part of a name to narrow \
                     them.",
                ));
            }
            column(rows).gap(2.0).boxed()
        },
    )
    .within(bare());
    let search = built(|cx| {
        let narrow = cx.world.resource::<Welcome>().narrow.clone();
        cx.build(crate::search::search(
            "Search models",
            narrow,
            |world, text| {
                let mut welcome = world.resource_mut::<Welcome>();
                if welcome.narrow != text {
                    welcome.narrow = text;
                }
            },
        ))
    });
    parts((
        beside((
            back("Back", |world| go(world, Step::Choice)),
            title(Glyph::Chip, "Pick a model"),
            frame().grow(1.0),
            note("Step 3 of 3"),
        )),
        description(resource::<Welcome, _>(|welcome| {
            format!(
                "{} listed these models. Pick the one your hamlet \
                 will think with.",
                welcome.from
            )
        })),
        group((search, news("Asking it to say hello..."), list)),
        note(
            "You can change it later in Settings, under Models, and \
             for one agent from the model under its chat.",
        ),
    ))
    .boxed()
}

fn local() -> AnyView {
    parts((
        head(Glyph::Chip, "Your own model"),
        said(
            "Hamlet asks the endpoints configured on the server. \
             Ollama or LM Studio must run on the server's machine.",
        ),
        news("Looking..."),
        good_action(Glyph::Restore, "Look again", look),
    ))
    .boxed()
}

pub fn view() -> impl View {
    built(|cx| {
        cx.world.init_resource::<super::providers::Login>();
        cx.world.init_resource::<super::Browser>();
        cx.world.init_resource::<super::Committing>();
        cx.build(screen())
    })
}

fn screen() -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            (
                welcome.open,
                welcome.step,
                welcome.adding,
                welcome.connection.clone(),
            )
        }),
        |(open, step, adding, connection)| {
            let page = if let Some(words) = connection {
                parts((
                    title(Glyph::Signal, "Connect to a server"),
                    said(words.clone()),
                    said(
                        "Start a server with hamlet server, then \
                         open its window with hamlet window. For \
                         local development, use hamlet dev.",
                    ),
                ))
                .boxed()
            } else if !open {
                return frame().boxed();
            } else {
                match step {
                    Step::Choice => choice(*adding),
                    Step::Key => super::providers::key_page(),
                    Step::SignIn => super::providers::sign_in_page(),
                    Step::Local => local(),
                    Step::Models => models(),
                }
            };
            with_cx(move |cx| over(cx, page)).boxed()
        },
    )
    .within(overlay(()))
    .boxed()
}

fn over(cx: &Cx, page: AnyView) -> impl View + use<> {
    let theme = cx.theme();
    let space = &theme.space;
    let card = column((scroll_area((column((page,))
        .gap(0.0)
        .padding(UiRect::all(px(space.overlay_padding))),)),))
    .width(percent(100.0))
    .max_width(px(space.overlay_width))
    .max_height(percent(100.0))
    .gap(0.0)
    .fill(theme.palette.card)
    .border(space.border)
    .border_color(theme.palette.gold_5)
    .radius(space.soft)
    .tagged(theme.shadow());
    // A row, so the card is as tall as its words once they wrap to
    // its width.
    row((card,))
        .width(percent(100.0))
        .height(percent(100.0))
        .padding(UiRect::all(px(space.panel)))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .fill(theme.palette.shadow)
}
