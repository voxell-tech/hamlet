//! The realm's MCP servers in Settings (C36): each server, how it is
//! and its tools; adding, changing, restarting and removing one; and
//! bringing in those other programs list.

use std::collections::{BTreeMap, HashSet};

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::views::{FrameProps, label, row};
use bevy_fynix::{ViewExt, derived, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Settings;
use hamlet_protocol::client::{
    self as protocol, McpAct, McpEntry, McpFound, McpServerView,
    McpState,
};
use hamlet_ui::styles::{
    Fills, action, bad_action, bare, beside, close, description,
    good_action, group, list, note, pill,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::{AnyView, View};

use crate::forms::{self, Asking, OpenForm};
use crate::roll::moved;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<McpPage>()
        .add_observer(answered)
        .add_systems(
            Update,
            open_sign_in.run_if(resource_changed::<Settings>),
        );
}

/// What the page asked the server and has no answer to, the words
/// of the last that failed, and the entry the form opened with.
#[derive(Resource, Default)]
struct McpPage {
    asked: HashSet<protocol::RequestId>,
    problem: Option<String>,
    /// The server the form changes, none when it adds one, and what
    /// its fields hold as it opens.
    editing: Option<String>,
    filled: Vec<String>,
    /// The server whose sign-in page opens once its address comes.
    opening: Option<String>,
    /// The server whose sign-in the address pasted finishes.
    pasting: Option<String>,
}

/// The fields of the form, in order.
const FIELDS: &[(&str, &str)] = &[
    ("Name", "Lowercase letters, digits and -, at most 24."),
    (
        "Command",
        "The program and its arguments, a word with spaces quoted; \
         or the https address of a remote server.",
    ),
    (
        "Headers",
        "For an address: Name=value, apart by spaces, as \
         Authorization=\"Bearer token\". A name alone keeps its \
         value.",
    ),
    (
        "Environment",
        "KEY=value, apart by spaces. A key alone keeps the value it \
         has; values never leave the server.",
    ),
    ("Inherited", "Names copied from the server's environment."),
    ("Reads", "Paths it may read, from its own folder."),
    ("Writes", "Paths it may write, besides its own folder."),
    (
        "Allow",
        "Tools that run without asking, though the server does not \
         say they only read; * for every tool.",
    ),
    ("Ask", "Tools that always wait for you; * for every tool."),
    ("Timeout", "Seconds a call may take; 120 when empty."),
];

/// Sends `act` to the server, its answer awaited by the page.
fn send(world: &mut World, act: McpAct) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Mcp(protocol::Mcp { id, act })
    });
    let mut page = world.resource_mut::<McpPage>();
    page.asked.insert(id);
    page.problem = None;
}

fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        let mut page = world.resource_mut::<McpPage>();
        if !page.asked.remove(&response.to) {
            return;
        }
        page.problem = match response.message {
            protocol::ServerMessage::Fault(fault) => Some(fault.text),
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Some(reply.text)
            }
            _ => None,
        };
    });
}

