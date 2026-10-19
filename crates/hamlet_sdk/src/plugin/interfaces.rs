//! Typed interfaces: the functions a plugin exports for others to
//! call through the host, the ones another imports, and the types
//! the host checks their arguments and results against.

use std::fmt;

use super::offers::valid;

/// The levels a type may nest.
const DEPTH: usize = 8;

/// An interface a plugin exports, under a name that several plugins
/// may export.
#[derive(Clone, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    #[serde(default)]
    pub functions: Vec<Function>,
}

/// An interface a plugin calls: the functions it calls, and the
/// plugins it calls them of, any that implements it where `from` is
/// empty.
#[derive(Clone, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Imported {
    pub name: String,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub functions: Vec<Function>,
}

/// A function of an interface: the types of its parameters and of
/// its result, as text such as `list<string>`. A function with no
/// result returns nothing.
#[derive(Clone, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub name: String,
    #[serde(default)]
    pub params: Vec<String>,
    pub result: Option<String>,
}

/// The types of a function, read: its parameters as one tuple.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Signature {
    pub params: Type,
    pub result: Type,
}

/// A type a value that crosses between plugins is checked against.
/// The value is bytes written with `postcard`: a parameter list is
/// its tuple, a Rust struct is the tuple of its fields in order and
/// no result is the empty tuple.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    Bool,
    U32,
    U64,
    S32,
    S64,
    F32,
    F64,
    String,
    Bytes,
    List(Box<Type>),
    Option(Box<Type>),
    Tuple(Vec<Type>),
}

impl Function {
    /// The types the text names, or why they cannot be read.
    pub fn signature(&self) -> Result<Signature, String> {
        let params = self
            .params
            .iter()
            .map(|text| Type::parse(text))
            .collect::<Result<Vec<_>, _>>()?;
        let result = match &self.result {
            Some(text) => Type::parse(text)?,
            None => Type::Tuple(Vec::new()),
        };
        Ok(Signature {
            params: Type::Tuple(params),
            result,
        })
    }
}

/// Why `functions` of the interface `name` cannot be, if they cannot.
fn problem(name: &str, functions: &[Function]) -> Option<String> {
    if !valid(name) {
        return Some(format!("the interface `{name}` is not a name"));
    }
    let mut seen = Vec::new();
    functions.iter().find_map(|function| {
        let at = format!("{name}/{}", function.name);
        if !valid(&function.name) {
            return Some(format!(
                "the function `{at}` is not a name"
            ));
        }
        if seen.contains(&&function.name) {
            return Some(format!("the function `{at}` is twice"));
        }
        seen.push(&function.name);
        function
            .signature()
            .err()
            .map(|why| format!("the function `{at}`: {why}"))
    })
}

impl Interface {
    /// Why the interface cannot be, if it cannot.
    pub fn problem(&self) -> Option<String> {
        problem(&self.name, &self.functions)
    }

    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|function| function.name == name)
    }
}

impl Imported {
    /// Why the import cannot be, if it cannot.
    pub fn problem(&self) -> Option<String> {
        problem(&self.name, &self.functions).or_else(|| {
            self.from.iter().find(|id| !valid(id)).map(|id| {
                format!(
                    "the interface `{}` is imported from `{id}`, \
                     which is not a name",
                    self.name
                )
            })
        })
    }

    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|function| function.name == name)
    }

    /// Whether the plugin `id` may be called for it.
    pub fn from(&self, id: &str) -> bool {
        self.from.is_empty()
            || self.from.iter().any(|from| from == id)
    }

    /// Why `exported` does not implement the functions this imports
    /// with the same types, if it does not.
    pub fn unlike(&self, exported: &Interface) -> Option<String> {
        self.functions.iter().find_map(|wanted| {
            let at = format!("{}/{}", self.name, wanted.name);
            let Some(found) = exported.function(&wanted.name) else {
                return Some(format!("it does not export `{at}`"));
            };
            match (wanted.signature(), found.signature()) {
                (Ok(want), Ok(has)) if want != has => Some(format!(
                    "it exports `{at}` as {has}, the import says \
                     {want}"
                )),
                (Ok(_), Ok(_)) => None,
                (Err(why), _) | (_, Err(why)) => Some(why),
            }
        })
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Type::Tuple(params) = &self.params else {
            return write!(f, "{} -> {}", self.params, self.result);
        };
        let params = params
            .iter()
            .map(Type::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "({params}) -> {}", self.result)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => write!(f, "bool"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::S32 => write!(f, "s32"),
            Type::S64 => write!(f, "s64"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::String => write!(f, "string"),
            Type::Bytes => write!(f, "bytes"),
            Type::List(of) => write!(f, "list<{of}>"),
            Type::Option(of) => write!(f, "option<{of}>"),
            Type::Tuple(of) => {
                let of = of
                    .iter()
                    .map(Type::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "tuple<{of}>")
            }
        }
    }
}

