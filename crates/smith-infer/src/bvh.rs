//! SketchSmith — picking BVHs | v1.0.0 | Thursday 12 November 2026
//!
//! A bounding volume hierarchy over every display triangle of a context
//!, so ray casts for picking and zoom-to-cursor cost a logarithmic
//! number of box tests instead of a scan of every face.
//!
//! Construction: top-down, splitting each node at the median centroid along the
//! longest axis of its centroid bounds; leaves hold up to [`LEAF_SIZE`]
//! triangles. Traversal: ray/box slab test, nearer child first, pruning boxes
//! farther than the best hit so far. All in `f64`.
//!
//! [`SegmentBvh`] does the same for edges (segments) and vertices (points),
//! so picking them near the cursor ray tests a few leaves instead of every
//! edge or vertex: each box is widened by the pick radius at its far side
//! before the slab test, so nothing within the radius is missed.

use smith_geom::DVec3;
use smith_model::{EdgeId, Entities, FaceId, VertexId};

use crate::pick::{RayHit, ray_triangle};

/// Maximum triangles per leaf.
pub const LEAF_SIZE: usize = 4;

#[derive(Debug, Clone, Copy)]
struct Tri {
    face: FaceId,
    pts: [DVec3; 3],
    centroid: DVec3,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    min: DVec3,
    max: DVec3,
    /// Leaf: index of first triangle. Interior: index of the left child.
    first: usize,
    /// Leaf: triangle count (> 0). Interior: 0; the right child is `first + 1`.
    count: usize,
}

/// BVH over a context's face triangles.
#[derive(Debug, Clone, Default)]
pub struct FaceBvh {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
}

impl FaceBvh {
    /// Builds the hierarchy for `entities`.
    pub fn build(entities: &Entities) -> Self {
        let mut tris: Vec<Tri> = entities
            .face_ids()
            .flat_map(|f| {
                entities.face_triangles(f).into_iter().map(move |pts| Tri {
                    face: f,
                    pts,
                    centroid: (pts[0] + pts[1] + pts[2]) / 3.0,
                })
            })
            .collect();
        let mut nodes = Vec::with_capacity(tris.len() * 2 / LEAF_SIZE + 1);
        if !tris.is_empty() {
            let n = tris.len();
            nodes.push(Node {
                min: DVec3::ZERO,
                max: DVec3::ZERO,
                first: 0,
                count: 0,
            });
            build_node(&mut tris, &mut nodes, 0, 0, n);
        }
        Self { tris, nodes }
    }

    /// Number of triangles indexed.
    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// Nearest face hit in front of `origin` along unit `dir`.
    pub fn ray_cast(&self, origin: DVec3, dir: DVec3) -> Option<RayHit> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = DVec3::ONE / dir;
        let mut best: Option<RayHit> = None;
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let node = self.nodes[i];
            let Some(t_box) = slab(origin, inv, node.min, node.max) else {
                continue;
            };
            if best.is_some_and(|b| t_box > b.t) {
                continue;
            }
            if node.count > 0 {
                for tri in &self.tris[node.first..node.first + node.count] {
                    if let Some(t) = ray_triangle(origin, dir, tri.pts)
                        && best.is_none_or(|b| t < b.t)
                    {
                        best = Some(RayHit {
                            t,
                            point: origin + dir * t,
                            face: tri.face,
                        });
                    }
                }
            } else {
                stack.push(node.first + 1);
                stack.push(node.first);
            }
        }
        best
    }
}

/// One segment (a point when both ends are the same) and its key.
#[derive(Debug, Clone, Copy)]
struct Seg<K> {
    a: DVec3,
    b: DVec3,
    key: K,
}

/// BVH over segments or points, each with a key (an edge or a vertex).
#[derive(Debug, Clone)]
pub struct SegmentBvh<K> {
    segs: Vec<Seg<K>>,
    nodes: Vec<Node>,
}

impl<K> Default for SegmentBvh<K> {
    fn default() -> Self {
        Self {
            segs: Vec::new(),
            nodes: Vec::new(),
        }
    }
}

impl SegmentBvh<EdgeId> {
    /// The edges of `entities`.
    pub fn edges(entities: &Entities) -> Self {
        Self::build(
            entities
                .edge_ids()
                .filter_map(|e| entities.edge_points(e).map(|(a, b)| Seg { a, b, key: e }))
                .collect(),
        )
    }
}

impl SegmentBvh<VertexId> {
    /// The vertices of `entities`.
    pub fn vertices(entities: &Entities) -> Self {
        Self::build(
            entities
                .vertex_ids()
                .filter_map(|v| {
                    entities.vertex(v).map(|x| Seg {
                        a: x.position(),
                        b: x.position(),
                        key: v,
                    })
                })
                .collect(),
        )
    }
}

