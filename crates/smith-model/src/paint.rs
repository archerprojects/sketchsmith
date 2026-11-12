//! SketchSmith — materials and paint | v1.0.0 | Thursday 12 November 2026
//!
//! **Materials** belong to the document ([`Material`]): a colour, an opacity,
//! a Metallic switch and an optional image texture whose width is set in mm
//! (its height follows the image). The image is kept exactly as imported
//! (PNG or JPEG bytes); decoding is the job of whoever shows or exports it.
//!
//! **Paint** belongs to a face side: each face carries an optional [`Paint`]
//! for its front and one for its back, in its attributes, so splitting,
//! healing and copying a face keep it. A paint names a material and a
//! [`Mapping`]: a rule anchored in the context's own space that turns a
//! point into texture coordinates in mm. No texture coordinates are stored on
//! vertices; they are worked out from positions whenever they are needed, so
//! any piece of a painted face shows the texture exactly where the whole face
//! showed it.
//!
//! * [`Mapping::Face`]: a flat [`TextureFrame`] on the face's plane.
//! * [`Mapping::Wrap`]: a [`Wrap`] record in the face's context, shared by
//!   every face one Wrap painted: one projection (Box, Cylinder or Planar)
//!   across all of them, with its offsets and rotation.
//!
//! Distances are mm; texture coordinates are mm until divided by the
//! texture's width and height.

use std::sync::Arc;

use std::collections::{BTreeMap, BTreeSet};

use smith_geom::{DMat3, DMat4, DVec2, DVec3};

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::{FaceId, MaterialId, WrapId};

/// Which side of a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The side the face normal points out of.
    Front,
    /// The other side.
    Back,
}

/// The file format of an imported texture image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
}

impl ImageFormat {
    /// The MIME type glTF uses for it.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

/// An imported image, kept byte for byte as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureImage {
    /// The file name it was imported from (no folder).
    pub name: String,
    /// PNG or JPEG.
    pub format: ImageFormat,
    /// The image file's bytes.
    pub bytes: Vec<u8>,
    /// Width and height in pixels.
    pub pixels: [u32; 2],
}

/// A material's image and its size on the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// The image (shared, so copies of the material do not copy it).
    pub image: Arc<TextureImage>,
    /// Width of one repeat in mm.
    pub width_mm: f64,
}

impl Texture {
    /// Height of one repeat in mm: the width scaled by the image's proportions.
    pub fn height_mm(&self) -> f64 {
        let [w, h] = self.image.pixels;
        if w == 0 {
            self.width_mm
        } else {
            self.width_mm * f64::from(h) / f64::from(w)
        }
    }

    /// One repeat's size in mm, `(width, height)`.
    pub fn size_mm(&self) -> DVec2 {
        DVec2::new(self.width_mm, self.height_mm())
    }
}

/// A material: how a painted face side looks in Present mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Display name (unique in the document).
    pub name: String,
    /// Base colour, linear RGB, 0 to 2: measured metal colours can pass 1
    /// (gold's red is 1.059). With a texture it tints the image (white shows
    /// the image as it is), as glTF's base colour factor does; glTF itself
    /// takes 0–1, so exports clamp.
    pub color: [f32; 3],
    /// Opacity, 0 (clear) to 1 (solid).
    pub opacity: f32,
    /// Metallic: on or off.
    pub metallic: bool,
    /// The image, if any.
    pub texture: Option<Texture>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            color: [0.8, 0.8, 0.8],
            opacity: 1.0,
            metallic: false,
            texture: None,
        }
    }
}

impl Material {
    /// Whether the values are usable: finite, colour within 0–2, opacity
    /// within 0–1,
    /// a positive texture width and image size.
    pub fn is_valid(&self) -> bool {
        self.color
            .iter()
            .all(|c| c.is_finite() && (0.0..=2.0).contains(c))
            && self.opacity.is_finite()
            && (0.0..=1.0).contains(&self.opacity)
            && self.texture.as_ref().is_none_or(|t| {
                t.width_mm.is_finite()
                    && t.width_mm > 0.0
                    && t.image.pixels[0] > 0
                    && t.image.pixels[1] > 0
            })
    }
}

