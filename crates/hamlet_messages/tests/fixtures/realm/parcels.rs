//! The fixtures of a parcel: the realm's measure and parts of it from
//! the workspace, and the parts an agent fetches from the realm.

use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::letters::Enclosed;
use hamlet_messages::realm::parcels::{
    FetchPart, ParcelId, Part, PartRefusal,
};
use hamlet_messages::workspace::refusal::{
    InvalidPath, PathRefusal, ReachRefusal, ToRead,
};
use hamlet_messages::workspace::{
    Parcel, ParcelAnswer, ParcelEntry, ParcelFault, ParcelPart,
    ParcelRefusal,
};

use super::super::cases::{Cases, Union, family, tagged, union};
use super::super::{AGENT, Fixtures};
use super::answers;

pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<Part<PartRefusal>>(),
        tagged::<PartRefusal>(),
        tagged::<ParcelAnswer>(),
        tagged::<ParcelFault>(),
    ]
}

pub fn unions() -> Vec<Union> {
    vec![union!(ParcelRefusal {
        Invalid: InvalidPath,
        Path: PathRefusal<ToRead>,
        Reach: ReachRefusal,
        Parcel: ParcelFault,
    })]
}

pub fn the_parcels(fixtures: &mut Fixtures) {
    the_parts_an_agent_fetches(fixtures);
    the_workspaces_parcel(fixtures);
}

fn parts<W>() -> [(&'static str, Part<W>); 2] {
    [
        (
            "bytes",
            Part::Bytes {
                bytes: Base64(b"# Report\n".to_vec()),
                last: false,
            },
        ),
        (
            "bytes.last",
            Part::Bytes {
                bytes: Base64(b"done\n".to_vec()),
                last: true,
            },
        ),
    ]
}

fn the_parts_an_agent_fetches(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/fetch_part",
        &FetchPart {
            parcel: ParcelId(8),
            file: 1,
            part: 3,
        },
    );
    answers(
        fixtures,
        "realm/fetch_part",
        parts(),
        vec![
            ("no_such_parcel", PartRefusal::NoSuchParcel),
            ("not_yours", PartRefusal::NotYours),
            ("no_such_part", PartRefusal::NoSuchPart),
        ],
    );
}

fn refusals() -> Vec<(&'static str, ParcelRefusal)> {
    vec![
        (
            "not_absolute",
            InvalidPath::NotAbsolute {
                path: "report.md".into(),
            }
            .into(),
        ),
        (
            "outside_reach",
            PathRefusal::OutsideReach {
                path: "/etc/passwd".into(),
                access: ToRead::Read,
            }
            .into(),
        ),
        (
            "agent_filesystem",
            PathRefusal::AgentFilesystem {
                path: "/agents/7/notes.md".into(),
            }
            .into(),
        ),
        ("no_reach", ReachRefusal::NoReach.into()),
        (
            "project_in_realm",
            ReachRefusal::ProjectInRealm {
                folder: "/realm/project".into(),
            }
            .into(),
        ),
        (
            "unreadable",
            ParcelFault::Unreadable {
                path: "/work/report.md".into(),
                words: "permission denied".into(),
            }
            .into(),
        ),
        (
            "unopened",
            ParcelFault::Unopened {
                words: "the project is not a directory".into(),
            }
            .into(),
        ),
        (
            "link",
            ParcelFault::Link {
                path: "/work/latest".into(),
            }
            .into(),
        ),
        ("no_name", ParcelFault::NoName { path: "/".into() }.into()),
        ("too_many_entries", ParcelFault::TooManyEntries.into()),
        (
            "not_a_name",
            ParcelFault::NotAName {
                path: "figures/a\nb".into(),
            }
            .into(),
        ),
        (
            "file_too_large",
            ParcelFault::FileTooLarge {
                path: "/work/dump.bin".into(),
                most: 256 << 20,
            }
            .into(),
        ),
        ("too_much", ParcelFault::TooMuch { most: 1 << 30 }.into()),
        (
            "too_many_files",
            ParcelFault::TooManyFiles { most: 10_000 }.into(),
        ),
        ("no_files", ParcelFault::NoFiles.into()),
        (
            "no_such_part",
            ParcelFault::NoSuchPart {
                path: "/work/report.md".into(),
                part: 9,
            }
            .into(),
        ),
    ]
}

fn the_workspaces_parcel(fixtures: &mut Fixtures) {
    fixtures.args(
        "workspace/parcel",
        &Parcel {
            agent: AGENT,
            paths: vec![
                Enclosed {
                    path: "/work/report.md".into(),
                },
                Enclosed {
                    path: "/work/figures".into(),
                },
            ],
        },
    );
    answers(
        fixtures,
        "workspace/parcel",
        [(
            "measured",
            ParcelAnswer::Measured {
                files: vec![
                    ParcelEntry {
                        path: "/work/report.md".into(),
                        name: "report.md".into(),
                        size: 2_048,
                        sha256: Sha256([0x2a; 32]),
                    },
                    ParcelEntry {
                        path: "/work/figures/plot.png".into(),
                        name: "figures/plot.png".into(),
                        size: 6 << 30,
                        sha256: Sha256([0x7f; 32]),
                    },
                ],
            },
        )],
        refusals(),
    );
    fixtures.args(
        "workspace/parcel_part",
        &ParcelPart {
            agent: AGENT,
            path: "/work/figures/plot.png".into(),
            part: 3,
        },
    );
    answers(fixtures, "workspace/parcel_part", parts(), refusals());
}
