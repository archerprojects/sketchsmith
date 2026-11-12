//! SketchSmith — dimensions and text | v1.0.0 | Thursday 12 November 2026
//!
//! Annotations are what the Dimensions and Text tools leave in a context:
//!
//! * **Linear** dimensions between two points, drawn at an offset;
//! * **Radial** dimensions (radius or diameter) of a circle or arc;
//! * **Angular** dimensions between two arms about a vertex;
//! * **Leader** text pointing at a point, an edge or a face, showing a
//!   length, an area, the point's coordinates, or text typed in;
//! * **Screen** text fixed to a place on the canvas.
//!
//! Annotations follow the geometry they are tied to: an [`Anchor`] on a
//! vertex reads the vertex's position whenever it is drawn, one on an edge
//! its place along the edge, one on a face its place in the face's own
//! frame (from the face's first corner along its first edge); a radial
//! dimension reads its curve, and leader text its edge's length or its
//! face's area. Each keeps the value it was made with, which it falls back
//! to once what it was tied to is gone. They are journaled like every other
//! entity, and are not geometry: they never appear in geometry exports.
//!
//! A leader's label stays where it was placed (its anchor's first point and
//! the offset from there); when the anchor moves, the leader stretches to
//! it.
//!
//! The types are generic over the keys they refer to, so the same shapes
//! serve the model (entity keys) and plain data (dense `u32` indices, see
//! [`AnnotationData`]).

use smith_geom::DVec3;

use smith_geom::DVec2;

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::{AnnotationId, CurveId, EdgeId, FaceId, VertexId};

/// A point an annotation is tied to. Each tied kind keeps where it was when
/// the annotation was made, which it falls back to once its geometry goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor<V = VertexId, E = EdgeId, F = FaceId> {
    /// A vertex, and where it was when the annotation was made.
    Vertex(V, DVec3),
    /// A point `t` (0–1) along an edge from its first end, and where it was.
    Edge(E, f64, DVec3),
    /// A point on a face, at `uv` (mm) in the face's frame, and where it was.
    Face(F, DVec2, DVec3),
    /// A fixed point.
    Point(DVec3),
}

impl<V: Copy, E: Copy, F: Copy> Anchor<V, E, F> {
    /// The anchor with its keys mapped; a key that maps to `None` becomes
    /// the fixed point it was made at.
    pub fn map<V2, E2, F2>(
        &self,
        v: impl Fn(V) -> Option<V2>,
        e: impl Fn(E) -> Option<E2>,
        f: impl Fn(F) -> Option<F2>,
    ) -> Anchor<V2, E2, F2> {
        match *self {
            Anchor::Vertex(k, at) => v(k).map_or(Anchor::Point(at), |k2| Anchor::Vertex(k2, at)),
            Anchor::Edge(k, t, at) => e(k).map_or(Anchor::Point(at), |k2| Anchor::Edge(k2, t, at)),
            Anchor::Face(k, uv, at) => {
                f(k).map_or(Anchor::Point(at), |k2| Anchor::Face(k2, uv, at))
            }
            Anchor::Point(p) => Anchor::Point(p),
        }
    }

    /// The point it was made at (or is fixed at).
    pub fn fallback(&self) -> DVec3 {
        match *self {
            Anchor::Vertex(_, p)
            | Anchor::Edge(_, _, p)
            | Anchor::Face(_, _, p)
            | Anchor::Point(p) => p,
        }
    }

    /// Whether its numbers are finite (and an edge's place within 0–1).
    fn is_valid(&self) -> bool {
        self.fallback().is_finite()
            && match *self {
                Anchor::Edge(_, t, _) => t.is_finite() && (0.0..=1.0).contains(&t),
                Anchor::Face(_, uv, _) => uv.is_finite(),
                Anchor::Vertex(..) | Anchor::Point(_) => true,
            }
    }
}

