use super::*;

/// A hamlet's id, in the one form an address writes it.
const OTHER: &str = "0b9e2c4a-6f1d-4e8b-9a3c-5d7f1e2a4b6c";

fn docs() -> HamletId {
    HamletId("docs".into())
}

/// The words `line` is refused with, said in `docs`.
fn refused(line: &str) -> String {
    parse(line, Some(&docs())).unwrap_err().to_string()
}

fn hire(number: u32, text: &str) -> Target {
    Target {
        named: Named::Agent {
            agent: AgentNumber(number),
        },
        text: text.into(),
    }
}

#[test]
fn mayor_is_the_office_of_the_hamlet_it_is_said_in() {
    assert_eq!(
        parse("/end mayor", Some(&docs())),
        Ok(Act::End {
            hire: Target {
                named: Named::MayorOf { hamlet: docs() },
                text: "mayor".into(),
            },
            why: None,
        })
    );
    assert_eq!(
        parse(&format!("/stop mayor@{OTHER}"), None),
        Ok(Act::Stop(Some(Target {
            named: Named::MayorOf {
                hamlet: HamletId(OTHER.into()),
            },
            text: format!("mayor@{OTHER}"),
        })))
    );
}

#[test]
fn a_hire_and_a_worker_are_named_by_number() {
    assert_eq!(
        parse("/nickname worker#7 Scout", None),
        Ok(Act::Nickname {
            hire: Target {
                named: Named::Worker {
                    worker: AgentNumber(7),
                },
                text: "worker#7".into(),
            },
            name: Some("Scout".into()),
        })
    );
    assert_eq!(
        parse("/road hire#2 hire#3 they share a page", None),
        Ok(Act::Road {
            a: hire(2, "hire#2"),
            b: hire(3, "hire#3"),
            why: Some("they share a page".into()),
            lay: true,
        })
    );
}

#[test]
fn a_bad_address_is_refused_in_todays_words() {
    assert_eq!(
        parse("/end scholar#2", None).unwrap_err().to_string(),
        "`scholar#2` is not an address"
    );
}

#[test]
fn effort_and_fast_each_set_one_part() {
    assert_eq!(
        parse("/effort hire#2 high", None),
        Ok(Act::Steer {
            hire: hire(2, "hire#2"),
            part: Part::Effort(Some(Level::High)),
        })
    );
    assert_eq!(
        parse("/fast hire#2 on", None),
        Ok(Act::Steer {
            hire: hire(2, "hire#2"),
            part: Part::Fast(true),
        })
    );
    assert_eq!(
        parse("/effort hire#2 hard", None).unwrap_err().to_string(),
        "Use /effort <address> \
         <off|minimal|low|medium|high|xhigh|max|none>"
    );
}

#[test]
fn a_wait_is_the_one_named_or_none() {
    assert_eq!(
        parse("/reply #4 Use the blue one.", None),
        Ok(Act::Answer {
            choice: Choice::Reply,
            wait: Some("4".into()),
            text: Some("Use the blue one.".into()),
        })
    );
    assert_eq!(
        parse("/deny", None),
        Ok(Act::Answer {
            choice: Choice::Deny,
            wait: None,
            text: None,
        })
    );
}

#[test]
fn a_hamlet_is_founded_under_a_blank_mayor_unless_a_profile_is_named()
{
    assert_eq!(
        parse("/create-hamlet Docs", None),
        Ok(Act::Found(Found {
            name: "Docs".into(),
            mayor: None,
            version: None,
            tier: Tier::Fifty,
            purpose: Some("Mayor of Docs.".into()),
        }))
    );
    assert_eq!(
        parse("/create-hamlet scholar#3 Docs: Keep the docs.", None),
        Ok(Act::Found(Found {
            name: "Docs".into(),
            mayor: Some(ProfileId(3)),
            version: None,
            tier: Tier::Fifty,
            purpose: Some("Keep the docs.".into()),
        }))
    );
}

#[test]
fn a_hire_goes_into_the_hamlet_here_unless_one_is_named() {
    assert_eq!(
        parse(
            "/hire blank under mayor write the guide",
            Some(&docs())
        ),
        Ok(Act::Hire(Hire {
            hamlet: Some(docs()),
            under: Some(Named::MayorOf { hamlet: docs() }),
            profile: None,
            version: None,
            purpose: Some("write the guide".into()),
            name: None,
        }))
    );
    assert_eq!(
        parse("/hire scholar#2", None).unwrap_err().to_string(),
        words::NO_HAMLET
    );
}

