//! SketchSmith — SKP model | v1.0.0 | Thursday 12 November 2026
//!
//! What a `.skp` holds, as [`crate::read`] hands it back: its layers and
//! materials, every component definition, and the root's entities. Each
//! definition and the root keep their own vertices, edges (with their
//! flags, layer and curve), faces (as loops of edges), the analytic arcs
//! their curves follow, the groups, components and images placed in them,
//! and their guides, section planes, texts and dimensions. Coordinates are
//! the file's own: inches, Z up. Ids are the objects' slots in the file and
//! tie the parts together (an edge's ends, a face's edges, an instance's
//! definition, a face's material or layer).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::archive::{Archive, Db, Entry, Item, Val};
use crate::walk::Walked;

/// A point or vector, in inches.
pub type Point = [f64; 3];

/// A `.skp` model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkpModel {
    /// The header's version (`{20.1.235}`).
    pub version: String,
    /// The layers, in file order.
    pub layers: Vec<Layer>,
    /// The materials, in file order.
    pub materials: Vec<Material>,
    /// Every component definition, in file order (groups and images have
    /// definitions too).
    pub definitions: Vec<Definition>,
    /// The model's own entities.
    pub root: Entities,
}

impl SkpModel {
    /// The definition with id `id`.
    pub fn definition(&self, id: usize) -> Option<&Definition> {
        self.definitions.iter().find(|d| d.id == id)
    }

    /// The layer with id `id`.
    pub fn layer(&self, id: usize) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }
}

/// A layer (a tag, in later SketchUp).
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Its id (its slot in the file).
    pub id: usize,
    /// Its name.
    pub name: String,
    /// Hidden in the file.
    pub hidden: bool,
    /// Its colour, RGBA.
    pub color: [u8; 4],
}

/// A material.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Its id (its slot in the file).
    pub id: usize,
    /// Its name.
    pub name: String,
    /// Its colour (a texture's average colour), RGBA.
    pub color: [u8; 4],
    /// 1 opaque, 0 clear.
    pub opacity: f64,
    /// Its image, if textured.
    pub texture: Option<Texture>,
}

/// A material's image.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// The image file's name as stored.
    pub file: String,
    /// The size one repeat is laid at, in inches.
    pub width: f64,
    /// The height of one repeat, in inches.
    pub height: f64,
    /// The image file's bytes (PNG or JPEG), when stored.
    pub data: Option<Vec<u8>>,
}

/// A component definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    /// Its id (its slot in the file).
    pub id: usize,
    /// Its name.
    pub name: String,
    /// Its GUID, as hex.
    pub guid: String,
    /// It is an Image entity's (placed by an image, not a component).
    pub image: bool,
    /// What it holds.
    pub entities: Entities,
}

/// A vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// Its id (its slot in the file).
    pub id: usize,
    /// Where it is.
    pub position: Point,
}

/// An edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    /// Its id (its slot in the file).
    pub id: usize,
    /// Its end vertices' ids.
    pub start: Option<usize>,
    /// Its end.
    pub end: Option<usize>,
    /// Soft: hidden between faces that read as one surface.
    pub soft: bool,
    /// Smooth: the faces either side are shaded as one.
    pub smooth: bool,
    /// Hidden in the file.
    pub hidden: bool,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
    /// The curve it belongs to, if any.
    pub curve: Option<usize>,
}

/// One edge of a face's loop, run backwards when `reversed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoEdge {
    /// The edge's id.
    pub edge: usize,
    /// Run from its end to its start.
    pub reversed: bool,
}

/// A face.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// Its id (its slot in the file).
    pub id: usize,
    /// The outer loop first, then the holes.
    pub loops: Vec<Vec<CoEdge>>,
    /// Its front's normal.
    pub normal: Point,
    /// Its front material's id.
    pub material: Option<usize>,
    /// Its back material's id.
    pub back_material: Option<usize>,
    /// Hidden in the file.
    pub hidden: bool,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
    /// A positioned texture's mapping on each side (row-major 3 × 3,
    /// texture space to the face's plane), when it has one.
    pub uv_front: Option<[f64; 9]>,
    /// The back's, likewise.
    pub uv_back: Option<[f64; 9]>,
}