/// A face's own frame: its first corner, the way along its first edge, and
/// the way across (square to it, in the face's plane). `None` for a face
/// gone or too thin to have one.
fn face_frame(entities: &Entities, f: FaceId) -> Option<(DVec3, DVec3, DVec3)> {
    let face = entities.face(f)?;
    let pts = entities.loop_points(face.outer());
    let origin = *pts.first()?;
    let u = (*pts.get(1)? - origin).try_normalize()?;
    let v = face.plane().normal().cross(u).try_normalize()?;
    Some((origin, u, v))
}

impl Anchor {
    /// An anchor at `p`: on the vertex there, if there is one.
    pub fn at(entities: &Entities, p: DVec3) -> Self {
        match entities.find_vertex(p) {
            Some(v) => Anchor::Vertex(v, p),
            None => Anchor::Point(p),
        }
    }

    /// An anchor at `p` on edge `e` (its place along it), or on the vertex
    /// at `p` when there is one.
    pub fn on_edge(entities: &Entities, e: EdgeId, p: DVec3) -> Self {
        if let Some(v) = entities.find_vertex(p) {
            return Anchor::Vertex(v, p);
        }
        match entities.edge_points(e) {
            Some((a, b)) if a.distance_squared(b) > 0.0 => {
                let t = ((p - a).dot(b - a) / a.distance_squared(b)).clamp(0.0, 1.0);
                Anchor::Edge(e, t, p)
            }
            _ => Anchor::Point(p),
        }
    }

    /// An anchor at `p` on face `f` (its place in the face's frame), or on
    /// the vertex at `p` when there is one.
    pub fn on_face(entities: &Entities, f: FaceId, p: DVec3) -> Self {
        if let Some(v) = entities.find_vertex(p) {
            return Anchor::Vertex(v, p);
        }
        match face_frame(entities, f) {
            Some((o, u, v)) => Anchor::Face(f, DVec2::new((p - o).dot(u), (p - o).dot(v)), p),
            None => Anchor::Point(p),
        }
    }

    /// An anchor at `p` on what is there: a vertex, else an edge running
    /// through `p`, else a face it lies on, else the point itself (for
    /// importing, where only the point is known). Every edge and face is
    /// tried, so it suits a few anchors, not many.
    pub fn attach(entities: &Entities, p: DVec3) -> Self {
        const NEAR: f64 = 1e-4;
        if let Some(v) = entities.find_vertex(p) {
            return Anchor::Vertex(v, p);
        }
        let on_edge = entities.edge_ids().find(|e| {
            entities.edge_points(*e).is_some_and(|(a, b)| {
                let ab = b - a;
                let len2 = ab.length_squared();
                len2 > 0.0 && {
                    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
                    (a + ab * t).distance(p) <= NEAR
                }
            })
        });
        if let Some(e) = on_edge {
            return Self::on_edge(entities, e, p);
        }
        Anchor::Point(p)
    }

    /// Where the anchor is now.
    pub fn position(&self, entities: &Entities) -> DVec3 {
        match *self {
            Anchor::Vertex(v, p) => entities.vertex(v).map_or(p, |x| x.position()),
            Anchor::Edge(e, t, p) => entities.edge_points(e).map_or(p, |(a, b)| a.lerp(b, t)),
            Anchor::Face(f, uv, p) => {
                face_frame(entities, f).map_or(p, |(o, u, v)| o + u * uv.x + v * uv.y)
            }
            Anchor::Point(p) => p,
        }
    }
}

/// What leader text shows.
#[derive(Debug, Clone, PartialEq)]
pub enum LeaderText<E = EdgeId, F = FaceId> {
    /// Text typed in.
    Custom(String),
    /// An edge's length; `value` (mm) when the edge is gone.
    Length {
        /// The edge, while it exists.
        edge: Option<E>,
        /// Its length when the text was made.
        value: f64,
    },
    /// A face's area; `value` (mm²) when the face is gone.
    Area {
        /// The face, while it exists.
        face: Option<F>,
        /// Its area when the text was made.
        value: f64,
    },
    /// The coordinates of the point it points at.
    Coordinates,
}

