//! SketchSmith — model imports | v1.0.0 | Thursday 12 November 2026
//!
//! Brings other programs' files into the model, mirroring the exports so a
//! round trip comes back the same: OBJ (with its MTL materials and images),
//! STL, DAE (COLLADA), SVG, glTF, 3MF and SKP (2013–2020 files, through
//! `smith-skp`). An SKP file keeps its own faces (with their holes), edge
//! flags, curves, layers, groups and components, its paint, front and
//! back, its guides, its section planes (inactive) and its text: labels on
//! leaders tied to the points they mark, and screen notes. A mesh of triangles (STL, DAE,
//! glTF, 3MF) is tidied once built: coplanar neighbours with the same paint join
//! and shallow edges soften, each as the window chooses. A drawing (SVG)
//! is drawn instead, as the Draw tools would: its lines and curves stick and
//! cut where they cross and close into faces, and the faces outside the
//! shapes' fills (holes, unfilled shapes) are taken away again.
//!
//! An import happens in two steps:
//!
//! 1. [`read_import`] reads the file (and, for OBJ, its MTL libraries and
//!    their images) into an [`Imported`]: each object's polygons, in model
//!    millimetres with Z up (OBJ is Y up, as Blender writes it), scaled by
//!    the Units chosen; the materials, with their images loaded; and what
//!    could not be found.
//! 2. [`place`] adds it to the model, inside one transaction the caller
//!    opens: one group named after the file, with one group inside per
//!    object (or per connected piece, with Separate Shells), built directly
//!    (see `smith_model`'s import building), placed at the drawing axes in
//!    the open context. A file with a root of its own (SKP) builds it straight
//!    into that group. Layers named in the file join the model's layers of the
//!    same name (made when missing). Materials join the model (a name already used gets
//!    a number, `Oak 2`) and paint the faces' fronts, each texture laid by a frame
//!    fitted to the face's texture coordinates. A polygon's back material
//!    paints the face's back (SKP). A two-sided file draws each
//!    face again reversed: that copy, painted, paints the face's back.
//!
//! [`ImportReport`] says what came in and what did not.

mod dae;
mod gltf;
mod obj;
mod skp;
mod stl;
mod svg;
mod threemf;

use std::path::Path;

use glam::{DMat4, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use smith_model::{
    Anchor, Annotation, DefinitionId, DefinitionKind, Document, FaceAttributes, Guide,
    ImportCounts, ImportedEdge, InstanceId, LayerId, LeaderText, Mapping, Material, ModelError,
    Paint, Pickable, SectionPlane, Side, Texture, TextureFrame,
};

use crate::error::IoError;
use crate::export::{FileUnit, UnitPreset};

/// An import format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// Wavefront OBJ, with its MTL file.
    Obj,
    /// STL, binary or text.
    Stl,
    /// COLLADA.
    Dae,
    /// SVG drawing.
    Svg,
    /// glTF 2.0, `.gltf` or `.glb`.
    Gltf,
    /// 3MF, core.
    ThreeMf,
    /// SKP (2013–2020).
    Skp,
}

impl ImportFormat {
    /// The formats, in menu order.
    pub const ALL: [ImportFormat; 7] = [
        Self::Obj,
        Self::Stl,
        Self::Dae,
        Self::Svg,
        Self::Gltf,
        Self::ThreeMf,
        Self::Skp,
    ];

    /// The format's short name, as the menu shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Obj => "OBJ",
            Self::Stl => "STL",
            Self::Dae => "DAE",
            Self::Svg => "SVG",
            Self::Gltf => "glTF",
            Self::ThreeMf => "3MF",
            Self::Skp => "SKP",
        }
    }

    /// The file extensions the open dialog shows, without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Obj => &["obj"],
            Self::Stl => &["stl"],
            Self::Dae => &["dae"],
            Self::Svg => &["svg"],
            Self::Gltf => &["gltf", "glb"],
            Self::ThreeMf => &["3mf"],
            Self::Skp => &["skp"],
        }
    }

    /// The open dialog's filter name.
    pub fn filter(self) -> &'static str {
        match self {
            Self::Obj => "Wavefront OBJ (.obj)",
            Self::Stl => "STL (.stl)",
            Self::Dae => "COLLADA (.dae)",
            Self::Svg => "SVG (.svg)",
            Self::Gltf => "glTF (.gltf, .glb)",
            Self::ThreeMf => "3MF (.3mf)",
            Self::Skp => "SKP (.skp)",
        }
    }

    /// Whether it is a mesh of triangles, tidied once built (the Merge
    /// Coplanar Faces and Soften Curved Surfaces choices).
    pub fn is_mesh(self) -> bool {
        matches!(self, Self::Stl | Self::Dae | Self::Gltf | Self::ThreeMf)
    }

    /// Whether the file names its own unit (the From File choice).
    pub fn has_own_units(self) -> bool {
        matches!(
            self,
            Self::Dae | Self::Svg | Self::Gltf | Self::ThreeMf | Self::Skp
        )
    }

    /// Whether Units offers only From File and Custom (a drawing's size, or
    /// a file that always stores one unit).
    pub fn file_or_custom(self) -> bool {
        matches!(self, Self::Svg | Self::Skp)
    }

    /// Whether Separate Shells applies (not to a drawing, nor to a file
    /// that keeps its own groups).
    pub fn has_shells(self) -> bool {
        !matches!(self, Self::Svg | Self::Skp)
    }
}

/// The values chosen in the import window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportOptions {
    /// Units.
    pub units: UnitPreset,
    /// Custom's unit.
    pub unit: FileUnit,
    /// Custom's Scale 1:N, the N: the file is a 1:N drawing.
    pub scale: f64,
    /// Each connected piece its own group.
    pub separate_shells: bool,
    /// Units From File (DAE, SVG, glTF): the file's own unit and up axis.
    pub from_file: bool,
    /// Merge Coplanar Faces (STL, DAE, glTF).
    pub merge_coplanar: bool,
    /// Soften Curved Surfaces (STL, DAE, glTF).
    pub soften: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            units: UnitPreset::Blender,
            unit: FileUnit::Mm,
            scale: 1.0,
            separate_shells: false,
            from_file: true,
            merge_coplanar: true,
            soften: true,
        }
    }
}

