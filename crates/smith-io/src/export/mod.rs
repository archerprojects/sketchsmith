//! SketchSmith — model exports | v1.0.0 | Thursday 12 November 2026
//!
//! Writes the model for other programs: OBJ (with its MTL file and a
//! textures folder) and plain glTF for Blender, STL and 3MF for printing,
//! STEP and 3D DXF for CAD; and [`draw`], the hidden-line drawing of a
//! view, laid on paper by [`sheet`] for PDF and SVG, or written as 2D DXF;
//! and PNG, from pixels the application renders ([`write_png`]). A saved
//! Render exports as PNG, PDF or SVG ([`RenderFormat`]).
//!
//! What goes out is what is shown: faces that are not hidden, on layers that
//! are shown, in groups and components that are not hidden. Guides, section
//! planes, text and dimensions never go out, nor do edges without a face.
//! With Selection Only, the selected faces and groups of the open context.
//!
//! [`gather`] walks the model once into [`Part`]s, one per piece of loose
//! geometry and one per group or component copy (nested ones included), each
//! face in world millimetres, Z up, its outer loop and its triangles
//! counter-clockwise about its normal even in a mirrored copy. Each writer
//! converts units and axes.
//!
//! [`target`] turns the name chosen in the save dialog into the file to
//! write, and refuses one that is a native SketchSmith model.

mod drawing;
mod dxf;
mod gltf;
mod helvetica;
mod marks;
mod obj;
mod page;
mod pdf;
mod png;
mod step;
mod stl;
mod svg;
mod threemf;
mod zip;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use glam::{DMat3, DMat4, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use smith_model::{DefinitionId, Document, Entities, FaceId, InstanceId, MaterialId, VertexId};

pub use drawing::{Drawing, DrawingView, DrawnLine, LineKind, draw};
pub use dxf::{write_dxf, write_dxf_drawing};
pub use gltf::write_gltf;
pub use marks::{Labels, Mark, Measure, marks};
pub use obj::write_obj;
pub use page::{HiddenLines, Orientation, Paper, Sheet, SheetLine, SheetText, sheet};
pub use pdf::write_pdf;
pub use png::{Background, MAX_SIDE, PictureSize, write_png};
pub use step::{StepReport, write_step};
pub use stl::write_stl;
#[cfg(test)]
pub(crate) use svg::svg_text as svg_text_for_tests_inner;
pub use svg::write_svg;
pub use threemf::write_3mf;

use crate::error::IoError;

/// An export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Wavefront OBJ, with an MTL file.
    Obj,
    /// STL.
    Stl,
    /// Plain glTF (`.gltf`).
    Gltf,
    /// 3MF.
    ThreeMf,
    /// STEP (AP214).
    Step,
    /// DXF.
    Dxf,
    /// PDF drawing.
    Pdf,
    /// SVG drawing.
    Svg,
    /// PNG picture.
    Png,
}

impl Format {
    /// The formats, in menu order.
    pub const ALL: [Format; 9] = [
        Self::Obj,
        Self::Stl,
        Self::Gltf,
        Self::ThreeMf,
        Self::Step,
        Self::Dxf,
        Self::Pdf,
        Self::Svg,
        Self::Png,
    ];

