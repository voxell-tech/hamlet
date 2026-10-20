//! The wire format: what crosses between the engine and a plugin, as
//! postcard bytes.

use core::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Bytes that are not a value of the type asked for.
#[derive(Debug)]
pub struct WireError(postcard::Error);

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not readable: {}", self.0)
    }
}

impl std::error::Error for WireError {}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    postcard::to_allocvec(value).map_err(WireError)
}

/// The value in `bytes`, which must hold nothing after it.
pub fn decode<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, WireError> {
    let (value, rest) =
        postcard::take_from_bytes::<T>(bytes).map_err(WireError)?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(WireError(postcard::Error::DeserializeBadEncoding))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        API_VERSION, Ask, Capability, Manifest, PluginId, TreeError,
        TreeLimits, Widget, WidgetId, WidgetNode, WidgetTree,
    };

    fn node(id: &str, children: u32, widget: Widget) -> WidgetNode {
        WidgetNode {
            id: WidgetId(id.into()),
            children,
            widget,
        }
    }

    fn tree() -> WidgetTree {
        WidgetTree {
            nodes: vec![
                node("root", 3, Widget::Column),
                node("title", 0, Widget::Heading("Forecast".into())),
                node("days", 2, Widget::List),
                node("monday", 0, Widget::Text("Rain".into())),
                node("tuesday", 0, Widget::Text("Sun".into())),
                node("bar", 3, Widget::Row),
                node(
                    "city",
                    0,
                    Widget::Field {
                        value: "Lisbon".into(),
                        placeholder: "City".into(),
                    },
                ),
                node(
                    "live",
                    0,
                    Widget::Toggle {
                        label: "Live".into(),
                        on: true,
                    },
                ),
                node("go", 0, Widget::Button("Fetch".into())),
            ],
        }
    }

    const LIMITS: TreeLimits = TreeLimits {
        nodes: 64,
        depth: 8,
        text: 64,
    };

    #[test]
    fn a_tree_and_a_manifest_cross_the_wire_unchanged() {
        let tree = tree();
        assert_eq!(tree.check(&LIMITS), Ok(()));
        let bytes = encode(&tree).unwrap();
        assert_eq!(decode::<WidgetTree>(&bytes).unwrap(), tree);

        let manifest = Manifest {
            id: PluginId::try_from("hamlet.weather").unwrap(),
            name: "Weather".into(),
            version: "0.1.0".into(),
            api: API_VERSION.into(),
            asks: vec![Ask {
                capability: Capability::NetFetch,
                target: "forecast.example.org".into(),
                reason: "Fetches the forecast".into(),
            }],
        };
        let bytes = encode(&manifest).unwrap();
        assert_eq!(decode::<Manifest>(&bytes).unwrap(), manifest);
        // A byte more is refused, as is an id that is not one.
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode::<Manifest>(&longer).is_err());
        let bad = encode(&(
            "Hamlet.Weather",
            "W",
            "0.1.0",
            API_VERSION,
            Vec::<Ask>::new(),
        ))
        .unwrap();
        assert!(decode::<Manifest>(&bad).is_err());
    }

    #[test]
    fn child_counts_that_do_not_make_one_tree_are_refused() {
        let mut short = tree();
        short.nodes.pop();
        assert_eq!(short.check(&LIMITS), Err(TreeError::Missing));

        let mut long = tree();
        long.nodes.push(node("extra", 0, Widget::Divider));
        assert_eq!(long.check(&LIMITS), Err(TreeError::Trailing));

        let mut leaf = tree();
        leaf.nodes[1].children = 1;
        assert!(matches!(
            leaf.check(&LIMITS),
            Err(TreeError::Leaf(_))
        ));

        let shallow = TreeLimits { depth: 2, ..LIMITS };
        assert_eq!(tree().check(&shallow), Err(TreeError::TooDeep));
    }
}