impl ImportOptions {
    /// The defaults for `format`: Print (mm) for STL, which has no unit;
    /// Blender (m) otherwise.
    pub fn for_format(format: ImportFormat) -> Self {
        Self {
            units: if format == ImportFormat::Stl {
                UnitPreset::Print
            } else {
                UnitPreset::Blender
            },
            ..Self::default()
        }
    }

    /// Model millimetres per file unit.
    pub fn mm_per_unit(&self) -> f64 {
        let n = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        };
        match self.units {
            UnitPreset::Blender => 1000.0,
            UnitPreset::Print => 1.0,
            UnitPreset::Custom => self.unit.mm() * n,
        }
    }
}

/// One polygon to build: corners in model mm, and its texture coordinates,
/// material (an index into [`Imported::materials`]), holes and layer (an
/// index into [`Imported::layers`]) when it has them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Polygon {
    /// Corners, in order (their winding is the front).
    pub points: Vec<DVec3>,
    /// Texture coordinates of the corners, in repeats.
    pub uvs: Option<Vec<DVec2>>,
    /// Its material.
    pub material: Option<usize>,
    /// Its back's material (laid by the default frame).
    pub back_material: Option<usize>,
    /// Holes, each a loop of corners.
    pub holes: Vec<Vec<DVec3>>,
    /// Its layer (`None`: Base).
    pub layer: Option<usize>,
}

/// An edge carrying its file's own flags and layer (an index into
/// [`Imported::layers`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MarkedEdge {
    /// Its ends.
    pub ends: [DVec3; 2],
    /// Soft.
    pub soft: bool,
    /// Smooth.
    pub smooth: bool,
    /// Hidden.
    pub hidden: bool,
    /// Its layer (`None`: Base).
    pub layer: Option<usize>,
}

/// A curve the file's edges were drawn from: the curve, its segment count,
/// and its edges, each by its ends.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeCurve {
    /// The curve.
    pub curve: smith_geom::curve::Curve,
    /// Its segment count.
    pub segments: u32,
    /// Its edges.
    pub edges: Vec<[DVec3; 2]>,
}

/// A line or curve to draw, as the Draw tools would.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawn {
    /// A straight line.
    Line(DVec3, DVec3),
    /// A curve, with its segment count.
    Curve(smith_geom::curve::Curve, u32),
}

/// A filled shape: its outlines on the group's ground plane and whether it
/// fills by the even-odd rule (else non-zero).
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    /// Each outline, as points.
    pub loops: Vec<Vec<DVec2>>,
    /// Even-odd (`true`) or non-zero.
    pub even_odd: bool,
}

impl Fill {
    /// Whether the shape covers `p`.
    pub fn contains(&self, p: DVec2) -> bool {
        let mut winding = 0i32;
        let mut crossings = 0u32;
        for l in &self.loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if (a.y <= p.y) != (b.y <= p.y) {
                    let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if x > p.x {
                        crossings += 1;
                        winding += if b.y > a.y { 1 } else { -1 };
                    }
                }
            }
        }
        if self.even_odd {
            crossings % 2 == 1
        } else {
            winding != 0
        }
    }
}

/// Text to place.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportText {
    /// A label on a leader from `anchor` (tied to the vertex there, if
    /// any), placed `label` from it.
    Leader {
        /// The point it marks.
        anchor: DVec3,
        /// The label's place, from the anchor.
        label: DVec3,
        /// What it says.
        text: String,
    },
    /// A note on the canvas, at `at` (0–1 across and down) when the file
    /// says where.
    Screen {
        /// Where it sits, if known.
        at: Option<[f64; 2]>,
        /// What it says.
        text: String,
    },
}

/// One group to build.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportGroup {
    /// Its name.
    pub name: String,
    /// Where it sits in the group holding it.
    pub transform: DMat4,
    /// Its faces.
    pub polygons: Vec<Polygon>,
    /// Its lines, as segments.
    pub lines: Vec<[DVec3; 2]>,
    /// Groups inside it.
    pub children: Vec<ImportGroup>,
    /// Copies of components inside it: an index into
    /// [`Imported::components`], where, a name, and a layer.
    pub uses: Vec<(usize, DMat4, String, Option<usize>)>,
    /// The layer it is placed on, as a group inside another.
    pub layer: Option<usize>,
    /// Edges with the file's own flags and layers, made if missing.
    pub edges: Vec<MarkedEdge>,
    /// Curves its edges were drawn from.
    pub curves: Vec<EdgeCurve>,
    /// Lines and curves to draw (a drawing's shapes).
    pub drawn: Vec<Drawn>,
    /// The filled shapes: faces the drawing closes outside them go.
    pub fills: Vec<Fill>,
    /// Guide lines and guide points.
    pub guides: Vec<Guide>,
    /// Section planes, which come in inactive.
    pub sections: Vec<SectionPlane>,
    /// Labels and notes.
    pub texts: Vec<ImportText>,
}

impl Default for ImportGroup {
    fn default() -> Self {
        Self {
            name: String::new(),
            transform: DMat4::IDENTITY,
            polygons: Vec::new(),
            lines: Vec::new(),
            children: Vec::new(),
            uses: Vec::new(),
            layer: None,
            edges: Vec::new(),
            curves: Vec::new(),
            drawn: Vec::new(),
            fills: Vec::new(),
            guides: Vec::new(),
            sections: Vec::new(),
            texts: Vec::new(),
        }
    }
}

