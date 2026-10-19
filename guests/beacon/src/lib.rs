//! A plugin that builds a small beacon beside the first hamlet: a
//! plinth and a pillar of Hamlet's own shapes, a spire of a mesh it
//! sends, and an orb lit by a shader of its own. A button sends a
//! shader the host refuses, to show why. What came of each is a line
//! of its panel.

use hamlet_guest::render::{
    self, Finish, Node, Shape, glowing, hamlet, node, point_light,
    shaded, surface,
};
use hamlet_guest::ui::{View, button, column, heading, note};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

const PANEL: &str = "main";

/// The hamlet the beacon stands beside.
const HAMLET: &str = "home";

/// The orb's shader.
const GLOW: &str = include_str!("glow.wgsl");

/// A shader whose loop has no count, which the host refuses.
const RUNAWAY: &str = include_str!("runaway.wgsl");

/// The spire: a pyramid on a square of 0.5 metres, 0.6 high.
const SPIRE: [[f32; 3]; 5] = [
    [-0.25, 0.0, -0.25],
    [0.25, 0.0, -0.25],
    [0.25, 0.0, 0.25],
    [-0.25, 0.0, 0.25],
    [0.0, 0.6, 0.0],
];
const SPIRE_TRIANGLES: [u32; 18] =
    [0, 4, 1, 1, 4, 2, 2, 4, 3, 3, 4, 0, 0, 1, 2, 0, 2, 3];

/// What the plugin keeps between calls: what came of each thing it
/// sent.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    shader: String,
    mesh: String,
    scene: String,
    runaway: String,
}

struct Beacon;

impl Plugin for Beacon {
    fn entry() {
        hamlet_guest::panel(PANEL, "BEACON", "sun");
        let mut model = State::<Model>::load();
        raise(&mut model);
        save(&model);
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        for incoming in inbox {
            match incoming {
                // What was refused before is sent again.
                Incoming::Granted(_) => raise(&mut model),
                Incoming::Click(press) if press.id == "runaway" => {
                    model.runaway = said(
                        "The runaway shader",
                        render::shader("runaway", RUNAWAY),
                    );
                }
                _ => {}
            }
        }
        save(&model);
        vec![Intent::Redraw(PANEL.into())]
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let mut rows = vec![heading("BEACON")];
        for (id, line) in [
            ("shader", &model.shader),
            ("mesh", &model.mesh),
            ("scene", &model.scene),
            ("runaway-line", &model.runaway),
        ] {
            if !line.is_empty() {
                rows.push(note(line).id(id));
            }
        }
        rows.push(
            button("Send a shader that never ends").id("runaway"),
        );
        column(rows).into()
    }
}

/// Sends the shader, the spire and the scene, and keeps what came of
/// each.
fn raise(model: &mut Model) {
    model.shader = said("The shader", render::shader("glow", GLOW));
    model.mesh = said(
        "The spire",
        render::mesh("spire", &SPIRE, &[], &SPIRE_TRIANGLES),
    );
    model.scene =
        said("The beacon", render::build(hamlet(HAMLET), beacon()));
}

/// The beacon, three metres east of the common.
fn beacon() -> Vec<Node> {
    let stone = surface(Finish::Solid, 0x8a8478ff);
    let brass = render::Material::Surface(render::Surface {
        finish: Finish::Solid,
        colour: 0xc8a050ff,
        glow: 0.0,
        roughness: 0.35,
        metallic: 0.8,
    });
    let light = shaded(
        "glow",
        &[
            [1.0, 0.85, 0.45, 0.0],
            [0.9, 0.35, 0.15, 0.0],
            [2.0, 0.0, 0.0, 0.0],
        ],
    );
    vec![
        node(1).at(3.0, 0.0, 0.0),
        node(2)
            .under(1)
            .at(0.0, 0.15, 0.0)
            .scaled(1.2, 0.3, 1.2)
            .shaped(Shape::Cube, stone.clone()),
        node(3)
            .under(1)
            .at(0.0, 1.1, 0.0)
            .scaled(0.35, 1.6, 0.35)
            .shaped(Shape::Cylinder, stone),
        node(4)
            .under(1)
            .at(0.0, 2.3, 0.0)
            .scaled(0.7, 0.7, 0.7)
            .shaped(Shape::Sphere, light)
            .lit(point_light(0xffd080, 6000.0, 8.0)),
        node(5)
            .under(1)
            .at(0.0, 2.3, 0.0)
            .scaled(1.1, 1.1, 1.1)
            .shaped(Shape::Torus, brass),
        node(6).under(1).at(0.0, 2.7, 0.0).shaped(
            Shape::Custom("spire".into()),
            glowing(0xffc860ff, 2.0),
        ),
    ]
}

/// A line of what came of sending `what`.
fn said(what: &str, done: Result<(), String>) -> String {
    match done {
        Ok(()) => format!("{what} was taken."),
        Err(why) => format!("{what} was refused: {why}"),
    }
}

fn save(model: &Model) {
    if let Err(why) = State::save(model) {
        hamlet_guest::log(Level::Warn, &why);
    }
}

hamlet_guest::export!(Beacon);