impl<K: Copy> SegmentBvh<K> {
    fn build(mut segs: Vec<Seg<K>>) -> Self {
        let mut nodes = Vec::with_capacity(segs.len() * 2 / LEAF_SIZE + 1);
        if !segs.is_empty() {
            nodes.push(Node {
                min: DVec3::ZERO,
                max: DVec3::ZERO,
                first: 0,
                count: 0,
            });
            let n = segs.len();
            build_seg_node(&mut segs, &mut nodes, 0, 0, n);
        }
        Self { segs, nodes }
    }

    /// Number of segments indexed.
    pub fn len(&self) -> usize {
        self.segs.len()
    }

    /// Whether nothing is indexed.
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    /// Calls `visit` with every segment (ends and key) that may lie within
    /// `radius(t)` of the ray from `origin` along unit `dir` at a distance
    /// `t` up to `far` plus that radius. `radius` must not shrink as `t`
    /// grows. Others are never visited; the caller tests the exact distance.
    pub fn near_ray(
        &self,
        origin: DVec3,
        dir: DVec3,
        far: f64,
        radius: impl Fn(f64) -> f64,
        mut visit: impl FnMut(DVec3, DVec3, K),
    ) {
        if self.nodes.is_empty() {
            return;
        }
        let inv = DVec3::ONE / dir;
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let node = self.nodes[i];
            // The box's far side along the ray sets the widest radius it needs.
            let mut t_far: f64 = 0.0;
            for k in 0..8 {
                let c = DVec3::new(
                    if k & 1 == 0 { node.min.x } else { node.max.x },
                    if k & 2 == 0 { node.min.y } else { node.max.y },
                    if k & 4 == 0 { node.min.z } else { node.max.z },
                );
                t_far = t_far.max((c - origin).dot(dir));
            }
            let m = radius(t_far.max(0.0));
            let pad = DVec3::splat(m);
            let Some(t_box) = slab(origin, inv, node.min - pad, node.max + pad) else {
                continue;
            };
            if t_box > far + m {
                continue;
            }
            if node.count > 0 {
                for s in &self.segs[node.first..node.first + node.count] {
                    visit(s.a, s.b, s.key);
                }
            } else {
                stack.push(node.first + 1);
                stack.push(node.first);
            }
        }
    }
}

fn build_seg_node<K: Copy>(
    segs: &mut [Seg<K>],
    nodes: &mut Vec<Node>,
    index: usize,
    start: usize,
    end: usize,
) {
    let slice = &segs[start..end];
    let (mut min, mut max) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    let (mut cmin, mut cmax) = (min, max);
    for s in slice {
        min = min.min(s.a).min(s.b);
        max = max.max(s.a).max(s.b);
        let c = (s.a + s.b) * 0.5;
        cmin = cmin.min(c);
        cmax = cmax.max(c);
    }
    let count = end - start;
    if count <= LEAF_SIZE {
        nodes[index] = Node {
            min,
            max,
            first: start,
            count,
        };
        return;
    }
    let extent = cmax - cmin;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let mid = start + count / 2;
    segs[start..end].select_nth_unstable_by(count / 2, |x, y| {
        (x.a[axis] + x.b[axis]).total_cmp(&(y.a[axis] + y.b[axis]))
    });
    let left = nodes.len();
    let blank = Node {
        min: DVec3::ZERO,
        max: DVec3::ZERO,
        first: 0,
        count: 0,
    };
    nodes.push(blank);
    nodes.push(blank);
    nodes[index] = Node {
        min,
        max,
        first: left,
        count: 0,
    };
    build_seg_node(segs, nodes, left, start, mid);
    build_seg_node(segs, nodes, left + 1, mid, end);
}

fn build_node(tris: &mut [Tri], nodes: &mut Vec<Node>, index: usize, start: usize, end: usize) {
    let slice = &tris[start..end];
    let (mut min, mut max) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    let (mut cmin, mut cmax) = (min, max);
    for t in slice {
        for p in t.pts {
            min = min.min(p);
            max = max.max(p);
        }
        cmin = cmin.min(t.centroid);
        cmax = cmax.max(t.centroid);
    }
    let count = end - start;
    if count <= LEAF_SIZE {
        nodes[index] = Node {
            min,
            max,
            first: start,
            count,
        };
        return;
    }
    let extent = cmax - cmin;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let mid = start + count / 2;
    tris[start..end].select_nth_unstable_by(count / 2, |a, b| {
        a.centroid[axis].total_cmp(&b.centroid[axis])
    });
    let left = nodes.len();
    let blank = Node {
        min: DVec3::ZERO,
        max: DVec3::ZERO,
        first: 0,
        count: 0,
    };
    nodes.push(blank);
    nodes.push(blank);
    nodes[index] = Node {
        min,
        max,
        first: left,
        count: 0,
    };
    build_node(tris, nodes, left, start, mid);
    build_node(tris, nodes, left + 1, mid, end);
}

