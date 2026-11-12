//! SketchSmith — display meshes | v1.0.0 | Thursday 12 November 2026
//!
//! Converts the model into GPU-ready vertex lists. Positions are stored
//! relative to a *render origin* (the centre of the model's bounds) and only
//! then converted to `f32`, so geometry far from the world origin keeps full
//! sub-millimetre precision on the GPU.
//!
//! * [`SceneMesh::build`] — Edit mode: every edge drawn, flat
//!   shading, plain front and back colours; selection highlighted.
//! * [`SceneMesh::build_present`] — Present mode: materials and smoothing,
//!   for the root and (through [`SceneMesh::append`]) every group alike. Each
//!   corner takes the averaged normal of the faces joined to it across smooth
//!   edges (the smoothing angle, overridden by edge marks), and smoothed edges
//!   are not drawn. A face plain on both sides is one double-sided set of
//!   triangles; a face with paint on either side is drawn as two single-sided
//!   sets, the back reversed, each with its own colour, texture coordinates
//!   and texture slot ([`SceneMesh::textures`]). Selection is tinted. An
//!   edge with a line style (its own, else its layer's) is a
//!   [`StyledSegment`], drawn at its pen weight and pattern; other edges keep
//!   the thin line. The model is never changed.
//! * Every builder leaves out hidden faces and edges, and anything on a layer
//!   that is switched off ([`Visibility`]; the plain builders show every
//!   layer).
//! * [`SceneMesh::append`] — adds a group's or component's geometry, placed by
//!   its instance transform: selected as a whole, faded (outside the context
//!   being edited), or plain.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use smith_geom::{DMat3, DMat4, DVec3};
use smith_model::{
    Entities, FaceId, LayerId, LineStyle, Material, MaterialId, Paint, Pickable, Selection, Side,
    TextureImage,
};

/// Which sides a face vertex's triangle shows.
pub mod sides {
    /// Both sides, in the plain front and back colours.
    pub const BOTH: f32 = 0.0;
    /// The model front only.
    pub const FRONT: f32 = 1.0;
    /// The model back only (the triangle is reversed).
    pub const BACK: f32 = 2.0;
}

/// A face vertex: position (relative to the render origin), shading normal,
/// and in Present mode the paint.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FaceVertex {
    /// Position relative to the render origin, mm.
    pub position: [f32; 3],
    /// Normal of the side the triangle faces.
    pub normal: [f32; 3],
    /// 1.0 if the face is selected (drawn with the selection tint), else 0.0;
    /// 2.0 marks a section fill.
    pub selected: f32,
    /// Texture coordinates in repeats (`u / width`, `-v / height`).
    pub uv: [f32; 2],
    /// Paint colour, linear RGB and opacity; alpha below 0 means unpainted
    /// (the plain front or back colour).
    pub color: [f32; 4],
    /// 1.0 for a metal.
    pub metallic: f32,
    /// [`sides`]: which sides the triangle shows.
    pub side: f32,
    /// Texture slot: 0 none, `i` is [`SceneMesh::textures`]`[i - 1]`.
    pub texture: u32,
}

impl Default for FaceVertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            selected: 0.0,
            uv: [0.0; 2],
            color: [0.0, 0.0, 0.0, -1.0],
            metallic: 0.0,
            side: sides::BOTH,
            texture: 0,
        }
    }
}

impl FaceVertex {
    /// An unpainted vertex shown on both sides.
    pub fn plain(position: [f32; 3], normal: [f32; 3], selected: f32) -> Self {
        Self {
            position,
            normal,
            selected,
            ..Self::default()
        }
    }

    /// Whether it belongs to a see-through triangle (opacity under 1).
    pub fn is_clear(&self) -> bool {
        self.color[3] >= 0.0 && self.color[3] < 1.0
    }
}

/// A line vertex: position, colour, distance along the line (for dashing) and
/// a dashed flag.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct LineVertex {
    /// Position relative to the render origin, mm.
    pub position: [f32; 3],
    /// Linear RGBA colour.
    pub color: [f32; 4],
    /// Distance from the line's start in mm (dash phase).
    pub along: f32,
    /// 1.0 for dashed lines, 0.0 for solid.
    pub dashed: f32,
}