impl Type {
    /// The type `text` names: `bool`, `u32`, `u64`, `s32`, `s64`,
    /// `f32`, `f64`, `string`, `bytes`, `list<T>`, `option<T>` or
    /// `tuple<T, ..>`, nested at most a few levels.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rest = text;
        let parsed = Self::read_text(&mut rest, 0)?;
        match rest.trim() {
            "" => Ok(parsed),
            left => Err(format!("`{text}` has `{left}` left over")),
        }
    }

    fn read_text(
        rest: &mut &str,
        depth: usize,
    ) -> Result<Self, String> {
        if depth >= DEPTH {
            return Err(format!(
                "a type nests more than {DEPTH} levels"
            ));
        }
        *rest = rest.trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        let (name, after) = rest.split_at(end);
        *rest = after;
        let plain = match name {
            "bool" => Some(Type::Bool),
            "u32" => Some(Type::U32),
            "u64" => Some(Type::U64),
            "s32" => Some(Type::S32),
            "s64" => Some(Type::S64),
            "f32" => Some(Type::F32),
            "f64" => Some(Type::F64),
            "string" => Some(Type::String),
            "bytes" => Some(Type::Bytes),
            _ => None,
        };
        if let Some(plain) = plain {
            return Ok(plain);
        }
        if !matches!(name, "list" | "option" | "tuple") {
            return Err(format!("`{name}` is not a type"));
        }
        expect(rest, '<')?;
        let made = match name {
            "list" => Type::List(Box::new(Self::read_text(
                rest,
                depth + 1,
            )?)),
            "option" => Type::Option(Box::new(Self::read_text(
                rest,
                depth + 1,
            )?)),
            _ => {
                let mut of = Vec::new();
                while !rest.trim_start().starts_with('>') {
                    if !of.is_empty() {
                        expect(rest, ',')?;
                    }
                    of.push(Self::read_text(rest, depth + 1)?);
                }
                Type::Tuple(of)
            }
        };
        expect(rest, '>')?;
        Ok(made)
    }

    /// Whether `bytes` are one value of this type, written with
    /// `postcard`, and nothing more.
    pub fn check(&self, bytes: &[u8]) -> Result<(), String> {
        let mut rest = bytes;
        self.read(&mut rest)
            .map_err(|why| format!("not a {self}: {why}"))?;
        match rest.len() {
            0 => Ok(()),
            left => Err(format!(
                "not a {self}: {left} bytes are left over"
            )),
        }
    }

    fn read(&self, rest: &mut &[u8]) -> Result<(), String> {
        match self {
            Type::Bool => match take(rest, 1)? {
                [0 | 1] => Ok(()),
                _ => Err("a bool is not 0 or 1".into()),
            },
            Type::U32 | Type::S32 => {
                let value = varint(rest, 5)?;
                match value <= u64::from(u32::MAX) {
                    true => Ok(()),
                    false => Err("a number passes 32 bits".into()),
                }
            }
            Type::U64 | Type::S64 => varint(rest, 10).map(drop),
            Type::F32 => take(rest, 4).map(drop),
            Type::F64 => take(rest, 8).map(drop),
            Type::String => {
                let len = length(rest)?;
                std::str::from_utf8(take(rest, len)?)
                    .map(drop)
                    .map_err(|_| "a string is not UTF-8".into())
            }
            Type::Bytes => {
                let len = length(rest)?;
                take(rest, len).map(drop)
            }
            Type::List(of) => {
                for _ in 0..length(rest)? {
                    of.read(rest)?;
                }
                Ok(())
            }
            Type::Option(of) => match take(rest, 1)? {
                [0] => Ok(()),
                [1] => of.read(rest),
                _ => Err("an option is not 0 or 1".into()),
            },
            Type::Tuple(of) => {
                of.iter().try_for_each(|of| of.read(rest))
            }
        }
    }
}