/// Metal colours from the Physically Based database (physicallybased.info),
/// linear sRGB. A value above 1 is the database's own.
pub const METAL_PRESETS: [(&str, [f32; 3]); 10] = [
    ("Gold", [1.059, 0.773, 0.307]),
    ("Silver", [0.991, 0.985, 0.974]),
    ("Platinum", [0.765, 0.730, 0.676]),
    ("Palladium", [0.734, 0.704, 0.662]),
    ("Copper", [0.932, 0.623, 0.522]),
    ("Brass", [0.910, 0.778, 0.423]),
    ("Stainless Steel", [0.669, 0.639, 0.598]),
    ("Titanium", [0.441, 0.400, 0.361]),
    ("Nickel", [0.697, 0.641, 0.563]),
    ("Aluminum", [0.916, 0.923, 0.924]),
];

/// A flat texture frame on a face's plane: texture `u` runs along `u`,
/// texture `v` along `v`, both from `origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureFrame {
    /// The point where texture coordinates are 0, 0.
    pub origin: DVec3,
    /// Unit direction of texture `u`, in the face plane.
    pub u: DVec3,
    /// Unit direction of texture `v`, in the face plane, square to `u`.
    pub v: DVec3,
}

impl TextureFrame {
    /// The frame for a face side whose outward normal is `side_normal`:
    /// `u` along X projected onto the plane (Y for a face square to X), `v`
    /// square to it so the image reads the right way round from that side,
    /// then turned by `rotation` (radians) and shifted by `offset` (mm).
    /// Anchored at the context origin, so neighbouring faces line up.
    pub fn for_side(side_normal: DVec3, offset: DVec2, rotation: f64) -> Self {
        let n = side_normal.normalize_or(DVec3::Z);
        let flat = |a: DVec3| a - n * n.dot(a);
        let x = flat(DVec3::X);
        let u = if x.length() > 1e-6 { x } else { flat(DVec3::Y) }.normalize_or(DVec3::X);
        let v = n.cross(u);
        let (s, c) = rotation.sin_cos();
        let (u, v) = (u * c + v * s, v * c - u * s);
        Self {
            origin: u * offset.x + v * offset.y,
            u,
            v,
        }
    }

    /// Texture coordinates of `p`, mm.
    pub fn uv(&self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.u), d.dot(self.v))
    }

    /// The frame carried by `m`: the origin moves with the geometry, the
    /// directions turn (and mirror) with it, and the repeat keeps its size
    /// in mm.
    pub fn transformed(&self, m: &DMat4) -> Self {
        let (u, v) = carried_axes(m, self.u, self.v);
        Self {
            origin: m.transform_point3(self.origin),
            u,
            v,
        }
    }

    /// Whether every value is finite and the directions are unit and square.
    pub fn is_valid(&self) -> bool {
        self.origin.is_finite()
            && (self.u.length() - 1.0).abs() < 1e-6
            && (self.v.length() - 1.0).abs() < 1e-6
            && self.u.dot(self.v).abs() < 1e-6
    }
}

/// `a`, `b` carried by `m`'s linear part, unit length and square again.
fn carried_axes(m: &DMat4, a: DVec3, b: DVec3) -> (DVec3, DVec3) {
    let m3 = DMat3::from_mat4(*m);
    let a2 = (m3 * a).normalize_or(a);
    let b2 = m3 * b;
    let b2 = (b2 - a2 * a2.dot(b2)).normalize_or(b);
    (a2, b2)
}

/// How a Wrap lays the texture across the faces it painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    /// Each face takes the texture from the side of a box it faces most
    /// squarely.
    #[default]
    Box,
    /// Round the axis: one repeat per texture width of circumference.
    Cylinder,
    /// Pressed straight through along the axis.
    Planar,
}

/// One of the drawing axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    /// X.
    X,
    /// Y.
    Y,
    /// Z.
    #[default]
    Z,
}

impl Axis {
    /// 0, 1 or 2.
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

/// One projection shared by every face a Wrap painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wrap {
    /// Box, Cylinder or Planar.
    pub projection: Projection,
    /// The drawing axis it was made about (shown in Operation Inputs).
    pub axis: Axis,
    /// The centre of what was painted.
    pub origin: DVec3,
    /// Unit, square axes (right-handed unless mirrored): `basis[2]` is the
    /// projection axis, `basis[0]` and `basis[1]` run along the texture's `u`
    /// and `v` on the plane square to it.
    pub basis: [DVec3; 3],
    /// Radius used to turn angle into mm round the axis (Cylinder).
    pub radius: f64,
    /// Offset in mm along the texture's own `u` and `v`.
    pub offset: DVec2,
    /// Turn of the texture in radians.
    pub rotation: f64,
}

