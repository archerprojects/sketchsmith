//! SketchSmith — document as plain data | v1.0.0 | Thursday 12 November 2026
//!
//! [`DocumentData`] is the whole model as plain values linked by *index*
//! instead of by key: the form the native file format (`smith-io`)
//! stores. [`Document::to_data`] and [`Document::from_data`] convert in both
//! directions exactly:
//!
//! * Entities are listed in key order, so index `i` is the `i`-th entity by
//!   creation. Reading inserts them in that order into fresh arenas, which
//!   issue keys `1..=n`; a document read back therefore has dense keys, and a
//!   second save of it is byte-identical to the first.
//! * Every list the model keeps — a vertex's edges, an edge's radial faces, a
//!   face's loops, a curve's edges — is stored in its own order, not
//!   recomputed, so behaviour that depends on order is unchanged by a save.
//! * Positions, planes and curve parameters are stored as `f64` and restored
//!   bit for bit (`Plane::from_stored`, `CurveFrame::from_stored`).
//!
//! Reading checks every index, then the linear-time model invariants
//! ([`Entities::validate_links`]); a damaged file is refused with
//! [`ModelError::InvalidData`] rather than producing a broken model.
//!
//! Dimensions and text refer to vertices, edges, faces and curves by those
//! same indices; one that refers past the end refuses the data.
//!
//! Each context's guides are listed in creation order; a malformed one
//! (non-finite, or a line without a unit direction) refuses the data.
//!
//! Layers are listed Base first; an edge, face or instance names its layer by
//! index, 0 being Base. Data with no layers (an older file) gets Base.
//!
//! Materials are listed in creation order and named by index from face paint;
//! only materials some face side is painted with are listed.
//! Each context lists only the wraps its faces use, named by index from face
//! paint; a wrap no face uses any more is left out.
//!
//! The undo history is not part of the data: a document read from a file
//! starts with an empty history.

use std::collections::BTreeMap;

use smallvec::SmallVec;
use smith_geom::curve::Curve;
use smith_geom::plane::Plane;
use smith_geom::{DMat4, DVec3};

use crate::annotations::{Anchor, Annotation, AnnotationData, LeaderText};
use crate::arena::Arena;
use crate::curves::CurveRecord;
use crate::document::{Definition, DefinitionKind, Document, Instance, Layer, ModelLook};
use crate::entities::{Edge, EdgeFlags, EdgeUse, Entities, Face, FaceAttributes, Vertex};
use crate::error::ModelError;
use crate::guides::Guide;
use crate::ids::{
    AnnotationId, CurveId, DefinitionId, EdgeId, FaceId, GuideId, InstanceId, Key, LayerId,
    MaterialId, SectionId, VertexId, WrapId,
};
use crate::look::LineStyle;
use crate::paint::{Mapping, Material, Paint, TextureFrame, Wrap};
use crate::scenes::{Scene, SceneData};
use crate::sections::SectionPlane;
use crate::shapes::{ShapeSpec, SweepRecipe};

/// One edge within a face loop, by index: the edge and whether it is walked
/// end → start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopUse {
    /// Index into [`EntitiesData::edges`].
    pub edge: u32,
    /// `true`: walked end → start.
    pub reversed: bool,
}

/// An edge, by index.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeData {
    /// Start and end vertex indices.
    pub vertices: [u32; 2],
    /// The radial face list, in model order.
    pub faces: Vec<u32>,
    /// The curve the edge belongs to, if any.
    pub curve: Option<u32>,
    /// Display flags (their `layer` is unused here: see `layer`).
    pub flags: EdgeFlags,
    /// Index into [`DocumentData::layers`]; 0 is Base.
    pub layer: u32,
}

/// Where a painted face side's texture lies, by index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MappingData {
    /// A flat frame of its own.
    Face(TextureFrame),
    /// Index into [`EntitiesData::wraps`].
    Wrap(u32),
}

