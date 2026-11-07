//! Enums the database keeps as text.

/// Declares an enum kept as the text given for each variant.
macro_rules! text_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$doc:meta])* $variant:ident = $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize,
            diesel::expression::AsExpression,
            diesel::deserialize::FromSqlRow,
        )]
        #[diesel(sql_type = diesel::sql_types::Text)]
        $vis enum $name {
            $(
                $(#[$doc])*
                #[serde(rename = $text)]
                $variant
            ),+
        }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn named(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        hamlet_store::named_text!($name);
    };
}
pub(crate) use text_enum;