    /// The format's short name, as the menu shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Obj => "OBJ",
            Self::Stl => "STL",
            Self::Gltf => "glTF",
            Self::ThreeMf => "3MF",
            Self::Step => "STEP",
            Self::Dxf => "DXF",
            Self::Pdf => "PDF",
            Self::Svg => "SVG",
            Self::Png => "PNG",
        }
    }

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Obj => "obj",
            Self::Stl => "stl",
            Self::Gltf => "gltf",
            Self::ThreeMf => "3mf",
            Self::Step => "step",
            Self::Dxf => "dxf",
            Self::Pdf => "pdf",
            Self::Svg => "svg",
            Self::Png => "png",
        }
    }

    /// Whether a typed extension is one of the format's own (`.stp` is
    /// STEP's too).
    pub fn accepts(self, extension: &str) -> bool {
        extension.eq_ignore_ascii_case(self.extension())
            || (self == Self::Step && extension.eq_ignore_ascii_case("stp"))
    }

    /// The save dialog's filter name.
    pub fn filter(self) -> &'static str {
        match self {
            Self::Obj => "Wavefront OBJ (.obj)",
            Self::Stl => "STL (.stl)",
            Self::Gltf => "glTF (.gltf)",
            Self::ThreeMf => "3MF (.3mf)",
            Self::Step => "STEP (.step)",
            Self::Dxf => "DXF (.dxf)",
            Self::Pdf => "PDF (.pdf)",
            Self::Svg => "SVG (.svg)",
            Self::Png => "PNG (.png)",
        }
    }

    /// Whether the Units choice applies (glTF is metres and 3MF millimetres
    /// by their specifications; STEP is written in millimetres).
    pub fn has_units(self) -> bool {
        matches!(self, Self::Obj | Self::Stl | Self::Dxf)
    }

    /// Whether Scale applies: with Custom units, or always for 3MF, PDF
    /// and SVG.
    pub fn has_scale(self) -> bool {
        !matches!(self, Self::Gltf | Self::Step | Self::Png)
    }

    /// Whether the Scale field shows with `options`.
    pub fn shows_scale(self, options: &ExportOptions) -> bool {
        match self {
            Self::ThreeMf | Self::Pdf | Self::Svg => true,
            Self::Obj | Self::Stl | Self::Dxf => options.units == UnitPreset::Custom,
            Self::Gltf | Self::Step | Self::Png => false,
        }
    }

    /// Whether it is a drawing of a view, as `options` choose (PDF, SVG,
    /// and DXF set to 2D Drawing).
    pub fn is_drawing(self, options: &ExportOptions) -> bool {
        matches!(self, Self::Pdf | Self::Svg)
            || (self == Self::Dxf && options.content == DxfContent::Drawing)
    }
}

/// The Units choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitPreset {
    /// Metres, as Blender works in.
    #[default]
    Blender,
    /// Millimetres, as printers work in.
    Print,
    /// A chosen unit and scale.
    Custom,
}

impl UnitPreset {
    /// The presets, in list order.
    pub const ALL: [UnitPreset; 3] = [Self::Blender, Self::Print, Self::Custom];

    /// The preset's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Blender => "Blender (m)",
            Self::Print => "Print (mm)",
            Self::Custom => "Custom",
        }
    }
}

/// A unit for Custom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileUnit {
    /// Millimetres.
    #[default]
    Mm,
    /// Centimetres.
    Cm,
    /// Metres.
    M,
    /// Inches.
    In,
    /// Feet.
    Ft,
}

impl FileUnit {
    /// The units, in list order.
    pub const ALL: [FileUnit; 5] = [Self::Mm, Self::Cm, Self::M, Self::In, Self::Ft];

    /// The unit's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::M => "m",
            Self::In => "in",
            Self::Ft => "ft",
        }
    }

    /// Millimetres in one of the unit.
    pub fn mm(self) -> f64 {
        match self {
            Self::Mm => 1.0,
            Self::Cm => 10.0,
            Self::M => 1000.0,
            Self::In => 25.4,
            Self::Ft => 304.8,
        }
    }
}

/// The format a saved Render is exported in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderFormat {
    /// A PNG picture.
    #[default]
    Png,
    /// A PDF drawing.
    Pdf,
    /// An SVG drawing.
    Svg,
}

impl RenderFormat {
    /// The choices, in list order.
    pub const ALL: [RenderFormat; 3] = [Self::Png, Self::Pdf, Self::Svg];

    /// The export format it is.
    pub fn format(self) -> Format {
        match self {
            Self::Png => Format::Png,
            Self::Pdf => Format::Pdf,
            Self::Svg => Format::Svg,
        }
    }
}

/// What a DXF holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DxfContent {
    /// The faces, in 3D.
    #[default]
    Faces,
    /// A 2D drawing of the view.
    Drawing,
}