/// A Present-mode edge with a line style: drawn as a band of its width in
/// screen pixels, patterned along its length.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct StyledSegment {
    /// Start, relative to the render origin, mm.
    pub a: [f32; 3],
    /// End, relative to the render origin, mm.
    pub b: [f32; 3],
    /// Linear RGBA colour.
    pub color: [f32; 4],
    /// Width in screen pixels (before display scaling).
    pub width: f32,
    /// [`smith_model::LinePattern`] index.
    pub pattern: f32,
}

impl StyledSegment {
    /// The segment `a`–`b` drawn in `style` and `color`.
    pub fn new(a: [f32; 3], b: [f32; 3], style: LineStyle, color: [f32; 4]) -> Self {
        Self {
            a,
            b,
            color,
            width: style.weight.screen_pixels(),
            pattern: f32::from(style.pattern.index()),
        }
    }
}

/// Edge colour in Edit mode (near-black, linear).
pub const EDGE_COLOR: [f32; 4] = [0.02, 0.02, 0.025, 1.0];
/// Selected edge colour (linear blue), as in SketchUp.
pub const SELECTED_EDGE_COLOR: [f32; 4] = [0.05, 0.25, 1.0, 1.0];
/// Edge colour outside the context being edited (light grey, linear).
pub const FADED_EDGE_COLOR: [f32; 4] = [0.55, 0.55, 0.57, 1.0];

/// How [`SceneMesh::append`] shows geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// As usual.
    Plain,
    /// Selected as a whole (a selected group or component).
    Selected,
    /// Outside the group or component being edited.
    Faded,
}

/// Which layers are shown: every layer except those listed (`None` is Base).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visibility {
    /// The layers switched off.
    pub hidden_layers: Vec<Option<LayerId>>,
}

impl Visibility {
    /// Whether something on `layer` is shown.
    pub fn shows(&self, layer: Option<LayerId>) -> bool {
        !self.hidden_layers.contains(&layer)
    }

    /// Whether face `f` is drawn: not hidden, and its layer shown.
    pub fn shows_face(&self, entities: &Entities, f: FaceId) -> bool {
        entities
            .face(f)
            .is_some_and(|x| !x.attributes.hidden && self.shows(x.attributes.layer))
    }
}

/// Display buffers for one model context.
#[derive(Debug, Clone, Default)]
pub struct SceneMesh {
    /// The render origin all positions are relative to.
    pub origin: DVec3,
    /// Triangle list.
    pub faces: Vec<FaceVertex>,
    /// Line list (edges).
    pub edges: Vec<LineVertex>,
    /// Present-mode edges with a line style.
    pub styled: Vec<StyledSegment>,
    /// Layers shown by [`SceneMesh::append`] and the `_shown` builders.
    pub visibility: Visibility,
    /// The images textured faces use, by slot (slot `i + 1`).
    pub textures: Vec<Arc<TextureImage>>,
    /// Set for a Present-mode mesh: how [`SceneMesh::append`] draws groups.
    pub present: Option<(PresentLook, Arc<PresentPaint>)>,
    /// Where each top-level face, edge and group lies in the buffers, for
    /// [`SceneMesh::reselect`]: filled by [`SceneMesh::build_shown`] and by
    /// the caller for the groups it appends; empty when the mesh cannot be
    /// reselected in place.
    pub spans: Vec<Span>,
}

/// Where one selectable item's vertices lie in a [`SceneMesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// The face, edge or group.
    pub item: Pickable,
    /// Its face vertices.
    pub faces: std::ops::Range<usize>,
    /// Its edge vertices.
    pub edges: std::ops::Range<usize>,
}

fn rel(p: DVec3, origin: DVec3) -> [f32; 3] {
    (p - origin).as_vec3().to_array()
}

