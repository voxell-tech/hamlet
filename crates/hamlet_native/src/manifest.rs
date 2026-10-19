//! `manifest.toml` of a native plugin.

use hamlet_sdk::plugin::{RESERVED, plain, valid};

/// The most characters of a plugin's name.
const NAME_MAX: usize = 64;

/// The kind a native plugin's manifest names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Native,
}

/// The contents of a native plugin's `manifest.toml`.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: Kind,
    /// The file name of the library in the plugin's folder.
    pub library: String,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Why the manifest cannot be, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if !valid(&self.id) {
            return Some(format!(
                "the id `{}` is not a name",
                plain(
                    &self
                        .id
                        .chars()
                        .take(NAME_MAX)
                        .collect::<String>()
                )
            ));
        }
        if RESERVED.contains(&self.id.as_str()) {
            return Some(format!("`{}` is a reserved id", self.id));
        }
        if self.name.chars().count() > NAME_MAX {
            return Some(format!(
                "the name has more than {NAME_MAX} characters"
            ));
        }
        let plain_file = !self.library.is_empty()
            && !self.library.contains(['/', '\\'])
            && !self.library.starts_with('.');
        if !plain_file {
            return Some(format!(
                "the library `{}` is not a file name in the \
                 plugin's folder",
                plain(&self.library)
            ));
        }
        let extension = std::env::consts::DLL_EXTENSION;
        (std::path::Path::new(&self.library)
            .extension()
            .and_then(|e| e.to_str())
            != Some(extension))
        .then(|| format!("the library is not a .{extension} file"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, library: &str) -> Manifest {
        Manifest::parse(&format!(
            "id = \"{id}\"\nname = \"Spark\"\nversion = \
             \"0.1.0\"\nkind = \"native\"\nlibrary = \"{library}\"\n"
        ))
        .unwrap()
    }

    #[test]
    fn a_manifest_is_read_and_its_mistakes_named() {
        let library =
            format!("libspark.{}", std::env::consts::DLL_EXTENSION);
        assert_eq!(manifest("spark", &library).problem(), None);
        let problem = |id, library: &str| {
            manifest(id, library).problem().unwrap()
        };
        assert!(problem("Spark!", &library).contains("not a name"));
        assert!(problem("chief", &library).contains("reserved"));
        assert!(problem("spark", "../x.so").contains("file name"));
        assert!(problem("spark", "spark.txt").contains("not a ."));
        assert!(
            Manifest::parse("id = \"a\"\nkind = \"wasm\"\n").is_err()
        );
    }
}