/// The page of the MCP servers.
pub(crate) fn page() -> impl View {
    let servers = keyed(
        resource::<Settings, _>(|settings| {
            settings.0.mcp.servers.clone()
        }),
        |servers| {
            list(servers.iter().map(server).collect::<Vec<_>>())
                .boxed()
        },
    )
    .within(bare());
    let faults = keyed(
        resource::<Settings, _>(|settings| {
            settings.0.mcp.faults.clone()
        }),
        |faults| {
            close(
                faults
                    .iter()
                    .map(|fault| note(fault.clone()).boxed())
                    .collect::<Vec<_>>(),
            )
            .boxed()
        },
    )
    .within(bare());
    let found = keyed(
        resource::<Settings, _>(|settings| {
            settings.0.mcp.found.clone()
        }),
        |found| found_list(found),
    )
    .within(bare());
    let form = keyed(
        resource::<McpPage, _>(|page| {
            (page.editing.clone(), page.filled.clone())
        }),
        |(editing, filled)| {
            let editing = editing.clone();
            forms::filled_form(
                Asking::McpServer,
                if editing.is_some() {
                    "Change the server."
                } else {
                    "A program that gives agents tools. It runs in \
                     the sandbox on the server, without the network \
                     until you turn it on."
                },
                FIELDS,
                filled.clone(),
                "Save",
                move |world, typed| {
                    save(world, editing.as_deref(), typed)
                },
            )
        },
    )
    .within(bare());
    let paste = forms::form(
        Asking::McpPaste,
        "The address your browser ended on.",
        &[("Address", "")],
        "Sign In",
        |world, typed| {
            let Some(name) =
                world.resource_mut::<McpPage>().pasting.take()
            else {
                return true;
            };
            let address = typed.first().cloned().unwrap_or_default();
            send(
                world,
                McpAct::SignIn {
                    name,
                    pasted: Some(address),
                },
            );
            true
        },
    );
    group((
        description(
            "Each server runs in the sandbox on the server, and its \
             tools are offered to every agent. A tool that may \
             change something waits for you.",
        ),
        servers,
        faults,
        problem(),
        form,
        paste,
        row((
            good_action(Glyph::Plus, "Add Server", |world| {
                open(world, None, Vec::new());
            }),
            action(Glyph::Lens, "Find in Other Programs", |world| {
                send(world, McpAct::Find);
            }),
        ))
        .gap(8.0),
        found,
    ))
}

/// The words of the last request that failed.
fn problem() -> impl View {
    note(
        derived(|world: &World| {
            world
                .get_resource::<McpPage>()
                .and_then(|page| page.problem.clone())
                .unwrap_or_default()
        })
        .when(moved::<McpPage>()),
    )
}

/// Opens the form on `editing`, none to add a server, with `filled`.
fn open(
    world: &mut World,
    editing: Option<String>,
    filled: Vec<String>,
) {
    let mut page = world.resource_mut::<McpPage>();
    page.editing = editing;
    page.filled = filled;
    world.resource_mut::<OpenForm>().0 = Some(Asking::McpServer);
}

/// One server: its name, how it is, its tools, and what a chief does
/// to it.
fn server(server: &McpServerView) -> AnyView {
    let name = server.name.clone();
    let state = match &server.state {
        McpState::Starting => pill("Starting").boxed(),
        McpState::Up => pill("Up").boxed(),
        McpState::Down { .. } => pill("Stopped").boxed(),
    };
    let asks = server.tools.iter().filter(|tool| tool.acts).count();
    let tools = match (server.tools.len(), asks) {
        (0, _) => "No tools".to_owned(),
        (1, 0) => "1 tool".to_owned(),
        (n, 0) => format!("{n} tools"),
        (n, asks) => {
            format!("{n} tools, {asks} ask before each call")
        }
    };
    let mut parts =
        vec![
            beside((
                row((label(name.clone()).bold(true), state))
                    .gap(8.0)
                    .align(AlignItems::Center)
                    .fills_width(),
                action(Glyph::Rename, "Change", {
                    let (name, entry) =
                        (name.clone(), server.entry.clone());
                    move |world| {
                        open(
                            world,
                            Some(name.clone()),
                            filled(&name, &entry),
                        );
                    }
                }),
                action(Glyph::Restore, "Restart", {
                    let name = name.clone();
                    move |world| {
                        send(
                            world,
                            McpAct::Restart { name: name.clone() },
                        );
                    }
                }),
                bad_action(Glyph::Gone, "Remove", {
                    let name = name.clone();
                    move |world| {
                        send(
                            world,
                            McpAct::Remove { name: name.clone() },
                        );
                    }
                }),
            ))
            .boxed(),
            note(server.entry.url.clone().unwrap_or_else(|| {
                command_line(&server.entry.command)
            }))
            .boxed(),
        ];
    if let McpState::Down { reason, .. } = &server.state {
        parts.push(note(format!("It stopped: {reason}")).boxed());
    }
    parts.push(note(tools).boxed());
    if server.entry.url.is_some() {
        parts.push(sign_in(&name, server.sign_in.as_deref()));
        return close(parts).boxed();
    }
    let entry = server.entry.clone();
    parts.push(
        labelled_switch(
            "Network",
            server.entry.net,
            move |world, net| {
                send(
                    world,
                    McpAct::Put {
                        name: name.clone(),
                        entry: McpEntry {
                            net,
                            ..entry.clone()
                        },
                    },
                );
            },
        )
        .boxed(),
    );
    close(parts).boxed()
}