impl Wrap {
    /// A wrap about `axis` of the drawing axes `axes` (X, Y, Z directions in
    /// the context's space), centred on `centre`.
    pub fn new(
        projection: Projection,
        axis: Axis,
        axes: [DVec3; 3],
        centre: DVec3,
        radius: f64,
    ) -> Self {
        let i = axis.index();
        let w = axes[i].normalize_or(DVec3::Z);
        let u = axes[(i + 1) % 3];
        let u = (u - w * w.dot(u)).normalize_or(w.any_orthonormal_vector());
        let v = w.cross(u);
        Self {
            projection,
            axis,
            origin: centre,
            basis: [u, v, w],
            radius: if radius.is_finite() && radius > 0.0 {
                radius
            } else {
                1.0
            },
            offset: DVec2::ZERO,
            rotation: 0.0,
        }
    }

    /// Texture coordinates (mm) of a face's `points`, for the face side
    /// whose outward normal is `side_normal`. Angles round a Cylinder are
    /// kept continuous across one face, so a face on the seam does not
    /// smear the whole image across itself.
    pub fn uv_face(&self, points: &[DVec3], side_normal: DVec3) -> Vec<DVec2> {
        let [bu, bv, bw] = self.basis;
        let local = |p: DVec3| {
            let d = p - self.origin;
            DVec3::new(d.dot(bu), d.dot(bv), d.dot(bw))
        };
        let n = DVec3::new(
            side_normal.dot(bu),
            side_normal.dot(bv),
            side_normal.dot(bw),
        );
        let raw: Vec<DVec2> = match self.projection {
            Projection::Planar => points
                .iter()
                .map(|p| {
                    let q = local(*p);
                    DVec2::new(q.x, q.y)
                })
                .collect(),
            Projection::Box => {
                let a = n.abs();
                points
                    .iter()
                    .map(|p| {
                        let q = local(*p);
                        if a.z >= a.x && a.z >= a.y {
                            DVec2::new(q.x * n.z.signum(), q.y)
                        } else if a.x >= a.y {
                            DVec2::new(q.y * n.x.signum(), q.z)
                        } else {
                            DVec2::new(-q.x * n.y.signum(), q.z)
                        }
                    })
                    .collect()
            }
            Projection::Cylinder => {
                if n.z.abs() > std::f64::consts::FRAC_1_SQRT_2 {
                    // An end cap: pressed through along the axis.
                    points
                        .iter()
                        .map(|p| {
                            let q = local(*p);
                            DVec2::new(q.x, q.y)
                        })
                        .collect()
                } else {
                    let mut first: Option<f64> = None;
                    points
                        .iter()
                        .map(|p| {
                            let q = local(*p);
                            let mut a = q.y.atan2(q.x);
                            match first {
                                None => first = Some(a),
                                Some(a0) => {
                                    let tau = std::f64::consts::TAU;
                                    a += ((a0 - a) / tau).round() * tau;
                                }
                            }
                            DVec2::new(a * self.radius, q.z)
                        })
                        .collect()
                }
            }
        };
        let (s, c) = self.rotation.sin_cos();
        raw.into_iter()
            .map(|t| DVec2::new(t.x * c + t.y * s, t.y * c - t.x * s) - self.offset)
            .collect()
    }

    /// The wrap carried by `m` (see [`TextureFrame::transformed`]): the
    /// radius follows the scale square to the axis.
    pub fn transformed(&self, m: &DMat4) -> Self {
        let m3 = DMat3::from_mat4(*m);
        let [u, v, _] = self.basis;
        let stretch = ((m3 * u).length() + (m3 * v).length()) * 0.5;
        let (u2, v2) = carried_axes(m, u, v);
        // A mirror leaves the frame left-handed, so every point of the
        // mirrored faces shows what its original showed: a mirror image.
        let w2 = if m3.determinant() < 0.0 {
            v2.cross(u2)
        } else {
            u2.cross(v2)
        };
        Self {
            origin: m.transform_point3(self.origin),
            basis: [u2, v2, w2],
            radius: self.radius * stretch,
            ..*self
        }
    }

