//! `hamlet import`: projects of other programs, on this machine or in
//! a file they exported, each brought in as a hamlet with a hire for
//! each of its conversations (C35).

use std::path::Path;
use std::time::SystemTime;

use anyhow::{Result, bail};
use hamlet_import::Project;
use hamlet_protocol::client::{
    ClientMessage, ImportProject, Imported, Reply, ReplyKind,
    RequestId, ServerMessage,
};

use crate::client::{self, Client, ServerAddress};

/// Which projects a command names.
pub enum Chosen {
    /// Those suggested: git repositories with three conversations or
    /// more.
    Suggested,
    /// Those named by their name or folder.
    Named(Vec<String>),
}

/// The projects of the export `file`, or of this machine: those of
/// the last 30 days, or of all time with `all`.
fn projects(file: Option<&Path>, all: bool) -> Result<Vec<Project>> {
    let found = match file {
        Some(file) => hamlet_import::find_in(file)?,
        None => hamlet_import::find(),
    };
    let since = match all {
        true => SystemTime::UNIX_EPOCH,
        false => SystemTime::now() - hamlet_import::RECENT,
    };
    Ok(hamlet_import::projects(found, since))
}

/// Prints the projects found, the suggested ones marked.
pub fn list(file: Option<&Path>, all: bool) -> Result<()> {
    let projects = projects(file, all)?;
    if projects.is_empty() {
        println!("No projects found.");
        return Ok(());
    }
    for project in &projects {
        let mark = if project.is_suggested() { "*" } else { " " };
        let day = chrono::DateTime::<chrono::Local>::from(
            project.modified(),
        )
        .format("%Y-%m-%d");
        let count = project.conversations.len();
        println!(
            "{mark} {}  {count} conversation{}  {day}",
            project.name,
            if count == 1 { "" } else { "s" }
        );
        if let Some(folder) = &project.folder {
            println!("    {folder}");
        }
    }
    let mut file = file.map_or(String::new(), |file| {
        format!("--file {} ", file.display())
    });
    // Those listed are found again only as far back.
    if all {
        file.push_str("--all ");
    }
    println!(
        "Bring them in, each as a hamlet with a hire for each \
         conversation: hamlet import {file}<project>... or hamlet \
         import {file}--suggested for those marked *."
    );
    Ok(())
}

/// Brings in the projects `chosen` of the export `file`, or of this
/// machine.
pub async fn bring_in(
    address: &ServerAddress,
    file: Option<&Path>,
    all: bool,
    chosen: &Chosen,
) -> Result<()> {
    let projects = projects(file, all)?;
    let picked = match chosen {
        Chosen::Suggested => projects
            .iter()
            .filter(|project| project.is_suggested())
            .collect::<Vec<_>>(),
        Chosen::Named(names) => {
            let mut picked = Vec::new();
            for name in names {
                let matching = projects
                    .iter()
                    .filter(|project| {
                        project.name == *name
                            || project.folder.as_deref()
                                == Some(name.as_str())
                    })
                    .collect::<Vec<_>>();
                match matching.as_slice() {
                    [project] => picked.push(*project),
                    [] => bail!("no project is named `{name}`"),
                    _ => bail!(
                        "several projects are named `{name}`; name \
                         it by its folder"
                    ),
                }
            }
            picked
        }
    };
    if picked.is_empty() {
        bail!("no project to bring in");
    }
    let (mut client, _) = client::connect(address).await?;
    for project in picked {
        let words = send(&mut client, project).await?;
        println!("{}: {words}", project.name);
    }
    let _ = client.close().await;
    Ok(())
}

/// Sends the conversations of `project` and asks for it to be
/// brought in; the server's words.
async fn send(
    client: &mut Client,
    project: &Project,
) -> Result<String> {
    let mut files = Vec::new();
    let read = hamlet_import::read_all(&project.conversations);
    let mut together = 0;
    for (found, imported) in project.conversations.iter().zip(read) {
        let imported = match imported {
            Ok(imported) => imported,
            Err(error) => {
                eprintln!(
                    "Left out {}, which could not be read: {error:#}",
                    found.title
                );
                continue;
            }
        };
        let bytes = serde_json::to_vec(&imported)?;
        let size = bytes.len() as u64;
        if size > Imported::MOST_BYTES {
            eprintln!("Left out {}, over 16 MiB.", found.title);
            continue;
        }
        if together + size > ImportProject::MOST_BYTES {
            eprintln!(
                "Left out {}, which would take the project past {} \
                 MiB; bring it in alone.",
                found.title,
                ImportProject::MOST_BYTES >> 20
            );
            continue;
        }
        together += size;
        files.push(client::put_file(client, &bytes).await?);
    }
    if files.is_empty() {
        bail!("{} has no conversation to bring in", project.name);
    }
    let id = RequestId::new();
    let answer = client
        .request(
            &id,
            ClientMessage::ImportProject(ImportProject {
                files,
                folder: project.folder.clone(),
                id: id.clone(),
                name: project.name.chars().take(40).collect(),
            }),
        )
        .await?;
    match answer {
        ServerMessage::Reply(Reply {
            kind: ReplyKind::Notice,
            text,
            ..
        }) => Ok(text),
        ServerMessage::Reply(reply) => bail!("{}", reply.text),
        ServerMessage::Fault(fault) => bail!("{}", fault.text),
        _ => bail!("The server answered with something else."),
    }
}