/// The sign-in to the server `name`, reached by a url: a press that
/// begins it, or while it waits for `address`, ways to finish it.
fn sign_in(name: &str, address: Option<&str>) -> AnyView {
    let name = name.to_owned();
    let Some(address) = address else {
        return row((action(Glyph::Key, "Sign In", move |world| {
            send(
                world,
                McpAct::SignIn {
                    name: name.clone(),
                    pasted: None,
                },
            );
            world.resource_mut::<McpPage>().opening =
                Some(name.clone());
        }),))
        .boxed();
    };
    let address = address.to_owned();
    close((
        note(
            "Sign in on the page your browser opened. If it ends on \
             a page that does not load, paste that page's address.",
        ),
        row((
            action(Glyph::Into, "Open Again", move |world| {
                browse(world, &address);
            }),
            action(Glyph::Notes, "Paste Address", move |world| {
                world.resource_mut::<McpPage>().pasting =
                    Some(name.clone());
                world.resource_mut::<OpenForm>().0 =
                    Some(Asking::McpPaste);
            }),
        ))
        .gap(8.0),
    ))
    .boxed()
}

/// Opens `address` in the chief's browser, when it is a web page.
fn browse(world: &mut World, address: &str) {
    if crate::welcome::providers::is_browsable(address) {
        let browse =
            world.resource::<crate::welcome::providers::Browser>().0;
        browse(world, address);
    }
}

/// Opens the page of the sign-in the chief began, once its server
/// gives its address.
fn open_sign_in(world: &mut World) {
    let Some(name) = world.resource::<McpPage>().opening.clone()
    else {
        return;
    };
    let address = world
        .resource::<Settings>()
        .0
        .mcp
        .servers
        .iter()
        .find(|server| server.name == name)
        .and_then(|server| server.sign_in.clone());
    if let Some(address) = address {
        world.resource_mut::<McpPage>().opening = None;
        browse(world, &address);
    }
}

/// The servers other programs list, each added with a press.
fn found_list(found: &[McpFound]) -> AnyView {
    if found.is_empty() {
        return close(()).boxed();
    }
    let rows = found
        .iter()
        .map(|found| {
            let name = found.name.clone();
            let add = match &found.unrunnable {
                None => {
                    good_action(Glyph::Plus, "Add", move |world| {
                        send(
                            world,
                            McpAct::Import {
                                names: vec![name.clone()],
                            },
                        );
                    })
                }
                Some(_) => close(()).boxed(),
            };
            let about = match (&found.entry, &found.unrunnable) {
                (_, Some(why)) => why.clone(),
                (Some(entry), None) => command_line(&entry.command),
                (None, None) => String::new(),
            };
            close((
                beside((
                    row((
                        label(found.name.clone()).bold(true),
                        pill(found.source.clone()),
                    ))
                    .gap(8.0)
                    .align(AlignItems::Center)
                    .fills_width(),
                    add,
                )),
                note(about),
            ))
            .boxed()
        })
        .collect::<Vec<_>>();
    let runs = found.iter().any(|found| found.unrunnable.is_none());
    close((
        list(rows),
        if runs {
            action(Glyph::CheckAll, "Add Every One", |world| {
                send(world, McpAct::Import { names: Vec::new() });
            })
        } else {
            close(()).boxed()
        },
    ))
    .boxed()
}