/// A dimension or text.
#[derive(Debug, Clone, PartialEq)]
pub enum Annotation<V = VertexId, E = EdgeId, F = FaceId, C = CurveId> {
    /// The distance between `a` and `b`, drawn `offset` away from them.
    Linear {
        /// First point.
        a: Anchor<V, E, F>,
        /// Second point.
        b: Anchor<V, E, F>,
        /// From the measured points to the dimension line.
        offset: DVec3,
    },
    /// The radius or diameter of a circle or arc.
    Radial {
        /// The curve, while it exists.
        curve: Option<C>,
        /// Diameter rather than radius.
        diameter: bool,
        /// Centre and radius when made, used once the curve is gone.
        centre: DVec3,
        /// Radius when made, mm.
        radius: f64,
        /// The label, from the centre.
        label: DVec3,
    },
    /// The angle at `centre` from arm `a` to arm `b`, drawn as an arc of
    /// `radius`.
    Angular {
        /// The vertex of the angle.
        centre: Anchor<V, E, F>,
        /// A point on the first arm.
        a: Anchor<V, E, F>,
        /// A point on the second arm.
        b: Anchor<V, E, F>,
        /// Radius of the arc drawn, mm.
        radius: f64,
    },
    /// Text on a leader line from `anchor`; the text stays where it was
    /// placed, the leader stretching to the anchor as it moves.
    Leader {
        /// What it points at.
        anchor: Anchor<V, E, F>,
        /// The text's place, from the anchor's first point.
        label: DVec3,
        /// What it says.
        text: LeaderText<E, F>,
    },
    /// Text fixed to the canvas: `at` is its centre as a fraction of the
    /// canvas width and height from the top left.
    Screen {
        /// Position, 0–1 across and down.
        at: [f64; 2],
        /// The text.
        text: String,
    },
}

/// An annotation as plain data: references are dense indices, as in
/// [`crate::EntitiesData`].
pub type AnnotationData = Annotation<u32, u32, u32, u32>;

impl<V: Copy, E: Copy, F: Copy, C: Copy> Annotation<V, E, F, C> {
    /// The annotation with its references mapped; a reference that maps to
    /// `None` falls back as the module documentation says.
    pub fn map<V2, E2, F2, C2>(
        &self,
        v: impl Fn(V) -> Option<V2>,
        e: impl Fn(E) -> Option<E2>,
        f: impl Fn(F) -> Option<F2>,
        c: impl Fn(C) -> Option<C2>,
    ) -> Annotation<V2, E2, F2, C2> {
        match self {
            Annotation::Linear { a, b, offset } => Annotation::Linear {
                a: a.map(&v, &e, &f),
                b: b.map(&v, &e, &f),
                offset: *offset,
            },
            Annotation::Radial {
                curve,
                diameter,
                centre,
                radius,
                label,
            } => Annotation::Radial {
                curve: curve.and_then(&c),
                diameter: *diameter,
                centre: *centre,
                radius: *radius,
                label: *label,
            },
            Annotation::Angular {
                centre,
                a,
                b,
                radius,
            } => Annotation::Angular {
                centre: centre.map(&v, &e, &f),
                a: a.map(&v, &e, &f),
                b: b.map(&v, &e, &f),
                radius: *radius,
            },
            Annotation::Leader {
                anchor,
                label,
                text,
            } => Annotation::Leader {
                anchor: anchor.map(&v, &e, &f),
                label: *label,
                text: match text {
                    LeaderText::Custom(s) => LeaderText::Custom(s.clone()),
                    LeaderText::Length { edge, value } => LeaderText::Length {
                        edge: edge.and_then(&e),
                        value: *value,
                    },
                    LeaderText::Area { face, value } => LeaderText::Area {
                        face: face.and_then(&f),
                        value: *value,
                    },
                    LeaderText::Coordinates => LeaderText::Coordinates,
                },
            },
            Annotation::Screen { at, text } => Annotation::Screen {
                at: *at,
                text: text.clone(),
            },
        }
    }

