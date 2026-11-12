//! SketchSmith — vertex merge grid | v1.0.0 | Thursday 12 November 2026
//!
//! A uniform spatial hash used to find an existing vertex within `TOL_MERGE` of
//! a new position. Cells are 0.001 mm cubes, ten times `TOL_MERGE`: a match
//! can only be in the point's own cell or one of its 26 neighbours, and only
//! those are searched. Cells this small hold a vertex or two even in a
//! densely detailed scan (1 mm cells held over a thousand, and each lookup
//! searched thousands of vertices).

use std::collections::HashMap;

use smallvec::SmallVec;
use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE_SQ;

use crate::arena::Arena;
use crate::entities::Vertex;
use crate::ids::VertexId;

/// Cell edge length in mm.
const CELL: f64 = 0.001;

type Cell = [i64; 3];

#[derive(Debug, Clone, Default)]
pub(crate) struct VertexGrid {
    cells: HashMap<Cell, SmallVec<[VertexId; 4]>>,
}

fn cell_of(p: DVec3) -> Cell {
    let c = (p / CELL).floor();
    // Coordinates are finite (checked on entry); `as` saturates for huge values.
    [c.x as i64, c.y as i64, c.z as i64]
}

impl VertexGrid {
    pub(crate) fn insert(&mut self, id: VertexId, p: DVec3) {
        self.cells.entry(cell_of(p)).or_default().push(id);
    }

    pub(crate) fn remove(&mut self, id: VertexId, p: DVec3) {
        let key = cell_of(p);
        if let Some(list) = self.cells.get_mut(&key) {
            list.retain(|v| *v != id);
            if list.is_empty() {
                self.cells.remove(&key);
            }
        }
    }

    /// The nearest vertex within `TOL_MERGE` of `p`, if any.
    pub(crate) fn find(&self, p: DVec3, vertices: &Arena<VertexId, Vertex>) -> Option<VertexId> {
        let [cx, cy, cz] = cell_of(p);
        let mut best: Option<(f64, VertexId)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.cells.get(&[cx + dx, cy + dy, cz + dz]) else {
                        continue;
                    };
                    for id in list {
                        let d = vertices[*id].position().distance_squared(p);
                        if d <= TOL_MERGE_SQ && best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, *id));
                        }
                    }
                }
            }
        }
        best.map(|(_, id)| id)
    }
}
