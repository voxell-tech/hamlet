//! The pages of the first-run screen, one for each step.

use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, overlay,
    row, scroll, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};

use super::providers::PROVIDERS;
use super::{
    Act, Narrowing, Step, Welcome, close, go, look, pick, request,
    sign_in,
};
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    action, back, bare, brass, buttons, door, good_action, headed,
    lamp, lettered, marked, note, pad, retrace, well,
};
use crate::models::narrowed;
use crate::platform::DisplayText;
use crate::theme::{paint, themed};
use crate::{AnyView, Cx, View};

fn title(which: Glyph, text: &'static str) -> impl View {
    headed(
        which,
        label(text).size(15.0).bold(true).tagged(DisplayText),
    )
}

fn said(text: impl Into<String>) -> AnyView {
    label(text.into()).size(13.0).tone(Tone::Dim).boxed()
}

fn small(text: &'static str) -> impl View {
    label(text)
        .size(11.0)
        .bold(true)
        .tone(Tone::Faint)
        .tagged(DisplayText)
}

/// One typed value, under its name and over a word of help.
fn typed(
    name: &'static str,
    get: fn(&Welcome) -> &String,
    set: fn(&mut Welcome, String),
    help: String,
) -> AnyView {
    column((
        small(name),
        text_field(
            resource::<Welcome, _>(move |welcome| {
                get(welcome).clone()
            }),
            move |world, text| {
                set(&mut world.resource_mut::<Welcome>(), text)
            },
        )
        .rules(well),
        note(help),
    ))
    .gap(4.0)
    .boxed()
}

/// The news of the last thing asked: that it is awaited, or the
/// reason it did not work.
fn news(waiting: &'static str) -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.asking, welcome.problem.clone())
        }),
        move |(asking, problem)| match problem {
            _ if *asking => said(waiting).boxed(),
            Some(why) => label(why.clone())
                .size(13.0)
                .tone(Tone::Critical)
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
            "ADD A PLACE FOR MODELS",
            "Where do the models come from? Your current model \
             stays as it is.",
            "CANCEL",
            "Its models are listed under Models, for you to allow.",
        )
    } else {
        (
            "WELCOME TO HAMLET",
            "Your hamlet needs a model to think with. How would you \
             like to give it one?",
            "I'LL SET THIS UP LATER",
            "You can come back to this from Settings.",
        )
    };
    column((
        title(Glyph::Hearth, name),
        said(says),
        door(
            Glyph::Signal,
            "SIGN IN WITH OPENROUTER",
            "The quickest way. You sign in with your browser and \
             pay OpenRouter for what you use.",
            sign_in,
        ),
        door(
            Glyph::Globe,
            "I HAVE A KEY",
            "Use a key from a provider such as OpenAI, DeepSeek or \
             OpenRouter. You pay them for what you use.",
            |world| go(world, Step::Key),
        ),
        door(
            Glyph::Chip,
            "I RUN MY OWN MODEL",
            "Use a model that runs on this computer, with Ollama or \
             LM Studio. Nothing leaves your machine.",
            look,
        ),
        row((
            marked(Glyph::Clock, leave)
                .rules(retrace)
                .tagged(Mood::Plain)
                .on_activate(close),
            note(after),
        ))
        .gap(8.0)
        .align(AlignItems::Center)
        .tagged(crate::platform::Wraps),
    ))
    .gap(14.0)
    .boxed()
}

/// The provider's key, and for an address of the chief's own, the
/// address.
fn key_form(chosen: usize) -> impl View {
    let names = PROVIDERS
        .iter()
        .map(|provider| provider.lettering)
        .chain(["OTHER"])
        .enumerate()
        .map(|(at, name)| {
            let look: fn(&mut Cx) =
                if at == chosen { lamp } else { brass };
            lettered(name)
                .rules(look)
                .tagged(Mood::Plain)
                .on_activate(move |world| {
                    request(world, Act::Provider(at))
                })
                .boxed()
        })
        .collect::<Vec<_>>();
    let known = PROVIDERS.get(chosen);
    let help = match known {
        Some(provider) => format!(
            "Make a key at {} and paste it here. It stays on this \
             computer.",
            provider.keys
        ),
        None => "Paste the key your provider gave you. Leave this \
                 empty if it needs none."
            .into(),
    };
    let mut parts = vec![
        column((small("WHO GIVES YOU YOUR MODEL"), buttons(names)))
            .gap(6.0)
            .boxed(),
    ];
    if known.is_none() {
        parts.push(
            typed(
                "WHERE YOUR MODEL RUNS",
                |welcome| &welcome.url,
                |welcome, text| welcome.url = text,
                "The web address your provider gave you. It usually \
                 ends in /v1."
                    .into(),
            )
            .boxed(),
        );
    }
    parts.extend([
        typed(
            "YOUR KEY",
            |welcome| &welcome.key,
            |welcome, text| welcome.key = text,
            help,
        ),
        news("Asking for its models..."),
        row((good_action(Glyph::Check, "CONNECT", |world| {
            request(world, Act::Connect)
        }),))
        .boxed(),
    ]);
    column(parts).gap(14.0)
}