impl DxfContent {
    /// The choices, in list order.
    pub const ALL: [DxfContent; 2] = [Self::Faces, Self::Drawing];

    /// The choice's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Faces => "3D Faces",
            Self::Drawing => "2D Drawing",
        }
    }
}

/// The values chosen in the export dialog for one format.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// Units (OBJ and STL).
    pub units: UnitPreset,
    /// Custom's unit.
    pub unit: FileUnit,
    /// Scale 1:N, the N (Custom, and 3MF).
    pub scale: f64,
    /// Selection Only.
    pub selection_only: bool,
    /// Paper (PDF, SVG).
    pub paper: Paper,
    /// Orientation (PDF, SVG).
    pub orientation: Orientation,
    /// Hidden Lines (drawings).
    pub hidden_lines: HiddenLines,
    /// Content (DXF).
    pub content: DxfContent,
    /// Size (PNG).
    pub picture: PictureSize,
    /// Custom width and height, pixels (PNG).
    pub pixels: [u32; 2],
    /// Background (PNG).
    pub background: Background,
    /// Format (Export Render).
    pub render_format: RenderFormat,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            units: UnitPreset::Blender,
            unit: FileUnit::Mm,
            scale: 1.0,
            selection_only: false,
            paper: Paper::A4,
            orientation: Orientation::Auto,
            hidden_lines: HiddenLines::Off,
            content: DxfContent::Faces,
            picture: PictureSize::Canvas,
            pixels: [1920, 1080],
            background: Background::Shown,
            render_format: RenderFormat::Png,
        }
    }
}

impl ExportOptions {
    /// The defaults for `format`: Print (mm) for STL and DXF, Blender (m)
    /// otherwise.
    pub fn for_format(format: Format) -> Self {
        Self {
            units: if matches!(format, Format::Stl | Format::Dxf) {
                UnitPreset::Print
            } else {
                UnitPreset::Blender
            },
            ..Self::default()
        }
    }

    /// File units per model millimetre for `format`.
    pub fn units_per_mm(&self, format: Format) -> f64 {
        let n = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        };
        match format {
            Format::Gltf => 0.001,
            Format::ThreeMf => 1.0 / n,
            Format::Step => 1.0,
            Format::Pdf | Format::Svg => 1.0 / n,
            Format::Png => 1.0,
            Format::Obj | Format::Stl | Format::Dxf => match self.units {
                UnitPreset::Blender => 0.001,
                UnitPreset::Print => 1.0,
                UnitPreset::Custom => 1.0 / (self.unit.mm() * n),
            },
        }
    }
}

/// What to export.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    /// The whole model.
    Model,
    /// The selected faces and groups of the open context.
    Selection {
        /// The open definition (`None`: the model root).
        context: Option<DefinitionId>,
        /// Its placement: its coordinates → world.
        local: DMat4,
        /// Selected faces.
        faces: &'a [FaceId],
        /// Selected groups and components.
        instances: &'a [InstanceId],
    },
}

/// One corner of an exported face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    /// The vertex, unique within its part.
    pub vertex: VertexId,
    /// Position, world mm, Z up.
    pub point: DVec3,
    /// Shading normal (smoothed as Present mode shows it), world, unit.
    pub normal: DVec3,
    /// Texture coordinates in repeats (u right, v up), when textured.
    pub uv: Option<DVec2>,
}

/// One exported face.
#[derive(Debug, Clone, PartialEq)]
pub struct OutFace {
    /// Every corner: the outer loop first, then each hole's.
    pub corners: Vec<Corner>,
    /// How many of `corners` each loop has: the outer loop first.
    pub loops: Vec<usize>,
    /// Triangles (indices into `corners`), counter-clockwise about the face
    /// normal.
    pub triangles: Vec<[usize; 3]>,
    /// The face's front direction, world, unit.
    pub normal: DVec3,
    /// The material shown: the front's paint, else the back's.
    pub material: Option<MaterialId>,
    /// The name of the layer it is on.
    pub layer: String,
}

impl OutFace {
    /// Whether the face has holes.
    pub fn has_holes(&self) -> bool {
        self.loops.len() > 1
    }
}

