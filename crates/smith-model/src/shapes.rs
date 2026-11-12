//! SketchSmith — shape and sweep recipes | v1.0.0 | Thursday 12 November 2026
//!
//! The sizes a shape (box, cylinder, cone, sphere, torus) was made with,
//! and what a sweep, loft or revolve was made from.
//! A group made by the Shapes tool keeps its recipe on its definition, so
//! Edit Shape can rebuild it with new sizes; a group made by Sweep or
//! Revolve keeps its path, profile and options the same way, for Edit
//! Sweep. A recipe goes as soon as the group's geometry is edited by hand:
//! the group is then simply geometry. A sweep recipe holds its own copy of
//! the path and profile (in the group's own coordinates), never a link to
//! the geometry they were picked from.
//! Building the geometry from a recipe is `smith-ops`' job.

use smith_geom::DVec3;

/// A shape and its sizes (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeSpec {
    /// Width (X), depth (Y), height (Z).
    Box {
        /// X size.
        width: f64,
        /// Y size.
        depth: f64,
        /// Z size.
        height: f64,
    },
    /// Radius, height, sides.
    Cylinder {
        /// Radius.
        radius: f64,
        /// Height.
        height: f64,
        /// Sides (at least 3).
        sides: u32,
    },
    /// Base radius, height, sides.
    Cone {
        /// Base radius.
        radius: f64,
        /// Height.
        height: f64,
        /// Sides (at least 3).
        sides: u32,
    },
    /// Radius and segments round (half as many from pole to pole).
    Sphere {
        /// Radius.
        radius: f64,
        /// Segments round the equator (at least 4, even).
        segments: u32,
    },
    /// Ring radius, tube radius, segments round the ring and round the tube.
    Torus {
        /// From the centre to the middle of the tube.
        ring: f64,
        /// The tube's radius.
        tube: f64,
        /// Segments round the ring (at least 3).
        segments: u32,
        /// Sides round the tube (at least 3).
        sides: u32,
    },
}

impl ShapeSpec {
    /// The shape's name, used for its group.
    pub fn name(&self) -> &'static str {
        match self {
            ShapeSpec::Box { .. } => "Box",
            ShapeSpec::Cylinder { .. } => "Cylinder",
            ShapeSpec::Cone { .. } => "Cone",
            ShapeSpec::Sphere { .. } => "Sphere",
            ShapeSpec::Torus { .. } => "Torus",
        }
    }
}

/// The profile a sweep carries along its path.
#[derive(Debug, Clone, PartialEq)]
pub enum SweepSection {
    /// A picked face's outline: its outer loop's points, in order.
    Outline(Vec<DVec3>),
    /// A typed round tube: radius (mm) and sides.
    Round {
        /// Radius, mm.
        radius: f64,
        /// Sides (at least 3).
        sides: u32,
    },
    /// Bare edges (Extrude Edges along Path): a chain's points in order,
    /// open or closed; the result is a surface, with no caps.
    Edges {
        /// The chain's points.
        points: Vec<DVec3>,
        /// The chain closes on itself.
        closed: bool,
    },
    /// A typed rectangular tube, mm.
    Rect {
        /// Across the frame's first axis.
        width: f64,
        /// Across its second axis.
        height: f64,
    },
}

/// A loft's Operation Inputs.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LoftOptions {
    /// Points round every profile (at least the most any profile has):
    /// the others get points added along their edges, never moved.
    pub vertices: u32,
    /// Turns the pairing of each profile with the one before by this many
    /// points.
    pub start_shift: u32,
    /// Rings added between each pair of profiles: 0 blends straight, more
    /// blends curved, easing into each profile.
    pub segments: u32,
    /// Turn over the loft's length, radians.
    pub twist: f64,
    /// Edges Only: the rings and the lines along them, no faces (a cage).
    pub edges_only: bool,
}

