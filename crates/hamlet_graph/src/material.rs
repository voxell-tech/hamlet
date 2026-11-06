//! Materials: a use of a shader, with values for some of its graph
//! inputs and no graph of its own, so many materials come from one
//! shader. An input the material does not set takes the shader's
//! default; a value the shader has no input for, or one of another
//! type, is an Error until it is removed or the shader has the input
//! again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::value::{Constant, ValueType};

/// A shader by name and values for some of its graph inputs, by the
/// inputs' names.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Material {
    pub shader: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Constant>,
}

/// A value of a material that its shader does not take.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum MaterialError {
    #[error("{0} is not an input of the shader")]
    NoInput(String),
    #[error("{name} takes {expected:?}, not {found:?}")]
    WrongType {
        name: String,
        expected: ValueType,
        found: ValueType,
    },
}

impl MaterialError {
    /// The name of the value it is about.
    pub fn name(&self) -> &str {
        match self {
            MaterialError::NoInput(name)
            | MaterialError::WrongType { name, .. } => name,
        }
    }
}

impl Material {
    /// A material of the shader `shader` that sets nothing.
    pub fn of(shader: &str) -> Self {
        Self {
            shader: shader.to_owned(),
            values: BTreeMap::new(),
        }
    }

    /// What the input `name` of `shader` is in this material: its own
    /// value, else the shader's default. Nothing for an input the
    /// shader does not have, or one with no default that is not set.
    pub fn value(&self, shader: &Graph, name: &str) -> Option<Constant> {
        let input =
            shader.inputs.iter().find(|input| input.name == name)?;
        match self.values.get(name) {
            Some(value) if fits(value, input.value_type) => {
                Some(value.clone())
            }
            _ => input.default.clone(),
        }
    }

    /// `shader` with each graph input's default what this material
    /// makes it, so the shader compiled from it shades as the material.
    pub fn apply(&self, shader: &Graph) -> Graph {
        let mut applied = shader.clone();
        for input in &mut applied.inputs {
            if let Some(value) = self.value(shader, &input.name) {
                input.default = Some(value);
            }
        }
        applied
    }

    /// Each value that `shader` takes no input for, or one of another
    /// type, in the order of their names.
    pub fn errors(&self, shader: &Graph) -> Vec<MaterialError> {
        self.values
            .iter()
            .filter_map(|(name, value)| {
                let Some(input) =
                    shader.inputs.iter().find(|input| input.name == *name)
                else {
                    return Some(MaterialError::NoInput(name.clone()));
                };
                (!fits(value, input.value_type)).then(|| {
                    MaterialError::WrongType {
                        name: name.clone(),
                        expected: input.value_type,
                        found: value.value_type(),
                    }
                })
            })
            .collect()
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        crate::graph::to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

/// Whether `value` can be what an input of `value_type` is: one of its
/// type, or a point for a vector, as a graph's file writes a vector.
pub fn fits(value: &Constant, value_type: ValueType) -> bool {
    value.value_type() == value_type
        || matches!(
            (value, value_type),
            (Constant::Point(_), ValueType::Vector2 | ValueType::Vector3)
        )
}

#[cfg(test)]
mod tests {
    use bevy_math::Vec4;

    use super::*;
    use crate::graph::GraphInput;

    fn shader() -> Graph {
        let input = |name: &str, value_type, default| GraphInput {
            name: name.to_owned(),
            value_type,
            default,
            vary: None,
        };
        Graph {
            inputs: vec![
                input(
                    "tint",
                    ValueType::Color,
                    Some(Constant::Color(Vec4::ONE)),
                ),
                input(
                    "roughness",
                    ValueType::Number,
                    Some(Constant::Number(0.5)),
                ),
            ],
            ..Graph::default()
        }
    }

    #[test]
    fn an_unset_input_takes_the_shader_default_and_a_stale_value_is_an_error()
     {
        let mut material = Material::of("plaster");
        material
            .values
            .insert("roughness".to_owned(), Constant::Number(0.9));
        material.values.insert("gone".to_owned(), Constant::Bool(true));
        material
            .values
            .insert("tint".to_owned(), Constant::Number(1.0));
        let shader = shader();

        assert_eq!(
            material.value(&shader, "roughness"),
            Some(Constant::Number(0.9))
        );
        // A value of another type is not used: the default is.
        assert_eq!(
            material.value(&shader, "tint"),
            Some(Constant::Color(Vec4::ONE))
        );
        assert_eq!(material.value(&shader, "gone"), None);
        assert_eq!(
            material.errors(&shader),
            [
                MaterialError::NoInput("gone".to_owned()),
                MaterialError::WrongType {
                    name: "tint".to_owned(),
                    expected: ValueType::Color,
                    found: ValueType::Number,
                },
            ]
        );
    }

    #[test]
    fn a_material_reads_back_as_it_was_written() {
        let mut material = Material::of("plaster");
        material
            .values
            .insert("tint".to_owned(), Constant::Color(Vec4::ONE));
        let text = material.to_ron().unwrap();
        assert_eq!(Material::from_ron(&text).unwrap(), material);
    }
}