/// A piece of the export: loose geometry, or one group or component copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The copy's name, else its definition's; empty for loose geometry.
    pub name: String,
    /// Which top-level piece it belongs to: 0 the loose geometry, then one
    /// number per top-level group or component copy, shared by the copies
    /// nested inside it.
    pub piece: usize,
    /// Its faces.
    pub faces: Vec<OutFace>,
}

/// Everything to export, gathered from the model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gathered {
    /// The parts, loose geometry first; parts with no faces left out.
    pub parts: Vec<Part>,
    /// How many top-level group and component copies were numbered.
    pieces: usize,
}

impl Gathered {
    /// Whether there is nothing to export.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// How many faces have holes (OBJ triangulates them).
    pub fn holed_faces(&self) -> usize {
        self.parts
            .iter()
            .flat_map(|p| &p.faces)
            .filter(|f| f.has_holes())
            .count()
    }
}

/// Gathers what `scope` exports from `doc`.
pub fn gather(doc: &Document, scope: Scope<'_>) -> Gathered {
    let mut out = Gathered::default();
    match scope {
        Scope::Model => {
            let root = doc.root();
            let faces: Vec<FaceId> = root.face_ids().collect();
            let instances: Vec<InstanceId> = root.instance_ids().collect();
            add_part(
                doc,
                root,
                &faces,
                (String::new(), 0),
                DMat4::IDENTITY,
                &mut out,
            );
            add_instances(doc, root, &instances, DMat4::IDENTITY, None, &mut out);
        }
        Scope::Selection {
            context,
            local,
            faces,
            instances,
        } => {
            let entities = match context {
                None => Some(doc.root()),
                Some(id) => doc.definition_entities(id),
            };
            if let Some(e) = entities {
                add_part(doc, e, faces, (String::new(), 0), local, &mut out);
                add_instances(doc, e, instances, local, None, &mut out);
            }
        }
    }
    out
}

/// Adds the shown `instances` of `e` (placed by `parent`) and everything
/// inside them, one part per copy: each a new piece at the top (`piece`
/// `None`), else in `piece`. Models cannot contain themselves, so this ends.
fn add_instances(
    doc: &Document,
    e: &Entities,
    instances: &[InstanceId],
    parent: DMat4,
    piece: Option<usize>,
    out: &mut Gathered,
) {
    for id in instances {
        let Some(inst) = e.instance(*id) else {
            continue;
        };
        if inst.hidden || !doc.layer_visible(inst.layer) {
            continue;
        }
        let Some(inner) = doc.definition_entities(inst.definition) else {
            continue;
        };
        let world = parent * inst.transform;
        let name = if inst.name.is_empty() {
            doc.definition(inst.definition)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        } else {
            inst.name.clone()
        };
        let piece = piece.unwrap_or_else(|| {
            out.pieces += 1;
            out.pieces
        });
        let faces: Vec<FaceId> = inner.face_ids().collect();
        add_part(doc, inner, &faces, (name, piece), world, out);
        let nested: Vec<InstanceId> = inner.instance_ids().collect();
        add_instances(doc, inner, &nested, world, Some(piece), out);
    }
}