/// An analytic arc a curve follows: `center + cos t · x_axis + sin t ·
/// y_axis` for `t` from `start` to `end` (a full circle when they are
/// equal). A stretched arc keeps axes of unequal length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcCurve {
    /// The centre.
    pub center: Point,
    /// The arc's point at angle zero, from the centre.
    pub x_axis: Point,
    /// The arc's point a quarter turn on, from the centre.
    pub y_axis: Point,
    /// Its start.
    pub start: f64,
    /// Its end.
    pub end: f64,
}

/// A group, component or image placed in a definition or the root.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The definition it places.
    pub definition: usize,
    /// A group (else a component copy or an image).
    pub group: bool,
    /// An Image entity.
    pub image: bool,
    /// Its name.
    pub name: String,
    /// Its placement: `[0..9]` the 3 × 3 part, row by row (a point maps to
    /// `(m0 x + m1 y + m2 z + m9, …)`), `[9..12]` the offset, `[12]` a
    /// uniform scale on both.
    pub transform: [f64; 13],
    /// Its front material's id.
    pub material: Option<usize>,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
    /// Hidden in the file.
    pub hidden: bool,
}

/// A guide line: through `point` along `direction`, from `start` to `end`
/// (`None`: unbounded that way).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuideLine {
    /// Its anchor point.
    pub point: Point,
    /// Its direction.
    pub direction: Point,
    /// Its start.
    pub start: Option<Point>,
    /// Its end.
    pub end: Option<Point>,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
}

/// A guide point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuidePoint {
    /// Where it is.
    pub position: Point,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
}

/// A section plane: `a x + b y + c z + d = 0`.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionPlane {
    /// Its plane.
    pub plane: [f64; 4],
    /// Its name.
    pub name: String,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
}

/// A text: a leader text on its anchor `point` (a point in space, the
/// vertex it is attached to, or a point along the edge it is attached to,
/// in its own context), its label `label` from there; or screen text (no
/// point, placed by `screen`). A leader whose anchor cannot be found (on a
/// face) has neither.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// Its text.
    pub text: String,
    /// Its anchor point.
    pub point: Option<Point>,
    /// Where its label sits, from the anchor.
    pub label: Option<Point>,
    /// Screen text's place, as the file stores it: two fractions of the
    /// view, across and down.
    pub screen: Option<[f64; 2]>,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
}

/// A linear dimension between `a` and `b` (when found), its line `offset`
/// (inches) out from them.
#[derive(Debug, Clone, PartialEq)]
pub struct Dimension {
    /// Its text.
    pub text: String,
    /// Its first end.
    pub a: Option<Point>,
    /// Its second end.
    pub b: Option<Point>,
    /// How far its line stands off.
    pub offset: f64,
    /// Its layer's id (`None`: the default layer).
    pub layer: Option<usize>,
}

/// The entities of a definition or the root.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entities {
    /// Its vertices.
    pub vertices: Vec<Vertex>,
    /// Its edges.
    pub edges: Vec<Edge>,
    /// Its faces.
    pub faces: Vec<Face>,
    /// The analytic arcs, by curve id.
    pub arcs: HashMap<usize, ArcCurve>,
    /// The groups, components and images placed in it.
    pub instances: Vec<Instance>,
    /// Its guide lines.
    pub guide_lines: Vec<GuideLine>,
    /// Its guide points.
    pub guide_points: Vec<GuidePoint>,
    /// Its section planes.
    pub section_planes: Vec<SectionPlane>,
    /// Its texts.
    pub texts: Vec<Text>,
    /// Its linear dimensions.
    pub dimensions: Vec<Dimension>,
}

impl Entities {
    /// The vertex with id `id`.
    pub fn vertex(&self, id: usize) -> Option<Point> {
        self.vertices
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.position)
    }
}

fn nonzero(v: u16) -> Option<usize> {
    (v != 0).then_some(usize::from(v))
}

/// Builds one entity list, as the original reader's builder does.
struct Builder<'s> {
    slots: &'s HashMap<usize, Entry>,
    out: Entities,
    edge_seen: HashSet<usize>,
    vertex_seen: HashSet<usize>,
}

impl<'s> Builder<'s> {
    fn new(slots: &'s HashMap<usize, Entry>) -> Self {
        Self {
            slots,
            out: Entities::default(),
            edge_seen: HashSet::new(),
            vertex_seen: HashSet::new(),
        }
    }

