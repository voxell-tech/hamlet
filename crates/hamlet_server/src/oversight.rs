//! The words of what waits for a chief (H11, H10, H34): the command
//! that answers a wait, and the permissions a chief grants.

use hamlet_protocol::client::{Choice, Permission};

/// The command that answers a wait with `choice`.
pub(crate) fn command(choice: &Choice) -> &'static str {
    match choice {
        Choice::Allow => "allow",
        Choice::Always => "always",
        Choice::Deny => "deny",
        Choice::Go => "go",
        Choice::Withdraw => "withdraw",
        Choice::Reply => "reply",
        Choice::Dismiss => "dismiss",
        Choice::Welcome => "welcome",
    }
}

/// `text` on one line, cut to `most` characters.
pub(crate) fn clip(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= most {
        return flat;
    }
    let mut cut = flat.chars().take(most).collect::<String>();
    cut.push('…');
    cut
}

/// The word for `permission`, as the chief writes it and a grant
/// keeps it.
pub(crate) fn word(permission: &Permission) -> &'static str {
    match permission {
        Permission::Network => "network",
        Permission::Unconfined => "unconfined",
    }
}

/// The permission named `name`.
pub(crate) fn permission(name: &str) -> Option<Permission> {
    [Permission::Network, Permission::Unconfined]
        .into_iter()
        .find(|permission| word(permission) == name)
}