/// A face side's paint, by index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintData {
    /// Index into [`DocumentData::materials`].
    pub material: u32,
    /// Where its texture lies.
    pub mapping: MappingData,
}

/// A face, by index.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceData {
    /// Outer loop (counter-clockwise about the normal).
    pub outer: Vec<LoopUse>,
    /// Inner loops (holes, clockwise about the normal).
    pub inner: Vec<Vec<LoopUse>>,
    /// Unit normal of the face plane.
    pub normal: DVec3,
    /// Plane constant `d` in `normal · p + d = 0`.
    pub d: f64,
    /// Index into [`DocumentData::layers`]; 0 is Base.
    pub layer: u32,
    /// Hidden by the user.
    pub hidden: bool,
    /// Front paint.
    pub front: Option<PaintData>,
    /// Back paint.
    pub back: Option<PaintData>,
}

/// A curve and its edges, by index.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// The analytic curve.
    pub curve: Curve,
    /// Segment count it was generated with.
    pub segments: u32,
    /// Its edges, in order.
    pub edges: Vec<u32>,
}

/// A placed group or component, by index.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceData {
    /// Index into [`DocumentData::definitions`].
    pub definition: u32,
    /// Placement: definition space → containing context.
    pub transform: DMat4,
    /// Instance name (may be empty).
    pub name: String,
    /// Index into [`DocumentData::layers`]; 0 is Base.
    pub layer: u32,
    /// Locked against editing.
    pub locked: bool,
    /// Hidden by the user.
    pub hidden: bool,
}

/// A layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerData {
    /// Display name.
    pub name: String,
    /// The number it was created with (Base is 0).
    pub number: u32,
    /// Display colour (sRGB).
    pub color: [u8; 3],
    /// Shown in the canvas.
    pub visible: bool,
    /// Present-mode line style of its edges.
    pub style: Option<LineStyle>,
    /// Locked.
    pub locked: bool,
}

/// The geometry of one context, by index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntitiesData {
    /// Vertex positions in mm.
    pub positions: Vec<DVec3>,
    /// Per vertex, the edges meeting at it, in model order.
    pub vertex_edges: Vec<Vec<u32>>,
    /// Edges.
    pub edges: Vec<EdgeData>,
    /// Faces.
    pub faces: Vec<FaceData>,
    /// Curves.
    pub curves: Vec<CurveData>,
    /// Placed groups and components.
    pub instances: Vec<InstanceData>,
    /// Guide lines and guide points, in creation order.
    pub guides: Vec<Guide>,
    /// Dimensions and text, in creation order.
    pub annotations: Vec<AnnotationData>,
    /// Section planes, in creation order (at most one active).
    pub sections: Vec<SectionPlane>,
    /// The wraps its faces are painted with.
    pub wraps: Vec<Wrap>,
}

/// A definition (group or component) and its geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionData {
    /// Display name.
    pub name: String,
    /// Group or component.
    pub kind: DefinitionKind,
    /// The shape it was made as, while not edited by hand.
    pub shape: Option<ShapeSpec>,
    /// What it was made from (Sweep, Revolve), while not edited by hand.
    pub sweep: Option<SweepRecipe>,
    /// Its geometry.
    pub entities: EntitiesData,
}

/// The whole document as plain, index-linked data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentData {
    /// The root context.
    pub root: EntitiesData,
    /// Definitions, in key order.
    pub definitions: Vec<DefinitionData>,
    /// Layers, Base first.
    pub layers: Vec<LayerData>,
    /// The layer new geometry goes on, as an index into `layers`; 0 is Base.
    pub active_layer: u32,
    /// Scenes, in the order they were made.
    pub scenes: Vec<SceneData>,
    /// Materials, in the order they were made.
    pub materials: Vec<Material>,
    /// Present-mode settings saved with the model.
    pub look: ModelLook,
}

// ----- writing ---------------------------------------------------------------