    fn value(&self, slot: usize) -> Option<&'s Rc<Val>> {
        match self.slots.get(&slot) {
            Some(Entry::Obj(_, v)) => v.as_ref(),
            _ => None,
        }
    }

    /// Where a text's leader points, in the text's own context: its own
    /// point (type 1), the vertex it is attached to (type 2), or a point
    /// along the edge it is attached to (type 5 at its place, type 4 at its
    /// middle), carried out through the groups on its path. `None` for
    /// screen text and anchors not found.
    fn text_anchor(
        &self,
        kind: u32,
        first: [f64; 3],
        target: Option<usize>,
        path: &[usize],
    ) -> Option<Point> {
        let vertex = |s: Option<usize>| match self.value(s?).map(|v| &**v) {
            Some(Val::Vertex(p)) => Some(*p),
            _ => None,
        };
        let mut p = match kind {
            0 => return None,
            1 => first,
            _ => match self.value(target?).map(|v| &**v)? {
                Val::Vertex(p) => *p,
                Val::Edge { v1, v2, .. } => {
                    let (a, b) = (vertex(*v1)?, vertex(*v2)?);
                    let t = if kind == 5 { first[0] } else { 0.5 };
                    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
                }
                _ => return None,
            },
        };
        // Innermost group first.
        for step in path.iter().rev() {
            if let Some(Val::Instance { xf, .. }) = self.value(*step).map(|v| &**v) {
                let s = if xf[12].is_finite() && xf[12] != 0.0 {
                    xf[12]
                } else {
                    1.0
                };
                p = [0, 1, 2].map(|i| {
                    s * (xf[3 * i] * p[0] + xf[3 * i + 1] * p[1] + xf[3 * i + 2] * p[2] + xf[9 + i])
                });
            }
        }
        p.iter().all(|c| c.is_finite()).then_some(p)
    }

    fn add_edge(&mut self, slot: usize, e: &Val) {
        let Val::Edge {
            db,
            v1,
            v2,
            curve,
            curve_value,
        } = e
        else {
            return;
        };
        if !self.edge_seen.insert(slot) {
            return;
        }
        for vs in [v1, v2].into_iter().flatten() {
            if let Some(v) = self.value(*vs)
                && let Val::Vertex(p) = &**v
                && self.vertex_seen.insert(*vs)
            {
                self.out.vertices.push(Vertex {
                    id: *vs,
                    position: *p,
                });
            }
        }
        self.out.edges.push(Edge {
            id: slot,
            start: *v1,
            end: *v2,
            soft: db.soft,
            smooth: db.smooth,
            hidden: db.hidden,
            layer: nonzero(db.layer),
            curve: *curve,
        });
        if let (Some(c), Some(v)) = (curve, curve_value)
            && let Val::ArcCurve(a) = &**v
        {
            self.out.arcs.insert(*c, arc(a));
        }
    }

    fn fill(&mut self, ents: &[Item]) {
        for it in ents {
            let (Some(s), Some(v)) = (it.slot, it.value.as_deref()) else {
                continue;
            };
            match v {
                Val::Edge { .. } => self.add_edge(s, v),
                Val::ArcCurve(a) => {
                    self.out.arcs.insert(s, arc(a));
                }
                Val::Face {
                    db,
                    normal,
                    loops,
                    back_mat,
                    attrs,
                } => self.add_face(s, *db, *normal, loops, *back_mat, attrs.as_deref()),
                Val::Instance {
                    db,
                    def: Some(def),
                    xf,
                    name,
                } => {
                    self.out.instances.push(Instance {
                        definition: *def,
                        group: it.is("CGroup"),
                        image: false,
                        name: name.clone(),
                        transform: *xf,
                        material: nonzero(db.mat),
                        layer: nonzero(db.layer),
                        hidden: db.hidden,
                    });
                }
                Val::Image { def: Some(def), xf } => {
                    let mut transform = [1.0; 13];
                    transform[..12].copy_from_slice(xf);
                    self.out.instances.push(Instance {
                        definition: *def,
                        group: false,
                        image: true,
                        name: String::new(),
                        transform,
                        material: None,
                        layer: None,
                        hidden: false,
                    });
                }
                Val::SectionPlane { db, plane, name } => {
                    self.out.section_planes.push(SectionPlane {
                        plane: *plane,
                        name: name.clone(),
                        layer: nonzero(db.layer),
                    });
                }
                Val::Text {
                    db,
                    text,
                    kind,
                    first,
                    target,
                    path,
                    offset,
                    screen,
                } => {
                    let point = self.text_anchor(*kind, *first, *target, path);
                    self.out.texts.push(Text {
                        text: text.clone(),
                        point,
                        label: *offset,
                        screen: *screen,
                        layer: nonzero(db.layer),
                    });
                }
                Val::Dimension {
                    db,
                    text,
                    connect,
                    offset,
                    free,
                } => {
                    // Free connections carry their points; anchored ones
                    // are their vertices'.
                    let end = |i: usize| {
                        free[i].or_else(|| {
                            connect[i]
                                .and_then(|c| self.value(c))
                                .and_then(|v| match &**v {
                                    Val::Vertex(p) => Some(*p),
                                    _ => None,
                                })
                        })
                    };
                    let (a, b) = (end(0), end(1));
                    self.out.dimensions.push(Dimension {
                        text: text.clone(),
                        a,
                        b,
                        offset: *offset,
                        layer: nonzero(db.layer),
                    });
                }
                Val::CLine {
                    db,
                    point,
                    direction,
                    start,
                    end,
                } => self.out.guide_lines.push(GuideLine {
                    point: *point,
                    direction: *direction,
                    start: *start,
                    end: *end,
                    layer: nonzero(db.layer),
                }),
                Val::CPoint { db, pos } => self.out.guide_points.push(GuidePoint {
                    position: *pos,
                    layer: nonzero(db.layer),
                }),
                _ => {}
            }
        }
    }

    fn add_face(
        &mut self,
        slot: usize,
        db: Db,
        normal: [f64; 3],
        loops: &[Option<Rc<Val>>],
        back_mat: u16,
        attrs: Option<&Val>,
    ) {
        let mut out_loops = Vec::new();
        for lp in loops.iter().flatten() {
            let Val::Loop(uses) = &**lp else {
                continue;
            };
            let mut l = Vec::new();
            for u in uses.iter().flatten() {
                let Val::EdgeUse {
                    edge: Some(es),
                    reversed,
                } = &**u
                else {
                    continue;
                };
                let Some(ev) = self.value(*es) else {
                    continue;
                };
                self.add_edge(*es, ev);
                l.push(CoEdge {
                    edge: *es,
                    reversed: *reversed,
                });
            }
            out_loops.push(l);
        }
        let (mut uv_front, mut uv_back) = (None, None);
        if let Some(Val::Attrs(children)) = attrs {
            for c in children.iter().flatten() {
                if let Val::Ftc { front, back } = &**c {
                    uv_front = Some(*front);
                    uv_back = Some(*back);
                }
            }
        }
        self.out.faces.push(Face {
            id: slot,
            loops: out_loops,
            normal,
            material: nonzero(db.mat),
            back_material: nonzero(back_mat),
            hidden: db.hidden,
            layer: nonzero(db.layer),
            uv_front,
            uv_back,
        });
    }
}