#[test]
fn every_command_served_is_parsed_into_its_act() {
    for name in [
        "hand",
        "hamlet",
        "agent",
        "new",
        "resume",
        "retry",
        "stop",
        "to",
        "reflect",
        "worker",
        "queue",
        "allow",
        "always",
        "deny",
        "go",
        "withdraw",
        "reply",
        "dismiss",
        "grants",
        "revoke",
        "hamlets",
        "create-hamlet",
        "name-hamlet",
        "move-hamlet",
        "tier",
        "mayor",
        "hire",
        "end",
        "model",
        "effort",
        "fast",
        "nickname",
        "tools",
        "allowance",
        "road",
        "unroad",
        "folder",
        "mode",
        "budget",
        "reset-budget",
        "merge",
        "restore",
        "erase",
        "remove-hamlet",
        "restore-hamlet",
        "erase-hamlet",
        "fork",
        "remove-scholar",
        "restore-scholar",
        "erase-scholar",
        "welcome",
        "turn-away",
        "move-house",
    ] {
        let parsed = parse(&format!("/{name}"), Some(&docs()));
        assert_ne!(
            parsed,
            Ok(Act::Unsupported(format!("`/{name}`"))),
            "/{name}"
        );
        if let Err(refusal) = parsed {
            assert_ne!(
                refusal.to_string(),
                words::no_command(name),
                "/{name}"
            );
        }
    }
}

#[test]
fn a_command_of_the_table_not_done_yet_is_unsupported() {
    assert_eq!(
        parse("/history", Some(&docs())),
        Ok(Act::Unsupported("`/history`".into()))
    );
    assert_eq!(
        parse("/dance", None).unwrap_err().to_string(),
        "There is no command /dance; the client lists the commands."
    );
}

#[test]
fn merge_names_two_hires_and_gives_the_chiefs_reason_when_none_is_said()
 {
    assert_eq!(
        refused("/merge hire#2 hire#3"),
        "Use /merge <address> into <address> [reason]"
    );
    assert_eq!(
        parse("/merge hire#3 into mayor", Some(&docs())),
        Ok(Act::Merge {
            absorbed: hire(3, "hire#3"),
            into: Ok(Target {
                named: Named::MayorOf { hamlet: docs() },
                text: "mayor".into(),
            }),
            why: "merged by the chief".into(),
        })
    );
    assert_eq!(
        parse("/merge hire#3 into hire#2 they write one guide", None),
        Ok(Act::Merge {
            absorbed: hire(3, "hire#3"),
            into: Ok(hire(2, "hire#2")),
            why: "they write one guide".into(),
        })
    );
}

#[test]
fn restore_names_a_hire_and_a_reason() {
    assert_eq!(
        refused("/restore"),
        "Use /restore <address> [reason]"
    );
    assert_eq!(
        parse("/restore hire#3 it is needed", None),
        Ok(Act::Restore {
            hire: hire(3, "hire#3"),
            why: Some("it is needed".into()),
        })
    );
}

#[test]
fn erase_names_a_hire_alone() {
    assert_eq!(refused("/erase"), "Use /erase <address>");
    assert_eq!(
        parse("/erase hire#3 for good", None),
        Ok(Act::Erase {
            hire: hire(3, "hire#3"),
        })
    );
}

#[test]
fn remove_hamlet_gives_the_chiefs_reason_when_none_is_said() {
    assert_eq!(
        refused("/remove-hamlet"),
        "Use /remove-hamlet <hamlet> [reason]"
    );
    assert_eq!(
        parse("/remove-hamlet docs", None),
        Ok(Act::RemoveHamlet(RemoveHamlet {
            hamlet: docs(),
            why: Some("closed by the chief".into()),
        }))
    );
    assert_eq!(
        parse("/remove-hamlet docs its work is done", None),
        Ok(Act::RemoveHamlet(RemoveHamlet {
            hamlet: docs(),
            why: Some("its work is done".into()),
        }))
    );
}

#[test]
fn restore_hamlet_names_a_hamlet_and_a_reason() {
    assert_eq!(
        refused("/restore-hamlet"),
        "Use /restore-hamlet <hamlet> [reason]"
    );
    assert_eq!(
        parse("/restore-hamlet docs", None),
        Ok(Act::RestoreHamlet(RestoreHamlet {
            hamlet: docs(),
            why: None,
        }))
    );
}

