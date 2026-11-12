//! `hamlet import`: conversations of other programs, on this machine
//! or in a file they exported, listed and brought in as a chat's
//! session (C35).

use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::{Result, bail};
use hamlet_import::Found;
use hamlet_protocol::client::{
    ClientMessage, Import, Imported, Reply, ReplyKind, RequestId,
    ServerMessage,
};

use crate::chat::resolve;
use crate::client::{self, ServerAddress};

/// How far back a list of this machine's conversations reaches
/// without `--all`.
const RECENT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The conversations of the file `file`, or of this machine.
fn found(file: Option<&Path>) -> Result<Vec<Found>> {
    match file {
        Some(file) => hamlet_import::find_in(file),
        None => Ok(hamlet_import::find()),
    }
}

/// Prints the conversations of the file `file`, or those found on
/// this machine in the last 30 days, or every one with `all`; newest
/// first.
pub fn list(file: Option<&Path>, all: bool) -> Result<()> {
    let found = found(file)?;
    let since = SystemTime::now() - RECENT;
    let shown = found
        .iter()
        .filter(|found| {
            all || file.is_some() || found.modified >= since
        })
        .collect::<Vec<_>>();
    if shown.is_empty() {
        println!("No conversations found.");
    }
    for found in &shown {
        let day =
            chrono::DateTime::<chrono::Local>::from(found.modified)
                .format("%Y-%m-%d");
        println!(
            "{}  {}  {}",
            found.session,
            day,
            found.source.word()
        );
        println!("    {}", found.title);
        if let Some(folder) = &found.folder {
            println!("    in {folder}");
        }
    }
    let older = found.len() - shown.len();
    if older > 0 {
        println!(
            "{older} older conversation{} not shown; hamlet import \
             --all lists every one.",
            if older == 1 { " is" } else { "s are" }
        );
    }
    if !shown.is_empty() {
        let file = file.map_or(String::new(), |file| {
            format!("--file {} ", file.display())
        });
        println!(
            "Bring one in with: hamlet import {file}<id> [--to \
             <agent>]"
        );
    }
    Ok(())
}

/// Brings in the conversation of the file `file`, or of this machine,
/// whose id is or begins with `session` as a new session of the
/// chief's chat with `to`.
pub async fn bring_in(
    address: &ServerAddress,
    file: Option<&Path>,
    session: &str,
    to: &str,
) -> Result<()> {
    let found = found(file)?;
    let matching = found
        .iter()
        .filter(|found| found.session.starts_with(session))
        .collect::<Vec<_>>();
    let found: &Found = match matching.as_slice() {
        [found] => found,
        [] => bail!("no conversation's id begins with `{session}`"),
        _ => {
            bail!("several conversations' ids begin with `{session}`")
        }
    };
    let imported = hamlet_import::read(found)?;
    let bytes = serde_json::to_vec(&imported)?;
    anyhow::ensure!(
        bytes.len() as u64 <= Imported::MOST_BYTES,
        "that conversation is over 16 MiB, more than Hamlet takes"
    );
    let (mut client, state) = client::connect(address).await?;
    let agent = resolve(&state, to)?;
    let file = client::put_file(&mut client, &bytes).await?;
    let id = RequestId::new();
    let answer = client
        .request(
            &id,
            ClientMessage::Import(Import {
                file,
                id: id.clone(),
                to: agent,
            }),
        )
        .await?;
    let _ = client.close().await;
    match answer {
        ServerMessage::Reply(Reply {
            kind: ReplyKind::Notice,
            text,
            ..
        }) => {
            println!("{text}");
            if imported.left_out > 0 {
                println!(
                    "Its first message and the newest {} were kept.",
                    Imported::NEWEST
                );
            }
            Ok(())
        }
        ServerMessage::Reply(reply) => bail!("{}", reply.text),
        ServerMessage::Fault(fault) => bail!("{}", fault.text),
        _ => bail!("The server answered with something else."),
    }
}