impl SceneMesh {
    /// Builds the display mesh for `entities`, highlighting `selection`.
    pub fn build(entities: &Entities, selection: &Selection) -> Self {
        Self::build_shown(entities, selection, &Visibility::default())
    }

    /// As [`SceneMesh::build`], leaving out what `vis` hides.
    pub fn build_shown(entities: &Entities, selection: &Selection, vis: &Visibility) -> Self {
        let origin = entities
            .bounds()
            .map(|(lo, hi)| (lo + hi) * 0.5)
            .unwrap_or(DVec3::ZERO);
        let mut faces = Vec::new();
        let mut spans = Vec::new();
        for f in entities.face_ids() {
            let Some(face) = entities.face(f) else {
                continue;
            };
            if !vis.shows_face(entities, f) {
                continue;
            }
            let normal = face.plane().normal().as_vec3().to_array();
            let selected = if selection.contains(Pickable::Face(f)) {
                1.0
            } else {
                0.0
            };
            let start = faces.len();
            for tri in entities.face_triangles(f) {
                for p in tri {
                    faces.push(FaceVertex::plain(rel(p, origin), normal, selected));
                }
            }
            spans.push(Span {
                item: Pickable::Face(f),
                faces: start..faces.len(),
                edges: 0..0,
            });
        }
        let mut edges = Vec::with_capacity(entities.edge_count() * 2);
        for e in entities.edge_ids() {
            let Some(edge) = entities.edge(e) else {
                continue;
            };
            if edge.flags.hidden || !vis.shows(edge.flags.layer) {
                continue;
            }
            let Some((a, b)) = entities.edge_points(e) else {
                continue;
            };
            let color = if selection.contains(Pickable::Edge(e)) {
                SELECTED_EDGE_COLOR
            } else {
                EDGE_COLOR
            };
            spans.push(Span {
                item: Pickable::Edge(e),
                faces: 0..0,
                edges: edges.len()..edges.len() + 2,
            });
            edges.push(LineVertex {
                position: rel(a, origin),
                color,
                along: 0.0,
                dashed: 0.0,
            });
            edges.push(LineVertex {
                position: rel(b, origin),
                color,
                along: a.distance(b) as f32,
                dashed: 0.0,
            });
        }
        Self {
            origin,
            faces,
            edges,
            visibility: vis.clone(),
            spans,
            ..Self::default()
        }
    }

    /// Re-marks what is selected without rebuilding: each span's face
    /// vertices take the selection tint, its edges the selection colour,
    /// or lose them. Only for an Edit-mode mesh of the model root (see
    /// [`SceneMesh::spans`]).
    pub fn reselect(&mut self, selection: &Selection) {
        for span in &self.spans {
            let on = selection.contains(span.item);
            let tint = if on { 1.0 } else { 0.0 };
            // A section fill (2.0) inside a group stays as it is.
            for v in &mut self.faces[span.faces.clone()] {
                if v.selected < 1.5 {
                    v.selected = tint;
                }
            }
            let color = if on { SELECTED_EDGE_COLOR } else { EDGE_COLOR };
            for v in &mut self.edges[span.edges.clone()] {
                v.color = color;
            }
        }
    }
}

impl SceneMesh {
    /// Adds `entities` placed by `transform` (definition space → world),
    /// shown as `shown`. Hidden edges are left out.
    pub fn append(&mut self, entities: &Entities, transform: &DMat4, shown: Shown) {
        self.append_with(entities, transform, shown, None);
    }

