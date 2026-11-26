//! An offer to the library: each reason it is refused, its parts as
//! they come, and an ended agent's offers under way.

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_is_refused_for_each_reason() {
    let dir = Scratch::new("library-offer-refusals");
    let host = open(&dir, Arc::default()).await;
    let refused = async |n: u32, asked: Offer| match call_as(
        &host,
        &agent(n),
        &asked,
    )
    .await
    .unwrap()
    {
        OfferAnswer::Refused { why } => why,
        other => panic!("{other:?}"),
    };

    for skill in ["../tidy", "a/b", ""] {
        let mut asked = tidy("bad-name");
        asked.skill = skill.into();
        assert_eq!(
            refused(7, asked).await,
            OfferRefusal::Shape(BadOffer::NotASkillName {
                skill: skill.into()
            })
        );
    }

    let long = "n".repeat(256);
    for path in ["a\\b.py", "/abs.py", "a//b.py", "", long.as_str()] {
        let asked = offer(
            "bad-path",
            "tidy",
            0,
            true,
            vec![
                piece("SKILL.md", 0, PAPER.as_bytes(), true),
                piece(path, 0, SCRIPT.as_bytes(), true),
            ],
        );
        assert_eq!(
            refused(7, asked).await,
            OfferRefusal::Shape(BadOffer::NotAPath {
                path: path.into()
            })
        );
    }

    let path = "big.bin";
    let over = MOST_PART - path.len() - PIECE_FRAME + 1;
    let big = offer(
        "big-part",
        "tidy",
        0,
        false,
        vec![piece(path, 0, &vec![0; over], false)],
    );
    assert_eq!(
        refused(7, big).await,
        OfferRefusal::Size(TooLarge::PartTooLarge {
            most: u32::try_from(MOST_PART).unwrap()
        })
    );

    let mut weather = tidy("shelved");
    weather.skill = "weather".into();
    assert_eq!(
        refused(7, weather).await,
        OfferRefusal::Shelves(OnTheShelves::AlreadyShelved {
            skill: "weather".into()
        })
    );

    let bare = offer(
        "bare",
        "tidy",
        0,
        true,
        vec![piece("SKILL.md", 0, b"---\nname: tidy\n---\n", true)],
    );
    assert_eq!(
        refused(7, bare).await,
        OfferRefusal::Skill(BadSkill::NoFrontMatter {
            skill: "tidy".into()
        })
    );

    // Six parts of 700 KiB: the sixth takes the skill over 4 MiB.
    let size = 700 * 1024;
    let mut answers = Vec::new();
    for part in 0..6 {
        let offset = part * u32::try_from(size).unwrap();
        let asked = offer(
            "large",
            "large",
            part,
            false,
            vec![piece("large.bin", offset, &vec![0; size], false)],
        );
        answers
            .push(call_as(&host, &agent(7), &asked).await.unwrap());
    }
    assert_eq!(answers[..5], [const { OfferAnswer::More }; 5]);
    assert_eq!(
        answers[5],
        OfferAnswer::Refused {
            why: OfferRefusal::Skill(BadSkill::SkillTooLarge {
                skill: "large".into()
            })
        }
    );

    let kept = offered(&host, 7, &tidy("whole")).await;
    assert_eq!(
        refused(8, tidy("judged")).await,
        OfferRefusal::Shelves(OnTheShelves::BeingJudged {
            skill: "tidy".into()
        })
    );
    assert_eq!(
        refused(7, tidy("again")).await,
        OfferRefusal::Shelves(OnTheShelves::BeingJudged {
            skill: "tidy".into()
        })
    );
    assert_eq!(
        call_as(&host, &realm(), &verdict(kept, true))
            .await
            .unwrap(),
        Done::Done
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_s_last_part_keeps_it_only_when_every_part_before_it_came()
 {
    let dir = Scratch::new("library-offer-order");
    let host = open(&dir, Arc::default()).await;
    let part = |call: &str, part: u32, last: bool, path: &str| {
        let bytes = match path {
            "SKILL.md" => PAPER.as_bytes(),
            _ => SCRIPT.as_bytes(),
        };
        offer(
            call,
            "tidy",
            part,
            last,
            vec![piece(path, 0, bytes, true)],
        )
    };
    let more = async |asked: Offer| {
        let answer = call_as(&host, &agent(7), &asked).await.unwrap();
        assert_eq!(answer, OfferAnswer::More);
    };

    more(part("o1", 0, false, "SKILL.md")).await;
    call_as(&host, &agent(7), &part("o1", 2, true, "c.py"))
        .await
        .unwrap_err();
    more(part("o2", 0, false, "SKILL.md")).await;
    let answer =
        call_as(&host, &agent(7), &part("o2", 1, false, "/b.py"))
            .await
            .unwrap();
    assert!(
        matches!(answer, OfferAnswer::Refused { .. }),
        "{answer:?}"
    );
    call_as(&host, &agent(7), &part("o2", 2, true, "c.py"))
        .await
        .unwrap_err();

    more(part("o3", 0, false, "SKILL.md")).await;
    host.stop(&library_at(), ShutdownKind::Sleep).await.unwrap();
    more(part("o3", 1, false, "b.py")).await;
    let kept = offered(&host, 7, &part("o3", 2, true, "c.py")).await;
    let offers = repository(&dir);
    let at =
        offers.tip(&format!("offers/{}", kept.0)).unwrap().unwrap();
    let paths = offers.files(at).unwrap();
    let paths =
        paths.iter().map(|(path, _)| path).collect::<Vec<_>>();
    assert_eq!(paths, ["SKILL.md", "b.py", "c.py"]);
    let made = offers.history(at, 1).unwrap().remove(0);
    assert_eq!(
        (made.message.as_str(), made.author.as_str()),
        ("Offered", "realm")
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_skill_of_many_empty_files_counts_their_paths_toward_its_size()
 {
    let dir = Scratch::new("library-many-empty-files");
    let host = open(&dir, Arc::default()).await;
    let name = "n".repeat(200);
    let mut refused = None;
    for part in 0..8 {
        let files = (0..2500)
            .map(|n| {
                piece(
                    &format!("empty/{name}{part}-{n}.md"),
                    0,
                    b"",
                    true,
                )
            })
            .collect();
        let asked = offer("empties", "empties", part, false, files);
        if let OfferAnswer::Refused { why } =
            call_as(&host, &agent(7), &asked).await.unwrap()
        {
            refused = Some(why);
            break;
        }
    }
    assert_eq!(
        refused,
        Some(OfferRefusal::Skill(BadSkill::SkillTooLarge {
            skill: "empties".into()
        }))
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_is_read_once_its_last_part_came() {
    let dir = Scratch::new("library-read-offer");
    let host = open(&dir, Arc::default()).await;
    let kept = offered(&host, 7, &tidy("c1")).await;
    let started = offer(
        "c2",
        "sweep",
        0,
        false,
        vec![piece("SKILL.md", 0, PAPER.as_bytes(), true)],
    );
    assert_eq!(
        call_as(&host, &agent(8), &started).await.unwrap(),
        OfferAnswer::More
    );
    let read = async |offer| {
        call_as(&host, &realm(), &ReadOffer { offer })
            .await
            .unwrap()
    };

    assert_eq!(
        read(kept).await,
        OfferRead::Offered {
            agent: AgentNumber(7),
            skill: "tidy".into(),
            reason: "folders get messy".into(),
            paths: vec!["SKILL.md".into(), "tidy.py".into()],
            skill_md: PAPER.into(),
        }
    );
    let unfinished = OfferId(kept.0 + 1);
    assert_eq!(
        read(unfinished).await,
        OfferRead::Refused {
            why: NoOffer::NoSuchOffer { offer: unfinished }
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_agents_offers_under_way_are_dropped() {
    let dir = Scratch::new("library-drop-offer");
    let host = open(&dir, Arc::default()).await;
    let kept = offered(&host, 8, &tidy("c1")).await;
    let started = |part, last| {
        offer(
            "c2",
            "sweep",
            part,
            last,
            vec![piece("SKILL.md", 0, PAPER.as_bytes(), true)],
        )
    };
    assert_eq!(
        call_as(&host, &agent(8), &started(0, false)).await.unwrap(),
        OfferAnswer::More
    );

    let drop = DropOfferParts {
        agent: AgentNumber(8),
    };
    call_as(&host, &realm(), &drop).await.unwrap();
    call_as(&host, &realm(), &drop).await.unwrap();

    call_as(&host, &agent(8), &started(1, true))
        .await
        .unwrap_err();
    let read = call_as(&host, &realm(), &ReadOffer { offer: kept })
        .await
        .unwrap();
    assert!(matches!(read, OfferRead::Offered { .. }), "{read:?}");
    host.shutdown().await.unwrap();
}
