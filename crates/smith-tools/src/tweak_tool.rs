//! SketchSmith — Tweak tools (Space, Circle, Flatten, Smooth, Relax) | v1.0.0 | Thursday 12 November 2026
//!
//! Click an edge and the tool works on its whole connected chain (a loop
//! for Circle); or select first and press Enter (Flatten takes selected
//! faces, flattening them all onto one plane). Selected vertices with no
//! selected edges or faces are worked on directly, in the order they were
//! picked (a loop for Circle). Operation Inputs:
//!
//! * Space: Corner Angle, Influence.
//! * Circle: Radius (the fitted one until typed), Influence.
//! * Flatten: Plane (Best Fit, X, Y or Z), Influence.
//! * Smooth: Iterations, Corner Angle, Influence.
//! * Relax: Iterations, Corner Angle.
//!
//! Changing a value right after does it again (one undo step).

use smith_geom::DVec3;
use smith_model::{EdgeId, FaceId, Pickable, VertexId};
use smith_ops::draw::DrawOp;
use smith_ops::sweep::edge_chain;
use smith_ops::tweak::{FlatPlane, Tweak, TweakKind};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Flatten's planes, as offered.
const PLANES: &[&str] = &["Best Fit", "X", "Y", "Z"];

/// Space, Circle, Flatten, Smooth or Relax.
#[derive(Debug)]
pub struct TweakTool {
    /// The values (and, once picked, the points).
    tweak: Tweak,
    /// Edges selected before the tool started.
    edges: Vec<EdgeId>,
    /// Faces selected before the tool started (Flatten).
    faces: Vec<FaceId>,
    /// Vertices selected before the tool started, in pick order.
    vertices: Vec<VertexId>,
    /// Something was done and can still be redone.
    made: bool,
}

impl TweakTool {
    /// The tool for `kind`.
    pub fn new(kind: TweakKind) -> Self {
        Self {
            tweak: Tweak {
                kind,
                points: Vec::new(),
                closed: false,
                normal: None,
                // Smooth and Relax hold only real corners, so a rough
                // chain still moves; Space keeps its turns from 30°.
                corner: match kind {
                    TweakKind::Smooth | TweakKind::Relax => 90f64,
                    _ => 30f64,
                }
                .to_radians(),
                radius: None,
                plane: FlatPlane::BestFit,
                iterations: 1,
                influence: 1.0,
            },
            edges: Vec::new(),
            faces: Vec::new(),
            vertices: Vec::new(),
            made: false,
        }
    }

    /// Works on the chain through `edge`.
    fn chain(&mut self, edge: EdgeId, ctx: &Ctx<'_>) -> Outcome {
        let Some((points, closed)) = edge_chain(ctx.entities, edge) else {
            return Outcome::Nothing;
        };
        self.tweak.points = points;
        self.tweak.closed = closed;
        self.tweak.normal = None;
        self.made = true;
        Outcome::Commit(DrawOp::Tweak(self.tweak.clone()))
    }

    /// The selected vertices as the chain, in the order they were picked:
    /// a loop for Circle, an open run otherwise.
    fn vertices(&mut self, ctx: &Ctx<'_>) -> Outcome {
        let points: Vec<DVec3> = std::mem::take(&mut self.vertices)
            .into_iter()
            .filter_map(|v| ctx.entities.vertex(v).map(|x| x.position()))
            .collect();
        self.tweak.closed = self.tweak.kind == TweakKind::Circle;
        self.tweak.points = points;
        self.tweak.normal = None;
        self.made = true;
        Outcome::Commit(DrawOp::Tweak(self.tweak.clone()))
    }

    /// Flatten on the selected faces: all their points, and their
    /// area-weighted normal for the best fit.
    fn faces(&mut self, ctx: &Ctx<'_>) -> Outcome {
        let mut points: Vec<DVec3> = Vec::new();
        let mut normal = DVec3::ZERO;
        for f in std::mem::take(&mut self.faces) {
            let Some(face) = ctx.entities.face(f) else {
                continue;
            };
            let loop_pts = ctx.entities.loop_points(face.outer());
            let n = face.plane().normal();
            // Area by the outline's own winding round its normal.
            let area: f64 = (0..loop_pts.len())
                .map(|i| loop_pts[i].cross(loop_pts[(i + 1) % loop_pts.len()]).dot(n))
                .sum::<f64>()
                .abs()
                * 0.5;
            let n = if normal.dot(n) < 0.0 { -n } else { n };
            normal += n * area;
            for p in face.loops().flat_map(|l| ctx.entities.loop_points(l)) {
                if !points.iter().any(|q| q.distance(p) < 1e-9) {
                    points.push(p);
                }
            }
        }
        self.tweak.points = points;
        self.tweak.closed = false;
        self.tweak.normal = normal.try_normalize();
        self.made = true;
        Outcome::Commit(DrawOp::Tweak(self.tweak.clone()))
    }

    /// The last tweak again with the current values.
    fn redo(&mut self) -> Outcome {
        if !self.made {
            return Outcome::Nothing;
        }
        Outcome::Redefine(DrawOp::Tweak(self.tweak.clone()))
    }
}