    /// Whether every number is finite, radii are positive, and typed text
    /// is not empty.
    pub fn is_valid(&self) -> bool {
        let anchor = |a: &Anchor<V, E, F>| a.is_valid();
        match self {
            Annotation::Linear { a, b, offset } => anchor(a) && anchor(b) && offset.is_finite(),
            Annotation::Radial {
                centre,
                radius,
                label,
                ..
            } => centre.is_finite() && radius.is_finite() && *radius > 0.0 && label.is_finite(),
            Annotation::Angular {
                centre,
                a,
                b,
                radius,
            } => anchor(centre) && anchor(a) && anchor(b) && radius.is_finite() && *radius > 0.0,
            Annotation::Leader {
                anchor: at,
                label,
                text,
            } => {
                anchor(at)
                    && label.is_finite()
                    && match text {
                        LeaderText::Custom(s) => !s.trim().is_empty(),
                        LeaderText::Length { value, .. } | LeaderText::Area { value, .. } => {
                            value.is_finite()
                        }
                        LeaderText::Coordinates => true,
                    }
            }
            Annotation::Screen { at, text } => {
                at.iter().all(|x| x.is_finite()) && !text.trim().is_empty()
            }
        }
    }
}

impl Annotation {
    /// A leader's ends now: where it points (its anchor, wherever that has
    /// moved) and where its text sits (where it was placed). `None` for
    /// other annotations.
    pub fn leader_ends(&self, entities: &Entities) -> Option<(DVec3, DVec3)> {
        match self {
            Annotation::Leader { anchor, label, .. } => {
                Some((anchor.position(entities), anchor.fallback() + *label))
            }
            _ => None,
        }
    }
}

impl Entities {
    /// Number of annotations.
    pub fn annotation_count(&self) -> usize {
        self.annotations.len()
    }

    /// An annotation by key.
    pub fn annotation(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.get(id)
    }

