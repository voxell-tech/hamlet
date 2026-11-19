//! What each provider last told of its limits: read when a client
//! asks, and laid over by what turns learn as they run.

use std::collections::BTreeMap;

use hamlet_protocol::client::{
    LimitMoney, LimitWindow, LimitsState, ProviderLimits,
};
use hamlet_protocol::runtime::{self, Limits};

/// The providers whose limits are read: the subscriptions, and
/// OpenRouter, whose keys carry a cap.
pub(crate) const READ: [&str; 3] =
    ["chatgpt", "claude", "openrouter"];

/// How long a read stands before a client's ask reads again, in
/// milliseconds.
pub(crate) const FRESH: i64 = 60_000;

/// What each provider last told of its limits, and when each was last
/// read.
#[derive(Default)]
pub(crate) struct Told(
    BTreeMap<String, Limits>,
    BTreeMap<String, i64>,
);

impl Told {
    /// What `provider` last told, for a client.
    pub fn view(&self, provider: &str) -> Option<ProviderLimits> {
        self.0.get(provider).map(view)
    }

    /// Whether `provider`'s limits were read within [`FRESH`] of
    /// `now`. What a turn saw is no read: it tells only part of them.
    pub fn fresh(&self, provider: &str, now: i64) -> bool {
        self.1.get(provider).is_some_and(|&at| now - at < FRESH)
    }

    /// Takes a read of `provider`'s limits. A refusal a turn saw
    /// stands against a read that tells nothing, since a provider
    /// that tells nothing of its limits cannot say the refusal
    /// lifted, and against a read made before it; such a read adds
    /// only the plan and the windows the refusal leaves out.
    pub fn read(&mut self, provider: &str, read: Limits) {
        self.1.insert(provider.into(), millis(read.checked));
        let keep = self.0.get(provider).is_some_and(|prior| {
            prior.state == runtime::LimitsState::Refused
                && ((read.state == runtime::LimitsState::Unknown
                    && read.windows.is_empty())
                    || prior.checked > read.checked)
        });
        if keep {
            if let Some(prior) = self.0.get_mut(provider) {
                prior.plan = read.plan.or(prior.plan.take());
                for window in read.windows {
                    if !prior
                        .windows
                        .iter()
                        .any(|w| w.id == window.id)
                    {
                        prior.windows.push(window);
                    }
                }
            }
            return;
        }
        self.0.insert(provider.into(), read);
    }

    /// Notes a read of `provider` that failed: it keeps what it told
    /// before, and is unknown if it told nothing yet.
    pub fn failed(&mut self, provider: &str) {
        self.0.entry(provider.into()).or_insert(Limits {
            checked: crate::app::clock::now() as f64,
            plan: None,
            state: runtime::LimitsState::Unknown,
            windows: Vec::new(),
        });
    }

    /// Lays what a turn learnt of `provider`'s limits over what it
    /// last told: its windows replace those of the same id, keeping
    /// what is known of the same window that they leave out, and its
    /// state is the provider's now. Returns whether the provider
    /// refused where it did not before.
    pub fn seen(&mut self, provider: &str, seen: Limits) -> bool {
        let prior = self.0.entry(provider.into()).or_insert(Limits {
            checked: seen.checked,
            plan: None,
            state: runtime::LimitsState::Unknown,
            windows: Vec::new(),
        });
        let refused = seen.state == runtime::LimitsState::Refused
            && prior.state != runtime::LimitsState::Refused;
        for window in seen.windows {
            match prior.windows.iter_mut().find(|w| w.id == window.id)
            {
                Some(known) => {
                    // A turn may name a window without its use; the
                    // use known of it stands until the window resets.
                    // Its reset is not compared: a turn tells it in
                    // whole seconds, a read to the millisecond.
                    let reset = known
                        .resets
                        .is_some_and(|at| at <= seen.checked);
                    let kept = known.clone();
                    *known = window;
                    if !reset {
                        known.used = known.used.or(kept.used);
                        known.resets = known.resets.or(kept.resets);
                        known.length = known.length.or(kept.length);
                        known.money =
                            known.money.take().or(kept.money);
                    }
                }
                None => prior.windows.push(window),
            }
        }
        prior.state = seen.state;
        prior.checked = prior.checked.max(seen.checked);
        if seen.plan.is_some() {
            prior.plan = seen.plan;
        }
        refused
    }

    /// Forgets `provider`'s limits, as when its sign-in changes.
    pub fn forget(&mut self, provider: &str) {
        self.0.remove(provider);
        self.1.remove(provider);
    }
}

fn view(limits: &Limits) -> ProviderLimits {
    ProviderLimits {
        checked: millis(limits.checked),
        plan: limits.plan.clone(),
        state: match limits.state {
            runtime::LimitsState::Ok => LimitsState::Ok,
            runtime::LimitsState::Warning => LimitsState::Warning,
            runtime::LimitsState::Refused => LimitsState::Refused,
            runtime::LimitsState::Unknown => LimitsState::Unknown,
        },
        windows: limits
            .windows
            .iter()
            .map(|w| LimitWindow {
                id: w.id.clone(),
                length: w.length.map(millis),
                money: w.money.as_ref().map(|m| LimitMoney {
                    cap: m.cap,
                    left: m.left,
                }),
                name: w.name.clone(),
                resets: w.resets.map(millis),
                used: w.used,
            })
            .collect(),
    }
}