/// What a Sweep or Revolve group was made from (points in the group's own
/// coordinates, mm; angles in radians).
#[derive(Debug, Clone, PartialEq)]
pub enum SweepRecipe {
    /// A profile carried along a path.
    Sweep {
        /// The path's points, start first.
        path: Vec<DVec3>,
        /// The path closes on itself.
        closed: bool,
        /// A second path (open, as the first): the profile is turned and
        /// scaled so that it keeps spanning the two.
        path2: Option<Vec<DVec3>>,
        /// The profile.
        section: SweepSection,
        /// The drawing axes' Z when it was made (a typed tube's first
        /// axis, and Upright's).
        up: DVec3,
        /// Profile scale at the path's end (1: none).
        taper: f64,
        /// Turn over the path's length.
        twist: f64,
        /// Upright.
        upright: bool,
        /// Align to Path.
        align: bool,
    },
    /// Profiles lofted along a path (Loft along Path) or along a smooth
    /// curve through them (Loft): the shape blended between them.
    Loft {
        /// The path's points, start first; `None` for Loft.
        path: Option<Vec<DVec3>>,
        /// The profiles' outlines: along a path, each where it lies across
        /// it; by spline, in the order they are lofted.
        profiles: Vec<Vec<DVec3>>,
        /// The drawing axes' Z when it was made.
        up: DVec3,
        /// Vertices, Start Shift, Segments and Twist.
        options: LoftOptions,
        /// A second path (with a first path): the profiles are turned and
        /// scaled so that they keep spanning the two.
        path2: Option<Vec<DVec3>>,
        /// Bare-edge profiles (Extrude Edges along Path), closed or not: a
        /// surface with no caps. `None` for face profiles.
        edges: Option<bool>,
        /// The path closes on itself: the loft runs through every profile
        /// and back into the first, with no caps.
        closed: bool,
        /// Each profile's own turn, radians, in `profiles` order (missing
        /// ones are 0).
        twists: Vec<f64>,
    },
    /// A face outline turned about an axis.
    Revolve {
        /// The profile's outline.
        outline: Vec<DVec3>,
        /// A point on the axis.
        axis_point: DVec3,
        /// The axis direction.
        axis_dir: DVec3,
        /// The angle turned (a full turn closes).
        angle: f64,
        /// Steps round.
        steps: u32,
    },
    /// A frame of lines skinned (Skinning): each cell of three or four
    /// sides covered.
    Skin {
        /// The frame's segments.
        segments: Vec<[DVec3; 2]>,
        /// Pieces each way across a cell.
        sampling: u32,
        /// The lines only, no faces.
        edges_only: bool,
    },
}

