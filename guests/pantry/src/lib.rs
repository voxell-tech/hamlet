//! A plugin that keeps its stock in its own database: one record for
//! each kind of thing, found by its kind. The panel reads the table
//! each time it is drawn, so what it shows is what was kept, also
//! after the app was restarted.

use hamlet_guest::db::Table;
use hamlet_guest::ui::{
    View, button, column, divider, heading, note, row,
};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

/// The records are kept by their lowercase name.
const STOCK: Table<Item> = Table::new("stock");

/// The most records the panel reads.
const SHOWN: u32 = 20;

/// What the host holds across a reload, and drops at a restart.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    /// What came of the last find.
    found: String,
}

/// A thing in the pantry.
#[derive(Serialize, Deserialize)]
struct Item {
    name: String,
    count: u32,
}

struct Pantry;

impl Plugin for Pantry {
    fn entry() {
        hamlet_guest::panel("pantry", "PANTRY", "robot");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut heard = false;
        for incoming in inbox {
            if let Incoming::Click(press) = incoming {
                heard = true;
                click(&press.id, &mut model);
            }
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        match heard {
            true => vec![Intent::Redraw("pantry".into())],
            false => Vec::new(),
        }
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let mut rows = vec![
            heading("PANTRY"),
            row([
                button("Add tea").id("tea"),
                button("Add bread").id("bread"),
                button("Find drinks").id("drinks"),
                button("Remove tea").id("remove"),
            ]),
            divider(),
        ];
        match STOCK.list(None, SHOWN) {
            Ok(page) => {
                rows.push(note(&format!(
                    "Items: {}",
                    page.rows.len()
                )));
                rows.extend(page.rows.iter().map(|row| {
                    note(&format!(
                        "{} x{}",
                        row.value.name, row.value.count
                    ))
                }));
            }
            Err(why) => rows.push(note(&why)),
        }
        if !model.found.is_empty() {
            rows.push(note(&model.found));
        }
        column(rows).into()
    }
}

fn click(id: &str, model: &mut Model) {
    let done = match id {
        "tea" => add("Tea", "drink"),
        "bread" => add("Bread", "food"),
        "drinks" => find(model),
        "remove" => STOCK.delete("tea"),
        _ => Ok(()),
    };
    match done {
        Err(why) => model.found = why,
        Ok(()) if id != "drinks" => model.found.clear(),
        Ok(()) => {}
    }
}

/// Adds one to the count of the thing called `name`, or keeps the
/// first of it.
fn add(name: &str, kind: &str) -> Result<(), String> {
    let id = name.to_lowercase();
    let count = STOCK.get(&id)?.map_or(0, |item| item.count);
    let item = Item {
        name: name.into(),
        count: count + 1,
    };
    STOCK.put(&id, &item, &[("kind", kind)])
}

/// Notes what the pantry holds of the kind `drink`.
fn find(model: &mut Model) -> Result<(), String> {
    let page = STOCK.find("kind", "drink", None, SHOWN)?;
    let names = page
        .rows
        .iter()
        .map(|row| format!("{} x{}", row.value.name, row.value.count))
        .collect::<Vec<_>>();
    model.found = format!("Drinks: {}", names.join(", "));
    Ok(())
}

hamlet_guest::export!(Pantry);
