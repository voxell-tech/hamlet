//! What a client's subscription gives, within a test's wait.

use std::time::Duration;

use hamlet_host::{Item, Subscription};

use crate::code::code;

/// The subscription's next item, within `within`.
pub async fn next(
    subscription: &mut Subscription,
    within: Duration,
) -> Item {
    tokio::time::timeout(within, subscription.next())
        .await
        .expect("the subscription gave nothing")
        .expect("the subscription ended")
}

/// The code the subscription ended with, within `within`.
pub async fn end(
    subscription: &mut Subscription,
    within: Duration,
) -> String {
    let next = tokio::time::timeout(within, subscription.next())
        .await
        .expect("the subscription did not end");
    match next {
        Ok(item) => panic!("the subscription gave {item:?}"),
        Err(error) => code(&error),
    }
}

/// The error of a subscribe that had to fail.
pub fn refusal(
    subscribed: anyhow::Result<Subscription>,
) -> anyhow::Error {
    match subscribed {
        Ok(_) => panic!("the subscribe succeeded"),
        Err(error) => error,
    }
}