    /// As [`SceneMesh::append`], also highlighting the entities in
    /// `selection` (the group or component being edited).
    pub fn append_with(
        &mut self,
        entities: &Entities,
        transform: &DMat4,
        shown: Shown,
        selection: Option<&Selection>,
    ) {
        if let Some((look, paint)) = self.present.clone() {
            self.push_present(entities, transform, shown, selection, look, &paint);
            return;
        }
        let picked = |p: Pickable| selection.is_some_and(|s| s.contains(p));
        let origin = self.origin;
        let at = |p: DVec3| rel(transform.transform_point3(p), origin);
        let normals = DMat3::from_mat4(*transform).inverse().transpose();
        let mirrored = DMat3::from_mat4(*transform).determinant() < 0.0;
        let vis = self.visibility.clone();
        for f in entities.face_ids() {
            let Some(face) = entities.face(f) else {
                continue;
            };
            if !vis.shows_face(entities, f) {
                continue;
            }
            let selected = if shown == Shown::Selected || picked(Pickable::Face(f)) {
                1.0
            } else {
                0.0
            };
            let normal = (normals * face.plane().normal())
                .normalize_or_zero()
                .as_vec3()
                .to_array();
            for mut tri in entities.face_triangles(f) {
                // A mirroring transform reverses the winding; keep it front-facing.
                if mirrored {
                    tri.swap(1, 2);
                }
                for p in tri {
                    self.faces.push(FaceVertex::plain(at(p), normal, selected));
                }
            }
        }
        let base = match shown {
            Shown::Plain => EDGE_COLOR,
            Shown::Selected => SELECTED_EDGE_COLOR,
            Shown::Faded => FADED_EDGE_COLOR,
        };
        for e in entities.edge_ids() {
            if entities
                .edge(e)
                .is_none_or(|x| x.flags.hidden || !vis.shows(x.flags.layer))
            {
                continue;
            }
            let color = if picked(Pickable::Edge(e)) {
                SELECTED_EDGE_COLOR
            } else {
                base
            };
            let Some((a, b)) = entities.edge_points(e) else {
                continue;
            };
            let len = transform
                .transform_point3(a)
                .distance(transform.transform_point3(b)) as f32;
            self.edges.push(LineVertex {
                position: at(a),
                color,
                along: 0.0,
                dashed: 0.0,
            });
            self.edges.push(LineVertex {
                position: at(b),
                color,
                along: len,
                dashed: 0.0,
            });
        }
    }
}

/// Which edges Present mode draws (Look settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeDisplay {
    /// Every edge, smoothed or not.
    All,
    /// Only edges that are not smoothed away (sharp edges and boundaries).
    #[default]
    Sharp,
    /// No edges.
    None,
}

/// Present-mode display choices that affect the mesh (the Styles tab).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentLook {
    /// Smooth (averaged) normals; `false` keeps flat shading.
    pub smooth_shading: bool,
    /// Which edges to draw.
    pub edges: EdgeDisplay,
}

impl Default for PresentLook {
    fn default() -> Self {
        Self {
            smooth_shading: true,
            edges: EdgeDisplay::Sharp,
        }
    }
}

/// What the model itself says about its Present-mode look.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentPaint {
    /// Faces meeting at less than this angle (degrees) are shaded as one
    /// surface, unless the edge between them is marked.
    pub smoothing_angle_deg: f64,
    /// The document's materials.
    pub materials: HashMap<MaterialId, Material>,
    /// The line style of each layer that has one (`None` is Base).
    pub layer_styles: HashMap<Option<LayerId>, LineStyle>,
}

impl Default for PresentPaint {
    fn default() -> Self {
        Self {
            smoothing_angle_deg: 30.0,
            materials: HashMap::new(),
            layer_styles: HashMap::new(),
        }
    }
}

impl SceneMesh {
    /// Builds the Present-mode mesh (see the module notes).
    pub fn build_present(
        entities: &Entities,
        look: PresentLook,
        paint: &Arc<PresentPaint>,
    ) -> Self {
        Self::build_present_shown(entities, look, paint, &Visibility::default(), None)
    }

    /// As [`SceneMesh::build_present`], leaving out what `vis` hides and
    /// tinting `selection`.
    pub fn build_present_shown(
        entities: &Entities,
        look: PresentLook,
        paint: &Arc<PresentPaint>,
        vis: &Visibility,
        selection: Option<&Selection>,
    ) -> Self {
        let origin = entities
            .bounds()
            .map(|(lo, hi)| (lo + hi) * 0.5)
            .unwrap_or(DVec3::ZERO);
        let mut mesh = Self {
            origin,
            visibility: vis.clone(),
            present: Some((look, paint.clone())),
            ..Self::default()
        };
        mesh.push_present(
            entities,
            &DMat4::IDENTITY,
            Shown::Plain,
            selection,
            look,
            paint,
        );
        mesh
    }