impl Tool for TweakTool {
    fn name(&self) -> &'static str {
        match self.tweak.kind {
            TweakKind::Space => "Space",
            TweakKind::Circle => "Circle Loop",
            TweakKind::Flatten => "Flatten",
            TweakKind::Smooth => "Smooth",
            TweakKind::Relax => "Relax",
        }
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.edges.clear();
        self.faces.clear();
        self.vertices.clear();
        for p in items {
            match p {
                Pickable::Edge(e) => self.edges.push(*e),
                Pickable::Face(f) => self.faces.push(*f),
                Pickable::Vertex(v) => self.vertices.push(*v),
                _ => {}
            }
        }
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => match inf.entity {
                Some(Pickable::Edge(e)) => self.chain(e, ctx),
                _ => Outcome::Message("Click an Edge".into()),
            },
            Input::Finish => {
                if self.tweak.kind == TweakKind::Flatten && !self.faces.is_empty() {
                    return self.faces(ctx);
                }
                if self.edges.is_empty() && self.faces.is_empty() && self.vertices.len() > 1 {
                    return self.vertices(ctx);
                }
                match self.edges.first().copied() {
                    Some(e) => {
                        self.edges.clear();
                        self.faces.clear();
                        self.chain(e, ctx)
                    }
                    None => Outcome::Nothing,
                }
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.wants_finish();
        self.edges.clear();
        self.faces.clear();
        self.vertices.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn wants_finish(&self) -> bool {
        !self.edges.is_empty() || !self.faces.is_empty() || self.vertices.len() > 1
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: Vec::new(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        match (self.tweak.kind, self.wants_finish()) {
            (_, true) => "Enter Applies to the Selection".into(),
            (TweakKind::Circle, _) => "Click an Edge of the Loop".into(),
            (TweakKind::Flatten, _) => {
                "Select Faces and Press Enter, or Click an Edge of a Loop".into()
            }
            _ => "Click an Edge of the Chain".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let t = &self.tweak;
        let corner = Field::number("Corner Angle", Some(t.corner.to_degrees()), true);
        let influence = Field::number("Influence", Some(t.influence * 100.0), true);
        match t.kind {
            TweakKind::Space => vec![corner, influence],
            TweakKind::Circle => vec![Field::length("Radius", t.radius, false, true), influence],
            TweakKind::Flatten => {
                let index = match t.plane {
                    FlatPlane::BestFit => 0,
                    FlatPlane::X => 1,
                    FlatPlane::Y => 2,
                    FlatPlane::Z => 3,
                };
                vec![Field::choice("Plane", PLANES, index, true), influence]
            }
            TweakKind::Smooth => vec![
                Field::count("Iterations", t.iterations, true),
                corner,
                influence,
            ],
            TweakKind::Relax => vec![Field::count("Iterations", t.iterations, true), corner],
        }
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if !value.is_finite() {
            return Outcome::Nothing;
        }
        let label = match self.fields().get(index) {
            Some(f) => f.label,
            None => return Outcome::Nothing,
        };
        let t2 = &mut self.tweak;
        match label {
            "Corner Angle" => t2.corner = value.clamp(0.0, 180.0).to_radians(),
            "Influence" => t2.influence = (value / 100.0).clamp(0.0, 1.0),
            "Radius" if value > 0.0 => t2.radius = Some(value),
            "Iterations" => t2.iterations = as_count(value).clamp(1, 100),
            "Plane" => {
                t2.plane = match value as usize {
                    1 => FlatPlane::X,
                    2 => FlatPlane::Y,
                    3 => FlatPlane::Z,
                    _ => FlatPlane::BestFit,
                }
            }
            _ => return Outcome::Nothing,
        }
        self.redo()
    }

    fn accept(&mut self) {
        self.made = false;
    }

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {
        self.made = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn selected_vertices_are_taken_in_pick_order() {
        let mut m = Entities::new();
        for x in [0.0, 3.0, 7.0] {
            m.add_line(v(x, 0.0, 0.0), v(x, 1.0, 0.0)).unwrap();
        }
        let picks: Vec<VertexId> = [7.0, 0.0, 3.0]
            .iter()
            .map(|x| m.find_vertex(v(*x, 0.0, 0.0)).unwrap())
            .collect();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let items: Vec<Pickable> = picks.iter().map(|p| Pickable::Vertex(*p)).collect();
        let mut t = TweakTool::new(TweakKind::Circle);
        t.use_selection(&items);
        assert!(t.wants_finish());
        let Outcome::Commit(DrawOp::Tweak(run)) = t.handle(Input::Finish, &c) else {
            panic!("no tweak");
        };
        assert_eq!(
            run.points,
            [v(7.0, 0.0, 0.0), v(0.0, 0.0, 0.0), v(3.0, 0.0, 0.0)]
        );
        assert!(run.closed, "Circle makes a loop");
        // With an edge selected too, the edge's chain is used instead.
        let mut t = TweakTool::new(TweakKind::Space);
        let e = m.edge_ids().next().unwrap();
        t.use_selection(&[items[0], items[1], Pickable::Edge(e)]);
        let Outcome::Commit(DrawOp::Tweak(run)) = t.handle(Input::Finish, &c) else {
            panic!("no tweak");
        };
        assert_eq!(run.points.len(), 2);
    }

    #[test]
    fn a_click_takes_the_chain_and_a_value_redoes_it() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(5.0, 0.0, 0.0)],
            false,
        )
        .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = TweakTool::new(TweakKind::Space);
        let mut inf = at(v(0.5, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        let out = t.handle(Input::Click(inf), &c);
        let Outcome::Commit(DrawOp::Tweak(tw)) = out else {
            panic!("no tweak: {out:?}");
        };
        assert_eq!(tw.points.len(), 3);
        let out = t.edit(1, 50.0, &c);
        let Outcome::Redefine(DrawOp::Tweak(tw)) = out else {
            panic!("no redo: {out:?}");
        };
        assert!((tw.influence - 0.5).abs() < 1e-12);
    }

    #[test]
    fn flatten_offers_its_planes() {
        let t = TweakTool::new(TweakKind::Flatten);
        let f = t.fields();
        assert_eq!(f[0].label, "Plane");
        assert!(matches!(f[0].kind, crate::FieldKind::Choice(o) if o.len() == 4));
    }
}