/// A file read, ready to place.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Imported {
    /// The file's name, without its extension: the outer group's name.
    pub name: String,
    /// Its own root's geometry, groups and copies, built straight into the
    /// file's group (SKP).
    pub loose: ImportGroup,
    /// The groups inside it.
    pub groups: Vec<ImportGroup>,
    /// Components, made once each and placed where groups use them.
    pub components: Vec<ImportGroup>,
    /// Join coplanar neighbours, a mesh of triangles' tidying.
    pub merge_coplanar: bool,
    /// Soften edges on curved surfaces.
    pub soften: bool,
    /// Its materials.
    pub materials: Vec<Material>,
    /// The layers it names: name and colour.
    pub layers: Vec<(String, [u8; 3])>,
    /// Points in the file, which are not imported.
    pub points: usize,
    /// Images named by the materials that could not be found or read.
    pub missing: Vec<String>,
    /// Holes in polygons, which are not imported.
    pub holes: usize,
    /// Cameras and lights, which are not imported.
    pub skipped: usize,
    /// Texts, which are not imported (they need converting to paths).
    pub texts: usize,
    /// Images, which are not imported.
    pub images: usize,
    /// Compressed meshes (Draco, meshopt), which cannot be read.
    pub compressed: usize,
    /// Faces whose texture the file positions but which are laid by the
    /// default frame.
    pub approximate: usize,
    /// Component copies whose own paint could not come in (copies of one
    /// component painted differently).
    pub copy_paint: usize,
}

/// What an import brought in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportReport {
    /// Faces made and polygons skipped.
    pub counts: ImportCounts,
    /// Groups made inside the file's group.
    pub groups: usize,
    /// Points not imported.
    pub points: usize,
    /// Images not found.
    pub missing: Vec<String>,
    /// Faces whose texture could only be laid approximately.
    pub approximate: usize,
    /// Holes in polygons, not imported.
    pub holes: usize,
    /// Cameras and lights, not imported.
    pub skipped: usize,
    /// Texts, not imported.
    pub texts: usize,
    /// Images, not imported.
    pub images: usize,
    /// Compressed meshes, not imported.
    pub compressed: usize,
    /// Component copies' paint, not imported.
    pub copy_paint: usize,
}

/// `n` with a space between thousands (`1 240`).
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// "`n` thing" or "`n` things".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", grouped(n), if n == 1 { one } else { many })
}

impl ImportReport {
    /// The bottom line after importing `file`.
    pub fn summary(&self, file: &str) -> String {
        // Faces as they stand, once coplanar ones have joined.
        let faces = self.counts.faces.saturating_sub(self.counts.merged);
        format!(
            "Imported {file}: {}, {}",
            count(faces, "Face", "Faces"),
            count(self.groups, "Object", "Objects")
        )
    }

    /// The Import Report's lines; empty when there is nothing to tell.
    pub fn lines(&self) -> Vec<String> {
        let c = &self.counts;
        let mut out = Vec::new();
        let mut add = |n: usize, one: &str, many: &str| {
            if n > 0 {
                out.push(count(n, one, many));
            }
        };
        add(c.merged, "Triangle Merged", "Triangles Merged");
        add(c.softened, "Edge Softened", "Edges Softened");
        add(c.bent, "Bent Face Triangulated", "Bent Faces Triangulated");
        add(
            c.zero_area,
            "Zero-Area Face Skipped",
            "Zero-Area Faces Skipped",
        );
        add(
            c.repeated,
            "Repeated Face Skipped",
            "Repeated Faces Skipped",
        );
        add(self.points, "Point Skipped", "Points Skipped");
        add(
            self.approximate,
            "Texture Placed Approximately",
            "Textures Placed Approximately",
        );
        add(c.lines, "Line Imported as Edges", "Lines Imported as Edges");
        add(self.holes, "Polygon Hole Skipped", "Polygon Holes Skipped");
        add(
            self.skipped,
            "Camera or Light Skipped",
            "Cameras and Lights Skipped",
        );
        add(
            self.texts,
            "Text Skipped: Convert Text to Paths First",
            "Texts Skipped: Convert Text to Paths First",
        );
        add(self.images, "Image Skipped", "Images Skipped");
        add(
            self.compressed,
            "Compressed Mesh Skipped",
            "Compressed Meshes Skipped",
        );
        add(
            self.copy_paint,
            "Component Copy's Paint Skipped",
            "Component Copies' Paint Skipped",
        );
        if !self.missing.is_empty() {
            out.push(format!(
                "{}: {}",
                count(
                    self.missing.len(),
                    "Texture Not Found",
                    "Textures Not Found"
                ),
                self.missing.join(", ")
            ));
        }
        out
    }
}

/// Reads `path` as `format` with `options` (see the module documentation).
///
/// # Errors
/// [`IoError::Os`] when the file cannot be read; [`IoError::Damaged`] when
/// it is malformed.
pub fn read_import(
    path: &Path,
    format: ImportFormat,
    options: &ImportOptions,
) -> Result<Imported, IoError> {
    match format {
        ImportFormat::Obj => read_obj(path, options),
        ImportFormat::Stl => stl::read_stl(path, options),
        ImportFormat::Dae => dae::read_dae(path, options),
        ImportFormat::Svg => svg::read_svg(path, options),
        ImportFormat::Gltf => gltf::read_gltf(path, options),
        ImportFormat::ThreeMf => threemf::read_3mf(path, options),
        ImportFormat::Skp => skp::read_skp(path, options),
    }
}

/// OBJ's Y up to Z up: (x, y, z) becomes (x, −z, y), the reverse of export.
fn z_up(p: DVec3) -> DVec3 {
    DVec3::new(p.x, -p.z, p.y)
}

fn read_obj(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let data = obj::parse_obj(&String::from_utf8_lossy(&bytes))?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let dir = path.parent().unwrap_or(Path::new("."));

    // Materials from every library found, each image loaded.
    let mut imported = Imported {
        name: stem.clone(),
        points: data.points,
        ..Imported::default()
    };
    let mut names: Vec<String> = Vec::new();
    for lib in &data.mtllibs {
        let Ok(text) = std::fs::read(dir.join(lib)) else {
            continue;
        };
        for m in obj::parse_mtl(&String::from_utf8_lossy(&text)) {
            let texture = m.map.as_ref().and_then(|file| {
                let found = [dir.join(file), dir.join(Path::new(file).file_name()?)]
                    .into_iter()
                    .find_map(|p| crate::textures::load_image(&p).ok());
                if found.is_none() && !imported.missing.contains(file) {
                    imported.missing.push(file.clone());
                }
                // The repeat size is set once a face shows it.
                found.map(|image| Texture {
                    image: std::sync::Arc::new(image),
                    width_mm: 100.0,
                })
            });
            names.push(m.name.clone());
            imported.materials.push(Material {
                name: m.name,
                color: m.color.map(|c| c.clamp(0.0, 2.0)),
                opacity: m.opacity,
                metallic: m.metallic,
                texture,
            });
        }
    }

    let k = options.mm_per_unit();
    for (i, o) in data.objects.into_iter().enumerate() {
        let name = if o.name.is_empty() {
            if i == 0 {
                stem.clone()
            } else {
                format!("{stem} {}", i + 1)
            }
        } else {
            o.name
        };
        let polygons = o
            .faces
            .into_iter()
            .map(|f| Polygon {
                points: f.points.into_iter().map(|p| z_up(p) * k).collect(),
                uvs: f.uvs,
                material: f.material.and_then(|m| names.iter().position(|n| *n == m)),
                ..Polygon::default()
            })
            .collect();
        let lines = o
            .lines
            .into_iter()
            .map(|[a, b]| [z_up(a) * k, z_up(b) * k])
            .collect();
        imported.groups.push(ImportGroup {
            name,
            polygons,
            lines,
            ..ImportGroup::default()
        });
    }
    if options.separate_shells {
        imported.groups = imported.groups.into_iter().flat_map(separate).collect();
    }
    Ok(imported)
}