/// What the form's fields hold for the server `name`'s `entry`.
fn filled(name: &str, entry: &McpEntry) -> Vec<String> {
    vec![
        name.to_owned(),
        entry
            .url
            .clone()
            .unwrap_or_else(|| command_line(&entry.command)),
        entry.headers.keys().cloned().collect::<Vec<_>>().join(" "),
        entry.env.keys().cloned().collect::<Vec<_>>().join(" "),
        entry.inherit_env.join(" "),
        command_line(&entry.fs_ro),
        command_line(&entry.fs_rw),
        entry.allow.join(" "),
        entry.ask.join(" "),
        entry
            .timeout_secs
            .map(|secs| secs.to_string())
            .unwrap_or_default(),
    ]
}

/// Sends the entry `typed` says, for the server `editing` or a new
/// one; false, with the problem shown, when it does not fit.
fn save(
    world: &mut World,
    editing: Option<&str>,
    typed: &[String],
) -> bool {
    let field = |at: usize| typed.get(at).map_or("", String::as_str);
    let before = editing.and_then(|name| {
        world
            .resource::<Settings>()
            .0
            .mcp
            .servers
            .iter()
            .find(|server| server.name == name)
            .map(|server| server.entry.clone())
    });
    let entry = (|| -> Result<McpEntry, String> {
        let mut command = words(field(1))?;
        let url = match command.as_slice() {
            [] => {
                return Err(
                    "Type the server's command or address.".into()
                );
            }
            [address]
                if address.starts_with("https://")
                    || address.starts_with("http://") =>
            {
                Some(command.remove(0))
            }
            _ => None,
        };
        let timeout_secs = match field(9) {
            "" => None,
            secs => Some(
                secs.parse::<u64>()
                    .ok()
                    .filter(|secs| *secs > 0)
                    .ok_or("The timeout is whole seconds above 0.")?,
            ),
        };
        Ok(McpEntry {
            command,
            url,
            headers: pairs(field(2))?,
            env: pairs(field(3))?,
            inherit_env: words(field(4))?,
            fs_ro: words(field(5))?,
            fs_rw: words(field(6))?,
            net: before.as_ref().is_some_and(|entry| entry.net),
            timeout_secs,
            allow: words(field(7))?,
            ask: words(field(8))?,
        })
    })();
    let entry = match entry {
        Ok(entry) => entry,
        Err(problem) => {
            world.resource_mut::<McpPage>().problem = Some(problem);
            return false;
        }
    };
    let name = field(0).to_owned();
    if let Some(before) = editing
        && before != name
    {
        world.resource_mut::<McpPage>().problem = Some(
            "A server's name stays; remove it and add it again to \
             rename it."
                .into(),
        );
        return false;
    }
    send(world, McpAct::Put { name, entry });
    true
}

/// The `Name=value` words of `line`; a name alone is `None`, its
/// value kept as it is.
fn pairs(
    line: &str,
) -> Result<BTreeMap<String, Option<String>>, String> {
    Ok(words(line)?
        .into_iter()
        .map(|word| match word.split_once('=') {
            Some((name, value)) => {
                (name.to_owned(), Some(value.to_owned()))
            }
            None => (word, None),
        })
        .collect())
}

/// `line` split into words at spaces outside quotes.
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut begun = false;
    for c in line.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                begun = true;
            }
            (Some(open), c) if c == open => quote = None,
            (None, c) if c.is_whitespace() => {
                if begun {
                    words.push(std::mem::take(&mut word));
                    begun = false;
                }
            }
            (_, c) => {
                word.push(c);
                begun = true;
            }
        }
    }
    if quote.is_some() {
        return Err("A quote is not closed.".into());
    }
    if begun {
        words.push(word);
    }
    Ok(words)
}

/// `words` on one line, a word with a space or a quote quoted.
fn command_line(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            if word.is_empty() || word.contains([' ', '"', '\'']) {
                if word.contains('"') {
                    format!("'{word}'")
                } else {
                    format!("\"{word}\"")
                }
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_reads_back_as_its_words() {
        let typed = r#"npx -y "@scope/a server" 'say "hi"'"#;
        let read = words(typed).unwrap();
        assert_eq!(
            read,
            ["npx", "-y", "@scope/a server", "say \"hi\""]
        );
        assert_eq!(words(&command_line(&read)).unwrap(), read);
        assert!(words("\"open").is_err());
    }
}
