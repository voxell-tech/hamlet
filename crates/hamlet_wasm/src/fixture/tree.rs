//! A tree laid out in memory as the canonical ABI has a `tree`, for a
//! test plugin to return.

/// What a node is.
pub enum Widget {
    Column,
    Row,
    Scroll,
    Label(&'static str),
    Heading(&'static str),
    Note(&'static str),
    Icon(&'static str),
    Button(&'static str),
    Checkbox(&'static str, bool),
    TextField(&'static str),
    Divider,
}

/// A node: its id, what it is and how many children follow it.
pub struct Item(pub Option<&'static str>, pub Widget, pub u32);

/// The size of a `node`.
const NODE: usize = 32;

struct Pool {
    next: u32,
    texts: Vec<(u32, &'static str)>,
}

impl Pool {
    fn put(&mut self, text: &'static str) -> (u32, u32) {
        let at = self.next;
        self.texts.push((at, text));
        self.next += text.len() as u32;
        (at, text.len() as u32)
    }
}

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn escaped(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// The data segments that hold `nodes` from address `at`: the
/// `tree` record is at `at`, so a function that returns `at` returns
/// the tree.
pub fn data(at: u32, nodes: &[Item]) -> String {
    let array = at + 8;
    let mut pool = Pool {
        next: array + (NODE * nodes.len()) as u32,
        texts: Vec::new(),
    };
    let mut bytes = vec![0u8; 8 + NODE * nodes.len()];
    word(&mut bytes, 0, array);
    word(&mut bytes, 4, nodes.len() as u32);
    for (n, Item(id, widget, children)) in nodes.iter().enumerate() {
        let node = 8 + n * NODE;
        if let Some(id) = id {
            let (ptr, len) = pool.put(id);
            bytes[node] = 1;
            word(&mut bytes, node + 4, ptr);
            word(&mut bytes, node + 8, len);
        }
        let (tag, text, checked) = match widget {
            Widget::Column => (0, None, false),
            Widget::Row => (1, None, false),
            Widget::Scroll => (2, None, false),
            Widget::Label(text) => (3, Some(*text), false),
            Widget::Heading(text) => (4, Some(*text), false),
            Widget::Note(text) => (5, Some(*text), false),
            Widget::Icon(text) => (6, Some(*text), false),
            Widget::Button(text) => (7, Some(*text), false),
            Widget::Checkbox(text, on) => (8, Some(*text), *on),
            Widget::TextField(text) => (9, Some(*text), false),
            Widget::Divider => (10, None, false),
        };
        bytes[node + 12] = tag;
        if let Some(text) = text {
            let (ptr, len) = pool.put(text);
            word(&mut bytes, node + 16, ptr);
            word(&mut bytes, node + 20, len);
        }
        bytes[node + 24] = u8::from(checked);
        word(&mut bytes, node + 28, *children);
    }
    let mut wat =
        format!("(data (i32.const {at}) \"{}\")\n", escaped(&bytes));
    for (at, text) in pool.texts {
        wat += &format!(
            "(data (i32.const {at}) \"{}\")\n",
            escaped(text.as_bytes())
        );
    }
    wat
}