#[test]
fn erase_hamlet_names_a_hamlet_alone() {
    assert_eq!(
        refused("/erase-hamlet"),
        "Use /erase-hamlet <hamlet>"
    );
    assert_eq!(
        parse("/erase-hamlet docs now", None),
        Ok(Act::EraseHamlet(EraseHamlet { hamlet: docs() }))
    );
}

#[test]
fn fork_takes_the_version_after_the_first_at_and_main_when_none() {
    assert_eq!(
        refused("/fork"),
        "Use /fork <scholar#id>[@<version>] <new name>"
    );
    assert_eq!(
        parse("/fork scholar#2@v3@x Notes keeper", None),
        Ok(Act::Fork(ForkProfile {
            from: ProfileId(2),
            at: Some(VersionName("v3@x".into())),
            name: "Notes keeper".into(),
        }))
    );
    for line in
        ["/fork scholar#2 Notes", "/fork scholar#2@main Notes"]
    {
        assert_eq!(
            parse(line, None),
            Ok(Act::Fork(ForkProfile {
                from: ProfileId(2),
                at: None,
                name: "Notes".into(),
            })),
            "{line}"
        );
    }
    assert_eq!(
        refused("/fork hire#2@v3 Notes"),
        "`hire#2` is not an address"
    );
}

#[test]
fn remove_profile_names_a_profile_and_a_reason() {
    assert_eq!(
        refused("/remove-scholar"),
        "Use /remove-scholar <scholar#id> [reason]"
    );
    assert_eq!(
        refused("/remove-scholar scholar#2@v1"),
        "`scholar#2@v1` is not an address"
    );
    assert_eq!(
        parse("/remove-scholar scholar#2 it is done", None),
        Ok(Act::ArchiveProfile(ArchiveProfile {
            profile: ProfileId(2),
            why: Some("it is done".into()),
        }))
    );
}

#[test]
fn restore_profile_names_a_profile_and_a_reason() {
    assert_eq!(
        refused("/restore-scholar"),
        "Use /restore-scholar <scholar#id> [reason]"
    );
    assert_eq!(
        parse("/restore-scholar scholar#2", None),
        Ok(Act::RestoreProfile(RestoreProfile {
            profile: ProfileId(2),
            why: None,
        }))
    );
}

#[test]
fn erase_profile_names_a_profile_alone() {
    assert_eq!(
        refused("/erase-scholar"),
        "Use /erase-scholar <scholar#id>"
    );
    assert_eq!(
        parse("/erase-scholar scholar#2 now", None),
        Ok(Act::EraseProfile(EraseProfile {
            profile: ProfileId(2),
        }))
    );
}

#[test]
fn welcome_names_a_profile_and_a_reason() {
    assert_eq!(
        refused("/welcome"),
        "Use /welcome <scholar#id> [reason]"
    );
    assert_eq!(
        parse("/welcome scholar#2 it may stay", None),
        Ok(Act::Welcome(Welcome {
            profile: ProfileId(2),
            why: Some("it may stay".into()),
        }))
    );
}

#[test]
fn turn_away_names_a_profile_and_a_reason() {
    assert_eq!(
        refused("/turn-away"),
        "Use /turn-away <scholar#id> [reason]"
    );
    assert_eq!(
        parse("/turn-away scholar#2", None),
        Ok(Act::TurnAway(TurnAway {
            profile: ProfileId(2),
            why: None,
        }))
    );
}

#[test]
fn move_house_reads_two_numbers_and_degrees_if_given() {
    assert_eq!(
        refused("/move-house"),
        "Use /move-house <scholar#id> <x> <z> [degrees]"
    );
    for line in [
        "/move-house scholar#2 4",
        "/move-house scholar#2 four 5",
        "/move-house scholar#2 4 5 east",
    ] {
        let Ok(Act::MoveHouse {
            profile: ProfileId(2),
            spot: Err(refusal),
        }) = parse(line, None)
        else {
            panic!("{line}");
        };
        assert_eq!(
            refusal.to_string(),
            "Use /move-house <scholar#id> <x> <z> [degrees]",
            "{line}"
        );
    }
    assert_eq!(
        parse("/move-house scholar#2 4 -5.5", None),
        Ok(Act::MoveHouse {
            profile: ProfileId(2),
            spot: Ok(Spot {
                x: 4.0,
                z: -5.5,
                degrees: None,
            }),
        })
    );
    assert_eq!(
        parse("/move-house scholar#2 4 5 90", None),
        Ok(Act::MoveHouse {
            profile: ProfileId(2),
            spot: Ok(Spot {
                x: 4.0,
                z: 5.0,
                degrees: Some(90.0),
            }),
        })
    );
}
