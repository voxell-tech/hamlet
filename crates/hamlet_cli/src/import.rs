//! `hamlet import`: conversations of other programs on this machine,
//! listed and brought in as a chat's session (C35).

use std::time::{Duration, SystemTime};

use anyhow::{Result, bail};
use hamlet_import::Found;
use hamlet_protocol::client::{
    ClientMessage, Import, Imported, Reply, ReplyKind, RequestId,
    ServerMessage,
};

use crate::chat::resolve;
use crate::client::{self, ServerAddress};

/// How far back a list reaches without `--all`.
const RECENT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Prints the conversations found, newest first: those of the last 30
/// days, or every one with `all`.
pub fn list(all: bool) {
    let found = hamlet_import::find();
    let since = SystemTime::now() - RECENT;
    let shown = found
        .iter()
        .filter(|found| all || found.modified >= since)
        .collect::<Vec<_>>();
    if shown.is_empty() {
        println!("No conversations found from the last 30 days.");
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
        println!(
            "Bring one in with: hamlet import <id> [--to <agent>]"
        );
    }
}

/// Brings in the conversation whose id is or begins with `session` as
/// a new session of the chief's chat with `to`.
pub async fn bring_in(
    address: &ServerAddress,
    session: &str,
    to: &str,
) -> Result<()> {
    let found = hamlet_import::find();
    let matching = found
        .iter()
        .filter(|found| found.session.starts_with(session))
        .collect::<Vec<_>>();
    let found: &Found = match matching.as_slice() {
        [found] => found,
        [] => bail!(
            "no conversation's id begins with `{session}`; hamlet \
             import --all lists them"
        ),
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