/// Adds the shown `faces` of `e`, placed by `world`, as one part with its
/// name and piece.
fn add_part(
    doc: &Document,
    e: &Entities,
    faces: &[FaceId],
    (name, piece): (String, usize),
    world: DMat4,
    out: &mut Gathered,
) {
    let normal_matrix = DMat3::from_mat4(world).inverse().transpose();
    let mirrored = DMat3::from_mat4(world).determinant() < 0.0;
    let to_world_n = |n: DVec3| (normal_matrix * n).normalize_or(DVec3::Z);
    let angle = Some(doc.look().smoothing_angle_deg.to_radians().cos());
    let mut part = Part {
        name,
        piece,
        faces: Vec::new(),
    };
    for f in faces {
        let Some(face) = e.face(*f) else { continue };
        if face.attributes.hidden || !doc.layer_visible(face.attributes.layer) {
            continue;
        }
        let n = face.plane().normal();
        let normals = e.corner_normals(*f, angle);
        let loops: Vec<Vec<VertexId>> = face.loops().map(|l| e.loop_vertices(l)).collect();
        let vertices: Vec<VertexId> = loops.iter().flatten().copied().collect();
        let points: Vec<DVec3> = vertices
            .iter()
            .map(|v| {
                e.vertex(*v)
                    .map_or(DVec3::ZERO, smith_model::Vertex::position)
            })
            .collect();
        // The front's paint, else the back's (seen from behind, so its
        // texture is laid out for the back's normal).
        let (paint, side_n) = match (face.attributes.front, face.attributes.back) {
            (Some(p), _) => (Some(p), n),
            (None, Some(p)) => (Some(p), -n),
            (None, None) => (None, n),
        };
        let uvs: Option<Vec<DVec2>> = paint.and_then(|p| {
            let size = doc.material(p.material)?.texture.as_ref()?.size_mm();
            let mm = e.paint_uvs(&p, side_n, &points)?;
            Some(mm.iter().map(|t| *t / size).collect())
        });
        let mut corners: Vec<Corner> = vertices
            .iter()
            .zip(&points)
            .enumerate()
            .map(|(i, (v, p))| Corner {
                vertex: *v,
                point: world.transform_point3(*p),
                normal: to_world_n(normals.get(v).copied().unwrap_or(n)),
                uv: uvs.as_ref().map(|u| u[i]),
            })
            .collect();
        let mut triangles: Vec<[usize; 3]> = e
            .face_triangles(*f)
            .iter()
            .filter_map(|t| {
                let at = |q: DVec3| points.iter().position(|p| p.to_array() == q.to_array());
                Some([at(t[0])?, at(t[1])?, at(t[2])?])
            })
            .collect();
        if mirrored {
            // A mirror turns every loop round: walk each back again, so the
            // outer loop stays counter-clockwise about the face's normal.
            let mut start = 0;
            for l in &loops {
                corners[start..start + l.len()].reverse();
                start += l.len();
            }
            let mut moved = vec![0; corners.len()];
            let mut start = 0;
            for l in &loops {
                for k in 0..l.len() {
                    moved[start + k] = start + l.len() - 1 - k;
                }
                start += l.len();
            }
            for t in &mut triangles {
                *t = [moved[t[0]], moved[t[2]], moved[t[1]]];
            }
        }
        part.faces.push(OutFace {
            corners,
            loops: loops.iter().map(Vec::len).collect(),
            triangles,
            normal: to_world_n(n),
            material: paint.map(|p| p.material),
            layer: face
                .attributes
                .layer
                .or_else(|| doc.base_layer())
                .and_then(|l| doc.layer(l))
                .map(|l| l.name.clone())
                .unwrap_or_default(),
        });
    }
    if !part.faces.is_empty() {
        out.parts.push(part);
    }
}

/// The file to write for the name chosen in the save dialog: the format's
/// extension added unless the name has it. `None` when the name, or the file
/// it becomes, is a native SketchSmith model, which an export never replaces.
pub fn target(chosen: &Path, format: Format) -> Option<PathBuf> {
    if is_native_model(chosen) {
        return None;
    }
    let has = chosen
        .extension()
        .is_some_and(|e| format.accepts(&e.to_string_lossy()));
    let path = if has {
        chosen.to_path_buf()
    } else {
        let mut name = chosen.as_os_str().to_owned();
        name.push(".");
        name.push(format.extension());
        PathBuf::from(name)
    };
    (!is_native_model(&path)).then_some(path)
}

/// Whether `path` is a native SketchSmith model: a binary glTF whose JSON
/// declares the model extension.
pub fn is_native_model(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return false;
    }
    let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let json = bytes.get(20..20 + len).unwrap_or(&[]);
    json.windows(crate::native::EXTENSION.len())
        .any(|w| w == crate::native::EXTENSION.as_bytes())
}

/// A name safe in a file name and in OBJ and MTL lines: letters, digits,
/// `-` and `_` kept, anything else `_`; never empty.
pub(crate) fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "unnamed".into() } else { s }
}

