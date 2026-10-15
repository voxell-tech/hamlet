//! The pages of first-run setup.

use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, overlay,
    row, scroll, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};

use super::{Narrowing, Step, Welcome, close, go, look, pick};
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    back, bare, brass, door, good_action, headed, marked, note, pad,
    retrace, well,
};
use crate::models::narrowed;
use crate::platform::DisplayText;
use crate::{AnyView, Cx, View, with_cx};

fn title(which: Glyph, text: &'static str) -> impl View {
    headed(
        which,
        label(text).size(15.0).bold(true).tagged(DisplayText),
    )
}

pub(super) fn said(text: impl Into<String>) -> AnyView {
    label(text.into()).size(13.0).tone(Tone::Dim).boxed()
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
            |world| {
                super::providers::start(world, "openrouter", false)
            },
        ),
        door(
            Glyph::Talk,
            "CONTINUE WITH CHATGPT",
            "Use your ChatGPT plan, with no key. You sign in with \
             your browser, and what your hamlet does counts toward \
             your plan's usage.",
            |world| super::providers::start(world, "chatgpt", false),
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
            "Use a model served on the server's machine, with \
             Ollama or LM Studio.",
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

/// The head of a step: the way back to the first, and its name.
pub(super) fn head(which: Glyph, text: &'static str) -> impl View {
    row((
        back("BACK", |world| go(world, Step::Choice)),
        title(which, text),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
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
    let list = keyed(
        resource::<Welcome, _>(|welcome| {
            narrowed(welcome.models.iter(), &welcome.narrow)
        }),
        |(ids, more)| {
            let mut rows =
                ids.iter().map(|id| offered(id)).collect::<Vec<_>>();
            if ids.is_empty() {
                rows.push(said(
                    "No model has those words in its name.",
                ));
            }
            if *more > 0 {
                rows.push(said(format!(
                    "{more} more. Type part of a name to narrow \
                     them."
                )));
            }
            column(rows).gap(6.0).boxed()
        },
    )
    .within(bare());
    column((
        head(Glyph::Check, "PICK A MODEL"),
        label(resource::<Welcome, _>(|welcome| {
            format!(
                "{} listed these models. Pick the model your hamlet \
                 will think with.",
                welcome.from
            )
        }))
        .size(13.0)
        .tone(Tone::Dim),
        text_field(
            resource::<Welcome, _>(|welcome| welcome.narrow.clone()),
            |world, text| {
                world.resource_mut::<Welcome>().narrow = text
            },
        )
        .rules(well)
        .tagged(Narrowing),
        news("Asking it to say hello..."),
        list,
    ))
    .gap(14.0)
    .boxed()
}

fn local() -> AnyView {
    column((
        head(Glyph::Chip, "YOUR OWN MODEL"),
        said(
            "Hamlet asks the endpoints configured on the server. \
             Ollama or LM Studio must run on the server's machine.",
        ),
        news("Looking..."),
        good_action(Glyph::Restore, "LOOK AGAIN", look),
    ))
    .gap(14.0)
    .boxed()
}

pub fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
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
                column((
                    title(Glyph::Signal, "CONNECT TO A SERVER"),
                    said(words.clone()),
                    said(
                        "Start a server with hamlet server, then \
                         open its window with hamlet window. For \
                         local development, use hamlet dev.",
                    ),
                ))
                .gap(14.0)
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
            with_cx(move |cx| over(cx, page))
        },
    )
    .within(overlay(()))
    .boxed()
}

fn over(cx: &Cx, page: AnyView) -> impl View + use<> {
    let theme = cx.theme();
    let card =
        column((scroll((page,)).padding(UiRect::all(px(24.0))),))
            .width(percent(100.0))
            .max_width(px(560.0))
            .max_height(percent(100.0))
            .gap(0.0)
            .fill(theme.palette.card)
            .border(1.0)
            .border_color(theme.palette.brass_5)
            .radius(theme.space.round);
    row((card,))
        .width(percent(100.0))
        .height(percent(100.0))
        .padding(UiRect::all(px(16.0)))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .fill(theme.shadow.color)
}