/// `group` split into its connected pieces (polygons and lines joined
/// through shared corners), each named after it and numbered; one piece
/// keeps the name as it is.
pub(super) fn separate(group: ImportGroup) -> Vec<ImportGroup> {
    let key = |p: DVec3| {
        let q = (p / smith_geom::tolerance::TOL_MERGE).round();
        (q.x as i64, q.y as i64, q.z as i64)
    };
    let n = group.polygons.len() + group.lines.len();
    let mut root: Vec<usize> = (0..n).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    let mut first: std::collections::HashMap<(i64, i64, i64), usize> =
        std::collections::HashMap::new();
    let corners = group
        .polygons
        .iter()
        .map(|p| p.points.clone())
        .chain(group.lines.iter().map(|l| l.to_vec()));
    for (i, pts) in corners.enumerate() {
        for p in pts {
            let at = *first.entry(key(p)).or_insert(i);
            let (a, b) = (find(&mut root, at), find(&mut root, i));
            root[a] = b;
        }
    }
    let mut pieces: Vec<(usize, ImportGroup)> = Vec::new();
    let np = group.polygons.len();
    let piece = |pieces: &mut Vec<(usize, ImportGroup)>, r: usize| -> usize {
        match pieces.iter().position(|(k, _)| *k == r) {
            Some(i) => i,
            None => {
                pieces.push((r, ImportGroup::default()));
                pieces.len() - 1
            }
        }
    };
    for (i, poly) in group.polygons.into_iter().enumerate() {
        let r = find(&mut root, i);
        let at = piece(&mut pieces, r);
        pieces[at].1.polygons.push(poly);
    }
    for (i, line) in group.lines.into_iter().enumerate() {
        let r = find(&mut root, np + i);
        let at = piece(&mut pieces, r);
        pieces[at].1.lines.push(line);
    }
    let many = pieces.len() > 1;
    pieces
        .into_iter()
        .enumerate()
        .map(|(i, (_, mut g))| {
            g.name = if many {
                format!("{} {}", group.name, i + 1)
            } else {
                group.name.clone()
            };
            g
        })
        .collect()
}

/// A group's own polygons split into its connected pieces, as groups inside
/// it (a single piece stays as it is).
pub(super) fn split(mut g: ImportGroup) -> ImportGroup {
    let own = ImportGroup {
        name: g.name.clone(),
        polygons: std::mem::take(&mut g.polygons),
        lines: std::mem::take(&mut g.lines),
        ..ImportGroup::default()
    };
    let pieces = separate(own);
    if pieces.len() == 1 {
        let mut one = pieces.into_iter().next().unwrap_or_default();
        g.polygons = std::mem::take(&mut one.polygons);
        g.lines = std::mem::take(&mut one.lines);
    } else {
        g.children.splice(0..0, pieces);
    }
    g
}

/// A path from an image's `init_from`: `file://` taken off and `%xx`
/// escapes undone.
pub(super) fn url_path(s: &str) -> String {
    let s = s.strip_prefix("file://").unwrap_or(s);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The texture frame laying an image `size` (mm, width × height) on a face
/// so its corners `points` get texture coordinates `uvs` (in repeats), and
/// whether it does so exactly. `size.x` of 0 takes the width from the face.
fn fit_frame(
    points: &[DVec3],
    uvs: &[DVec2],
    normal: DVec3,
    mut size: DVec2,
    aspect: f64,
) -> Option<(TextureFrame, DVec2, bool)> {
    // Coordinates in the face's plane, from the first corner.
    let e1 = normal.any_orthonormal_vector();
    let e2 = normal.cross(e1);
    let x: Vec<DVec2> = points
        .iter()
        .map(|p| DVec2::new((*p - points[0]).dot(e1), (*p - points[0]).dot(e2)))
        .collect();
    // An affine map from the first three corners that are not in a line.
    let (i, j) = (1..x.len())
        .flat_map(|i| (i + 1..x.len()).map(move |j| (i, j)))
        .find(|&(i, j)| x[i].perp_dot(x[j]).abs() > 1e-9)?;
    let m = glam::DMat2::from_cols(x[i], x[j]);
    let du = glam::DMat2::from_cols(uvs[i] - uvs[0], uvs[j] - uvs[0]);
    // uv = A x + uv0, with A = du · m⁻¹ (rows: the u and v gradients).
    let a = du * m.inverse();
    let (gu, gv) = (a.row(0), a.row(1));
    if size.x <= 0.0 {
        let len = gu.length();
        if len < 1e-12 {
            return None;
        }
        size = DVec2::new(1.0 / len, aspect / len);
    }
    // In mm: rows times the repeat size should be unit and square.
    let (u2, v2) = (gu * size.x, gv * size.y);
    let u = (e1 * u2.x + e2 * u2.y).normalize_or(e1);
    // v lies in the face, square to u: one way or the other, as the
    // coordinates run (exactly square even when they barely do).
    let v_raw = e1 * v2.x + e2 * v2.y;
    let side = normal.cross(u).normalize_or(e2);
    let v = if side.dot(v_raw) < 0.0 { -side } else { side };
    let origin = points[0] - u * (uvs[0].x * size.x) - v * (uvs[0].y * size.y);
    let frame = TextureFrame { origin, u, v };
    let exact = points.iter().zip(uvs).all(|(p, t)| {
        let got = frame.uv(*p) / size;
        (got - *t).length() < 1e-4
    });
    Some((frame, size, exact))
}

/// The direction `points` wind round (Newell's method; not unit length).
fn winding(points: &[DVec3]) -> DVec3 {
    let n = points.len();
    (0..n).fold(DVec3::ZERO, |acc, i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        acc + DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        )
    })
}

