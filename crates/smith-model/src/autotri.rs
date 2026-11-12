//! SketchSmith — Auto-Tri: folding bent faces into triangles | v1.0.0 | Thursday 12 November 2026
//!
//! Faces stay flat. An operation that would bend faces is refused
//! ([`ModelError::NotFlat`]) — unless it runs inside [`with_auto_tri`],
//! which the canvas does only for the one operation the user has just
//! switched Auto-Tri on for. Then only the faces that would bend are folded
//! into triangles first (in their present, flat shape), and the operation is
//! tried again; every other face is left as it is. The folds' new edges are
//! soft and smooth, so the surface still draws and shades as one.
//!
//! The switch lives in a thread-local flag rather than in each operation's
//! arguments: it belongs to one run of one operation, never to the model,
//! and it is cleared again as soon as that run ends.

use std::cell::Cell;
use std::collections::HashSet;

use smith_geom::DVec2;
use smith_geom::triangulate::triangulate_polygon;

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::{FaceId, VertexId};

thread_local! {
    static AUTO_TRI: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with Auto-Tri on: a move that would bend faces folds them into
/// triangles instead of being refused. Off again afterwards, whatever `f`
/// returns.
pub fn with_auto_tri<R>(f: impl FnOnce() -> R) -> R {
    let before = AUTO_TRI.with(|a| a.replace(true));
    let out = f();
    AUTO_TRI.with(|a| a.set(before));
    out
}

/// Whether the current operation may fold bent faces (for operations
/// outside this crate that build new geometry, such as Sweep).
pub fn auto_tri() -> bool {
    AUTO_TRI.with(Cell::get)
}

impl Entities {
    /// Folds each of `faces` into triangles across its present plane: the
    /// diagonals a triangulation of the face (holes included) needs are
    /// drawn as soft, smooth edges, splitting the face.
    ///
    /// # Errors
    /// As [`Entities::add_line`].
    pub(crate) fn fold_faces(&mut self, faces: &[FaceId]) -> Result<(), ModelError> {
        // Work out every diagonal first, by vertex: drawing one splits the
        // face and changes its key.
        let mut diagonals: Vec<(VertexId, VertexId)> = Vec::new();
        for f in faces {
            let Some(face) = self.face(*f) else {
                continue;
            };
            let n = face.plane().normal();
            let u = n.any_orthonormal_vector();
            let w = n.cross(u);
            let loops: Vec<Vec<VertexId>> = face.loops().map(|l| self.loop_vertices(l)).collect();
            let Some((outer, holes)) = loops.split_first() else {
                continue;
            };
            let flat = |vs: &[VertexId]| -> Vec<DVec2> {
                vs.iter()
                    .map(|v| {
                        let p = self.vertices[*v].position();
                        DVec2::new(p.dot(u), p.dot(w))
                    })
                    .collect()
            };
            let all: Vec<VertexId> = loops.iter().flatten().copied().collect();
            let holes2: Vec<Vec<DVec2>> = holes.iter().map(|h| flat(h)).collect();
            for [a, b, c] in triangulate_polygon(&flat(outer), &holes2) {
                for (i, j) in [(a, b), (b, c), (c, a)] {
                    let (x, y) = (all[i], all[j]);
                    let key = (x.min(y), x.max(y));
                    if !diagonals.contains(&key) && !self.joined(x, y) {
                        diagonals.push(key);
                    }
                }
            }
        }
        let mut made: HashSet<crate::ids::EdgeId> = HashSet::new();
        for (a, b) in diagonals {
            let (pa, pb) = (self.vertices[a].position(), self.vertices[b].position());
            made.extend(self.add_line(pa, pb)?);
        }
        for e in made {
            if let Some(edge) = self.edges.get_mut(e) {
                edge.flags.soft = true;
                edge.flags.smooth = true;
            }
        }
        Ok(())
    }

    /// Marks `edges` soft and smooth: the folds Auto-Tri adds, hidden and
    /// shaded across.
    pub fn soften_edges(&mut self, edges: &[crate::ids::EdgeId]) {
        for e in edges {
            if let Some(edge) = self.edges.get_mut(*e) {
                edge.flags.soft = true;
                edge.flags.smooth = true;
            }
        }
    }

    /// Whether an edge joins `a` and `b`.
    fn joined(&self, a: VertexId, b: VertexId) -> bool {
        self.vertices[a]
            .edges()
            .iter()
            .any(|e| self.edges.get(*e).is_some_and(|x| x.other(a) == b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::DVec3;

    fn square() -> Entities {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        e
    }

    #[test]
    fn a_bending_move_is_refused_without_auto_tri_and_folds_with_it() {
        let mut e = square();
        let corner = e.find_vertex(DVec3::new(10.0, 10.0, 0.0)).unwrap();
        let lift = DVec3::new(0.0, 0.0, 3.0);
        assert!(matches!(
            e.move_vertices(&[corner], lift),
            Err(ModelError::NotFlat(_))
        ));
        assert_eq!(e.face_count(), 1);
        with_auto_tri(|| e.move_vertices(&[corner], lift)).unwrap();
        // Folded into two triangles across one soft, smooth diagonal.
        assert_eq!(e.face_count(), 2);
        assert_eq!(e.edge_count(), 5);
        let soft = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|y| y.flags.soft && y.flags.smooth))
            .count();
        assert_eq!(soft, 1);
        // Off again: the next bend is refused.
        assert!(!auto_tri());
    }

    #[test]
    fn faces_that_stay_flat_are_not_folded() {
        let mut e = square();
        let corner = e.find_vertex(DVec3::new(10.0, 10.0, 0.0)).unwrap();
        with_auto_tri(|| e.move_vertices(&[corner], DVec3::new(2.0, 1.0, 0.0))).unwrap();
        assert_eq!(e.face_count(), 1);
        assert_eq!(e.edge_count(), 4);
    }
}
