//! The pages of first-run setup.

use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, overlay,
    row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{
    self, back, back_link, bare, beside, description, good_action,
    group, headed, marked, nav_card, note, pad, parts,
    scroll_area_on, small_rules, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::fade::FadeSurface;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, View, built, with_cx};

use super::{Narrowing, Step, Welcome, close, go, look, pick};
use crate::account::{self, Form, Page, Place};
use crate::models::narrowed;

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
            "ADD A PLACE FOR MODELS",
            "Where do the models come from? Your current model \
             stays as it is.",
            "CANCEL",
            "Its models are listed under Models, for you to allow.",
        )
    } else {
        (
            "GIVE YOUR HAMLET A MODEL",
            "Your hamlet needs a model to think with. How would you \
             like to give it one?",
            "I'LL SET THIS UP LATER",
            "You can come back to this from Settings.",
        )
    };
    parts((
        title(Glyph::Hearth, name),
        said(says),
        nav_card(
            Glyph::Signal,
            "CONNECT OPENROUTER",
            "The quickest way. You connect with your browser and \
             pay OpenRouter for what you use.",
            |world| {
                go(world, Step::Limit);
            },
        ),
        nav_card(
            Glyph::Talk,
            "CONTINUE WITH CHATGPT",
            "Use your ChatGPT plan, with no key. You sign in with \
             your browser, and what your hamlet does counts toward \
             your plan's usage.",
            |world| super::providers::start(world, "chatgpt", false),
        ),
        nav_card(
            Glyph::Globe,
            "I HAVE A KEY",
            "Use a key from a provider such as OpenAI, DeepSeek or \
             OpenRouter. You pay them for what you use.",
            |world| go(world, Step::Key),
        ),
        nav_card(
            Glyph::Chip,
            "I RUN MY OWN MODEL",
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

/// The first step: a Hamlet account made, signed in to or skipped,
/// or the account's page open here.
fn account() -> AnyView {
    keyed(
        resource::<Form, _>(|form| form.at(Place::Welcome)),
        |page| match page {
            Page::Shut => parts((
                title(Glyph::Hearth, "WELCOME TO HAMLET"),
                said(
                    "A Hamlet account lets you buy and sell at the \
                     Agent Bazaar, and keeps your OpenRouter key on \
                     every computer you sign in on. Hamlet works \
                     without one.",
                ),
                nav_card(
                    Glyph::Plus,
                    "CREATE A HAMLET ACCOUNT",
                    "An email, a password and a username. It is \
                     needed for the Agent Bazaar.",
                    |world| {
                        account::open(
                            world,
                            Place::Welcome,
                            Page::SignUp,
                        );
                    },
                ),
                nav_card(
                    Glyph::Signpost,
                    "SIGN IN",
                    "You have a Hamlet account already.",
                    |world| {
                        account::open(
                            world,
                            Place::Welcome,
                            Page::SignIn,
                        );
                    },
                ),
                beside((
                    marked(Glyph::Clock, "SKIP")
                        .rules(back_link)
                        .tagged(Variant::Default)
                        .on_activate(|world| go(world, Step::Choice)),
                    note("You can create one later from Settings."),
                ))
                .tagged(hamlet_ui::widgets::platform::Wraps),
            ))
            .boxed(),
            page => parts((
                beside((
                    back("BACK", account::shut),
                    title(Glyph::Hearth, page.title()),
                )),
                account::body(*page, Place::Welcome),
            ))
            .boxed(),
        },
    )
    .within(bare())
    .boxed()
}

/// The head of a step: the way back to the first, and its name.
pub(super) fn head(which: Glyph, text: &'static str) -> impl View {
    beside((
        back("BACK", |world| go(world, Step::Choice)),
        title(which, text),
    ))
}

/// One model, as a button that chooses it.
fn offered(id: &str) -> AnyView {
    let chosen = id.to_string();
    button(row((label(id.to_string()),)).grow(1.0))
        .padding(pad())
        .min_height(px(styles::CONTROL))
        .rules(styles::button)
        .tagged(Variant::Default)
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
            group(rows).boxed()
        },
    )
    .within(bare());
    parts((
        head(Glyph::Check, "PICK A MODEL"),
        description(resource::<Welcome, _>(|welcome| {
            format!(
                "{} listed these models. Pick the model your hamlet \
                 will think with.",
                welcome.from
            )
        })),
        text_field(
            resource::<Welcome, _>(|welcome| welcome.narrow.clone()),
            |world, text| {
                world.resource_mut::<Welcome>().narrow = text;
            },
        )
        .rules(text_input)
        .tagged(Narrowing),
        news("Asking it to say hello..."),
        list,
        bazaar(),
    ))
    .boxed()
}

/// The way to every model in the Agent Bazaar, while a place for
/// models is added to a home that has one already.
fn bazaar() -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| welcome.adding),
        |adding| {
            if !*adding {
                return bare().boxed();
            }
            row((
                styles::action(
                    Glyph::Coins,
                    "BROWSE THE BAZAAR",
                    |world| {
                        close(world);
                        crate::bazaar::browse_models(world);
                    },
                ),
                note(
                    "Or choose among every model in the Agent \
                     Bazaar.",
                ),
            ))
            .gap(12.0)
            .align(AlignItems::Center)
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

fn local() -> AnyView {
    parts((
        head(Glyph::Chip, "YOUR OWN MODEL"),
        said(
            "Hamlet asks the endpoints configured on the server. \
             Ollama or LM Studio must run on the server's machine.",
        ),
        news("Looking..."),
        good_action(Glyph::Restore, "LOOK AGAIN", look),
    ))
    .boxed()
}

pub fn view() -> impl View {
    built(|cx| {
        cx.world.init_resource::<super::providers::Login>();
        cx.world.init_resource::<super::providers::SpendLimit>();
        cx.world.init_resource::<super::Browser>();
        cx.world.init_resource::<super::Committing>();
        cx.world.init_resource::<Form>();
        cx.world.init_resource::<account::Committing>();
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
                    title(Glyph::Signal, "CONNECT TO A SERVER"),
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
                    Step::Account => account(),
                    Step::Choice => choice(*adding),
                    Step::Key => super::providers::key_page(),
                    Step::Limit => super::providers::limit_page(),
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
    let card = column((scroll_area_on(
        FadeSurface::Card,
        (column((page,))
            .gap(0.0)
            .padding(UiRect::all(px(space.overlay_padding))),),
    ),))
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
        .fill(theme.shadow.color)
}
