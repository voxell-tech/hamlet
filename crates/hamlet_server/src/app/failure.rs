//! How an input fails: the error handler records each failure, and
//! the runner reads them after the update. Only a [`Fatal`] error, or
//! a database that is no longer a realm's, stops the server.

use std::any::Any;
use std::cell::RefCell;

use bevy_ecs::error::{BevyError, ErrorContext};

/// An error after which the realm cannot be served safely.
#[derive(Debug, thiserror::Error)]
#[error("{0:#}")]
pub(crate) struct Fatal(pub anyhow::Error);

/// SQLite's own words for SQLITE_CORRUPT and SQLITE_NOTADB, which are
/// all diesel keeps of the code.
const BROKEN: [&str; 2] =
    ["database disk image is malformed", "file is not a database"];

/// The check that `error` means the realm cannot be served safely:
/// built as [`Fatal`], or a database SQLite calls corrupt or not a
/// database.
fn is_fatal(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.is::<Fatal>()
            || cause
                .downcast_ref::<diesel::result::Error>()
                .is_some_and(|error| match error {
                    diesel::result::Error::DatabaseError(_, info) => {
                        BROKEN.contains(&info.message())
                    }
                    _ => false,
                })
    })
}

/// A failure of an input: the words whoever sent it is told, every
/// cause for the log, and whether the realm can still be served.
#[derive(Clone, Debug)]
pub(crate) struct Failure {
    pub shown: String,
    pub logged: String,
    pub fatal: bool,
}

impl Failure {
    pub fn of(error: &anyhow::Error) -> Self {
        Self {
            shown: format!("{error}"),
            logged: format!("{error:#}"),
            fatal: is_fatal(error),
        }
    }

    /// The failure a system returned or panicked with. Its causes
    /// are kept only in its debug form, as `anyhow` writes them.
    fn of_system(mut error: BevyError) -> Self {
        if let Some(panic) = error.take_payload() {
            return Self::panicked(&*panic);
        }
        let whole = format!("{error:?}");
        Self {
            shown: error.to_string().trim_end().to_owned(),
            fatal: BROKEN.iter().any(|words| whole.contains(words)),
            logged: causes(&whole),
        }
    }

    pub fn panicked(panic: &(dyn Any + Send)) -> Self {
        let words = panicked(panic);
        Self {
            shown: words.clone(),
            logged: words,
            fatal: false,
        }
    }
}

/// The words a handler that panicked gives for it.
pub(crate) fn panicked(panic: &(dyn Any + Send)) -> String {
    let words = panic
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    format!("the handler panicked: {words}")
}

/// The causes of an error's debug form on one line, as `{:#}` writes
/// them: `anyhow` writes each on a line of its own under "Caused
/// by:", numbered when there are several.
fn causes(debug: &str) -> String {
    debug
        .lines()
        .map(str::trim)
        .take_while(|line| *line != "Stack backtrace:")
        .filter(|line| !line.is_empty() && *line != "Caused by:")
        .map(|line| {
            line.split_once(": ")
                .filter(|(number, _)| number.parse::<usize>().is_ok())
                .map_or(line, |(_, cause)| cause)
        })
        .collect::<Vec<_>>()
        .join(": ")
}

thread_local! {
    /// The failures of the input being run, as the error handler
    /// records them. The server runs on one thread.
    static FAILURES: RefCell<Vec<Failure>> = const { RefCell::new(Vec::new()) };
}

/// The world's error handler: a system that fails or panics fails
/// only the input it ran for, which `PostUpdate` rolls back.
pub(super) fn record(error: BevyError, _: ErrorContext) {
    fail(Failure::of_system(error));
}

/// Fails the input being run with `failure`.
pub(crate) fn fail(failure: Failure) {
    FAILURES.with_borrow_mut(|failures| failures.push(failure));
}

/// The input's first failure so far, if it failed.
pub(crate) fn first_failure() -> Option<Failure> {
    FAILURES.with_borrow(|failures| failures.first().cloned())
}

/// The failures recorded since the last call.
pub(super) fn recorded() -> Vec<Failure> {
    FAILURES.with_borrow_mut(std::mem::take)
}
