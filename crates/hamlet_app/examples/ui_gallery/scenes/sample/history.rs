//! The archive gallery scenes.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::ecs::world::World;
use hamlet_app::history::{self, History};
use hamlet_client::chats::{Chat, Entry, Item, Transcript};
use hamlet_protocol::client::{self as protocol, Past};

/// Sample sessions for the History page, one open if `reading`.
pub(super) fn setup(world: &mut World, reading: bool) {
    let told = [
        ("Tidy the orchard notes and tell me what is missing.", 0, 4),
        ("Which rows were grafted this spring?", 2 * 3600, 12),
        ("Draft the letter to the cooper about the casks.", 86400, 7),
        ("Count the hives and say which are weak.", 3 * 86400, 22),
        ("Where is the key to the press kept?", 9 * 86400, 2),
    ];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let pasts = told
        .iter()
        .enumerate()
        .map(|(i, (title, ago, messages))| Past {
            title: title.to_string(),
            by: "chief".into(),
            modified: now - ago * 1000,
            began: now - ago * 1000,
            number: i as i64 + 1,
            current: i == 0,
            messages: *messages,
            tokens: 1800 * messages.unsigned_abs(),
        })
        .collect::<Vec<_>>();
    let reading = reading.then(|| {
        let said = super::chat::conversation(&[
            ("chief", told[1].0),
            (
                "",
                "Three rows, all on the south wall:\n\n- the \
                 **Conference** pears, grafted in March\n- the two \
                 rows of Comice beside them",
            ),
            ("chief", "Note the quince for next year."),
            ("", "Noted, under next spring's work."),
        ]);
        // It read its notes before it answered.
        let step = |at: usize, file: &str| Item {
            id: format!("tool-{at}"),
            at: 0,
            entry: Entry::Tool {
                name: "bash".into(),
                summary: format!(
                    "{{\"command\": \"grep -n grafted {file}\"}}"
                ),
                exit: Some(0),
                ms: Some(380),
                failure: None,
                change: None,
            },
        };
        let mut items = said.items().to_vec();
        items.splice(
            1..1,
            [step(0, "notes/rows.md"), step(1, "notes/spring.md")],
        );
        // A minute apart, the last when the session was last used.
        let last = items.len() as u64 - 1;
        for (at, item) in items.iter_mut().enumerate() {
            item.at = pasts[1].modified as u64
                - (last - at as u64) * 60_000;
        }
        let transcript = Transcript::from_items(items);
        let holder = world
            .spawn((
                Chat(protocol::ChatRef {
                    kind: None,
                    from: None,
                    with: protocol::AgentId(2),
                    past: Some(pasts[1].number),
                }),
                transcript,
            ))
            .id();
        (pasts[1].number, holder)
    });
    world.insert_resource(History {
        pasts,
        fresh: true,
        ..Default::default()
    });
    if let Some((number, holder)) = reading {
        history::read(world, number, holder);
    }
}