    /// Whether every value is finite and the basis is unit and square.
    pub fn is_valid(&self) -> bool {
        let [u, v, w] = self.basis;
        let unit = |a: DVec3| (a.length() - 1.0).abs() < 1e-6;
        self.origin.is_finite()
            && unit(u)
            && unit(v)
            && unit(w)
            && u.dot(v).abs() < 1e-6
            && u.dot(w).abs() < 1e-6
            && v.dot(w).abs() < 1e-6
            && self.radius.is_finite()
            && self.radius > 0.0
            && self.offset.is_finite()
            && self.rotation.is_finite()
    }
}

/// How a face side's texture is placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mapping {
    /// A flat frame of its own.
    Face(TextureFrame),
    /// A wrap shared with the other faces painted with it.
    Wrap(WrapId),
}

/// A face side's material and how its texture is placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    /// The material.
    pub material: MaterialId,
    /// Where its texture lies.
    pub mapping: Mapping,
}

impl Entities {
    /// A wrap by key.
    pub fn wrap(&self, id: WrapId) -> Option<&Wrap> {
        self.wraps.get(id)
    }

    /// Every wrap key, in creation order.
    pub fn wrap_ids(&self) -> impl Iterator<Item = WrapId> + '_ {
        self.wraps.keys()
    }

    /// Adds a wrap and returns its key.
    ///
    /// # Errors
    /// [`ModelError::NonFinite`] for a malformed wrap.
    pub fn add_wrap(&mut self, wrap: Wrap) -> Result<WrapId, ModelError> {
        if !wrap.is_valid() {
            return Err(ModelError::NonFinite);
        }
        Ok(self.wraps.insert(wrap))
    }

    /// Changes wrap `id` (offset, rotation, projection): every face painted
    /// with it follows.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`] for a
    /// malformed wrap.
    pub fn set_wrap(&mut self, id: WrapId, wrap: Wrap) -> Result<(), ModelError> {
        if !wrap.is_valid() {
            return Err(ModelError::NonFinite);
        }
        *self.wraps.get_mut(id).ok_or(ModelError::NoSuchEntity)? = wrap;
        Ok(())
    }

    /// The paint on face `f`'s `side`.
    pub fn face_paint(&self, f: FaceId, side: Side) -> Option<&Paint> {
        self.faces
            .get(f)
            .and_then(|face| face.attributes.paint(side))
    }

    /// Paints face `f`'s `side` (`None` unpaints it).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing face, or a wrap that is
    /// not in this context; [`ModelError::NonFinite`] for a malformed frame.
    pub fn set_face_paint(
        &mut self,
        f: FaceId,
        side: Side,
        paint: Option<Paint>,
    ) -> Result<(), ModelError> {
        match paint.map(|p| p.mapping) {
            Some(Mapping::Wrap(w)) if !self.wraps.contains_key(w) => {
                return Err(ModelError::NoSuchEntity);
            }
            Some(Mapping::Face(frame)) if !frame.is_valid() => {
                return Err(ModelError::NonFinite);
            }
            _ => {}
        }
        let face = self.faces.get(f).ok_or(ModelError::NoSuchEntity)?;
        if face.attributes.paint(side) == paint.as_ref() {
            return Ok(());
        }
        *self.faces[f].attributes.paint_mut(side) = paint;
        Ok(())
    }

    /// Faces with either side painted with wrap `id`.
    pub fn wrap_users(&self, id: WrapId) -> Vec<FaceId> {
        self.faces
            .iter()
            .filter(|(_, face)| {
                [Side::Front, Side::Back].iter().any(|s| {
                    matches!(face.attributes.paint(*s), Some(Paint { mapping: Mapping::Wrap(w), .. }) if *w == id)
                })
            })
            .map(|(f, _)| f)
            .collect()
    }

    /// Texture coordinates (mm) of `points` on a face side painted with
    /// `paint`, whose outward normal is `side_normal`; `None` when its wrap
    /// is missing.
    pub fn paint_uvs(
        &self,
        paint: &Paint,
        side_normal: DVec3,
        points: &[DVec3],
    ) -> Option<Vec<DVec2>> {
        match paint.mapping {
            Mapping::Face(frame) => Some(points.iter().map(|p| frame.uv(*p)).collect()),
            Mapping::Wrap(w) => Some(self.wraps.get(w)?.uv_face(points, side_normal)),
        }
    }

    /// Repaints every face side painted with `old` with `new` (`None`
    /// unpaints), keeping where the texture lies. Returns how many sides
    /// changed.
    pub fn replace_material(&mut self, old: MaterialId, new: Option<MaterialId>) -> usize {
        let painted: Vec<(FaceId, Side)> = self
            .faces
            .iter()
            .flat_map(|(f, face)| {
                [Side::Front, Side::Back]
                    .into_iter()
                    .filter(move |s| face.attributes.paint(*s).is_some_and(|p| p.material == old))
                    .map(move |s| (f, s))
            })
            .collect();
        for (f, side) in &painted {
            let slot = self.faces[*f].attributes.paint_mut(*side);
            *slot = match new {
                Some(m) => slot.map(|p| Paint { material: m, ..p }),
                None => None,
            };
        }
        painted.len()
    }

    /// Carries the paint of `faces`, which `m` moved whole, along with them.
    /// A wrap all of whose faces moved moves too; one only partly moved is
    /// copied for the moved faces, so one record never describes two places.
    pub(crate) fn carry_paint(&mut self, faces: &[FaceId], m: &DMat4) {
        let moved: BTreeSet<FaceId> = faces.iter().copied().collect();
        let mut wraps: BTreeSet<WrapId> = BTreeSet::new();
        for f in &moved {
            let Some(face) = self.faces.get(*f) else {
                continue;
            };
            let mut attributes = face.attributes.clone();
            for side in [Side::Front, Side::Back] {
                if let Some(p) = attributes.paint_mut(side) {
                    match &mut p.mapping {
                        Mapping::Face(frame) => *frame = frame.transformed(m),
                        Mapping::Wrap(w) => {
                            wraps.insert(*w);
                        }
                    }
                }
            }
            if attributes != face.attributes {
                self.faces[*f].attributes = attributes;
            }
        }
        for w in wraps {
            let Some(wrap) = self.wraps.get(w).copied() else {
                continue;
            };
            let carried = wrap.transformed(m);
            if self.wrap_users(w).iter().all(|f| moved.contains(f)) {
                self.wraps[w] = carried;
                continue;
            }
            let copy = self.wraps.insert(carried);
            for f in &moved {
                for side in [Side::Front, Side::Back] {
                    let slot = self.faces[*f].attributes.paint_mut(side);
                    if let Some(p) = slot
                        && p.mapping == Mapping::Wrap(w)
                    {
                        p.mapping = Mapping::Wrap(copy);
                    }
                }
            }
        }
    }

    /// Every material a face side here is painted with.
    pub(crate) fn painted_materials(&self) -> BTreeSet<MaterialId> {
        self.faces
            .iter()
            .flat_map(|(_, face)| {
                [Side::Front, Side::Back]
                    .into_iter()
                    .filter_map(|s| face.attributes.paint(s).map(|p| p.material))
            })
            .collect()
    }

    /// The wraps faces use, each with the index it is saved under (unused
    /// wraps are left out of the file).
    pub(crate) fn used_wraps(&self) -> BTreeMap<WrapId, u32> {
        let mut used: BTreeSet<WrapId> = BTreeSet::new();
        for (_, face) in self.faces.iter() {
            for side in [Side::Front, Side::Back] {
                if let Some(Paint {
                    mapping: Mapping::Wrap(w),
                    ..
                }) = face.attributes.paint(side)
                {
                    used.insert(*w);
                }
            }
        }
        used.into_iter()
            .enumerate()
            .map(|(i, w)| (w, u32::try_from(i).unwrap_or(u32::MAX)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn a_face_frame_reads_the_right_way_from_each_side() {
        let front = TextureFrame::for_side(DVec3::Z, DVec2::ZERO, 0.0);
        assert!(close(
            front.uv(DVec3::new(10.0, 5.0, 3.0)),
            DVec2::new(10.0, 5.0)
        ));
        let back = TextureFrame::for_side(-DVec3::Z, DVec2::ZERO, 0.0);
        // Seen from below, +Y runs the other way across the image.
        assert!(close(
            back.uv(DVec3::new(10.0, 5.0, 3.0)),
            DVec2::new(10.0, -5.0)
        ));
        let side = TextureFrame::for_side(DVec3::X, DVec2::ZERO, 0.0);
        assert!(side.is_valid());
    }

    #[test]
    fn offset_and_rotation_shift_and_turn_the_frame() {
        let f = TextureFrame::for_side(DVec3::Z, DVec2::new(5.0, 0.0), 0.0);
        assert!(close(f.uv(DVec3::new(5.0, 0.0, 0.0)), DVec2::ZERO));
        let r = TextureFrame::for_side(DVec3::Z, DVec2::ZERO, std::f64::consts::FRAC_PI_2);
        assert!(close(r.uv(DVec3::new(0.0, 7.0, 0.0)), DVec2::new(7.0, 0.0)));
    }

    #[test]
    fn a_moved_frame_keeps_the_texture_on_the_face() {
        let f = TextureFrame::for_side(DVec3::Z, DVec2::ZERO, 0.0);
        let m = DMat4::from_translation(DVec3::new(3.0, 4.0, 0.0)) * DMat4::from_rotation_z(0.7);
        let g = f.transformed(&m);
        let p = DVec3::new(12.0, -3.0, 0.0);
        assert!(close(f.uv(p), g.uv(m.transform_point3(p))));
        // Scaled: the repeat keeps its size in mm.
        let s = f.transformed(&DMat4::from_scale(DVec3::splat(2.0)));
        assert!(close(
            s.uv(DVec3::new(20.0, 0.0, 0.0)),
            DVec2::new(20.0, 0.0)
        ));
    }

    #[test]
    fn a_cylinder_wrap_repeats_once_per_width_of_circumference() {
        let axes = [DVec3::X, DVec3::Y, DVec3::Z];
        let r = 50.0 / std::f64::consts::TAU;
        let w = Wrap::new(Projection::Cylinder, Axis::Z, axes, DVec3::ZERO, r);
        // A quarter turn is a quarter of 50 mm.
        let pts = [DVec3::new(r, 0.0, 0.0), DVec3::new(0.0, r, 10.0)];
        let uv = w.uv_face(&pts, DVec3::new(1.0, 1.0, 0.0).normalize());
        assert!((uv[1].x - uv[0].x - 12.5).abs() < 1e-9);
        assert!((uv[1].y - 10.0).abs() < 1e-9);
        // A face on the seam stays continuous.
        let seam = [DVec3::new(-r, 0.01, 0.0), DVec3::new(-r, -0.01, 0.0)];
        let uv = w.uv_face(&seam, -DVec3::X);
        assert!((uv[0].x - uv[1].x).abs() < 1.0);
    }

    #[test]
    fn a_box_wrap_takes_each_face_from_its_side() {
        let axes = [DVec3::X, DVec3::Y, DVec3::Z];
        let w = Wrap::new(Projection::Box, Axis::Z, axes, DVec3::ZERO, 1.0);
        let top = w.uv_face(&[DVec3::new(3.0, 4.0, 9.0)], DVec3::Z);
        assert!(close(top[0], DVec2::new(3.0, 4.0)));
        let side = w.uv_face(&[DVec3::new(9.0, 4.0, 2.0)], DVec3::X);
        assert!(close(side[0], DVec2::new(4.0, 2.0)));
    }

    #[test]
    fn a_wrap_moved_with_its_faces_keeps_the_texture_on_them() {
        let axes = [DVec3::X, DVec3::Y, DVec3::Z];
        let w = Wrap::new(Projection::Planar, Axis::Z, axes, DVec3::ZERO, 1.0);
        let m = DMat4::from_translation(DVec3::new(5.0, 0.0, 2.0)) * DMat4::from_rotation_z(1.1);
        let moved = w.transformed(&m);
        assert!(moved.is_valid());
        let p = DVec3::new(7.0, 2.0, 0.0);
        let a = w.uv_face(&[p], DVec3::Z)[0];
        let b = moved.uv_face(&[m.transform_point3(p)], m.transform_vector3(DVec3::Z))[0];
        assert!(close(a, b));
    }

    #[test]
    fn every_metal_preset_is_a_valid_material() {
        for (name, color) in METAL_PRESETS {
            let m = Material {
                name: name.into(),
                color,
                metallic: true,
                ..Material::default()
            };
            assert!(m.is_valid(), "{name}");
        }
    }

    #[test]
    fn a_texture_height_follows_the_image() {
        let t = Texture {
            image: Arc::new(TextureImage {
                name: "a.png".into(),
                format: ImageFormat::Png,
                bytes: Vec::new(),
                pixels: [200, 100],
            }),
            width_mm: 50.0,
        };
        assert!((t.height_mm() - 25.0).abs() < 1e-12);
    }
}