    /// The texture slot of `image`, added if new.
    fn texture_slot(&mut self, image: &Arc<TextureImage>) -> u32 {
        let i = self
            .textures
            .iter()
            .position(|t| Arc::ptr_eq(t, image))
            .unwrap_or_else(|| {
                self.textures.push(image.clone());
                self.textures.len() - 1
            });
        u32::try_from(i + 1).unwrap_or(0)
    }

    /// Adds `entities` placed by `transform` in Present mode.
    fn push_present(
        &mut self,
        entities: &Entities,
        transform: &DMat4,
        shown: Shown,
        selection: Option<&Selection>,
        look: PresentLook,
        paint_of: &PresentPaint,
    ) {
        let picked = |p: Pickable| selection.is_some_and(|s| s.contains(p));
        let origin = self.origin;
        let at = |p: DVec3| rel(transform.transform_point3(p), origin);
        let m3 = DMat3::from_mat4(*transform);
        let normals = m3.inverse().transpose();
        let mirrored = m3.determinant() < 0.0;
        let turn = |n: DVec3| (normals * n).normalize_or_zero().as_vec3().to_array();
        let cos_limit = paint_of.smoothing_angle_deg.to_radians().cos();
        let smooth = look.smooth_shading.then_some(cos_limit);
        let vis = self.visibility.clone();

        for f in entities.face_ids() {
            let Some(face) = entities.face(f) else {
                continue;
            };
            if !vis.shows_face(entities, f) {
                continue;
            }
            let selected = if shown == Shown::Selected || picked(Pickable::Face(f)) {
                1.0
            } else {
                0.0
            };
            let n = face.plane().normal();
            let corner_normals = entities.corner_normals(f, smooth);
            let corners: Vec<smith_model::VertexId> = face
                .loops()
                .flat_map(|l| entities.loop_vertices(l))
                .collect();
            let points: Vec<DVec3> = corners
                .iter()
                .filter_map(|v| entities.vertex(*v).map(smith_model::Vertex::position))
                .collect();
            let tris = entities.face_triangles(f);
            let front = face.attributes.paint(Side::Front).copied();
            let back = face.attributes.paint(Side::Back).copied();
            let sets: Vec<(Option<Paint>, f32)> = if front.is_none() && back.is_none() {
                vec![(None, sides::BOTH)]
            } else {
                vec![(front, sides::FRONT), (back, sides::BACK)]
            };
            for (paint, side) in sets {
                let reversed = side == sides::BACK;
                let side_n = if reversed { -n } else { n };
                let material = paint.and_then(|p| paint_of.materials.get(&p.material));
                let (color, metallic, slot, uvs) = match (paint, material) {
                    (Some(p), Some(m)) => {
                        let slot = m
                            .texture
                            .as_ref()
                            .map_or(0, |t| self.texture_slot(&t.image));
                        let uvs = m.texture.as_ref().and_then(|t| {
                            let size = t.size_mm();
                            let mm = entities.paint_uvs(&p, side_n, &points)?;
                            Some(
                                mm.iter()
                                    .map(|q| [(q.x / size.x) as f32, (-q.y / size.y) as f32])
                                    .collect::<Vec<_>>(),
                            )
                        });
                        let [r, g, b] = m.color;
                        (
                            [r, g, b, m.opacity],
                            if m.metallic { 1.0 } else { 0.0 },
                            slot,
                            uvs,
                        )
                    }
                    _ => ([0.0, 0.0, 0.0, -1.0], 0.0, 0, None),
                };
                for t in &tris {
                    // A back set is reversed; a mirroring transform reverses
                    // again. Either way it stays facing the side it shows.
                    let order = if reversed != mirrored {
                        [0, 2, 1]
                    } else {
                        [0, 1, 2]
                    };
                    for k in order {
                        let p = t[k];
                        let ci = points.iter().position(|q| *q == p);
                        let normal = ci
                            .and_then(|i| corner_normals.get(&corners[i]).copied())
                            .unwrap_or(n);
                        let normal = if reversed { -normal } else { normal };
                        self.faces.push(FaceVertex {
                            position: at(p),
                            normal: turn(normal),
                            selected,
                            uv: ci
                                .and_then(|i| uvs.as_ref().map(|u| u[i]))
                                .unwrap_or([0.0; 2]),
                            color,
                            metallic,
                            side,
                            texture: slot,
                        });
                    }
                }
            }
        }

        if look.edges == EdgeDisplay::None {
            return;
        }
        let base = if shown == Shown::Selected {
            SELECTED_EDGE_COLOR
        } else {
            EDGE_COLOR
        };
        for e in entities.edge_ids() {
            let Some(edge) = entities.edge(e) else {
                continue;
            };
            if edge.flags.hidden || !vis.shows(edge.flags.layer) {
                continue;
            }
            if look.edges == EdgeDisplay::Sharp && entities.edge_is_smooth(e, cos_limit) {
                continue;
            }
            let Some((a, b)) = entities.edge_points(e) else {
                continue;
            };
            let color = if picked(Pickable::Edge(e)) {
                SELECTED_EDGE_COLOR
            } else {
                base
            };
            // An edge's own style wins over its layer's.
            let style = edge
                .flags
                .style
                .or_else(|| paint_of.layer_styles.get(&edge.flags.layer).copied());
            if let Some(style) = style {
                self.styled
                    .push(StyledSegment::new(at(a), at(b), style, color));
                continue;
            }
            let len = transform
                .transform_point3(a)
                .distance(transform.transform_point3(b)) as f32;
            self.edges.push(LineVertex {
                position: at(a),
                color,
                along: 0.0,
                dashed: 0.0,
            });
            self.edges.push(LineVertex {
                position: at(b),
                color,
                along: len,
                dashed: 0.0,
            });
        }
    }
}