/// Building an import: the materials added so far and their repeat sizes,
/// the layers found or made, the components made, and the report.
struct Builder<'a> {
    imported: &'a Imported,
    materials: Vec<Option<smith_model::MaterialId>>,
    sizes: Vec<DVec2>,
    layers: Vec<Option<LayerId>>,
    components: Vec<Option<DefinitionId>>,
    report: ImportReport,
    /// Screen notes placed down the top left so far.
    notes: usize,
}

impl Builder<'_> {
    /// The model's material for `m`, added the first time it is used (a
    /// name already used gets a number: the model's rule).
    fn material(
        &mut self,
        doc: &mut Document,
        m: usize,
    ) -> Result<smith_model::MaterialId, ModelError> {
        if let Some(id) = self.materials[m] {
            return Ok(id);
        }
        let id = doc.add_material(self.imported.materials[m].clone())?;
        self.materials[m] = Some(id);
        Ok(id)
    }

    /// The model's layer for `l`: the one with its name, made the first
    /// time it is needed when there is none.
    fn layer(&mut self, doc: &mut Document, l: Option<usize>) -> Option<LayerId> {
        let l = l?;
        if let Some(id) = self.layers.get(l).copied().flatten() {
            return Some(id);
        }
        let (name, color) = self.imported.layers.get(l)?;
        let found = doc
            .layer_ids()
            .find(|id| doc.layer(*id).is_some_and(|x| x.name == *name));
        let id = found.unwrap_or_else(|| doc.add_layer(name, *color));
        if let Some(slot) = self.layers.get_mut(l) {
            *slot = Some(id);
        }
        Some(id)
    }

    /// Makes `g` as a definition of `kind` (see [`Builder::fill`]); `None`
    /// when it holds nothing.
    fn build(
        &mut self,
        doc: &mut Document,
        g: &ImportGroup,
        kind: DefinitionKind,
    ) -> Result<Option<DefinitionId>, ModelError> {
        let def = doc.create_definition(g.name.clone(), kind);
        let (holds, _) = self.fill(doc, def, g)?;
        Ok(holds.then_some(def))
    }

    /// Builds `g` into `def`: its geometry, painted, tidied, then its
    /// groups and component copies. Returns whether `def` holds anything,
    /// and how many groups and copies went in.
    fn fill(
        &mut self,
        doc: &mut Document,
        def: DefinitionId,
        g: &ImportGroup,
    ) -> Result<(bool, usize), ModelError> {
        // The layers it uses, found or made before building.
        let mut face_layers = Vec::with_capacity(g.polygons.len());
        for poly in &g.polygons {
            face_layers.push(self.layer(doc, poly.layer));
        }
        let mut edge_layers = Vec::with_capacity(g.edges.len());
        for m in &g.edges {
            edge_layers.push(self.layer(doc, m.layer));
        }
        // Each face side to paint, its material, and the polygon it came
        // from.
        let mut faces: Vec<(smith_model::FaceId, Side, usize, &Polygon)> = Vec::new();
        {
            let e = doc
                .definition_entities_mut(def)
                .ok_or(ModelError::NoSuchEntity)?;
            for (poly, layer) in g.polygons.iter().zip(face_layers) {
                let repeats = self.report.counts.repeated;
                let made = e.add_polygon_with_holes(
                    &poly.points,
                    &poly.holes,
                    &FaceAttributes {
                        layer,
                        ..FaceAttributes::default()
                    },
                    &mut self.report.counts,
                );
                // The back's own material (no texture coordinates of its
                // own: laid by the default frame).
                if let Some(b) = poly.back_material {
                    for f in &made {
                        faces.push((*f, Side::Back, b, poly));
                    }
                }
                let Some(m) = poly.material else { continue };
                for f in made {
                    faces.push((f, Side::Front, m, poly));
                }
                // A painted copy of a face, reversed: the face's back, if
                // nothing has painted it yet.
                if self.report.counts.repeated > repeats
                    && let Some(f) = e.repeated_face(&poly.points)
                    && let Some(face) = e.face(f)
                    && winding(&poly.points).dot(face.plane().normal()) < 0.0
                    && !faces.iter().any(|(g, s, ..)| *g == f && *s == Side::Back)
                {
                    faces.push((f, Side::Back, m, poly));
                    self.report.counts.repeated -= 1;
                }
            }
            for [a, b] in &g.lines {
                e.add_segment(*a, *b, &mut self.report.counts);
            }
            for (m, layer) in g.edges.iter().zip(edge_layers) {
                let flags = ImportedEdge {
                    soft: m.soft,
                    smooth: m.smooth,
                    hidden: m.hidden,
                    layer,
                };
                e.import_edge(m.ends[0], m.ends[1], flags);
            }
            for c in &g.curves {
                // Edges that did not come in (or are on a curve already)
                // stay plain.
                e.import_curve(c.curve.clone(), c.segments, &c.edges);
            }
            for guide in &g.guides {
                e.add_guide(*guide)?;
            }
            // Adding a plane makes it active: none is, once in.
            for plane in &g.sections {
                e.add_section(*plane)?;
            }
            if !g.sections.is_empty() {
                e.set_active_section(None)?;
            }
            // Labels tie to the vertices and edges built above; notes
            // without a place go down the top left, one under another.
            for t in &g.texts {
                let a = match t {
                    ImportText::Leader {
                        anchor,
                        label,
                        text,
                    } => Annotation::Leader {
                        anchor: Anchor::attach(e, *anchor),
                        label: *label,
                        text: LeaderText::Custom(text.clone()),
                    },
                    ImportText::Screen { at, text } => {
                        let at = at.unwrap_or_else(|| {
                            #[allow(clippy::cast_precision_loss)]
                            let down = (0.06 + 0.05 * self.notes as f64).min(0.95);
                            self.notes += 1;
                            [0.15, down]
                        });
                        Annotation::Screen {
                            at,
                            text: text.clone(),
                        }
                    }
                };
                // Text that will not form (empty, not finite) stays out.
                let _ = e.add_annotation(a);
            }
        }
        for (f, side, m, poly) in faces {
            let id = self.material(doc, m)?;
            let e = doc
                .definition_entities_mut(def)
                .ok_or(ModelError::NoSuchEntity)?;
            let Some(face) = e.face(f) else { continue };
            // The side's own normal: the back looks the other way.
            let normal = match side {
                Side::Front => face.plane().normal(),
                Side::Back => -face.plane().normal(),
            };
            let mut frame = TextureFrame::for_side(normal, DVec2::ZERO, 0.0);
            let textured = self.imported.materials[m].texture.as_ref();
            // A back painted by the polygon's back material has no
            // coordinates of its own.
            let own_side = !(side == Side::Back && poly.back_material == Some(m));
            if let (Some(t), Some(uvs), true) = (textured, poly.uvs.as_ref(), own_side)
                && uvs.len() == poly.points.len()
            {
                let [w, h] = t.image.pixels;
                let aspect = if w == 0 {
                    1.0
                } else {
                    f64::from(h) / f64::from(w)
                };
                // A split face's triangles share the polygon's frame.
                match fit_frame(&poly.points, uvs, normal, self.sizes[m], aspect) {
                    Some((fit, size, exact)) => {
                        frame = fit;
                        if self.sizes[m].x <= 0.0 {
                            self.sizes[m] = size;
                        }
                        if !exact {
                            self.report.approximate += 1;
                        }
                    }
                    None => self.report.approximate += 1,
                }
            }
            e.set_face_paint(
                f,
                side,
                Some(Paint {
                    material: id,
                    mapping: Mapping::Face(frame),
                }),
            )?;
        }
        // Tidy a mesh of triangles, once painted (merging keeps paint apart).
        let angle = doc.look().smoothing_angle_deg.to_radians();
        let e = doc
            .definition_entities_mut(def)
            .ok_or(ModelError::NoSuchEntity)?;
        // A drawing: drawn as the Draw tools would, then its faces outside
        // every fill taken away (their edges stay).
        if !g.drawn.is_empty() {
            for d in &g.drawn {
                // A piece too small to draw is left out.
                let _ = match d {
                    Drawn::Line(a, b) => e.add_line(*a, *b).map(|_| ()),
                    Drawn::Curve(c, n) => e.add_curve(c.clone(), *n).map(|_| ()),
                };
            }
            let faces: Vec<smith_model::FaceId> = e.face_ids().collect();
            for f in faces {
                let inside = e
                    .face_triangles(f)
                    .first()
                    .map(|[a, b, c]| ((*a + *b + *c) / 3.0).truncate());
                let filled = inside.is_some_and(|p| g.fills.iter().any(|s| s.contains(p)));
                if filled {
                    self.report.counts.faces += 1;
                } else {
                    e.erase_face(f)?;
                }
            }
        }
        if self.imported.merge_coplanar {
            self.report.counts.merged += e.merge_coplanar_faces();
        }
        if self.imported.soften {
            self.report.counts.softened += e.soften_curved(angle);
        }
        let mut holds = e.face_ids().next().is_some() || e.edge_ids().next().is_some();
        let mut placed = 0;
        for child in &g.children {
            if let Some(c) = self.build(doc, child, DefinitionKind::Group)? {
                let i = doc.add_instance(Some(def), c, child.transform, child.name.clone())?;
                self.put_on_layer(doc, def, i, child.layer)?;
                holds = true;
                placed += 1;
            }
        }
        for (k, transform, name, layer) in &g.uses {
            let comp = match self.components.get(*k).copied().flatten() {
                Some(c) => Some(c),
                None => {
                    let made = match self.imported.components.get(*k) {
                        Some(c) => self.build(doc, c, DefinitionKind::Component)?,
                        None => None,
                    };
                    if let Some(slot) = self.components.get_mut(*k) {
                        *slot = made;
                    }
                    made
                }
            };
            if let Some(c) = comp {
                let i = doc.add_instance(Some(def), c, *transform, name.clone())?;
                self.put_on_layer(doc, def, i, *layer)?;
                holds = true;
                placed += 1;
            }
        }
        Ok((holds, placed))
    }

    /// Puts copy `i` in `def` on layer `l`, when it names one.
    fn put_on_layer(
        &mut self,
        doc: &mut Document,
        def: DefinitionId,
        i: InstanceId,
        l: Option<usize>,
    ) -> Result<(), ModelError> {
        let Some(layer) = self.layer(doc, l) else {
            return Ok(());
        };
        doc.definition_entities_mut(def)
            .ok_or(ModelError::NoSuchEntity)?
            .set_layer(&[Pickable::Instance(i)], Some(layer))
    }
}

