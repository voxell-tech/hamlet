//! The realm's MCP servers from the command line (C36): `hamlet mcp`.

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::{
    ClientMessage, McpAct, McpEntry, McpServerView, McpState,
    RequestId, ServerMessage, State, Update,
};

use crate::client::Client;
use crate::terminal::{checked, in_editor};

/// What `hamlet mcp` was asked to do.
pub enum Mcp {
    List,
    Tools(String),
    /// Boxed, as an entry is larger than the rest together.
    Add {
        name: String,
        entry: Box<McpEntry>,
    },
    Edit(String),
    Remove(String),
    Restart(String),
    /// Signs in to a server reached by a url, or finishes the sign-in
    /// with the address the browser ended on.
    SignIn {
        name: String,
        pasted: Option<String>,
    },
    /// Finds the servers other programs list, and adds those named,
    /// or every one that runs.
    Import {
        names: Vec<String>,
        all: bool,
    },
}

/// The command `hamlet mcp <args>` names.
pub fn parse(args: &[String]) -> Result<Mcp> {
    let action = args.first().map_or("list", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    let name = || -> Result<String> {
        match rest {
            [name] if !name.starts_with('-') => Ok(name.clone()),
            [] => bail!("mcp {action} needs a server's name"),
            [_, extra, ..] | [extra] => {
                bail!("unknown argument `{extra}`")
            }
        }
    };
    Ok(match action {
        "list" if rest.is_empty() => Mcp::List,
        "tools" => Mcp::Tools(name()?),
        "edit" => Mcp::Edit(name()?),
        "remove" => Mcp::Remove(name()?),
        "restart" => Mcp::Restart(name()?),
        "sign-in" => match rest {
            [name, ..] if name.starts_with('-') => {
                bail!("unknown option `{name}`")
            }
            [name] => Mcp::SignIn {
                name: name.clone(),
                pasted: None,
            },
            [name, address] => Mcp::SignIn {
                name: name.clone(),
                pasted: Some(address.clone()),
            },
            [] => bail!("mcp sign-in needs a server's name"),
            [_, _, extra, ..] => bail!("unknown argument `{extra}`"),
        },
        "add" => add(rest)?,
        "import" => {
            let all = rest.iter().any(|arg| arg == "--all");
            let names = rest
                .iter()
                .filter(|arg| *arg != "--all")
                .cloned()
                .collect::<Vec<_>>();
            if let Some(flag) =
                names.iter().find(|n| n.starts_with('-'))
            {
                bail!("unknown option `{flag}`");
            }
            if all && !names.is_empty() {
                bail!("mcp import takes --all or names, not both");
            }
            Mcp::Import { names, all }
        }
        other => bail!("unknown mcp command `{other}`"),
    })
}

/// `hamlet mcp add <name> [flags] -- <command> [<arg>...]`, or
/// `hamlet mcp add <name> --url <address> [--header
/// <Name=value>]...`.
fn add(args: &[String]) -> Result<Mcp> {
    let (name, args) = args
        .split_first()
        .filter(|(name, _)| !name.starts_with('-'))
        .context("mcp add needs a server's name")?;
    let split = args.iter().position(|arg| arg == "--");
    let mut entry = McpEntry {
        command: split
            .map_or_else(Vec::new, |at| args[at + 1..].to_vec()),
        ..McpEntry::default()
    };
    let mut flags = args[..split.unwrap_or(args.len())].iter();
    while let Some(flag) = flags.next() {
        if flag == "--net" {
            entry.net = true;
            continue;
        }
        let value = flags
            .next()
            .with_context(|| format!("{flag} needs a value"))?
            .clone();
        match flag.as_str() {
            "--url" => entry.url = Some(value),
            "--header" => {
                let (name, value) = value
                    .split_once('=')
                    .context("--header takes Name=value")?;
                entry.headers.insert(name.into(), Some(value.into()));
            }
            "--env" => {
                let (key, value) = value
                    .split_once('=')
                    .context("--env takes KEY=VALUE")?;
                entry.env.insert(key.into(), Some(value.into()));
            }
            "--inherit-env" => entry.inherit_env.push(value),
            "--read" => entry.fs_ro.push(value),
            "--write" => entry.fs_rw.push(value),
            "--allow" => entry.allow.push(value),
            "--ask" => entry.ask.push(value),
            "--timeout" => {
                entry.timeout_secs = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|secs| *secs > 0)
                        .context(
                            "--timeout takes whole seconds above 0",
                        )?,
                );
            }
            other => bail!("unknown option `{other}`"),
        }
    }
    if entry.command.is_empty() == entry.url.is_none() {
        bail!(
            "mcp add needs the server's command after `--`, or its \
             address with --url"
        );
    }
    Ok(Mcp::Add {
        name: name.clone(),
        entry: Box::new(entry),
    })
}