fn arc(a: &crate::archive::ArcFrame) -> ArcCurve {
    ArcCurve {
        center: a.center,
        x_axis: a.x_axis,
        y_axis: a.y_axis,
        start: a.start,
        end: a.end,
    }
}

/// The model from what the walk read.
pub(crate) fn build(data: &[u8], w: Walked<'_>) -> SkpModel {
    let Walked {
        ar,
        root,
        layers,
        materials,
    } = w;
    let Archive { slots, .. } = ar;
    let slots = &slots;

    // Definitions an image places are images'.
    let image_defs: HashSet<usize> = slots
        .values()
        .filter_map(|e| match e {
            Entry::Obj(_, Some(v)) => match &**v {
                Val::Image { def, .. } => *def,
                _ => None,
            },
            _ => None,
        })
        .collect();

    let materials = materials
        .iter()
        .filter_map(|(s, v)| match &**v {
            Val::Material {
                name,
                rgba,
                opacity,
                use_opacity,
                texture,
            } => {
                // The stored value is a transparency, used when flagged.
                let opacity = if *use_opacity {
                    (1.0 - opacity).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let texture = texture.as_ref().map(|t| {
                    let from_dib = t.dib.and_then(|d| match slots.get(&d) {
                        Some(Entry::Obj(_, Some(v))) => match &**v {
                            Val::Dib(bytes) => Some(bytes.to_vec()),
                            _ => None,
                        },
                        _ => None,
                    });
                    Texture {
                        file: t.file.clone(),
                        width: t.width,
                        height: t.height,
                        data: from_dib.or_else(|| t.data.as_ref().map(|d| d.to_vec())),
                    }
                });
                Some(Material {
                    id: *s,
                    name: name.clone(),
                    color: *rgba,
                    opacity,
                    texture,
                })
            }
            _ => None,
        })
        .collect();

    let layers = layers
        .iter()
        .filter_map(|(s, v)| match &**v {
            Val::Layer { name, hidden, rgba } => Some(Layer {
                id: *s,
                name: name.clone(),
                hidden: *hidden,
                color: *rgba,
            }),
            _ => None,
        })
        .collect();

    let mut def_slots: Vec<usize> = slots
        .iter()
        .filter_map(|(s, e)| match e {
            Entry::Obj(n, Some(_)) if &**n == "CComponentDefinition" => Some(*s),
            _ => None,
        })
        .collect();
    def_slots.sort_unstable();
    let definitions = def_slots
        .into_iter()
        .filter_map(|s| match slots.get(&s) {
            Some(Entry::Obj(_, Some(v))) => match &**v {
                Val::Definition { name, guid, ents } => {
                    let mut b = Builder::new(slots);
                    b.fill(ents);
                    Some(Definition {
                        id: s,
                        name: name.clone(),
                        guid: guid.clone(),
                        image: image_defs.contains(&s),
                        entities: b.out,
                    })
                }
                _ => None,
            },
            _ => None,
        })
        .collect();

    let mut b = Builder::new(slots);
    b.fill(&root);
    SkpModel {
        version: crate::walk::header_version_text(data),
        layers,
        materials,
        definitions,
        root: b.out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Db;

    fn obj(name: &str, v: Val) -> Entry {
        Entry::Obj(Rc::from(name), Some(Rc::new(v)))
    }

    #[test]
    fn a_label_anchors_on_its_vertex_along_its_edge_and_through_its_group() {
        let mut slots = HashMap::new();
        slots.insert(1, obj("CVertex", Val::Vertex([0.0, 0.0, 0.0])));
        slots.insert(2, obj("CVertex", Val::Vertex([10.0, 0.0, 0.0])));
        slots.insert(
            3,
            obj(
                "CEdge",
                Val::Edge {
                    db: Db::default(),
                    v1: Some(1),
                    v2: Some(2),
                    curve: None,
                    curve_value: None,
                },
            ),
        );
        // A group moved 5 along Y, scale 2.
        let mut xf = [0.0; 13];
        xf[0] = 1.0;
        xf[4] = 1.0;
        xf[8] = 1.0;
        xf[10] = 5.0;
        xf[12] = 2.0;
        slots.insert(
            4,
            obj(
                "CGroup",
                Val::Instance {
                    db: Db::default(),
                    def: None,
                    xf,
                    name: String::new(),
                },
            ),
        );
        let b = Builder::new(&slots);
        assert_eq!(
            b.text_anchor(2, [0.0; 3], Some(2), &[]),
            Some([10.0, 0.0, 0.0])
        );
        assert_eq!(
            b.text_anchor(5, [0.3, 0.0, 0.0], Some(3), &[]),
            Some([3.0, 0.0, 0.0])
        );
        assert_eq!(
            b.text_anchor(4, [0.0; 3], Some(3), &[]),
            Some([5.0, 0.0, 0.0])
        );
        assert_eq!(
            b.text_anchor(1, [1.0, 2.0, 3.0], None, &[]),
            Some([1.0, 2.0, 3.0])
        );
        assert_eq!(
            b.text_anchor(2, [0.0; 3], Some(2), &[4]),
            Some([20.0, 10.0, 0.0])
        );
        // Screen text, and an anchor not found.
        assert_eq!(b.text_anchor(0, [0.0; 3], None, &[]), None);
        assert_eq!(b.text_anchor(7, [0.0; 3], Some(99), &[]), None);
    }
}
