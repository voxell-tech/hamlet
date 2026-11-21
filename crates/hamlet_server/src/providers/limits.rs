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

/// What each provider last told of its limits.
#[derive(Default)]
pub(crate) struct Told(BTreeMap<String, Limits>);

impl Told {
    /// What `provider` last told, for a client.
    pub fn view(&self, provider: &str) -> Option<ProviderLimits> {
        self.0.get(provider).map(view)
    }

    /// Whether `provider` told its limits within [`FRESH`] of `now`.
    pub fn fresh(&self, provider: &str, now: i64) -> bool {
        self.0
            .get(provider)
            .is_some_and(|l| now - millis(l.checked) < FRESH)
    }

    /// Takes a read of `provider`'s limits. A read that tells nothing
    /// keeps a refusal a turn saw, since a provider that tells
    /// nothing of its limits cannot say the refusal lifted.
    pub fn read(&mut self, provider: &str, read: Limits) {
        let keep = read.state == runtime::LimitsState::Unknown
            && read.windows.is_empty()
            && self.0.get(provider).is_some_and(|prior| {
                prior.state == runtime::LimitsState::Refused
            });
        if keep {
            if let Some(prior) = self.0.get_mut(provider) {
                prior.plan = read.plan.or(prior.plan.take());
            }
            return;
        }
        self.0.insert(provider.into(), read);
    }

    /// Lays what a turn learnt of `provider`'s limits over what it
    /// last told: its windows replace those of the same id, and its
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
                Some(known) => *known = window,
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
    }
}