/// Each material used by `gathered`, in first-use order.
pub(crate) fn used_materials(gathered: &Gathered) -> Vec<MaterialId> {
    let mut seen = BTreeMap::new();
    let mut out = Vec::new();
    for f in gathered.parts.iter().flat_map(|p| &p.faces) {
        if let Some(m) = f.material
            && seen.insert(m, ()).is_none()
        {
            out.push(m);
        }
    }
    out
}

/// A number as text: up to six decimals, trailing zeros dropped.
pub(crate) fn num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.into()
    }
}

/// The Z-up → Y-up turn OBJ and glTF use: (x, y, z) becomes (x, z, −y).
pub(crate) fn y_up(p: DVec3) -> DVec3 {
    DVec3::new(p.x, p.z, -p.y)
}

/// Writes the companion file `path` atomically.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    crate::write_atomic(path, bytes, 0)
}

/// Clears the textures folder `dir` left by an earlier export of the same
/// name: its files are removed, and the folder too once empty. A folder
/// holding other folders is left as it is.
pub(crate) fn clear_textures(dir: &Path) -> Result<(), IoError> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    let entries: Vec<_> = entries.flatten().collect();
    if entries.iter().any(|e| e.path().is_dir()) {
        return Ok(());
    }
    for e in entries {
        fs::remove_file(e.path())?;
    }
    fs::remove_dir(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_follow_the_choice() {
        let o = ExportOptions::default();
        assert!((o.units_per_mm(Format::Obj) - 0.001).abs() < 1e-12);
        let print = ExportOptions {
            units: UnitPreset::Print,
            ..o
        };
        assert!((print.units_per_mm(Format::Obj) - 1.0).abs() < 1e-12);
        let inch = ExportOptions {
            units: UnitPreset::Custom,
            unit: FileUnit::In,
            scale: 1.0,
            ..o
        };
        assert!((25.4 * inch.units_per_mm(Format::Obj) - 1.0).abs() < 1e-12);
        let hundredth = ExportOptions {
            units: UnitPreset::Custom,
            unit: FileUnit::Mm,
            scale: 100.0,
            ..o
        };
        assert!((5000.0 * hundredth.units_per_mm(Format::Obj) - 50.0).abs() < 1e-9);
        // Units apply only where the format has them.
        assert!((print.units_per_mm(Format::Gltf) - 0.001).abs() < 1e-12);
        assert!((hundredth.units_per_mm(Format::ThreeMf) - 0.01).abs() < 1e-12);
        // A bad scale counts as 1:1.
        let bad = ExportOptions {
            scale: 0.0,
            ..hundredth
        };
        assert!((bad.units_per_mm(Format::Obj) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn numbers_are_short() {
        assert_eq!(num(10.0), "10");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(0.0254), "0.0254");
        assert_eq!(num(-1.5), "-1.5");
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("Oak Wood #2"), "Oak_Wood__2");
        assert_eq!(safe_name(""), "unnamed");
        assert_eq!(safe_name("steel-01_a"), "steel-01_a");
    }

    #[test]
    fn target_adds_the_extension_and_spares_models() {
        let dir = std::env::temp_dir().join(format!("smith-io-target-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            target(&dir.join("chair"), Format::Obj),
            Some(dir.join("chair.obj"))
        );
        assert_eq!(
            target(&dir.join("chair.OBJ"), Format::Obj),
            Some(dir.join("chair.OBJ"))
        );
        let model = dir.join("chair.glb");
        let bytes = crate::write_parts(&Document::new(), None, None, false).unwrap();
        fs::write(&model, bytes).unwrap();
        assert!(is_native_model(&model));
        assert_eq!(target(&model, Format::Obj), None);
        // A native model renamed to the export's name is spared too.
        fs::copy(&model, dir.join("bench.obj")).unwrap();
        assert_eq!(target(&dir.join("bench"), Format::Obj), None);
        assert!(!is_native_model(&dir.join("missing.glb")));
        fs::remove_dir_all(&dir).unwrap();
    }
}