/// The head of a step: the way back to the first, and its name.
fn head(which: Glyph, text: &'static str) -> impl View {
    row((
        back("BACK", |world| go(world, Step::Choice)),
        title(which, text),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
}

fn key() -> AnyView {
    column((
        head(Glyph::Globe, "USE A KEY"),
        keyed(
            resource::<Welcome, _>(|welcome| welcome.provider),
            |chosen| key_form(*chosen).boxed(),
        )
        .within(bare()),
    ))
    .gap(14.0)
    .boxed()
}

/// The page shown while the chief approves in the browser.
fn signing_in() -> AnyView {
    column((
        head(Glyph::Signal, "SIGN IN"),
        said(
            "Sign in to OpenRouter in your browser and choose \
             Authorize. Hamlet carries on once you have.",
        ),
        note(
            "Nothing opened? Go back and choose I Have a Key \
             instead.",
        ),
    ))
    .gap(14.0)
    .boxed()
}

fn local() -> AnyView {
    column((
        head(Glyph::Chip, "YOUR OWN MODEL"),
        said(
            "Hamlet looks for Ollama on this computer, then LM \
             Studio.",
        ),
        news("Looking..."),
        row((action(Glyph::Restore, "LOOK AGAIN", look),)),
    ))
    .gap(14.0)
    .boxed()
}

/// One model, as a button that chooses it.
fn offered(id: &str) -> AnyView {
    let chosen = id.to_string();
    button(row((label(id.to_string()),)).grow(1.0))
        .padding(pad())
        .min_height(px(crate::looks::CONTROL))
        .rules(brass)
        .tagged(Mood::Plain)
        .on_activate(move |world| pick(world, &chosen))
        .boxed()
}

fn models() -> AnyView {
    let shown = resource::<Welcome, _>(|welcome| {
        narrowed(welcome.models.iter(), &welcome.narrow)
    });
    let list = keyed(shown, |(ids, more)| {
        let mut rows =
            ids.iter().map(|id| offered(id)).collect::<Vec<_>>();
        if ids.is_empty() {
            rows.push(said("No model has those words in its name."));
        }
        if *more > 0 {
            rows.push(said(format!(
                "{more} more. Type part of a name to narrow them."
            )));
        }
        column(rows).gap(6.0).boxed()
    })
    .within(bare());
    AnyView::new(move |cx: &mut Cx| {
        let welcome = cx.world.resource::<Welcome>();
        let found = format!(
            "{} answered with {} {}. Pick the one your hamlet will \
             think with.",
            welcome.from,
            welcome.models.len(),
            if welcome.models.len() == 1 {
                "model"
            } else {
                "models"
            }
        );
        cx.build(
            column((
                head(Glyph::Check, "PICK A MODEL"),
                said(found),
                row((
                    small("FIND"),
                    text_field(
                        resource::<Welcome, _>(|welcome| {
                            welcome.narrow.clone()
                        }),
                        |world, text| {
                            world.resource_mut::<Welcome>().narrow =
                                text
                        },
                    )
                    .grow(1.0)
                    .rules(well)
                    .tagged(Narrowing),
                ))
                .gap(12.0)
                .align(AlignItems::Center),
                news("Asking it to say hello..."),
                list,
            ))
            .gap(14.0),
        )
    })
}

/// The screen over the panels, or nothing while it is closed.
pub fn view() -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            welcome.open.then_some((welcome.step, welcome.adding))
        }),
        |open| {
            let Some((step, adding)) = *open else {
                return frame().boxed();
            };
            let page = match step {
                Step::Choice => choice(adding),
                Step::Key => key(),
                Step::SignIn => signing_in(),
                Step::Local => local(),
                Step::Models => models(),
            };
            let card = column((
                scroll((page,)).padding(UiRect::all(px(24.0))),
            ))
            .width(percent(100.0))
            .max_width(px(560.0))
            .max_height(percent(100.0))
            .gap(0.0)
            .fill(paint(|p| p.card))
            .border(1.0)
            .border_color(paint(|p| p.brass_5))
            .radius(themed(|t| t.space.round));
            // It is filled, so that no press reaches the panels
            // under it. A row, so that the card is as tall as its
            // words are once they are wrapped to its width.
            row((card,))
                .width(percent(100.0))
                .height(percent(100.0))
                .padding(UiRect::all(px(16.0)))
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .fill(themed(|t| t.shadow.color))
                .boxed()
        },
    )
    .within(overlay(()))
    .boxed()
}
