//! The files of a parcel, measured and read for the realm under the
//! sending agent's reach.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hamlet_messages::bytes::Base64;
use hamlet_messages::realm::Enclosed;
use hamlet_messages::realm::parcels::{PARCEL_PART, Part};
use hamlet_messages::workspace::{
    Parcel, ParcelAnswer, ParcelEntry, ParcelFault, ParcelPart,
    ParcelRefusal,
};
use hamlet_testing::Scratch;
use sha2::{Digest, Sha256};

use super::files::{ask, project, working_in};
use super::{AGENT, WRONG_CALLER, agent, error_of, realm};

fn parcel(paths: &[&Path]) -> Parcel {
    Parcel {
        agent: AGENT,
        paths: paths
            .iter()
            .map(|path| Enclosed {
                path: path.to_path_buf(),
            })
            .collect(),
    }
}

fn part(path: &Path, part: u32) -> ParcelPart {
    ParcelPart {
        agent: AGENT,
        path: path.to_owned(),
        part,
    }
}

fn measured(answer: ParcelAnswer) -> Vec<ParcelEntry> {
    match answer {
        ParcelAnswer::Measured { files } => files,
        refused => panic!("the parcel was refused: {refused:?}"),
    }
}

fn refused(answer: ParcelAnswer) -> ParcelRefusal {
    match answer {
        ParcelAnswer::Refused { why } => why,
        measured => panic!("the parcel was measured: {measured:?}"),
    }
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_is_measured_file_by_file_without_its_links() {
    let dir = Scratch::new("workspace-parcel-folder");
    let (_project, folder) = project(
        "workspace-parcel-folder-project",
        &[("out/b.txt", "bee"), ("out/a/c.txt", "sea")],
    );
    std::os::unix::fs::symlink(
        folder.join("out/b.txt"),
        folder.join("out/link"),
    )
    .unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;

    let out = folder.join("out");
    let files =
        measured(ask(&host, &realm(), &parcel(&[&out])).await);
    host.shutdown().await.unwrap();

    let seen = files
        .iter()
        .map(|file| {
            (
                file.name.clone(),
                file.path.clone(),
                file.size,
                file.sha256.0,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        seen,
        [
            (
                PathBuf::from("out/a/c.txt"),
                folder.join("out/a/c.txt"),
                3,
                hash(b"sea"),
            ),
            (
                PathBuf::from("out/b.txt"),
                folder.join("out/b.txt"),
                3,
                hash(b"bee"),
            ),
        ]
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_file_is_read_part_by_part() {
    let dir = Scratch::new("workspace-parcel-parts");
    let bytes = (0..PARCEL_PART * 2 + 100)
        .map(|n| (n % 251) as u8)
        .collect::<Vec<_>>();
    let (_project, folder) =
        project("workspace-parcel-parts-project", &[]);
    let big = folder.join("big.bin");
    std::fs::write(&big, &bytes).unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;

    let mut read = Vec::new();
    let mut lasts = Vec::new();
    for n in 0..3 {
        match ask(&host, &realm(), &part(&big, n)).await {
            Part::Bytes { bytes, last } => {
                read.extend(bytes.0);
                lasts.push(last);
            }
            Part::Refused { why } => panic!("part {n}: {why}"),
        }
    }
    let past = ask(&host, &realm(), &part(&big, 3)).await;
    host.shutdown().await.unwrap();

    assert!(read == bytes, "the parts make the file");
    assert_eq!(lasts, [false, false, true]);
    assert_eq!(
        past,
        Part::Refused {
            why: ParcelFault::NoSuchPart {
                path: big.clone(),
                part: 3,
            }
            .into()
        }
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_empty_file_is_one_empty_part() {
    let dir = Scratch::new("workspace-parcel-empty");
    let (_project, folder) =
        project("workspace-parcel-empty-project", &[("empty", "")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let empty = folder.join("empty");
    let read = ask(&host, &realm(), &part(&empty, 0)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        read,
        Part::Bytes {
            bytes: Base64(Vec::new()),
            last: true
        }
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_path_outside_the_reach_is_refused() {
    let dir = Scratch::new("workspace-parcel-outside");
    let (_project, folder) =
        project("workspace-parcel-outside-project", &[("a", "a")]);
    let secret = dir.join("credentials.json");
    std::fs::write(&secret, "{}").unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;

    let measuring = ask(&host, &realm(), &parcel(&[&secret])).await;
    let reading = ask(&host, &realm(), &part(&secret, 0)).await;
    host.shutdown().await.unwrap();

    let outside: ParcelRefusal = ParcelFault::Unreadable {
        path: secret.clone(),
        words: format!(
            "{} is out of this agent's reach to read",
            secret.display()
        ),
    }
    .into();
    assert_eq!(refused(measuring), outside);
    assert_eq!(reading, Part::Refused { why: outside });
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_that_cannot_be_opened_is_told() {
    let dir = Scratch::new("workspace-parcel-unopened");
    let (_project, folder) =
        project("workspace-parcel-unopened-project", &[("a", "a")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let a = folder.join("a");
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::write(&folder, "no longer a folder").unwrap();

    let measuring = ask(&host, &realm(), &parcel(&[&a])).await;
    let reading = ask(&host, &realm(), &part(&a, 0)).await;
    host.shutdown().await.unwrap();

    let unopened = |why: &ParcelRefusal| {
        matches!(
            why,
            ParcelRefusal::Parcel(ParcelFault::Unopened { words })
                if !words.is_empty()
        )
    };
    let measuring = refused(measuring);
    assert!(unopened(&measuring), "{measuring:?}");
    assert!(
        matches!(&reading, Part::Refused { why } if unopened(why)),
        "{reading:?}"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_named_link_is_not_sent() {
    let dir = Scratch::new("workspace-parcel-link");
    let (_project, folder) =
        project("workspace-parcel-link-project", &[("a", "a")]);
    let link = folder.join("link");
    std::os::unix::fs::symlink(folder.join("a"), &link).unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;

    let measuring = ask(&host, &realm(), &parcel(&[&link])).await;
    let reading = ask(&host, &realm(), &part(&link, 0)).await;
    host.shutdown().await.unwrap();

    let why: ParcelRefusal =
        ParcelFault::Link { path: link.clone() }.into();
    assert_eq!(
        why.to_string(),
        format!(
            "`{}` is a link, and links are not sent",
            link.display()
        )
    );
    assert_eq!(refused(measuring), why);
    assert_eq!(reading, Part::Refused { why });
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_with_no_files_is_refused() {
    let dir = Scratch::new("workspace-parcel-none");
    let (_project, folder) =
        project("workspace-parcel-none-project", &[]);
    let hollow = folder.join("hollow");
    std::fs::create_dir_all(&hollow).unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;
    let answer = ask(&host, &realm(), &parcel(&[&hollow])).await;
    host.shutdown().await.unwrap();

    assert_eq!(refused(answer), ParcelFault::NoFiles.into());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_realm_asks_for_a_parcel() {
    let dir = Scratch::new("workspace-parcel-caller");
    let (_project, folder) =
        project("workspace-parcel-caller-project", &[("a", "a")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let a = folder.join("a");
    let me = agent(AGENT);
    let measuring = error_of(&host, &me, &parcel(&[&a])).await;
    let reading = error_of(&host, &me, &part(&a, 0)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        (measuring.as_str(), reading.as_str()),
        (WRONG_CALLER, WRONG_CALLER)
    );
}
