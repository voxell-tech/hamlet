//! The rules' record of a hamlet and of the realm as a plugin reads
//! them: the names of the first hamlet and of a mayor, the work a
//! hamlet is pointed at, its votes and the chief's commands.

pub use hamlet_rules::budget::format_tokens;
pub use hamlet_rules::chief::{Target, switch};
pub use hamlet_rules::commands::RESUME;
pub use hamlet_rules::hamlet::{HOME, Hamlet, MAYOR};
pub use hamlet_rules::realm::{Origin, Realm};
pub use hamlet_rules::vote::{self, Standing};
pub use hamlet_rules::work::{Mode, Work, auto_in};