/// Ray/box slab test. Returns the entry distance (≥ 0) if the ray hits the box.
/// Axes where the ray direction is zero are handled explicitly, so a ray lying
/// exactly in a box face plane (common with flat, axis-aligned geometry) never
/// produces `0 × ∞ = NaN`.
fn slab(origin: DVec3, inv: DVec3, min: DVec3, max: DVec3) -> Option<f64> {
    let mut tmin = 0.0f64;
    let mut tmax = f64::INFINITY;
    for axis in 0..3 {
        let (o, i, lo, hi) = (origin[axis], inv[axis], min[axis], max[axis]);
        if i.is_infinite() {
            // Direction component is zero: the ray must already be within the slab.
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o) * i, (hi - o) * i);
        tmin = tmin.max(a.min(b));
        tmax = tmax.min(a.max(b));
        if tmax < tmin {
            return None;
        }
    }
    Some(tmin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pick::ray_cast_faces;

    #[test]
    fn segment_index_finds_everything_a_scan_finds() {
        use smith_geom::intersect::closest_points_segments;
        let mut m = Entities::new();
        for i in 0..20 {
            for j in 0..20 {
                let (x, y, z) = (
                    f64::from(i) * 3.0,
                    f64::from(j) * 3.0,
                    f64::from((i * j) % 5),
                );
                let v = |a: f64, b: f64| DVec3::new(x + a, y + b, z);
                m.add_polyline(&[v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0)], false)
                    .unwrap();
            }
        }
        let edges = SegmentBvh::edges(&m);
        let vertices = SegmentBvh::vertices(&m);
        assert_eq!(edges.len(), m.edge_count());
        // A radius growing with distance, as in perspective.
        let radius = |t: f64| 0.05 + 0.01 * t;
        for k in 0..200 {
            let o = DVec3::new(f64::from(k % 61), f64::from(k * 7 % 59), 30.0);
            let d = DVec3::new(0.05 * f64::from(k % 5) - 0.1, 0.03, -1.0).normalize();
            let end = o + d * 1e6;
            let near = |a: DVec3, b: DVec3| {
                let (s, _, p, q) = closest_points_segments(o, end, a, b);
                p.distance(q) <= radius(s * 1e6)
            };
            let mut want: Vec<EdgeId> = m
                .edge_ids()
                .filter(|e| m.edge_points(*e).is_some_and(|(a, b)| near(a, b)))
                .collect();
            let mut got = Vec::new();
            edges.near_ray(o, d, 1e9, radius, |a, b, e| {
                if near(a, b) {
                    got.push(e);
                }
            });
            want.sort();
            got.sort();
            assert_eq!(got, want, "edges, ray {k}");
            let mut want: Vec<VertexId> = m
                .vertex_ids()
                .filter(|v| {
                    m.vertex(*v)
                        .is_some_and(|x| near(x.position(), x.position()))
                })
                .collect();
            let mut got = Vec::new();
            vertices.near_ray(o, d, 1e9, radius, |a, b, v| {
                if near(a, b) {
                    got.push(v);
                }
            });
            want.sort();
            got.sort();
            assert_eq!(got, want, "vertices, ray {k}");
        }
    }

    #[test]
    fn bvh_matches_linear_scan() {
        let mut m = Entities::new();
        for i in 0..20 {
            for j in 0..20 {
                let (x, y, z) = (
                    f64::from(i) * 3.0,
                    f64::from(j) * 3.0,
                    f64::from((i + j) % 3),
                );
                let v = |a: f64, b: f64| DVec3::new(x + a, y + b, z);
                m.add_polyline(&[v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)], true)
                    .unwrap();
            }
        }
        let bvh = FaceBvh::build(&m);
        assert_eq!(bvh.triangle_count(), 800);
        for k in 0..200 {
            let o = DVec3::new(f64::from(k % 61), f64::from(k * 7 % 59), 10.0);
            let d = DVec3::new(0.05 * f64::from(k % 5) - 0.1, 0.03, -1.0).normalize();
            let a = bvh.ray_cast(o, d).map(|h| (h.face, (h.t * 1e9).round()));
            let b = ray_cast_faces(&m, o, d).map(|h| (h.face, (h.t * 1e9).round()));
            assert_eq!(a, b, "ray {k}");
        }
    }

    #[test]
    fn empty_model() {
        assert!(
            FaceBvh::build(&Entities::new())
                .ray_cast(DVec3::ZERO, DVec3::Z)
                .is_none()
        );
    }
}