/// Length of each drawn axis half, in mm (effectively "to the horizon").
pub const AXIS_LENGTH: f64 = 1e6;

/// The red, green and blue drawing axes through `axes_origin` along `basis`
/// (x, y, z), relative to the render `origin`: solid in the positive direction,
/// dashed in the negative.
pub fn axes(origin: DVec3, axes_origin: DVec3, basis: [DVec3; 3]) -> Vec<LineVertex> {
    // Saturated and fairly dark, for contrast on the white background.
    const RED: [f32; 4] = [0.80, 0.02, 0.02, 1.0];
    const GREEN: [f32; 4] = [0.02, 0.45, 0.04, 1.0];
    const BLUE: [f32; 4] = [0.02, 0.08, 0.80, 1.0];
    let mut v = Vec::with_capacity(12);
    for (dir, color) in [(basis[0], RED), (basis[1], GREEN), (basis[2], BLUE)] {
        for (sign, dashed) in [(1.0, 0.0), (-1.0, 1.0)] {
            v.push(LineVertex {
                position: rel(axes_origin, origin),
                color,
                along: 0.0,
                dashed,
            });
            v.push(LineVertex {
                position: rel(axes_origin + dir * (sign * AXIS_LENGTH), origin),
                color,
                along: AXIS_LENGTH as f32,
                dashed,
            });
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_is_relative_to_bounds_centre() {
        let mut m = Entities::new();
        let o = DVec3::new(1000.0, 2000.0, 0.0);
        m.add_polyline(
            &[
                o,
                o + DVec3::X * 0.002,
                o + DVec3::new(0.002, 0.002, 0.0),
                o + DVec3::Y * 0.002,
            ],
            true,
        )
        .unwrap();
        let mesh = SceneMesh::build(&m, &Selection::new());
        assert_eq!(mesh.faces.len(), 6);
        assert_eq!(mesh.edges.len(), 8);
        // Sub-millimetre detail survives the f32 conversion: corners are ±0.001 mm.
        for v in &mesh.faces {
            for c in &v.position[..2] {
                assert!((c.abs() - 0.001).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn selection_is_highlighted() {
        let mut m = Entities::new();
        let v = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        m.add_polyline(&[v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)], true)
            .unwrap();
        let mut sel = Selection::new();
        sel.apply(m.all_pickables(), smith_model::SelectMode::Replace);
        let mesh = SceneMesh::build(&m, &sel);
        assert!(mesh.faces.iter().all(|f| f.selected == 1.0));
        assert!(mesh.edges.iter().all(|e| e.color == SELECTED_EDGE_COLOR));
    }

    /// A 24-sided prism (cylinder stand-in) built from lines.
    fn prism() -> Entities {
        let mut m = Entities::new();
        let n = 24;
        let pt = |i: usize, z: f64| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            DVec3::new(5.0 * a.cos(), 5.0 * a.sin(), z)
        };
        let bottom: Vec<DVec3> = (0..n).map(|i| pt(i, 0.0)).collect();
        let top: Vec<DVec3> = (0..n).map(|i| pt(i, 10.0)).collect();
        m.add_polyline(&bottom, true).unwrap();
        m.add_polyline(&top, true).unwrap();
        for i in 0..n {
            m.add_line(pt(i, 0.0), pt(i, 10.0)).unwrap();
        }
        m
    }

    #[test]
    fn present_mode_hides_smooth_side_edges_only() {
        let m = prism();
        assert_eq!(m.face_count(), 26);
        let edit = SceneMesh::build(&m, &Selection::new());
        assert_eq!(edit.edges.len() / 2, 72);
        let paint = Arc::new(PresentPaint::default());
        let present = SceneMesh::build_present(&m, PresentLook::default(), &paint);
        // The 24 vertical side edges (15° apart) are smoothed; 48 cap edges stay.
        assert_eq!(present.edges.len() / 2, 48);
        // Smoothing never changes geometry: same triangles, same positions.
        assert_eq!(present.faces.len(), edit.faces.len());
        let none = SceneMesh::build_present(
            &m,
            PresentLook {
                edges: EdgeDisplay::None,
                ..PresentLook::default()
            },
            &paint,
        );
        assert!(none.edges.is_empty());
        let all = SceneMesh::build_present(
            &m,
            PresentLook {
                edges: EdgeDisplay::All,
                ..PresentLook::default()
            },
            &paint,
        );
        assert_eq!(all.edges.len() / 2, 72);
    }

    #[test]
    fn smoothing_angle_controls_the_result() {
        let m = prism();
        let tight = SceneMesh::build_present(
            &m,
            PresentLook::default(),
            &Arc::new(PresentPaint {
                smoothing_angle_deg: 10.0,
                ..PresentPaint::default()
            }),
        );
        assert_eq!(tight.edges.len() / 2, 72, "15° steps are sharper than 10°");
    }

    #[test]
    fn a_face_painted_on_one_side_is_drawn_one_sided_with_its_paint() {
        use smith_model::{
            Document, Mapping, Material, Paint, Side, Texture, TextureFrame, TextureImage,
        };
        let mut doc = Document::new();
        let image = Arc::new(TextureImage {
            name: "a.png".into(),
            format: smith_model::ImageFormat::Png,
            bytes: Vec::new(),
            pixels: [100, 50],
        });
        let wood = doc
            .add_material(Material {
                name: "Wood".into(),
                opacity: 0.5,
                texture: Some(Texture {
                    image: image.clone(),
                    width_mm: 50.0,
                }),
                ..Material::default()
            })
            .unwrap();
        let r = doc.root_mut();
        r.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(50.0, 0.0, 0.0),
                DVec3::new(50.0, 25.0, 0.0),
                DVec3::new(0.0, 25.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let f = r.face_ids().next().unwrap();
        r.set_face_paint(
            f,
            Side::Front,
            Some(Paint {
                material: wood,
                mapping: Mapping::Face(TextureFrame::for_side(
                    DVec3::Z,
                    smith_geom::DVec2::ZERO,
                    0.0,
                )),
            }),
        )
        .unwrap();
        let paint = Arc::new(PresentPaint {
            materials: doc
                .material_ids()
                .filter_map(|id| doc.material(id).map(|m| (id, m.clone())))
                .collect(),
            ..PresentPaint::default()
        });
        let mesh = SceneMesh::build_present(doc.root(), PresentLook::default(), &paint);
        // Front and back each drawn once, one-sided.
        assert_eq!(mesh.faces.len(), 12);
        assert_eq!(mesh.textures.len(), 1);
        let front: Vec<_> = mesh
            .faces
            .iter()
            .filter(|v| v.side == sides::FRONT)
            .collect();
        assert!(front.iter().all(|v| v.texture == 1 && v.is_clear()));
        // A 50 mm texture spans one repeat across the 50 mm face.
        let us: Vec<f32> = front.iter().map(|v| v.uv[0]).collect();
        let span = us.iter().copied().fold(f32::MIN, f32::max)
            - us.iter().copied().fold(f32::MAX, f32::min);
        assert!((span - 1.0).abs() < 1e-6);
        let back: Vec<_> = mesh
            .faces
            .iter()
            .filter(|v| v.side == sides::BACK)
            .collect();
        assert!(back.iter().all(|v| v.color[3] < 0.0 && v.normal[2] < 0.0));
    }

    #[test]
    fn a_mirrored_placement_keeps_faces_front_side_out() {
        // Each triangle's winding must agree with its normal, mirrored or not.
        let e = prism();
        for m in [
            smith_geom::DMat4::IDENTITY,
            smith_geom::DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)),
        ] {
            let mut mesh = SceneMesh::default();
            mesh.append(&e, &m, Shown::Plain);
            assert!(!mesh.faces.is_empty());
            for tri in mesh.faces.chunks(3) {
                let p = |i: usize| DVec3::from_array(tri[i].position.map(f64::from));
                let winding = (p(1) - p(0)).cross(p(2) - p(0));
                let normal = DVec3::from_array(tri[0].normal.map(f64::from));
                assert!(
                    winding.dot(normal) > 0.0,
                    "mirrored: {}",
                    m.determinant() < 0.0
                );
            }
        }
    }

    #[test]
    fn axes_have_dashed_negative_halves() {
        let a = axes(DVec3::ZERO, DVec3::ZERO, [DVec3::X, DVec3::Y, DVec3::Z]);
        assert_eq!(a.len(), 12);
        assert_eq!(a.iter().filter(|v| v.dashed == 1.0).count(), 6);
    }

    #[test]
    fn an_edges_own_style_beats_its_layers_and_plain_edges_stay_thin() {
        use smith_model::{LinePattern, LineStyle, PenWeight};
        let mut m = Entities::new();
        m.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
            ],
            false,
        )
        .unwrap();
        let edges: Vec<_> = m.edge_ids().collect();
        let own = LineStyle {
            weight: PenWeight::W100,
            pattern: LinePattern::DashDot,
        };
        m.set_edge_style(edges[0], Some(own)).unwrap();
        let layer = LineStyle {
            weight: PenWeight::W050,
            pattern: LinePattern::Dashed,
        };
        let paint = Arc::new(PresentPaint {
            layer_styles: [(None, layer)].into_iter().collect(),
            ..PresentPaint::default()
        });
        let mesh = SceneMesh::build_present(&m, PresentLook::default(), &paint);
        assert_eq!(mesh.styled.len(), 2);
        assert!(
            mesh.styled
                .iter()
                .any(|s| s.width == 6.0 && s.pattern == 2.0)
        );
        assert!(
            mesh.styled
                .iter()
                .any(|s| s.width == 3.0 && s.pattern == 1.0)
        );
        // With no layer style, the unstyled edge is a plain thin line.
        let plain = SceneMesh::build_present(
            &m,
            PresentLook::default(),
            &Arc::new(PresentPaint::default()),
        );
        assert_eq!((plain.styled.len(), plain.edges.len()), (1, 2));
        // Edit mode never styles.
        assert!(SceneMesh::build(&m, &Selection::new()).styled.is_empty());
    }
}
