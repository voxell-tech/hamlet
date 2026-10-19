//! A plugin that draws a chart on a canvas: a bar for each day, a
//! line over their tops and a disc on each, in the colours of the
//! theme. A button changes the numbers, so the canvas is drawn again.

use hamlet_guest::draw::{circle, line, rect, text, theme};
use hamlet_guest::ui::{
    View, button, canvas, column, heading, note, row,
};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

const DAYS: [&str; 5] = ["Mon", "Tue", "Wed", "Thu", "Fri"];

/// The pixels the baseline is from the top of the canvas.
const BASE: f32 = 130.0;

/// What the host holds across a reload.
#[derive(Serialize, Deserialize)]
struct Model {
    heights: [f32; 5],
    shuffles: u32,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            heights: [40.0, 72.0, 55.0, 96.0, 64.0],
            shuffles: 0,
        }
    }
}

struct Charts;

impl Plugin for Charts {
    fn entry() {
        hamlet_guest::panel("chart", "CHARTS", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut heard = false;
        for incoming in inbox {
            if let Incoming::Click(press) = incoming
                && press.id == "shuffle"
            {
                heard = true;
                model.shuffles += 1;
                let turn = model.shuffles as f32;
                for (at, height) in
                    model.heights.iter_mut().enumerate()
                {
                    *height = 30.0
                        + (turn * 37.0 + at as f32 * 53.0) % 80.0;
                }
            }
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        match heard {
            true => vec![Intent::Redraw("chart".into())],
            false => Vec::new(),
        }
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let mut shapes = vec![line(
            (10.0, BASE),
            (290.0, BASE),
            1.0,
            theme("seam"),
        )];
        let mut tops = Vec::new();
        for (at, (day, height)) in
            DAYS.iter().zip(model.heights).enumerate()
        {
            let x = 24.0 + at as f32 * 54.0;
            shapes.push(rect(
                x,
                BASE - height,
                30.0,
                height,
                theme("brass"),
            ));
            shapes.push(text(
                x + 4.0,
                BASE + 4.0,
                day,
                11.0,
                theme("dim"),
            ));
            tops.push((x + 15.0, BASE - height));
        }
        for pair in tops.windows(2) {
            shapes.push(line(pair[0], pair[1], 2.0, theme("lamp")));
        }
        for (x, y) in tops {
            shapes.push(circle(x, y, 4.0, theme("flame")));
        }
        column([
            heading("TURNS THIS WEEK"),
            canvas(300, 156, shapes),
            row([button("Shuffle").id("shuffle")]),
            note(&format!("Shuffled {} times", model.shuffles)),
        ])
        .into()
    }
}

hamlet_guest::export!(Charts);
