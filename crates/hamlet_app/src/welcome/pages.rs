//! The pages of first-run setup.

use bevy::color::Alpha;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, overlay, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::Settings;
use hamlet_ui::markdown::{Face, Ink, Span, Style, Target, rich};
use hamlet_ui::styles::{
    self, ICON, back, back_link, bare, beside, description,
    good_action, group, headed, marked, nav_card, note, parts,
    scroll_area, small_rules,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, View, built, with_cx};

use super::{NOTHING_LOCAL, Step, Welcome, close, go, look, pick};
use crate::models::{
    Detail, ModelRow, mark, model_row, narrowed, provider_of,
};
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
            Some(why) if why == NOTHING_LOCAL => install_hint(),
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

/// No local model server yet, said as the way to start one, with
/// Ollama's site as a link: the usual first state, not a failure.
fn install_hint() -> AnyView {
    const SITE: &str = "ollama.com";
    let (before, after) = NOTHING_LOCAL
        .split_once(SITE)
        .unwrap_or((NOTHING_LOCAL, ""));
    let span = |text: &str, link| Span {
        text: text.into(),
        style: Style::default(),
        link,
    };
    with_cx(move |cx| {
        rich(
            &[
                span(before, None),
                span(
                    SITE,
                    Some(Target::Web(format!("https://{SITE}"))),
                ),
                span(after, None),
            ],
            Ink {
                size: Some(cx.theme().text.small),
                tone: Tone::Body,
                face: Face::Body,
            },
        )
    })
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
        beside((
            title(Glyph::Hearth, name),
            frame().grow(1.0),
            counted(1),
        )),
        said(says),
        nav_card(
            Glyph::Globe,
            "Sign in with OpenRouter",
            "Recommended, and the quickest way. You sign in with \
             your browser and pay OpenRouter for what you use.",
            |world| {
                super::providers::start(world, "openrouter", false);
            },
        ),
        nav_card(
            Glyph::Talk,
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

/// The head of a step after the first: the way back to the first,
/// its name, and where it stands among the steps.
pub(super) fn head(
    which: Glyph,
    text: &'static str,
    step: usize,
) -> impl View {
    beside((
        back("Back", |world| go(world, Step::Choice)),
        title(which, text),
        frame().grow(1.0),
        counted(step),
    ))
}

/// Where the step `step` stands among them: two while a place for
/// models is added, as its models go to Settings, else three.
fn counted(step: usize) -> impl View {
    note(resource::<Welcome, _>(move |welcome| {
        let of = if welcome.adding { 2 } else { 3 };
        format!("Step {step} of {of}")
    }))
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

/// The model recommended of those listed.
fn best(world: &World) -> Option<String> {
    let default = world
        .get_resource::<Settings>()
        .map(|settings| settings.0.model.clone())
        .unwrap_or_default();
    recommended(&world.resource::<Welcome>().models, &default)
}

/// The model Continue tries: the one picked, else the one
/// recommended.
fn chosen(world: &World) -> Option<String> {
    let welcome = world.resource::<Welcome>();
    welcome
        .chosen
        .clone()
        .filter(|name| welcome.models.contains(name))
        .or_else(|| best(world))
}

/// The provider every listed model comes from, when they share one.
fn one_provider(models: &[String]) -> Option<String> {
    let first = provider_of(models.first()?);
    models
        .iter()
        .all(|name| provider_of(name) == first)
        .then(|| first.to_string())
}

fn models() -> AnyView {
    let list = keyed(
        derived(|world: &World| {
            let welcome = world.resource::<Welcome>();
            let best = best(world);
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
            let detail = match one_provider(&welcome.models) {
                Some(_) => Detail::Id,
                None => Detail::Source,
            };
            (ids, pages, best, chosen(world), detail)
        })
        .when(either(moved::<Welcome>(), moved::<Settings>())),
        |(ids, pages, best, chosen, detail)| {
            let mut rows = ids
                .iter()
                .map(|id| {
                    let picked = id.clone();
                    model_row(
                        ModelRow {
                            name: id.clone(),
                            picked: Some(id) == chosen.as_ref(),
                            tag: (Some(id) == best.as_ref())
                                .then_some("Recommended"),
                            end: None,
                            detail: *detail,
                        },
                        move |world| {
                            world.resource_mut::<Welcome>().chosen =
                                Some(picked.clone());
                        },
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
    // The provider's mark leads what it listed, once for every row.
    let from = keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.from.clone(), one_provider(&welcome.models))
        }),
        |(from, provider)| {
            let says = description(format!(
                "{from} listed these models. Pick the one your \
                 hamlet will think with."
            ));
            match provider {
                Some(id) => beside((mark(id, ICON), says)).boxed(),
                None => says.boxed(),
            }
        },
    )
    .within(bare());
    let send =
        row((good_action(Glyph::Check, "Continue", |world| {
            if let Some(model) = chosen(world) {
                pick(world, &model);
            }
        }),));
    parts((
        head(Glyph::Chip, "Pick a model", 3),
        from,
        group((search, news("Asking it to say hello..."), list)),
        send,
        note(
            "You can change it later in Settings, under Models, and \
             for one agent from the model under its chat.",
        ),
    ))
    .boxed()
}

fn local() -> AnyView {
    parts((
        head(Glyph::Chip, "Your own model", 2),
        said(
            "Hamlet asks the endpoints configured on the server. \
             Ollama or LM Studio must run on the server's machine.",
        ),
        news("Looking..."),
        row((good_action(Glyph::Restore, "Look again", look),)),
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

/// The height of every step's card, the tallest step's, so neither
/// its head nor its buttons move from one step to the next.
const TALL: f32 = 520.0;

/// How much of what is left of the window the scrim under the card
/// takes away, over what the theme's shadow does.
const SCRIM: f32 = 0.7;

fn over(cx: &Cx, page: AnyView) -> impl View + use<> {
    let theme = cx.theme();
    let space = &theme.space;
    let shadow = theme.palette.shadow;
    let scrim = shadow
        .with_alpha(shadow.alpha() + (1.0 - shadow.alpha()) * SCRIM);
    let card = column((scroll_area((column((page,))
        .gap(0.0)
        .padding(UiRect::all(px(space.overlay_padding))),)),))
    .width(percent(100.0))
    .max_width(px(space.overlay_width))
    .height(px(TALL))
    .max_height(percent(100.0))
    .gap(0.0)
    .fill(theme.palette.card)
    .border(space.border)
    .border_color(theme.palette.gold_5)
    .radius(space.soft)
    .tagged(theme.shadow());
    row((card,))
        .width(percent(100.0))
        .height(percent(100.0))
        .padding(UiRect::all(px(space.panel)))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .fill(scrim)
}
