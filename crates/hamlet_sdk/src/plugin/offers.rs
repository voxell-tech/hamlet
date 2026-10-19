//! What a plugin offers the others (`exports`) and what it uses of
//! theirs (`imports`). Nothing outside these lists is reachable.

/// The name the host publishes its events under, which no plugin
/// may take.
pub const HOST: &str = "host";

/// The ids no installed plugin may take: the host, the chief and the
/// plugins compiled into the app.
pub const RESERVED: [&str; 10] = [
    HOST,
    "chief",
    "window",
    "agents",
    "hamlets",
    "inspector",
    "needs",
    "realm",
    "settings",
    "skills",
];

pub use hamlet_core::tools::plugged::plain;

/// A collection a plugin offers: the keys of its storage that start
/// with `prefix`.
#[derive(Clone, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub name: String,
    pub prefix: String,
    /// Whether others, once allowed, may write to it.
    #[serde(default)]
    pub writable: bool,
}

/// The commands, events and collections a plugin offers, each by a
/// name of its own.
#[derive(
    Clone, PartialEq, Eq, Default, Debug, serde::Deserialize,
)]
#[serde(default, deny_unknown_fields)]
pub struct Exports {
    pub commands: Vec<String>,
    pub events: Vec<String>,
    pub collections: Vec<Collection>,
}

/// The commands, events and collections a plugin uses, each as
/// `plugin/name`.
#[derive(
    Clone, PartialEq, Eq, Default, Debug, serde::Deserialize,
)]
#[serde(default, deny_unknown_fields)]
pub struct Imports {
    pub commands: Vec<String>,
    pub events: Vec<String>,
    pub collections: Vec<String>,
}

/// `name` of the plugin `plugin`, as an import lists it.
pub fn link(plugin: &str, name: &str) -> String {
    format!("{plugin}/{name}")
}

/// The plugin and the name of an import.
pub fn unlink(import: &str) -> Option<(&str, &str)> {
    let (plugin, name) = import.split_once('/')?;
    (valid(plugin) && valid(name)).then_some((plugin, name))
}

/// Whether `name` can name a plugin, command, event or collection:
/// 1 to 64 bytes of `a-z`, `0-9`, `.`, `_` and `-`, not starting
/// with `.`.
pub fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.bytes().all(|byte| {
            matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')
        })
}

impl Exports {
    /// Why the exports cannot be, if they cannot.
    pub fn problem(&self) -> Option<String> {
        let names =
            self.commands
                .iter()
                .map(|name| ("command", name))
                .chain(self.events.iter().map(|name| ("event", name)))
                .chain(self.collections.iter().map(|collection| {
                    ("collection", &collection.name)
                }));
        let mut seen = Vec::new();
        for (kind, name) in names {
            if !valid(name) {
                return Some(format!(
                    "the {kind} `{name}` is not a name"
                ));
            }
            if seen.contains(&(kind, name)) {
                return Some(format!("the {kind} `{name}` is twice"));
            }
            seen.push((kind, name));
        }
        self.collections.iter().find_map(|collection| {
            (!valid(&collection.prefix)).then(|| {
                format!(
                    "the prefix `{}` of the collection `{}` is not \
                     a key start",
                    collection.prefix, collection.name
                )
            })
        })
    }

    pub fn collection(&self, name: &str) -> Option<&Collection> {
        self.collections
            .iter()
            .find(|collection| collection.name == name)
    }
}

impl Imports {
    /// Why the imports cannot be, if they cannot.
    pub fn problem(&self) -> Option<String> {
        self.commands
            .iter()
            .chain(&self.events)
            .chain(&self.collections)
            .find(|import| unlink(import).is_none())
            .map(|import| {
                format!("the import `{import}` is not `plugin/name`")
            })
    }

    /// The plugins any import is of, once each, in order. The host is
    /// not a plugin.
    pub fn plugins(&self) -> Vec<&str> {
        let mut plugins = Vec::new();
        for import in self
            .commands
            .iter()
            .chain(&self.events)
            .chain(&self.collections)
        {
            if let Some((plugin, _)) = unlink(import)
                && plugin != HOST
                && !plugins.contains(&plugin)
            {
                plugins.push(plugin);
            }
        }
        plugins
    }

    pub fn command(&self, plugin: &str, name: &str) -> bool {
        self.commands.contains(&link(plugin, name))
    }

    pub fn event(&self, plugin: &str, name: &str) -> bool {
        self.events.contains(&link(plugin, name))
    }

    pub fn collection(&self, plugin: &str, name: &str) -> bool {
        self.collections.contains(&link(plugin, name))
    }
}
