//! The Charter page: the words every agent's prompt opens with, the
//! way to edit them, and the way back to the ones built in.

use hamlet_engine_sdk::api::agents::AgentsRequest;
use hamlet_engine_sdk::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Size, Space, Tone, UiExt,
    ViewExt, button, column, divider, icon_button, keyed, label,
    resource, row,
};

use crate::parts::{note, problem};
use crate::{Known, Page, SettingsWorldExt};

pub(crate) fn page() -> AnyView {
    let actions = keyed(
        resource::<Known, _>(|known| {
            let own = known
                .charter
                .as_ref()
                .is_some_and(|charter| charter.own);
            (own, own && known.confirming)
        }),
        |&(own, confirming)| {
            if confirming {
                return confirm();
            }
            let mut actions = vec![
                icon_button("pencil", "EDIT")
                    .on_click(|world| {
                        world.ask(
                            Page::Charter,
                            AgentsRequest::EditCharter,
                        );
                    })
                    .boxed(),
            ];
            if own {
                actions.push(
                    icon_button("restore", "BACK TO THE DEFAULT")
                        .on_click(|world| {
                            world
                                .resource_mut::<Known>()
                                .confirming = true;
                        })
                        .boxed(),
                );
            }
            row(actions).boxed()
        },
    );
    let shown = keyed(
        resource::<Known, _>(|known| {
            known
                .charter
                .as_ref()
                .map(|charter| charter.text.clone())
                .unwrap_or_default()
        }),
        |text| markdown(text),
    );
    column((
        note("Every agent reads this at the start of each turn."),
        note("Edit opens the file in your editor."),
        actions,
        problem(Page::Charter),
        divider(),
        shown,
    ))
    .boxed()
}

/// The question before the home's own charter is deleted.
fn confirm() -> AnyView {
    column((
        label("GO BACK TO THE DEFAULT CHARTER?")
            .bold(true)
            .face(Face::Display),
        note(
            "Your own charter is deleted from disk. This cannot be \
             undone.",
        ),
        row((
            button("USE THE DEFAULT")
                .style(ButtonStyle::Critical)
                .on_click(|world| {
                    world.resource_mut::<Known>().confirming = false;
                    world.ask(
                        Page::Charter,
                        AgentsRequest::ResetCharter,
                    );
                }),
            button("CANCEL").style(ButtonStyle::Quiet).on_click(
                |world| {
                    world.resource_mut::<Known>().confirming = false;
                },
            ),
        )),
    ))
    .space(Space::Half)
    .boxed()
}

/// `text`, Markdown, as words: a heading a line in bold, an item of a
/// list a line of its own, and the lines of a paragraph joined.
fn markdown(text: &str) -> AnyView {
    let mut blocks = Vec::new();
    let mut paragraph = Vec::<&str>::new();
    let mut fenced = false;
    let end = |paragraph: &mut Vec<&str>,
               blocks: &mut Vec<AnyView>| {
        if !paragraph.is_empty() {
            blocks.push(label(paragraph.join(" ")).boxed());
            paragraph.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            end(&mut paragraph, &mut blocks);
            fenced = !fenced;
            continue;
        }
        if fenced {
            blocks.push(
                label(line.to_string()).tone(Tone::Dim).boxed(),
            );
            continue;
        }
        if trimmed.is_empty() {
            end(&mut paragraph, &mut blocks);
        } else if let Some(title) = trimmed.strip_prefix('#') {
            end(&mut paragraph, &mut blocks);
            blocks.push(
                label(
                    title.trim_start_matches('#').trim().to_string(),
                )
                .bold(true)
                .size(Size::Heading)
                .boxed(),
            );
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            end(&mut paragraph, &mut blocks);
            blocks.push(label(format!("- {item}")).boxed());
        } else {
            paragraph.push(trimmed);
        }
    }
    end(&mut paragraph, &mut blocks);
    column(blocks).boxed()
}