/// Does `mcp` over `client`, whose realm was `state` when it
/// connected.
pub async fn run(
    client: &mut Client,
    state: &State,
    mcp: Mcp,
) -> Result<()> {
    let view = &state.settings.mcp;
    let server = |name: &str| -> Result<&McpServerView> {
        view.servers
            .iter()
            .find(|server| server.name == name)
            .with_context(|| {
                format!("There is no MCP server {name}.")
            })
    };
    match mcp {
        Mcp::List => {
            if view.servers.is_empty() {
                println!(
                    "No MCP server is listed. Add one with hamlet \
                     mcp add, or bring in another program's with \
                     hamlet mcp import."
                );
            }
            for server in &view.servers {
                println!("{}", line(server));
            }
            for fault in &view.faults {
                println!("! {fault}");
            }
            Ok(())
        }
        Mcp::Tools(name) => {
            let server = server(&name)?;
            if server.tools.is_empty() {
                println!("{name} lists no tools.");
            }
            for tool in &server.tools {
                let asks = if tool.acts { "asks" } else { "runs" };
                let about =
                    tool.description.lines().next().unwrap_or("");
                println!("{}  {asks}  {about}", tool.name);
            }
            Ok(())
        }
        Mcp::Add { name, entry } => {
            if server(&name).is_ok() {
                bail!(
                    "MCP server {name} is listed already; change it \
                     with hamlet mcp edit {name}."
                );
            }
            let entry = *entry;
            act(client, McpAct::Put { name, entry }).await
        }
        Mcp::Edit(name) => {
            let before =
                serde_json::to_string_pretty(&server(&name)?.entry)?;
            let after = in_editor(
                &format!("{name}.json"),
                &format!("{before}\n"),
            )
            .await?;
            if after.trim() == before.trim() {
                println!("Nothing changed.");
                return Ok(());
            }
            let entry = serde_json::from_str::<McpEntry>(&after)
                .context(
                    "The entry does not fit, so nothing was saved",
                )?;
            act(client, McpAct::Put { name, entry }).await
        }
        Mcp::Remove(name) => {
            act(client, McpAct::Remove { name }).await
        }
        Mcp::Restart(name) => {
            act(client, McpAct::Restart { name }).await
        }
        Mcp::SignIn { name, pasted } => {
            let follow = pasted.is_none();
            let begun = McpAct::SignIn {
                name: name.clone(),
                pasted,
            };
            println!("{}", request(client, begun).await?);
            if follow {
                sign_in(client, &name).await?;
            }
            Ok(())
        }
        Mcp::Import { names, all } => {
            println!("{}", request(client, McpAct::Find).await?);
            if names.is_empty() && !all {
                println!(
                    "Add those you want with hamlet mcp import \
                     <name>..., or every one that runs here with \
                     --all."
                );
                return Ok(());
            }
            act(client, McpAct::Import { names }).await
        }
    }
}

/// Follows the sign-in to the server `name`: shows the address to
/// sign in at, then how it ended.
async fn sign_in(client: &mut Client, name: &str) -> Result<()> {
    let failed = format!("MCP server {name}: the sign-in failed");
    let mut shown = false;
    loop {
        // A client that fell behind is sent the whole state again.
        let seen = match client.next().await? {
            ServerMessage::State(state) => vec![state.settings],
            ServerMessage::Change(changes) => changes
                .changes
                .into_iter()
                .filter_map(|update| match update {
                    Update::Settings { settings } => Some(settings),
                    _ => None,
                })
                .collect(),
            _ => continue,
        };
        for settings in seen {
            if let Some(fault) = settings
                .mcp
                .faults
                .iter()
                .find(|fault| fault.starts_with(&failed))
            {
                bail!("{fault}");
            }
            let Some(server) =
                settings.mcp.servers.iter().find(|s| s.name == name)
            else {
                bail!("MCP server {name} was removed.");
            };
            match (&server.sign_in, shown) {
                (Some(address), false) => {
                    shown = true;
                    println!(
                        "Open this address to sign \
                         in:\n\n{address}\n\nIf your browser ends \
                         on a page that does not load, run hamlet \
                         mcp sign-in {name} with that page's \
                         address, quoted."
                    );
                }
                (None, true) => {
                    println!(
                        "Signed in; MCP server {name} starts again."
                    );
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

/// A server on one line: its name, how it is, its tools.
fn line(server: &McpServerView) -> String {
    let state = match &server.state {
        McpState::Starting => "starting".to_owned(),
        McpState::Up => "up".to_owned(),
        McpState::Down { reason, .. } => format!("down: {reason}"),
    };
    let asks = server.tools.iter().filter(|tool| tool.acts).count();
    let net = if server.entry.net { ", network" } else { "" };
    format!(
        "{}  {state}  {} tools, {asks} ask{net}",
        server.name,
        server.tools.len()
    )
}

async fn act(client: &mut Client, act: McpAct) -> Result<()> {
    println!("{}", request(client, act).await?);
    Ok(())
}

/// The server's words on `act`.
async fn request(client: &mut Client, act: McpAct) -> Result<String> {
    let id = RequestId::new();
    let message = ClientMessage::Mcp(hamlet_protocol::client::Mcp {
        id: id.clone(),
        act,
    });
    match checked(client.request(&id, message).await?)? {
        ServerMessage::Reply(reply) => Ok(reply.text),
        _ => Ok(String::new()),
    }
}