/// Adds `imported` to `doc` in context `ctx`, placed by `at`, inside the
/// caller's transaction. Returns the file's group and what came in.
///
/// # Errors
/// [`ModelError`] from the model.
pub fn place(
    doc: &mut Document,
    ctx: Option<DefinitionId>,
    at: DMat4,
    imported: &Imported,
) -> Result<(InstanceId, ImportReport), ModelError> {
    let mut b = Builder {
        imported,
        materials: vec![None; imported.materials.len()],
        sizes: vec![DVec2::ZERO; imported.materials.len()],
        layers: vec![None; imported.layers.len()],
        components: vec![None; imported.components.len()],
        notes: 0,
        report: ImportReport {
            points: imported.points,
            missing: imported.missing.clone(),
            holes: imported.holes,
            skipped: imported.skipped,
            texts: imported.texts,
            images: imported.images,
            compressed: imported.compressed,
            approximate: imported.approximate,
            copy_paint: imported.copy_paint,
            ..ImportReport::default()
        },
    };
    let outer = doc.create_definition(imported.name.clone(), DefinitionKind::Group);
    let (_, placed) = b.fill(doc, outer, &imported.loose)?;
    b.report.groups += placed;
    for g in &imported.groups {
        if let Some(def) = b.build(doc, g, DefinitionKind::Group)? {
            doc.add_instance(Some(outer), def, g.transform, g.name.clone())?;
            b.report.groups += 1;
        }
    }
    // The repeat sizes found go on the materials; without one, the file's
    // own (or the reader's default).
    for (m, id) in b.materials.iter().enumerate() {
        if let (Some(id), Some(own)) = (id, imported.materials[m].texture.as_ref()) {
            let mut mat = doc.material(*id).cloned().ok_or(ModelError::NoSuchEntity)?;
            if let Some(t) = mat.texture.as_mut() {
                t.width_mm = if b.sizes[m].x > 0.0 {
                    b.sizes[m].x
                } else {
                    own.width_mm
                };
            }
            doc.set_material(*id, mat)?;
        }
    }
    let instance = doc.add_instance(ctx, outer, at, imported.name.clone())?;
    Ok((instance, b.report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{ExportOptions, Scope, gather, write_obj};

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("smith-io-import-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn print() -> ImportOptions {
        ImportOptions {
            units: UnitPreset::Print,
            ..ImportOptions::default()
        }
    }

    /// Places `imported` in a new model; the model and the report.
    fn placed(imported: &Imported) -> (Document, InstanceId, ImportReport) {
        let mut doc = Document::new();
        let (id, report) = doc
            .transaction("Import", |d| place(d, None, DMat4::IDENTITY, imported))
            .unwrap();
        (doc, id, report)
    }

    /// Every face of the import, in model coordinates, as corner lists.
    fn faces(doc: &Document) -> Vec<Vec<DVec3>> {
        let g = gather(doc, Scope::Model);
        g.parts
            .iter()
            .flat_map(|p| &p.faces)
            .map(|f| f.corners.iter().map(|c| c.point).collect())
            .collect()
    }

    /// A Blender-style OBJ: a cube with an octagonal top cap, in metres,
    /// Y up, as two objects.
    const BLENDER: &str = "\
o Box
v 0 0 0
v 1 0 0
v 1 0 -1
v 0 0 -1
v 0 1 0
v 1 1 0
v 1 1 -1
v 0 1 -1
f 1 4 3 2
f 5 6 7 8
f 1 2 6 5
f 2 3 7 6
f 3 4 8 7
f 4 1 5 8
o Cap
v 2 0 0
v 2.5 0 0
v 2.85 0 -0.35
v 2.85 0 -0.85
v 2.5 0 -1.2
v 2 0 -1.2
v 1.65 0 -0.85
v 1.65 0 -0.35
f 9 10 11 12 13 14 15 16
";

    #[test]
    fn a_blender_obj_keeps_its_ngons_objects_and_size() {
        let d = dir("blender");
        let path = d.join("parts.obj");
        std::fs::write(&path, BLENDER).unwrap();
        let imported = read_import(&path, ImportFormat::Obj, &ImportOptions::default()).unwrap();
        assert_eq!(imported.name, "parts");
        assert_eq!(imported.groups.len(), 2);
        let (doc, _, report) = placed(&imported);
        // Six quads and the octagon: 7 faces, the octagon one face.
        assert_eq!(report.counts.faces, 7);
        assert_eq!(report.groups, 2);
        let fs = faces(&doc);
        assert!(fs.iter().any(|f| f.len() == 8));
        // Metres to mm, Y up to Z up: the box is 1000 mm tall in Z and runs
        // into +Y (OBJ's −Z).
        let (lo, hi) = doc.model_bounds().unwrap();
        assert!((hi.z - 1000.0).abs() < 1e-6 && lo.z.abs() < 1e-6);
        assert!(lo.y.abs() < 1e-6 && (hi.y - 1200.0).abs() < 1e-6);
        assert_eq!(
            report.summary("parts.obj"),
            "Imported parts.obj: 7 Faces, 2 Objects"
        );
        assert!(report.lines().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_export_comes_back_the_same() {
        let d = dir("round");
        // A box of 50 × 30 × 20 mm, painted with a texture.
        let mut doc = Document::new();
        let wood = doc
            .add_material(Material {
                name: "Oak".into(),
                texture: Some(Texture {
                    image: std::sync::Arc::new(oak_png()),
                    width_mm: 40.0,
                }),
                ..Material::default()
            })
            .unwrap();
        doc.transaction("box", |dd| {
            let r = dd.root_mut();
            let c = [
                DVec3::ZERO,
                DVec3::new(50.0, 0.0, 0.0),
                DVec3::new(50.0, 30.0, 0.0),
                DVec3::new(0.0, 30.0, 0.0),
            ];
            let up = DVec3::new(0.0, 0.0, 20.0);
            for i in 0..4 {
                let (a, b) = (c[i], c[(i + 1) % 4]);
                r.add_line(a, b)?;
                r.add_line(a + up, b + up)?;
                r.add_line(a, a + up)?;
            }
            let f = r.face_ids().next().ok_or(ModelError::NoSuchEntity)?;
            let n = r.face(f).map_or(DVec3::Z, |x| x.plane().normal());
            r.set_face_paint(
                f,
                smith_model::Side::Front,
                Some(Paint {
                    material: wood,
                    mapping: Mapping::Face(TextureFrame::for_side(n, DVec2::new(3.0, 4.0), 0.4)),
                }),
            )
        })
        .unwrap();
        let path = d.join("box.obj");
        let out = ExportOptions::default();
        write_obj(&path, &doc, &gather(&doc, Scope::Model), &out).unwrap();
        let back = read_import(&path, ImportFormat::Obj, &ImportOptions::default()).unwrap();
        let (doc2, _, report) = placed(&back);
        assert_eq!(report.counts.faces, 6);
        let (lo, hi) = doc2.model_bounds().unwrap();
        assert!((hi - lo - DVec3::new(50.0, 30.0, 20.0)).length() < 1e-6);
        // The texture is laid exactly, at its own repeat size.
        assert_eq!(report.approximate, 0);
        let id = doc2.material_ids().next().unwrap();
        let m = doc2.material(id).unwrap();
        assert_eq!(m.name, "Oak");
        // OBJ carries six decimals.
        assert!((m.texture.as_ref().unwrap().width_mm - 40.0).abs() < 1e-3);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn bent_skipped_lines_points_and_missing_textures_are_reported() {
        let d = dir("report");
        std::fs::write(
            d.join("bits.mtl"),
            "newmtl Steel\nKd 0.5 0.5 0.5\nPm 1\nmap_Kd steel.jpg\n",
        )
        .unwrap();
        let path = d.join("bits.obj");
        std::fs::write(
            &path,
            "mtllib bits.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0.5\nv 0 1 0\nv 2 0 0\nusemtl Steel\nf 1 2 3 4\nf 1 2 5\nf 1 2 3 4\nl 1 3\np 1 2 3 4\n",
        )
        .unwrap();
        let imported = read_import(&path, ImportFormat::Obj, &print()).unwrap();
        let (doc, _, report) = placed(&imported);
        assert_eq!(report.counts.bent, 1);
        assert_eq!(report.counts.zero_area, 1);
        assert_eq!(report.counts.repeated, 1);
        assert_eq!(report.counts.lines, 1);
        assert_eq!(
            report.lines(),
            [
                "1 Bent Face Triangulated",
                "1 Zero-Area Face Skipped",
                "1 Repeated Face Skipped",
                "4 Points Skipped",
                "1 Line Imported as Edges",
                "1 Texture Not Found: steel.jpg",
            ]
        );
        let m = doc.material(doc.material_ids().next().unwrap()).unwrap();
        assert!(m.metallic && m.texture.is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_summary_counts_faces_after_merging() {
        let r = ImportReport {
            counts: ImportCounts {
                faces: 12,
                merged: 6,
                softened: 1_806,
                ..ImportCounts::default()
            },
            groups: 1,
            ..ImportReport::default()
        };
        assert_eq!(
            r.summary("part.stl"),
            "Imported part.stl: 6 Faces, 1 Object"
        );
        assert_eq!(r.lines(), ["6 Triangles Merged", "1 806 Edges Softened"]);
    }

    #[test]
    fn separate_shells_and_names() {
        let mut imported = read_obj_text(
            "two",
            BLENDER,
            &ImportOptions {
                separate_shells: true,
                ..ImportOptions::default()
            },
        );
        assert_eq!(imported.groups.len(), 2);
        // Both objects in one: split into two pieces, numbered.
        let mut one = imported.groups.remove(0);
        one.polygons.extend(imported.groups.remove(0).polygons);
        one.name = "All".into();
        let pieces = separate(one);
        let names: Vec<&str> = pieces.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["All 1", "All 2"]);
        assert_eq!(grouped(1_234_567), "1 234 567");
    }

    #[test]
    fn a_reversed_painted_copy_paints_the_back() {
        let tri = vec![
            DVec3::ZERO,
            DVec3::new(100.0, 0.0, 0.0),
            DVec3::new(0.0, 100.0, 0.0),
        ];
        let back: Vec<DVec3> = tri.iter().rev().copied().collect();
        let poly = |points: &Vec<DVec3>, m| Polygon {
            points: points.clone(),
            uvs: None,
            material: Some(m),
            ..Polygon::default()
        };
        let mat = |name: &str| Material {
            name: name.into(),
            ..Material::default()
        };
        let imported = Imported {
            name: "sides".into(),
            groups: vec![ImportGroup {
                name: "Sheet".into(),
                // Front Red; back Blue; then Green on the back too late, and
                // Green the same way round as the front: both skipped.
                polygons: vec![poly(&tri, 0), poly(&back, 1), poly(&back, 2), poly(&tri, 2)],
                ..ImportGroup::default()
            }],
            materials: vec![mat("Red"), mat("Blue"), mat("Green")],
            ..Imported::default()
        };
        let (doc, _, report) = placed(&imported);
        assert_eq!((report.counts.faces, report.counts.repeated), (1, 2));
        let def = doc
            .definition_ids()
            .find(|d| doc.definition(*d).is_some_and(|x| x.name == "Sheet"))
            .unwrap();
        let e = doc.definition_entities(def).unwrap();
        let f = e.face_ids().next().unwrap();
        let name = |side| {
            let p = e.face_paint(f, side).unwrap();
            doc.material(p.material).unwrap().name.clone()
        };
        assert_eq!(name(Side::Front), "Red");
        assert_eq!(name(Side::Back), "Blue");
        // Green was never used, so it never joined the model.
        assert!(
            doc.material_ids()
                .all(|m| doc.material(m).unwrap().name != "Green")
        );
    }

    #[test]
    fn a_mapping_with_two_corners_alike_still_lays_a_square_frame() {
        // From Sponza: two corners share their texture coordinates, so u
        // and v run the same way across the face.
        let points = [
            DVec3::new(-25.331200849163515, -4598.070042615347, 1409.6585142208085),
            DVec3::new(-25.331200849163515, -4689.44992976816, 1409.6585142208085),
            DVec3::new(-37.73920238455686, -4689.44992976816, 1431.8168404685848),
        ];
        let uvs = [
            DVec2::new(0.5651999711990356, -0.1592000126838684),
            DVec2::new(0.5651999711990356, -0.1592000126838684),
            DVec2::new(0.5687000155448914, -0.15280002355575562),
        ];
        let normal = DVec3::new(-0.8725171143877147, 0.0, -0.48858354976455726);
        let size = DVec2::splat(3564.0971498167755);
        let (frame, ..) = fit_frame(&points, &uvs, normal, size, 1.0).unwrap();
        assert!(frame.is_valid());
    }

    /// A 4 × 2 pixel PNG, as a material's image.
    fn oak_png() -> smith_model::TextureImage {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([120, 80, 40, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        crate::textures::image_from_bytes("oak.png", bytes).unwrap()
    }

    /// Reads OBJ `text` as if from a file named `name`.
    fn read_obj_text(name: &str, text: &str, options: &ImportOptions) -> Imported {
        let d = dir(name);
        let path = d.join(format!("{name}.obj"));
        std::fs::write(&path, text).unwrap();
        let out = read_import(&path, ImportFormat::Obj, options).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        out
    }
}
