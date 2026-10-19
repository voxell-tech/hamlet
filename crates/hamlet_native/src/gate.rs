//! Whether a library was built for this Hamlet.

use std::fmt;

use crate::abi::Info;

/// The first thing in which a library differs from the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Mismatch {
    pub field: &'static str,
    pub host: String,
    pub found: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "built for another Hamlet, its {} differs: this Hamlet \
             has {}, the library has {}",
            self.field, self.host, self.found
        )
    }
}

/// Lets a library through only when it was built just as the host
/// was, and names the first thing that differs if not.
pub fn check(host: &Info, found: &Info) -> Result<(), Mismatch> {
    let mismatch = |field, host: String, found: String| {
        Err(Mismatch { field, host, found })
    };
    if host.abi != found.abi {
        return mismatch(
            "contract version",
            host.abi.to_string(),
            found.abi.to_string(),
        );
    }
    if host.hamlet != found.hamlet {
        return mismatch(
            "Hamlet version",
            host.hamlet.clone(),
            found.hamlet.clone(),
        );
    }
    if host.rustc != found.rustc {
        return mismatch(
            "compiler",
            host.rustc.clone(),
            found.rustc.clone(),
        );
    }
    if host.profile != found.profile {
        return mismatch(
            "profile",
            host.profile.clone(),
            found.profile.clone(),
        );
    }
    if host.fingerprint != found.fingerprint {
        return mismatch(
            "engine crates (Cargo.lock)",
            host.fingerprint.to_string(),
            found.fingerprint.to_string(),
        );
    }
    if host.sources != found.sources {
        return mismatch(
            "SDK sources (hamlet_native, hamlet_sdk, hamlet_base, \
             hamlet_assets, bevy_fynix)",
            host.sources.to_string(),
            found.sources.to_string(),
        );
    }
    if host.types != found.types {
        return mismatch(
            "engine types (other sources, features or settings)",
            host.types.to_string(),
            found.types.to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_field_that_differs_is_named() {
        let host = Info::ours();
        assert_eq!(check(&host, &host), Ok(()));
        let other = |change: fn(&mut Info)| {
            let mut found = host.clone();
            change(&mut found);
            check(&host, &found).unwrap_err().field
        };
        assert_eq!(other(|i| i.abi += 1), "contract version");
        assert_eq!(
            other(|i| i.hamlet = "9.9.9".into()),
            "Hamlet version"
        );
        assert_eq!(
            other(|i| i.rustc = "rustc 0.1".into()),
            "compiler"
        );
        assert_eq!(other(|i| i.profile = "other".into()), "profile");
        assert_eq!(
            other(|i| i.fingerprint += 1),
            "engine crates (Cargo.lock)"
        );
        assert!(other(|i| i.sources += 1).starts_with("SDK sources"));
        assert!(other(|i| i.types += 1).starts_with("engine types"));
        let mut both = host.clone();
        both.hamlet = "9.9.9".into();
        both.types += 1;
        assert_eq!(
            check(&host, &both).unwrap_err().field,
            "Hamlet version"
        );
    }
}
