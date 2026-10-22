//! The archive gallery scenes.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::ecs::world::World;
use hamlet_client::chats::Chat;
use hamlet_protocol::client::{self as protocol, Past};
use hamlet_ui::history::{self, History, Reading};

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
        let transcript = super::chat::conversation(&[
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
        Reading {
            number: pasts[1].number,
            said: history::said(&transcript),
            holder: world
                .spawn((
                    Chat(protocol::ChatRef {
                        kind: None,
                        from: None,
                        with: world
                            .resource::<hamlet_ui::Me>()
                            .to
                            .clone(),
                        past: Some(pasts[1].number),
                    }),
                    transcript,
                ))
                .id(),
            reach: history::PAGE,
        }
    });
    world.insert_resource(History {
        pasts,
        reading,
        fresh: true,
        ..Default::default()
    });
}