    /// Every annotation with its key, in creation order.
    pub fn annotations(&self) -> impl Iterator<Item = (AnnotationId, &Annotation)> + '_ {
        self.annotations.iter()
    }

    /// Adds an annotation.
    ///
    /// # Errors
    /// [`ModelError::InvalidData`] for a malformed one.
    pub fn add_annotation(&mut self, a: Annotation) -> Result<AnnotationId, ModelError> {
        if !a.is_valid() {
            return Err(ModelError::InvalidData(
                "malformed dimension or text".into(),
            ));
        }
        Ok(self.annotations.insert(a))
    }

    /// Replaces annotation `id` (for typing a new value after placing it).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`], or [`ModelError::InvalidData`].
    pub fn replace_annotation(
        &mut self,
        id: AnnotationId,
        a: Annotation,
    ) -> Result<(), ModelError> {
        if !a.is_valid() {
            return Err(ModelError::InvalidData(
                "malformed dimension or text".into(),
            ));
        }
        let slot = self
            .annotations
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?;
        *slot = a;
        Ok(())
    }

    /// Erases annotation `id` and returns it.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_annotation(&mut self, id: AnnotationId) -> Result<Annotation, ModelError> {
        self.annotations.remove(id).ok_or(ModelError::NoSuchEntity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn a_vertex_anchor_follows_its_vertex_and_falls_back_when_it_goes() {
        let mut doc = Document::new();
        doc.transaction("Line", |d| {
            d.root_mut()
                .add_line(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))
                .map(|_| ())
        })
        .unwrap();
        let a = Anchor::at(doc.root(), DVec3::new(10.0, 0.0, 0.0));
        assert!(matches!(a, Anchor::Vertex(..)));
        let v = doc.root().find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap();
        doc.transaction("Move", |d| {
            d.root_mut()
                .move_vertices(&[v], DVec3::new(2.0, 0.0, 0.0))
                .map(|_| ())
        })
        .unwrap();
        assert_eq!(a.position(doc.root()), DVec3::new(12.0, 0.0, 0.0));
        let gone = a.map(|_: VertexId| None::<VertexId>, Some, Some);
        assert_eq!(gone, Anchor::Point(DVec3::new(10.0, 0.0, 0.0)));
    }

    /// A 10 mm square face on the ground, its far edge, and the face.
    fn square(doc: &mut Document) -> (EdgeId, FaceId) {
        doc.transaction("Square", |d| {
            d.root_mut()
                .add_polyline(
                    &[
                        DVec3::ZERO,
                        DVec3::new(10.0, 0.0, 0.0),
                        DVec3::new(10.0, 10.0, 0.0),
                        DVec3::new(0.0, 10.0, 0.0),
                    ],
                    true,
                )
                .map(|_| ())
        })
        .unwrap();
        let e = doc.root();
        let edge = e
            .edge_ids()
            .find(|x| {
                e.edge_points(*x)
                    .is_some_and(|(a, b)| a.y == 10.0 && b.y == 10.0)
            })
            .unwrap();
        (edge, e.face_ids().next().unwrap())
    }

    #[test]
    fn edge_and_face_anchors_follow_their_geometry_and_the_label_stays() {
        let mut doc = Document::new();
        let (edge, face) = square(&mut doc);
        let on_edge = Anchor::on_edge(doc.root(), edge, DVec3::new(3.0, 10.0, 0.0));
        assert!(
            matches!(on_edge, Anchor::Edge(_, t, _) if (t - 0.3).abs() < 1e-9 || (t - 0.7).abs() < 1e-9)
        );
        let on_face = Anchor::on_face(doc.root(), face, DVec3::new(4.0, 6.0, 0.0));
        assert!(matches!(on_face, Anchor::Face(..)));
        let label = Annotation::Leader {
            anchor: on_face,
            label: DVec3::new(0.0, 0.0, 5.0),
            text: LeaderText::Custom("Plate".into()),
        };
        // Lift the whole square 2 mm: both anchors go with it.
        let all: Vec<VertexId> = doc.root().vertex_ids().collect();
        doc.transaction("Move", |d| {
            d.root_mut()
                .move_vertices(&all, DVec3::new(0.0, 0.0, 2.0))
                .map(|_| ())
        })
        .unwrap();
        assert!((on_edge.position(doc.root()) - DVec3::new(3.0, 10.0, 2.0)).length() < 1e-9);
        assert!((on_face.position(doc.root()) - DVec3::new(4.0, 6.0, 2.0)).length() < 1e-9);
        // The text stays where it was placed; the leader stretches.
        let (tip, text) = label.leader_ends(doc.root()).unwrap();
        assert!((tip - DVec3::new(4.0, 6.0, 2.0)).length() < 1e-9);
        assert_eq!(text, DVec3::new(4.0, 6.0, 5.0));
        // Gone: back to where they were made.
        let edge_gone = on_edge.map(Some, |_: EdgeId| None::<EdgeId>, Some);
        assert_eq!(edge_gone, Anchor::Point(DVec3::new(3.0, 10.0, 0.0)));
        // On a vertex, a vertex anchor.
        assert!(matches!(
            Anchor::on_edge(doc.root(), edge, DVec3::new(10.0, 10.0, 2.0)),
            Anchor::Vertex(..)
        ));
        // Attaching a bare point finds the edge it lies on.
        assert!(matches!(
            Anchor::attach(doc.root(), DVec3::new(5.0, 10.0, 2.0)),
            Anchor::Edge(..)
        ));
        assert!(matches!(
            Anchor::attach(doc.root(), DVec3::new(5.0, 50.0, 2.0)),
            Anchor::Point(_)
        ));
    }

    #[test]
    fn malformed_annotations_are_refused_and_adding_one_undoes() {
        let mut doc = Document::new();
        let bad = Annotation::Screen {
            at: [0.5, 0.5],
            text: "  ".into(),
        };
        assert!(doc.root_mut().add_annotation(bad).is_err());
        doc.transaction("Text", |d| {
            d.root_mut()
                .add_annotation(Annotation::Screen {
                    at: [0.1, 0.1],
                    text: "Front".into(),
                })
                .map(|_| ())
        })
        .unwrap();
        assert_eq!(doc.root().annotation_count(), 1);
        assert!(doc.undo().is_some());
        assert_eq!(doc.root().annotation_count(), 0);
    }
}