/// A whole number from the runtime's JSON number, which a runtime in
/// JavaScript sends as a double.
#[expect(
    clippy::cast_possible_truncation,
    reason = "times and minutes are whole and well within an i64"
)]
fn millis(value: f64) -> i64 {
    value.round() as i64
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::runtime::LimitsState::{
        Ok, Refused, Unknown, Warning,
    };

    use super::*;

    fn window(id: &str, used: f64) -> runtime::LimitWindow {
        runtime::LimitWindow {
            id: id.into(),
            length: Some(300.0),
            money: None,
            name: id.into(),
            resets: Some(2_000.0),
            used: Some(used),
        }
    }

    fn limits(
        checked: f64,
        state: runtime::LimitsState,
        windows: Vec<runtime::LimitWindow>,
    ) -> Limits {
        Limits {
            checked,
            plan: None,
            state,
            windows,
        }
    }

    #[test]
    fn a_turn_replaces_the_window_it_names_and_keeps_the_others() {
        let mut told = Told::default();
        told.read(
            "claude",
            Limits {
                plan: Some("Max".into()),
                ..limits(
                    1_000.0,
                    Ok,
                    vec![
                        window("five_hour", 40.0),
                        window("week", 10.0),
                    ],
                )
            },
        );
        let refused = told.seen(
            "claude",
            limits(1_500.0, Warning, vec![window("five_hour", 91.0)]),
        );
        assert!(!refused);
        let view = told.view("claude").unwrap();
        assert_eq!(view.checked, 1_500);
        assert_eq!(view.plan.as_deref(), Some("Max"));
        assert_eq!(view.state, LimitsState::Warning);
        let used: Vec<_> =
            view.windows.iter().map(|w| (&*w.id, w.used)).collect();
        assert_eq!(
            used,
            [("five_hour", Some(91.0)), ("week", Some(10.0))]
        );

        // A turn that names the window without its use keeps the use
        // known, and is no read.
        told.seen(
            "claude",
            limits(
                1_600.0,
                Ok,
                vec![runtime::LimitWindow {
                    used: None,
                    resets: Some(2_001.0),
                    ..window("five_hour", 0.0)
                }],
            ),
        );
        let view = told.view("claude").unwrap();
        assert_eq!(view.windows[0].used, Some(91.0));
        assert!(!told.fresh("claude", 1_000 + FRESH));

        // Once the window resets, what was used of it is gone.
        told.seen(
            "claude",
            limits(
                2_500.0,
                Ok,
                vec![runtime::LimitWindow {
                    used: None,
                    ..window("five_hour", 0.0)
                }],
            ),
        );
        assert_eq!(
            told.view("claude").unwrap().windows[0].used,
            None
        );
    }

    #[test]
    fn a_refusal_is_told_once_and_a_read_that_tells_nothing_keeps_it()
    {
        let mut told = Told::default();
        assert!(
            told.seen("chatgpt", limits(1_000.0, Refused, vec![]))
        );
        assert!(
            !told.seen("chatgpt", limits(1_200.0, Refused, vec![]))
        );
        // A read made before the refusal, ending after it, keeps it.
        told.read(
            "chatgpt",
            limits(1_100.0, Ok, vec![window("primary", 40.0)]),
        );
        let view = told.view("chatgpt").unwrap();
        assert_eq!(
            (view.state, view.checked, view.windows.len()),
            (LimitsState::Refused, 1_200, 1)
        );
        told.read("chatgpt", limits(5_000.0, Unknown, vec![]));
        let view = told.view("chatgpt").unwrap();
        assert_eq!(view.state, LimitsState::Refused);
        assert_eq!(view.checked, 1_200);

        // A turn the plan answers lifts it.
        assert!(!told.seen("chatgpt", limits(6_000.0, Ok, vec![])));
        assert_eq!(
            told.view("chatgpt").unwrap().state,
            LimitsState::Ok
        );
        told.read("chatgpt", limits(7_000.0, Unknown, vec![]));
        assert_eq!(
            told.view("chatgpt").unwrap().state,
            LimitsState::Unknown
        );
    }

    #[test]
    fn a_read_stands_for_a_minute() {
        let mut told = Told::default();
        assert!(!told.fresh("claude", 0));
        told.read("claude", limits(10_000.0, Ok, vec![]));
        assert!(told.fresh("claude", 10_000 + FRESH - 1));
        assert!(!told.fresh("claude", 10_000 + FRESH));

        // Another sign-in is read at once.
        told.forget("claude");
        assert!(!told.fresh("claude", 10_001));
    }
}
