//! A plugin that says hello when it starts and logs each tick.

use hamlet_guest::{Incoming, Intent, Level, Plugin};

struct Hello;

impl Plugin for Hello {
    fn entry() {
        hamlet_guest::log(Level::Info, "hello from the plugin");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        inbox
            .into_iter()
            .map(|Incoming::Tick(tick)| {
                Intent::LogLine(format!("tick {tick}"))
            })
            .collect()
    }
}

hamlet_guest::export!(Hello);
