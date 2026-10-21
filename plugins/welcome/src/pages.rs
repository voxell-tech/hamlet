//! The pages of the card, one for each step.

use agents_ui::{name, narrowed, note};
use hamlet_engine_sdk::api::agents::{AgentsRequest, SignInTo};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, button, column, field, icon, icon_button,
    keyed, label, resource, row,
};

use crate::{Step, Welcome, WelcomeWorldExt};

/// The most models the card lists at once.
const SHOWN: usize = 40;

/// The card's page for the step the person is at.
pub(crate) fn card() -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            let adding = welcome
                .setup
                .as_ref()
                .is_some_and(|setup| setup.adding);
            (welcome.step, adding)
        }),
        |&(step, adding)| match step {
            Step::Choice => choice(adding),
            Step::Key => key(),
            Step::SignIn(to) => signing_in(to),
            Step::Local => local(),
            Step::Models => models(),
        },
    )
    .boxed()
}

fn title(which: &str, text: &str) -> AnyView {
    row((
        icon(which.to_string()).tone(Tone::Accent),
        label(text.to_string())
            .size(Size::Heading)
            .bold(true)
            .face(Face::Display),
    ))
    .space(Space::Half)
    .boxed()
}

fn said(text: impl Into<String>) -> AnyView {
    label(text.into()).tone(Tone::Dim).boxed()
}

/// The news of the last thing asked: that it is awaited, or why it
/// did not work.
fn news(waiting: &'static str) -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.busy(), welcome.trouble())
        }),
        move |(busy, trouble)| match trouble {
            _ if *busy => said(waiting),
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => column(()).boxed(),
        },
    )
    .boxed()
}

/// One way in: a button and what it means.
fn door(
    which: &str,
    text: &str,
    means: &str,
    open: impl Fn(&mut hamlet_engine_sdk::World) + Send + Sync + 'static,
) -> AnyView {
    column((
        row((icon_button(which.to_string(), text.to_string())
            .on_click(open),)),
        note(means),
    ))
    .space(Space::Half)
    .boxed()
}

/// The head of a step past the first: the way back, and its name.
fn head(which: &str, text: &str) -> AnyView {
    row((
        icon_button("back", "BACK")
            .style(ButtonStyle::Quiet)
            .on_click(|world| {
                world.resource_mut::<Welcome>().go(Step::Choice)
            }),
        title(which, text),
    ))
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
        title("house", name),
        said(says),
        door(
            "signal",
            "SIGN IN WITH OPENROUTER",
            "The quickest way. You sign in with your browser and \
             pay OpenRouter for what you use.",
            |world| {
                world
                    .ask(AgentsRequest::SignIn(SignInTo::OpenRouter));
            },
        ),
        door(
            "speech",
            "CONTINUE WITH CHATGPT",
            "Use your ChatGPT plan, with no key. You sign in with \
             your browser, and what your hamlet does counts toward \
             your plan's usage.",
            |world| {
                world.ask(AgentsRequest::SignIn(SignInTo::ChatGpt))
            },
        ),
        door(
            "globe",
            "I HAVE A KEY",
            "Use a key from a provider such as OpenAI, DeepSeek or \
             OpenRouter. You pay them for what you use.",
            |world| world.resource_mut::<Welcome>().go(Step::Key),
        ),
        door(
            "chip",
            "I RUN MY OWN MODEL",
            "Use a model that runs on this computer, with Ollama or \
             LM Studio. Nothing leaves your machine.",
            |world| {
                world.resource_mut::<Welcome>().go(Step::Local);
                world.ask(AgentsRequest::FindLocal);
            },
        ),
        row((
            icon_button("clock", leave)
                .style(ButtonStyle::Quiet)
                .on_click(|world| world.ask(AgentsRequest::EndSetup)),
            note(after),
        )),
    ))
    .space(Space::Double)
    .boxed()
}

fn key() -> AnyView {
    column((
        head("globe", "USE A KEY"),
        keyed(
            resource::<Welcome, _>(|welcome| {
                (welcome.provider, welcome.providers.clone())
            }),
            |(chosen, providers)| key_form(*chosen, providers),
        ),
    ))
    .space(Space::Double)
    .boxed()
}