/// Skips the spaces before `token` and `token` itself.
fn expect(rest: &mut &str, token: char) -> Result<(), String> {
    match rest.trim_start().strip_prefix(token) {
        Some(after) => {
            *rest = after;
            Ok(())
        }
        None => Err(format!("`{token}` is missing")),
    }
}

/// The next `len` bytes.
fn take<'a>(
    rest: &mut &'a [u8],
    len: usize,
) -> Result<&'a [u8], String> {
    if rest.len() < len {
        return Err("the bytes end inside a value".into());
    }
    let (taken, after) = rest.split_at(len);
    *rest = after;
    Ok(taken)
}

/// A number written in at most `most` bytes of seven bits each, the
/// last with its top bit clear.
fn varint(rest: &mut &[u8], most: u32) -> Result<u64, String> {
    let mut value = 0u64;
    for at in 0..most {
        let (&byte, after) = rest
            .split_first()
            .ok_or("the bytes end inside a value")?;
        *rest = after;
        value |= u64::from(byte & 0x7f) << (7 * at);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("a number is too long".into())
}

/// The length of a string, bytes or a list, which no more bytes than
/// are left can hold.
fn length(rest: &mut &[u8]) -> Result<usize, String> {
    let len = varint(rest, 10)?;
    match usize::try_from(len) {
        Ok(len) if len <= rest.len() => Ok(len),
        _ => Err("a length passes the bytes left".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
        postcard::to_allocvec(value).expect("the value is written")
    }

    #[test]
    fn a_value_written_with_postcard_is_its_type_and_no_other() {
        let height = Type::parse("tuple<string, bool>").unwrap();
        let ports =
            Type::parse("option<list<tuple<string, f64>>>").unwrap();
        let numbers = Type::parse("tuple<s32, u64, bytes>").unwrap();

        assert_eq!(
            height.check(&bytes(&("Kettlebay", true))),
            Ok(())
        );
        assert_eq!(
            ports.check(&bytes(&Some(vec![("Puddlewick", 7.4)]))),
            Ok(())
        );
        assert_eq!(
            numbers.check(&bytes(&(-70_000, u64::MAX, vec![1u8, 2]))),
            Ok(())
        );
        assert!(height.check(&bytes(&(7u32, true))).is_err());
        assert!(
            height.check(&bytes(&("Kettlebay", true, 1u8))).is_err()
        );
        assert!(height.check(&bytes(&("Kettlebay",))).is_err());
        assert!(Type::U32.check(&bytes(&u64::MAX)).is_err());
        assert!(
            ports
                .check(&bytes(&Some(vec![("Puddlewick", 7.4f32)])))
                .is_err()
        );
    }

    #[test]
    fn a_type_reads_back_as_it_is_written() {
        for text in
            ["list<option<s64>>", "tuple<>", "tuple<bool, bytes>"]
        {
            assert_eq!(Type::parse(text).unwrap().to_string(), text);
        }
        assert!(Type::parse("list<string").is_err());
        assert!(Type::parse("map<string>").is_err());
        assert!(Type::parse(&"list<".repeat(9)).is_err());
    }
}