impl SweepRecipe {
    /// The name of its group.
    pub fn name(&self) -> &'static str {
        match self {
            SweepRecipe::Sweep { .. } => "Extrude",
            // Extrude Edges along Path, with an end profile.
            SweepRecipe::Loft { edges: Some(_), .. } => "Extrude",
            SweepRecipe::Loft { .. } => "Loft",
            SweepRecipe::Skin { .. } => "Skin",
            SweepRecipe::Revolve { .. } => "Revolve",
        }
    }

    /// The recipe with every point moved by `m` (directions turned only).
    pub fn transformed(&self, m: &smith_geom::DMat4) -> Self {
        let p = |x: &DVec3| m.transform_point3(*x);
        match self {
            SweepRecipe::Sweep {
                path,
                closed,
                path2,
                section,
                up,
                taper,
                twist,
                upright,
                align,
            } => SweepRecipe::Sweep {
                path: path.iter().map(p).collect(),
                closed: *closed,
                path2: path2.as_ref().map(|x| x.iter().map(p).collect()),
                section: match section {
                    SweepSection::Outline(pts) => {
                        SweepSection::Outline(pts.iter().map(p).collect())
                    }
                    SweepSection::Edges { points, closed } => SweepSection::Edges {
                        points: points.iter().map(p).collect(),
                        closed: *closed,
                    },
                    other => other.clone(),
                },
                up: m.transform_vector3(*up).try_normalize().unwrap_or(*up),
                taper: *taper,
                twist: *twist,
                upright: *upright,
                align: *align,
            },
            SweepRecipe::Loft {
                path,
                profiles,
                up,
                options,
                path2,
                edges,
                closed,
                twists,
            } => SweepRecipe::Loft {
                path: path.as_ref().map(|x| x.iter().map(p).collect()),
                profiles: profiles.iter().map(|o| o.iter().map(p).collect()).collect(),
                up: m.transform_vector3(*up).try_normalize().unwrap_or(*up),
                options: *options,
                path2: path2.as_ref().map(|x| x.iter().map(p).collect()),
                edges: *edges,
                closed: *closed,
                twists: twists.clone(),
            },
            SweepRecipe::Revolve {
                outline,
                axis_point,
                axis_dir,
                angle,
                steps,
            } => SweepRecipe::Revolve {
                outline: outline.iter().map(p).collect(),
                axis_point: p(axis_point),
                axis_dir: m.transform_vector3(*axis_dir),
                angle: *angle,
                steps: *steps,
            },
            SweepRecipe::Skin {
                segments,
                sampling,
                edges_only,
            } => SweepRecipe::Skin {
                segments: segments.iter().map(|[a, b]| [p(a), p(b)]).collect(),
                sampling: *sampling,
                edges_only: *edges_only,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DefinitionKind, Document};
    use smith_geom::DVec3;

    #[test]
    fn a_recipe_set_with_its_geometry_stays_until_a_hand_edit_and_undo_restores_it() {
        let mut doc = Document::new();
        let spec = ShapeSpec::Box {
            width: 1.0,
            depth: 2.0,
            height: 3.0,
        };
        let def = doc
            .transaction("Box", |d| {
                let def = d.create_definition("Box", DefinitionKind::Group);
                d.context_entities_mut(Some(def))
                    .ok_or(crate::ModelError::NoSuchEntity)?
                    .add_line(DVec3::ZERO, DVec3::X)?;
                d.set_definition_shape(def, Some(spec))?;
                Ok(def)
            })
            .unwrap();
        assert_eq!(doc.definition(def).unwrap().shape, Some(spec));
        doc.transaction("Line", |d| {
            d.context_entities_mut(Some(def))
                .ok_or(crate::ModelError::NoSuchEntity)?
                .add_line(DVec3::ZERO, DVec3::Y)
                .map(|_| ())
        })
        .unwrap();
        assert_eq!(doc.definition(def).unwrap().shape, None);
        assert!(doc.undo().is_some());
        assert_eq!(doc.definition(def).unwrap().shape, Some(spec));
    }

    #[test]
    fn a_sweep_recipe_goes_with_a_hand_edit_like_a_shape() {
        let mut doc = Document::new();
        let recipe = SweepRecipe::Sweep {
            path: vec![DVec3::ZERO, DVec3::X * 10.0],
            closed: false,
            path2: None,
            section: SweepSection::Round {
                radius: 1.0,
                sides: 8,
            },
            up: DVec3::Z,
            taper: 1.0,
            twist: 0.0,
            upright: false,
            align: false,
        };
        let def = doc
            .transaction("Sweep", |d| {
                let def = d.create_definition("Sweep", DefinitionKind::Group);
                d.context_entities_mut(Some(def))
                    .ok_or(crate::ModelError::NoSuchEntity)?
                    .add_line(DVec3::ZERO, DVec3::X)?;
                d.set_definition_sweep(def, Some(recipe.clone()))?;
                Ok(def)
            })
            .unwrap();
        assert_eq!(doc.definition(def).unwrap().sweep, Some(recipe.clone()));
        doc.transaction("Line", |d| {
            d.context_entities_mut(Some(def))
                .ok_or(crate::ModelError::NoSuchEntity)?
                .add_line(DVec3::ZERO, DVec3::Y)
                .map(|_| ())
        })
        .unwrap();
        assert_eq!(doc.definition(def).unwrap().sweep, None);
        assert!(doc.undo().is_some());
        assert_eq!(doc.definition(def).unwrap().sweep, Some(recipe));
    }

    #[test]
    fn a_recipe_moves_with_its_points_and_turns_its_directions() {
        let r = SweepRecipe::Revolve {
            outline: vec![DVec3::X, DVec3::X * 2.0, DVec3::new(2.0, 0.0, 1.0)],
            axis_point: DVec3::ZERO,
            axis_dir: DVec3::Z,
            angle: 1.0,
            steps: 4,
        };
        let m = smith_geom::DMat4::from_translation(DVec3::new(5.0, 0.0, 0.0));
        let SweepRecipe::Revolve {
            outline,
            axis_point,
            axis_dir,
            ..
        } = r.transformed(&m)
        else {
            panic!("not a revolve");
        };
        assert_eq!(outline[0], DVec3::new(6.0, 0.0, 0.0));
        assert_eq!(axis_point, DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(axis_dir, DVec3::Z);
    }
}