/// The provider's key, and for an address of the person's own, the
/// address.
fn key_form(
    chosen: usize,
    providers: &[hamlet_engine_sdk::api::agents::Provider],
) -> AnyView {
    let names = providers
        .iter()
        .map(|provider| provider.name.to_uppercase())
        .chain(["OTHER".to_string()])
        .enumerate()
        .map(|(at, name)| {
            let style = if at == chosen {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Plain
            };
            button(name)
                .style(style)
                .on_click(move |world| {
                    let welcome = world.resource_mut::<Welcome>();
                    welcome.provider = at;
                    welcome.forget_problem();
                })
                .boxed()
        })
        .collect::<Vec<_>>();
    let known = providers.get(chosen);
    let help = match known {
        Some(provider) => format!(
            "Make a key at {} and paste it here. It stays on this \
             computer.",
            provider.keys_at
        ),
        None => "Paste the key your provider gave you. Leave this \
                 empty if it needs none."
            .into(),
    };
    let mut parts = vec![
        column((name("WHO GIVES YOU YOUR MODEL"), row(names)))
            .space(Space::Half)
            .boxed(),
    ];
    if known.is_none() {
        parts.push(
            column((
                name("WHERE YOUR MODEL RUNS"),
                field(resource::<Welcome, _>(|welcome| {
                    welcome.url.clone()
                }))
                .placeholder("https://")
                .on_commit(|world, text| {
                    world.resource_mut::<Welcome>().url = text;
                }),
                note(
                    "The web address your provider gave you. It \
                     usually ends in /v1.",
                ),
            ))
            .space(Space::Half)
            .boxed(),
        );
    }
    let endpoint = known.map(|provider| provider.endpoint.clone());
    parts.extend([
        column((
            name("YOUR KEY"),
            field(resource::<Welcome, _>(|welcome| {
                welcome.key.clone()
            }))
            .placeholder("Paste your key")
            .on_commit(|world, text| {
                world.resource_mut::<Welcome>().key = text;
            }),
            note(help),
        ))
        .space(Space::Half)
        .boxed(),
        news("Asking for its models..."),
        row((icon_button("check", "CONNECT")
            .style(ButtonStyle::Primary)
            .on_click(move |world| {
                let welcome = world.resource_mut::<Welcome>();
                // The key goes to the service, which keeps it.
                let key = std::mem::take(&mut welcome.key);
                let url = welcome.url.clone();
                world.ask(AgentsRequest::Connect {
                    provider: endpoint.clone(),
                    url,
                    key,
                });
            }),))
        .boxed(),
    ]);
    column(parts).space(Space::Double).boxed()
}

/// The page shown while the person signs in in the browser.
fn signing_in(to: SignInTo) -> AnyView {
    let asked = match to {
        SignInTo::ChatGpt => {
            "Sign in to ChatGPT in your browser and allow Hamlet to \
             use your plan. Hamlet carries on once you have."
        }
        SignInTo::OpenRouter => {
            "Sign in to OpenRouter in your browser and choose \
             Authorize. Hamlet carries on once you have."
        }
    };
    column((
        head("signal", "SIGN IN"),
        said(asked),
        news("Asking for its models..."),
        note(
            "Nothing opened? Go back and choose I Have a Key \
             instead.",
        ),
    ))
    .space(Space::Double)
    .boxed()
}

fn local() -> AnyView {
    column((
        head("chip", "YOUR OWN MODEL"),
        said(
            "Hamlet looks for Ollama on this computer, then LM \
             Studio.",
        ),
        news("Looking..."),
        row((icon_button("restore", "LOOK AGAIN")
            .on_click(|world| world.ask(AgentsRequest::FindLocal)),)),
    ))
    .space(Space::Double)
    .boxed()
}

fn models() -> AnyView {
    let found = resource::<Welcome, _>(|welcome| {
        let Some(setup) = &welcome.setup else {
            return String::new();
        };
        let count = setup.models.len();
        let mut found = format!(
            "{} answered with {count} {}. Pick the one your hamlet \
             will think with.",
            setup.from,
            if count == 1 { "model" } else { "models" }
        );
        if setup.plan {
            found.push_str(
                " You're using your ChatGPT plan: its models count \
                 toward the plan's usage, which Manage Usage under \
                 Endpoints in Settings opens.",
            );
        }
        found
    });
    let list = keyed(
        resource::<Welcome, _>(|welcome| {
            let models = welcome
                .setup
                .as_ref()
                .map(|setup| setup.models.as_slice())
                .unwrap_or_default();
            narrowed(models, &welcome.narrow, SHOWN, String::as_str)
        }),
        |(ids, more)| {
            let mut rows = ids
                .iter()
                .map(|id| {
                    let picked = id.clone();
                    button(id.clone())
                        .on_click(move |world| {
                            world.ask(AgentsRequest::PickModel(
                                picked.clone(),
                            ));
                        })
                        .boxed()
                })
                .collect::<Vec<_>>();
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
            column(rows).space(Space::Half).boxed()
        },
    );
    column((
        head("check", "PICK A MODEL"),
        label(found).tone(Tone::Dim),
        row((
            name("FIND"),
            field(resource::<Welcome, _>(|welcome| {
                welcome.narrow.clone()
            }))
            .placeholder("Part of a name")
            .on_commit(|world, text| {
                world.resource_mut::<Welcome>().narrow = text;
            }),
        )),
        news("Asking it to say hello..."),
        list,
    ))
    .space(Space::Double)
    .boxed()
}