/// Key → index maps for one context.
struct IndexMaps {
    vertices: BTreeMap<VertexId, u32>,
    edges: BTreeMap<EdgeId, u32>,
    faces: BTreeMap<FaceId, u32>,
    curves: BTreeMap<CurveId, u32>,
}

/// Index of the `n`-th item (models are far below `u32::MAX` entities).
fn index(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn dense<K: Key>(keys: impl Iterator<Item = K>) -> BTreeMap<K, u32> {
    keys.enumerate().map(|(i, k)| (k, index(i))).collect()
}

impl Entities {
    fn to_data(
        &self,
        definitions: &BTreeMap<DefinitionId, u32>,
        layers: &BTreeMap<LayerId, u32>,
        materials: &BTreeMap<MaterialId, u32>,
    ) -> EntitiesData {
        let layer = |l: Option<LayerId>| l.and_then(|k| layers.get(&k).copied()).unwrap_or(0);
        let wraps = self.used_wraps();
        let paint = |p: Option<&Paint>| -> Option<PaintData> {
            let p = p?;
            Some(PaintData {
                material: *materials.get(&p.material)?,
                mapping: match p.mapping {
                    Mapping::Face(frame) => MappingData::Face(frame),
                    Mapping::Wrap(w) => MappingData::Wrap(*wraps.get(&w)?),
                },
            })
        };
        let maps = IndexMaps {
            vertices: dense(self.vertices.keys()),
            edges: dense(self.edges.keys()),
            faces: dense(self.faces.keys()),
            curves: dense(self.curves.keys()),
        };
        let uses = |l: &[EdgeUse]| -> Vec<LoopUse> {
            l.iter()
                .map(|u| LoopUse {
                    edge: maps.edges[&u.edge],
                    reversed: u.reversed,
                })
                .collect()
        };
        EntitiesData {
            positions: self.vertices.iter().map(|(_, v)| v.position()).collect(),
            vertex_edges: self
                .vertices
                .iter()
                .map(|(_, v)| v.edges().iter().map(|e| maps.edges[e]).collect())
                .collect(),
            edges: self
                .edges
                .iter()
                .map(|(_, e)| EdgeData {
                    vertices: e.vertices().map(|v| maps.vertices[&v]),
                    faces: e.faces().iter().map(|f| maps.faces[f]).collect(),
                    curve: e.curve.map(|c| maps.curves[&c]),
                    flags: EdgeFlags {
                        layer: None,
                        ..e.flags
                    },
                    layer: layer(e.flags.layer),
                })
                .collect(),
            faces: self
                .faces
                .iter()
                .map(|(_, f)| FaceData {
                    outer: uses(f.outer()),
                    inner: f.inner().iter().map(|l| uses(l)).collect(),
                    normal: f.plane().normal(),
                    d: f.plane().d(),
                    layer: layer(f.attributes.layer),
                    hidden: f.attributes.hidden,
                    front: paint(f.attributes.front.as_ref()),
                    back: paint(f.attributes.back.as_ref()),
                })
                .collect(),
            curves: self
                .curves
                .iter()
                .map(|(_, c)| CurveData {
                    curve: c.curve.clone(),
                    segments: c.segments,
                    edges: c.edges.iter().map(|e| maps.edges[e]).collect(),
                })
                .collect(),
            instances: self
                .instances
                .iter()
                .map(|(_, i)| InstanceData {
                    definition: definitions[&i.definition],
                    transform: i.transform,
                    name: i.name.clone(),
                    layer: layer(i.layer),
                    locked: i.locked,
                    hidden: i.hidden,
                })
                .collect(),
            guides: self.guides.iter().map(|(_, g)| *g).collect(),
            sections: self.sections.iter().map(|(_, s)| *s).collect(),
            wraps: wraps
                .keys()
                .filter_map(|w| self.wraps.get(*w).copied())
                .collect(),
            annotations: self
                .annotations
                .iter()
                .map(|(_, a)| {
                    a.map(
                        |v| maps.vertices.get(&v).copied(),
                        |e| maps.edges.get(&e).copied(),
                        |f| maps.faces.get(&f).copied(),
                        |c| maps.curves.get(&c).copied(),
                    )
                })
                .collect(),
        }
    }
}

// ----- reading ---------------------------------------------------------------

fn bad(what: impl Into<String>) -> ModelError {
    ModelError::InvalidData(what.into())
}

/// Checks `i < len` and turns it into the key the fresh arena will issue.
fn key<K: Key>(i: u32, len: usize, what: &str) -> Result<K, ModelError> {
    if (i as usize) < len {
        Ok(K::from_raw(u64::from(i) + 1))
    } else {
        Err(bad(format!("{what} index {i} out of range ({len})")))
    }
}

/// Whether annotation `a` refers to a vertex, edge, face or curve index
/// past the given counts.
fn references_out_of_range(a: &AnnotationData, nv: usize, ne: usize, nf: usize, nc: usize) -> bool {
    let past = |i: u32, n: usize| usize::try_from(i).map_or(true, |x| x >= n);
    let anchor = |x: &Anchor<u32, u32, u32>| match *x {
        Anchor::Vertex(v, _) => past(v, nv),
        Anchor::Edge(e, ..) => past(e, ne),
        Anchor::Face(f, ..) => past(f, nf),
        Anchor::Point(_) => false,
    };
    match a {
        Annotation::Linear { a, b, .. } => anchor(a) || anchor(b),
        Annotation::Radial { curve, .. } => curve.is_some_and(|c| past(c, nc)),
        Annotation::Angular { centre, a, b, .. } => anchor(centre) || anchor(a) || anchor(b),
        Annotation::Leader {
            anchor: at, text, ..
        } => {
            anchor(at)
                || match text {
                    LeaderText::Length { edge, .. } => edge.is_some_and(|e| past(e, ne)),
                    LeaderText::Area { face, .. } => face.is_some_and(|f| past(f, nf)),
                    _ => false,
                }
        }
        Annotation::Screen { .. } => false,
    }
}

/// Inserts `items` into a fresh arena, checking each gets the predicted key.
fn fill<K: Key, T: Clone + PartialEq>(items: Vec<T>) -> Arena<K, T> {
    let mut arena = Arena::default();
    for (i, item) in items.into_iter().enumerate() {
        let k = arena.insert(item);
        debug_assert_eq!(k, K::from_raw(i as u64 + 1));
    }
    arena
}

impl EntitiesData {
    fn into_entities(
        self,
        definition_count: usize,
        layer_count: usize,
        material_count: usize,
    ) -> Result<Entities, ModelError> {
        if self.wraps.iter().any(|w| !w.is_valid()) {
            return Err(bad("malformed wrap"));
        }
        let nw = self.wraps.len();
        let paint = |p: Option<PaintData>| -> Result<Option<Paint>, ModelError> {
            let Some(p) = p else { return Ok(None) };
            Ok(Some(Paint {
                material: key::<MaterialId>(p.material, material_count, "material")?,
                mapping: match p.mapping {
                    MappingData::Face(frame) if frame.is_valid() => Mapping::Face(frame),
                    MappingData::Face(_) => return Err(bad("malformed texture frame")),
                    MappingData::Wrap(w) => Mapping::Wrap(key::<WrapId>(w, nw, "wrap")?),
                },
            }))
        };
        // Index 0 is Base (`None`); others are the keys the layers were read into.
        let layer = |i: u32| -> Result<Option<LayerId>, ModelError> {
            match i {
                0 => Ok(None),
                _ => key::<LayerId>(i, layer_count, "layer").map(Some),
            }
        };
        let (nv, ne, nf, nc) = (
            self.positions.len(),
            self.edges.len(),
            self.faces.len(),
            self.curves.len(),
        );
        if self.vertex_edges.len() != nv {
            return Err(bad("vertex edge lists do not match the vertex count"));
        }

        let mut vertices = Vec::with_capacity(nv);
        for (p, list) in self.positions.into_iter().zip(self.vertex_edges) {
            if !p.is_finite() {
                return Err(bad("non-finite vertex position"));
            }
            let edges = list
                .into_iter()
                .map(|e| key::<EdgeId>(e, ne, "edge"))
                .collect::<Result<SmallVec<_>, _>>()?;
            vertices.push(Vertex::from_parts(p, edges));
        }

        let mut edges = Vec::with_capacity(ne);
        for e in self.edges {
            edges.push(Edge::from_parts(
                [
                    key::<VertexId>(e.vertices[0], nv, "vertex")?,
                    key::<VertexId>(e.vertices[1], nv, "vertex")?,
                ],
                e.faces
                    .into_iter()
                    .map(|f| key::<FaceId>(f, nf, "face"))
                    .collect::<Result<_, _>>()?,
                e.curve
                    .map(|c| key::<CurveId>(c, nc, "curve"))
                    .transpose()?,
                EdgeFlags {
                    layer: layer(e.layer)?,
                    ..e.flags
                },
            ));
        }

        let uses = |l: Vec<LoopUse>| -> Result<Vec<EdgeUse>, ModelError> {
            l.into_iter()
                .map(|u| {
                    Ok(EdgeUse {
                        edge: key::<EdgeId>(u.edge, ne, "edge")?,
                        reversed: u.reversed,
                    })
                })
                .collect()
        };
        let mut faces = Vec::with_capacity(nf);
        for f in self.faces {
            let plane = Plane::from_stored(f.normal, f.d)?;
            let inner = f.inner.into_iter().map(uses).collect::<Result<_, _>>()?;
            faces.push(Face::from_parts(
                uses(f.outer)?,
                inner,
                plane,
                FaceAttributes {
                    layer: layer(f.layer)?,
                    hidden: f.hidden,
                    front: paint(f.front)?,
                    back: paint(f.back)?,
                },
            ));
        }

        let mut curves = Vec::with_capacity(nc);
        for c in self.curves {
            let edges = c
                .edges
                .into_iter()
                .map(|e| key::<EdgeId>(e, ne, "edge"))
                .collect::<Result<_, _>>()?;
            curves.push(CurveRecord {
                curve: c.curve,
                segments: c.segments,
                edges,
            });
        }

        let mut instances = Vec::with_capacity(self.instances.len());
        for i in self.instances {
            if !i.transform.is_finite() {
                return Err(bad("non-finite instance transform"));
            }
            instances.push(Instance {
                definition: key::<DefinitionId>(i.definition, definition_count, "definition")?,
                transform: i.transform,
                name: i.name,
                layer: layer(i.layer)?,
                locked: i.locked,
                hidden: i.hidden,
            });
        }

        if self.guides.iter().any(|g| !g.is_valid()) {
            return Err(bad("malformed guide"));
        }
        if self.sections.iter().any(|s| !s.is_valid())
            || self.sections.iter().filter(|s| s.active).count() > 1
        {
            return Err(bad("malformed section planes"));
        }
        let key = |i: u32, n: usize| {
            usize::try_from(i)
                .is_ok_and(|x| x < n)
                .then_some(u64::from(i) + 1)
        };
        let mut annotations = Vec::with_capacity(self.annotations.len());
        for a in &self.annotations {
            if !a.is_valid() {
                return Err(bad("malformed dimension or text"));
            }
            if references_out_of_range(a, nv, ne, nf, nc) {
                return Err(bad("dimension or text refers to missing geometry"));
            }
            annotations.push(a.map(
                |v| key(v, nv).map(VertexId::from_raw),
                |e| key(e, ne).map(EdgeId::from_raw),
                |f| key(f, nf).map(FaceId::from_raw),
                |c| key(c, nc).map(CurveId::from_raw),
            ));
        }

        let mut entities = Entities::new();
        entities.vertices = fill(vertices);
        entities.edges = fill(edges);
        entities.faces = fill(faces);
        entities.curves = fill::<CurveId, _>(curves);
        entities.instances = fill::<InstanceId, _>(instances);
        entities.guides = fill::<GuideId, _>(self.guides);
        entities.annotations = fill::<AnnotationId, _>(annotations);
        entities.sections = fill::<SectionId, _>(self.sections);
        entities.wraps = fill::<WrapId, _>(self.wraps);
        for (id, v) in entities.vertices.iter() {
            entities.grid.insert(id, v.position());
        }

        // Curve ↔ edge membership must agree both ways.
        for (cid, c) in entities.curves.iter() {
            for e in &c.edges {
                if entities.edges.get(*e).and_then(|x| x.curve) != Some(cid) {
                    return Err(bad("curve lists an edge that does not belong to it"));
                }
            }
        }
        for (eid, e) in entities.edges.iter() {
            if let Some(c) = e.curve
                && !entities.curves[c].edges.contains(&eid)
            {
                return Err(bad("edge names a curve that does not list it"));
            }
        }
        entities.validate_links().map_err(bad)?;
        Ok(entities)
    }
}

impl Document {
    /// The document as plain, index-linked data (for saving).
    pub fn to_data(&self) -> DocumentData {
        let definitions = dense(self.definition_ids());
        let layers = dense(self.layer_ids());
        let materials = self.used_materials();
        let root_sections = dense(self.root().sections.keys());
        DocumentData {
            root: self.root().to_data(&definitions, &layers, &materials),
            definitions: self
                .definition_ids()
                .map(|id| {
                    let d = self.definition(id).cloned().unwrap_or(Definition {
                        name: String::new(),
                        kind: DefinitionKind::Group,
                        shape: None,
                        sweep: None,
                    });
                    let entities = self
                        .definition_entities(id)
                        .map(|g| g.to_data(&definitions, &layers, &materials))
                        .unwrap_or_default();
                    DefinitionData {
                        name: d.name,
                        kind: d.kind,
                        shape: d.shape,
                        sweep: d.sweep,
                        entities,
                    }
                })
                .collect(),
            active_layer: self
                .active_layer()
                .and_then(|l| layers.get(&l).copied())
                .unwrap_or(0),
            scenes: self
                .scenes()
                .map(|(_, s)| SceneData {
                    name: s.name.clone(),
                    view: s.view,
                    present: s.present,
                    style: s.style,
                    hidden_layers: s
                        .hidden_layers
                        .iter()
                        .filter_map(|l| layers.get(l).copied())
                        .collect(),
                    section: s.section.and_then(|k| root_sections.get(&k).copied()),
                })
                .collect(),
            layers: self
                .layer_ids()
                .filter_map(|id| self.layer(id))
                .map(|l| LayerData {
                    name: l.name.clone(),
                    number: l.number,
                    color: l.color,
                    visible: l.visible,
                    style: l.style,
                    locked: l.locked,
                })
                .collect(),
            materials: materials
                .keys()
                .filter_map(|id| self.material(*id).cloned())
                .collect(),
            look: self.look(),
        }
    }

    /// Rebuilds a document from plain data (for opening a file). The history
    /// starts empty.
    ///
    /// # Errors
    /// [`ModelError::InvalidData`] for an index out of range or a broken model
    /// invariant; [`ModelError::Geometry`] for an invalid stored plane.
    pub fn from_data(data: DocumentData) -> Result<Document, ModelError> {
        let n = data.definitions.len();
        let mut layers: Vec<Layer> = data
            .layers
            .into_iter()
            .enumerate()
            .map(|(i, l)| Layer {
                name: l.name,
                number: l.number,
                color: l.color,
                visible: l.visible,
                style: l.style,
                locked: l.locked,
                // Saved in stack order, Base first.
                order: i as u32,
            })
            .collect();
        if layers.is_empty() {
            layers.push(Layer::base());
        }
        let nl = layers.len();
        if data.materials.iter().any(|m| !m.is_valid()) {
            return Err(bad("malformed material"));
        }
        let look = data.look;
        if !look.smoothing_angle_deg.is_finite()
            || !(0.0..=180.0).contains(&look.smoothing_angle_deg)
        {
            return Err(bad("smoothing angle out of range"));
        }
        let nm = data.materials.len();
        let root = data.root.into_entities(n, nl, nm)?;
        let mut definitions = Vec::with_capacity(n);
        let mut geometry = BTreeMap::new();
        for (i, d) in data.definitions.into_iter().enumerate() {
            geometry.insert(
                DefinitionId::from_raw(i as u64 + 1),
                d.entities.into_entities(n, nl, nm)?,
            );
            definitions.push(Definition {
                name: d.name,
                kind: d.kind,
                shape: d.shape,
                sweep: d.sweep,
            });
        }
        let active = match data.active_layer {
            0 => None,
            i => Some(key::<LayerId>(i, nl, "active layer")?),
        };
        let mut scenes = Vec::with_capacity(data.scenes.len());
        for s in data.scenes {
            let hidden_layers = s
                .hidden_layers
                .iter()
                .map(|i| key::<LayerId>(*i, nl, "scene layer"))
                .collect::<Result<Vec<_>, _>>()?;
            let section = match s.section {
                Some(i) => Some(key::<SectionId>(
                    i,
                    root.sections.len(),
                    "scene section plane",
                )?),
                None => None,
            };
            let scene = Scene {
                name: s.name,
                view: s.view,
                present: s.present,
                style: s.style,
                hidden_layers,
                section,
            };
            if scene.name.trim().is_empty() || !scene.view.is_valid() {
                return Err(bad("malformed scene"));
            }
            scenes.push(scene);
        }
        let mut doc = Document::from_parts(
            root,
            fill(definitions),
            fill(layers),
            fill(scenes),
            geometry,
            active,
        );
        doc.materials = fill(data.materials);
        doc.look = look;
        doc.check_no_recursion()?;
        Ok(doc)
    }

    /// Refuses a definition that contains itself, directly or through others.
    fn check_no_recursion(&self) -> Result<(), ModelError> {
        // Depth-first walk with colours: 1 = on the current path, 2 = done.
        fn visit(
            doc: &Document,
            id: DefinitionId,
            state: &mut BTreeMap<DefinitionId, u8>,
        ) -> Result<(), ModelError> {
            match state.get(&id) {
                Some(2) => return Ok(()),
                Some(_) => return Err(ModelError::RecursiveInstance),
                None => {}
            }
            state.insert(id, 1);
            if let Some(g) = doc.definition_entities(id) {
                for (_, inst) in g.instances.iter() {
                    visit(doc, inst.definition, state)?;
                }
            }
            state.insert(id, 2);
            Ok(())
        }
        let mut state = BTreeMap::new();
        for id in self.definition_ids() {
            visit(self, id, &mut state)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::curve::{Circle, CurveFrame};

    /// A square with a circular hole and a dangling line.
    fn sample() -> Document {
        let mut doc = Document::new();
        doc.transaction("draw", |d| {
            let r = d.root_mut();
            let p = |x, y| DVec3::new(x, y, 0.0);
            r.add_line(p(0.0, 0.0), p(40.0, 0.0))?;
            r.add_line(p(40.0, 0.0), p(40.0, 40.0))?;
            r.add_line(p(40.0, 40.0), p(0.0, 40.0))?;
            r.add_line(p(0.0, 40.0), p(0.0, 0.0))?;
            r.add_line(p(40.0, 40.0), p(60.0, 55.0))?;
            let frame = CurveFrame::new(p(20.0, 20.0), DVec3::X, DVec3::Z)?;
            r.add_curve(Curve::Circle(Circle::new(frame, 8.0)?), 24)?;
            Ok(())
        })
        .expect("sample");
        doc
    }

    fn layer_names(doc: &Document) -> Vec<String> {
        doc.layer_ids()
            .filter_map(|l| doc.layer(l).map(|x| x.name.clone()))
            .collect()
    }

    #[test]
    fn layers_keep_their_stack_order_through_moves_undo_and_saving() {
        let mut doc = Document::new();
        let [a, b, c] = doc
            .transaction("layers", |d| {
                Ok([
                    d.add_layer("A", [1, 1, 1]),
                    d.add_layer("B", [2, 2, 2]),
                    d.add_layer("C", [3, 3, 3]),
                ])
            })
            .expect("layers");
        // A new layer goes on top.
        assert_eq!(layer_names(&doc), ["Base", "A", "B", "C"]);
        assert_eq!(doc.layer_position(c), Some(2));
        assert_eq!(doc.layer_position(doc.base_layer().unwrap()), None);

        // C to just above Base: one undo step.
        doc.transaction("Move Layer", |d| d.reorder_layer(c, 0))
            .expect("move");
        assert_eq!(layer_names(&doc), ["Base", "C", "A", "B"]);
        assert_eq!(doc.undo().as_deref(), Some("Move Layer"));
        assert_eq!(layer_names(&doc), ["Base", "A", "B", "C"]);
        doc.redo();
        assert_eq!(layer_names(&doc), ["Base", "C", "A", "B"]);

        // A position past the top puts it on top; Base never moves.
        doc.transaction("Move Layer", |d| d.reorder_layer(a, 99))
            .expect("move");
        assert_eq!(layer_names(&doc), ["Base", "C", "B", "A"]);
        let base = doc.base_layer().unwrap();
        assert!(
            doc.transaction("Move Layer", |d| d.reorder_layer(base, 2))
                .is_err()
        );
        assert_eq!(layer_names(&doc), ["Base", "C", "B", "A"]);

        // Changing a layer keeps its place, whatever order it is given.
        let renamed = Layer {
            name: "Bee".into(),
            order: 0,
            ..doc.layer(b).unwrap().clone()
        };
        doc.transaction("Change Layer", |d| d.set_layer(b, renamed))
            .expect("rename");
        assert_eq!(layer_names(&doc), ["Base", "C", "Bee", "A"]);

        // The order is saved and read back; Base stays first in the file.
        let data = doc.to_data();
        assert_eq!(data.layers[0].name, "Base");
        let back = Document::from_data(data.clone()).expect("read");
        assert_eq!(layer_names(&back), ["Base", "C", "Bee", "A"]);
        assert_eq!(back.to_data(), data);
    }

    #[test]
    fn data_round_trip_is_exact_and_stable() {
        let doc = sample();
        let data = doc.to_data();
        assert_eq!(data.root.faces.len(), 2, "square with hole, plus hole fill");
        let back = Document::from_data(data.clone()).expect("read");
        assert_eq!(back.to_data(), data);
        // Keys were already dense here, so the documents are equal outright.
        assert!(back.same_content(&doc));
        back.root().validate().expect("valid");
        assert!(!back.can_undo());
    }

    #[test]
    fn damaged_data_is_refused() {
        let mut data = sample().to_data();
        data.root.edges[0].vertices[1] = 9999;
        assert!(matches!(
            Document::from_data(data),
            Err(ModelError::InvalidData(_))
        ));

        let mut data = sample().to_data();
        data.root.faces[0].outer.swap(0, 1);
        assert!(Document::from_data(data).is_err(), "disconnected loop");

        let mut data = sample().to_data();
        data.root.curves[0].edges.pop();
        assert!(Document::from_data(data).is_err(), "curve membership");
    }
}
