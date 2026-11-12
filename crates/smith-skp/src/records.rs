//! SketchSmith — SKP object records | v1.0.0 | Thursday 12 November 2026
//!
//! One reader per class the 2013–2020 stream holds. Each reads exactly the
//! bytes its record takes, so the walk stays aligned for the objects after
//! it; what SketchSmith does not use is still read through. Byte counts that
//! changed between SketchUp versions are keyed on the header's version, as
//! the original reader found them on real files.

use std::rc::Rc;

use crate::archive::{
    ArcFrame, Archive, Db, Item, PREMODEL, TextureRec, Val, is_class_ref, strict_next_tag,
};
use crate::cursor::{Fault, R, Res, STR_MARKER, clip, f64_at, find, u16_at, u32_at};

/// SketchUp 3 predates several fields every later file carries.
const FIRST_V4: u32 = 4;
/// The texture image became an object of its own in SketchUp 6.
const FIRST_DIB_OBJECT: u32 = 6;
/// The first version whose instances and groups carry a name.
const FIRST_NAMED_INSTANCE: u32 = 5;

/// Reads one object of class `name`.
pub(crate) fn read(ar: &mut Archive<'_>, r: &mut R<'_>, name: &str) -> Res<Val> {
    match name {
        "CVertex" => read_vertex(ar, r),
        "CEdge" => read_edge(ar, r),
        "CCurve" => read_curve(ar, r),
        "CArcCurve" => read_arccurve(ar, r),
        "CEdgeUse" => read_edgeuse(ar, r),
        "CLoop" => read_loop(ar, r),
        "CFace" => read_face(ar, r),
        "CLayer" => read_layer(ar, r),
        "CMaterial" => read_material(ar, r),
        "CDib" => read_dib(r),
        "CAttributeContainer" => read_attr_container(ar, r),
        "CAttributeNamed" => read_attr_named(ar, r),
        "CCamera" => read_camera(ar, r),
        "CThumbnail" => read_thumbnail(ar, r),
        "CRelationship" => read_relationship(ar, r),
        "CComponentDefinition" => read_definition(ar, r),
        "CImage" => read_image(ar, r),
        "CComponentInstance" | "CGroup" => read_instance(ar, r),
        "CFaceTextureCoords" => read_ftc(ar, r),
        "CConstructionLine" => read_constructionline(ar, r),
        "CConstructionPoint" => read_constructionpoint(ar, r),
        "CSectionPlane" => read_sectionplane(ar, r),
        "CSkFont" => read_skfont(ar, r),
        "CDimensionLinear" => read_dimlinear(ar, r),
        "CDimensionRadial" => read_dimradial(ar, r),
        "CText" => read_text(ar, r),
        _ => Err(Fault::at(r, format!("no reader for class {name}"))),
    }
}

/// The attribute container a preamble points at, with its slot.
struct Preamble {
    attrs: Option<Rc<Val>>,
    attrs_slot: Option<usize>,
    attrs_is_material: bool,
}

/// Entity preamble: attribute pointer, plus the persistent-id mask and its
/// bytes from 2017 on.
fn preamble(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Preamble> {
    let it = ar.read_object(r, Some("CAttributeContainer"))?;
    if ar.has_pid {
        let mask = r.u8()?;
        for bit in 0..8 {
            if mask & (1 << bit) != 0 {
                r.u8()?;
            }
        }
    }
    let attrs_is_material = matches!(it.value.as_deref(), Some(Val::Material { .. }));
    Ok(Preamble {
        attrs: it.value,
        attrs_slot: it.slot,
        attrs_is_material,
    })
}

/// The preamble of the entity types that carry no attribute pointer in a
/// SketchUp 3 file (vertex, loop, edge use, material, layer).
fn early_preamble(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<()> {
    if ar.ver < FIRST_V4 {
        return Ok(());
    }
    preamble(ar, r).map(|_| ())
}

/// The draw block: material, flags, layer. The layer is normally an id, but
/// an entity can carry it as an object (a full layer record on first use, an
/// escaped back-ref later); both object forms keep the default id.
fn drawbase(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Db> {
    let n = if ar.ver < FIRST_V4 {
        5
    } else if ar.ver < 5 {
        7
    } else {
        8
    };
    let b = r.raw(n).to_vec();
    let lay_cls = ar.class_slot.get("CLayer").copied();
    let tag = r.peek_u16()?;
    let layer = if lay_cls.is_some_and(|c| usize::from(tag) == (0x8000 | c)) {
        ar.read_object(r, Some("CLayer"))?;
        0
    } else if tag == 0x7FFF {
        r.u16()?;
        let big = r.u32()?;
        if big & 0x8000_0000 != 0 {
            return Err(Fault::at(r, "drawbase layer: unexpected class"));
        }
        0
    } else {
        r.u16()?
    };
    if ar.ver < FIRST_V4 {
        return Ok(Db {
            layer,
            ..Db::default()
        });
    }
    if b.len() < 7 {
        return Err(Fault::Bounds);
    }
    Ok(Db {
        mat: u16::from_le_bytes([b[0], b[1]]),
        hidden: b[2] != 0,
        soft: b[5] != 0,
        smooth: b[6] != 0,
        layer,
    })
}

fn read_vertex(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    early_preamble(ar, r)?;
    Ok(Val::Vertex(r.f64s::<3>()?))
}

fn read_edge(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let v1 = ar.read_object(r, Some("CVertex"))?.slot;
    let v2 = ar.read_object(r, Some("CVertex"))?.slot;
    let c = ar.read_object(r, None)?;
    if !(c.name.is_none() || c.is("CCurve") || c.is("CArcCurve")) {
        let n = c.name.as_deref().unwrap_or("");
        return Err(Fault::at(r, format!("edge curve pointer resolved to {n}")));
    }
    // The curve's value too: an arc is written in full on its first edge
    // only, and later edges refer back to it.
    Ok(Val::Edge {
        db,
        v1,
        v2,
        curve: c.slot,
        curve_value: c.value,
    })
}

fn read_curve(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    r.u8()?;
    r.u32()?;
    Ok(Val::Curve)
}

/// One analytic arc: centre, normal, x axis, start and end angles, y axis.
fn read_arccurve(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    r.raw(5);
    let v = r.f64s::<14>()?;
    Ok(Val::ArcCurve(ArcFrame {
        center: [v[0], v[1], v[2]],
        x_axis: [v[6], v[7], v[8]],
        start: v[9],
        end: v[10],
        y_axis: [v[11], v[12], v[13]],
    }))
}

fn read_edgeuse(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    early_preamble(ar, r)?;
    let edge = ar.read_object(r, Some("CEdge"))?.slot;
    let sense = r.u8()?;
    // The parent loop's back-ref, read as a raw file index: after
    // annotations it can run ahead of the walk (burned indices).
    let p0 = r.pos;
    let tag = r.u16()?;
    let ps: Option<usize> = if tag == 0x7FFF {
        let ps = r.u32()?;
        if ps & 0x8000_0000 != 0 {
            return Err(Fault::at(r, "edge-use parent is a new object"));
        }
        Some(ps as usize)
    } else if tag == 0xFFFF || tag & 0x8000 != 0 {
        return Err(Fault::at(r, "edge-use parent is a new object"));
    } else if tag == 0 {
        None
    } else {
        Some(usize::from(tag))
    };
    let expected = ar.current_loop.map(|l| l + ar.cum_delta);
    if ps != expected {
        let delta = match (ps, expected) {
            (Some(p), Some(e)) => p as i64 - e as i64,
            _ => 0,
        };
        if 0 < delta && delta <= 4096 && ar.annot_watermark.is_some() {
            ar.register_burn(delta as usize);
        } else {
            r.pos = p0;
            let show = |s: Option<usize>| s.map_or_else(|| "None".to_string(), |s| s.to_string());
            return Err(Fault::at(
                r,
                format!(
                    "edge-use parent slot {} != current loop {}",
                    show(ps),
                    show(expected)
                ),
            ));
        }
    }
    Ok(Val::EdgeUse {
        edge,
        reversed: sense != 0,
    })
}

fn read_loop(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    let my_slot = ar.next_slot - 1;
    let prev = ar.current_loop;
    ar.current_loop = Some(my_slot);
    let result = (|| {
        early_preamble(ar, r)?;
        r.raw(2);
        let mut uses = Vec::new();
        loop {
            if r.peek_u16()? == 0 {
                r.pos += 2;
                break;
            }
            uses.push(ar.read_object(r, Some("CEdgeUse"))?.value);
        }
        Ok(uses)
    })();
    ar.current_loop = prev;
    Ok(Val::Loop(result?))
}

fn read_face(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    let pre = preamble(ar, r)?;
    let mut db = drawbase(ar, r)?;
    let mut attrs = pre.attrs;
    if ar.ver < FIRST_V4 && pre.attrs_is_material {
        // SketchUp 3: the pointer opening a face is its front material.
        db.mat = pre.attrs_slot.map_or(0, |s| s as u16);
        attrs = None;
    }
    let plane = r.f64s::<4>()?;
    let nloops = r.u32()?;
    if nloops > 10_000 {
        return Err(Fault::at(r, format!("implausible loop count {nloops}")));
    }
    let mut loops = Vec::with_capacity(nloops as usize);
    for _ in 0..nloops {
        loops.push(ar.read_object(r, Some("CLoop"))?.value);
    }
    // Edges first written inside this face's loops follow the back
    // material as redundant list items; the entity list reads them.
    let back_mat = r.u16()?;
    Ok(Val::Face {
        db,
        normal: [plane[0], plane[1], plane[2]],
        loops,
        back_mat,
        attrs,
    })
}

fn read_attr_container(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let mut children = Vec::new();
    loop {
        if r.peek_u16()? == 0 {
            r.pos += 2;
            break;
        }
        children.push(ar.read_object(r, Some("CAttributeNamed"))?.value);
    }
    Ok(Val::Attrs(children))
}

/// One typed attribute value, read through.
fn skip_typed(r: &mut R<'_>, t: u8, depth: usize) -> Res<()> {
    match t {
        0x00 => {}
        0x04 => {
            r.u32()?;
        }
        0x06 | 0x0C => {
            r.f64()?;
        }
        0x07 => {
            r.u8()?;
        }
        0x09 => {
            r.u32()?;
        }
        0x0A => {
            r.utf16()?;
        }
        0x0B => {
            let n = r.u32()?;
            if n > 100_000 {
                return Err(Fault::at(r, "implausible attr array count"));
            }
            if depth > 64 {
                return Err(Fault::at(r, "attribute arrays nested too deep"));
            }
            for _ in 0..n {
                let t = r.u8()?;
                skip_typed(r, t, depth + 1)?;
            }
        }
        0x11 | 0x12 => {
            r.f64s::<3>()?;
        }
        _ => return Err(Fault::at(r, format!("unknown attribute value type {t:#x}"))),
    }
    Ok(())
}

fn read_attr_named(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    r.raw(4);
    r.utf16()?;
    loop {
        let key = r.utf16()?;
        if key.is_empty() {
            break;
        }
        let t = r.u8()?;
        skip_typed(r, t, 0)?;
    }
    // Its own trailing u32 came with SketchUp 7.
    if ar.ver >= 7 {
        r.u32()?;
    }
    Ok(Val::Dict)
}

fn read_layer(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    let me = ar.current_obj_slot;
    early_preamble(ar, r)?;
    let name = r.utf16()?;
    let mut mid: Vec<u8> = Vec::new();
    while mid.len() < 8 && r.peek(3) != STR_MARKER {
        mid.extend_from_slice(r.raw(1));
        if r.pos >= r.data.len() {
            break;
        }
    }
    r.utf16()?;
    let flags = r.u16()?;
    let hidden = mid.first().is_some_and(|b| *b != 0);
    if flags & 0x00FF != 0 {
        // Colour by layer with a textured material: the texture block a
        // material carries, instead of the plain colour.
        let tex = texture_block(ar, r)?;
        r.raw(4);
        return Ok(Val::Layer {
            name,
            hidden,
            rgba: tex.rgba,
        });
    }
    let rgba = four(r.raw(4))?;
    r.utf16()?;
    r.raw(if ar.ver < FIRST_V4 { 17 } else { 21 });
    ar.skip_clayer_colour_ext(r);
    ar.skip_clayer_parent_ref(r, me);
    Ok(Val::Layer { name, hidden, rgba })
}

fn four(b: &[u8]) -> Res<[u8; 4]> {
    b.get(..4)
        .map(|b| [b[0], b[1], b[2], b[3]])
        .ok_or(Fault::Bounds)
}

/// The textured-material payload, shared by materials and textured colour
/// layers: the image, its applied size, file name, average colour and
/// opacity.
fn texture_block(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<TextureRec> {
    let (dib, data) = if ar.ver < FIRST_DIB_OBJECT {
        match read_dib(r)? {
            Val::Dib(d) => (None, Some(d)),
            _ => (None, None),
        }
    } else {
        // The flag was read as a u16: its high byte opens the preamble.
        r.pos = r.pos.saturating_sub(1);
        preamble(ar, r)?;
        let it = ar.read_object(r, Some("CDib"))?;
        if !matches!(it.value.as_deref(), Some(Val::Dib(_))) {
            return Err(Fault::at(r, "texture object is not a dib"));
        }
        (it.slot, None)
    };
    // An optional u32 between the image and the applied size.
    match find(r.data, STR_MARKER, r.pos, r.pos + 28).map(|m| m - r.pos) {
        Some(20) => {
            r.u32()?;
        }
        Some(16) => {}
        _ => return Err(Fault::at(r, "texture size block misaligned")),
    }
    let width = r.f64()?;
    let height = r.f64()?;
    let file = r.utf16()?;
    let avg = r.raw(9).to_vec();
    r.utf16()?;
    r.raw(8);
    let opacity = r.f64()?;
    let use_opacity = r.u8()? != 0;
    Ok(TextureRec {
        rgba: four(&avg)?,
        opacity,
        use_opacity,
        dib,
        data,
        width,
        height,
        file,
    })
}

fn read_material(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    early_preamble(ar, r)?;
    let name = r.utf16()?;
    let texflag = r.u16()?;
    if texflag == 0 {
        let rgba = four(r.raw(4))?;
        r.utf16()?;
        r.raw(8);
        let opacity = r.f64()?;
        let use_opacity = r.u8()? != 0;
        return Ok(Val::Material {
            name,
            rgba,
            opacity,
            use_opacity,
            texture: None,
        });
    }
    let t = texture_block(ar, r)?;
    Ok(Val::Material {
        name,
        rgba: t.rgba,
        opacity: t.opacity,
        use_opacity: t.use_opacity,
        texture: Some(t),
    })
}

fn read_dib(r: &mut R<'_>) -> Res<Val> {
    r.u32()?;
    let length = r.u32()? as usize;
    if length > r.data.len() {
        return Err(Fault::at(r, format!("implausible dib length {length}")));
    }
    Ok(Val::Dib(Rc::from(r.raw(length))))
}

/// Texture-mapping matrices and pins.
fn read_ftc(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    r.u32()?;
    let ks = r.f64s::<24>()?;
    for _ in 0..2 {
        let n = r.u32()?;
        if (n as usize).saturating_mul(32) > r.data.len() {
            return Err(Fault::Bounds);
        }
        for _ in 0..n {
            r.f64s::<4>()?;
        }
    }
    r.u32()?;
    r.u32()?;
    let mut front = [0.0; 9];
    let mut back = [0.0; 9];
    front.copy_from_slice(&ks[0..9]);
    back.copy_from_slice(&ks[12..21]);
    Ok(Val::Ftc { front, back })
}

fn read_camera(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    r.raw(137);
    r.u16()?;
    r.utf16()?;
    r.raw(if ar.ver >= 6 { 33 } else { 10 });
    Ok(Val::Camera)
}

fn read_thumbnail(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    if ar.ver < FIRST_V4 {
        r.raw(129);
        return Ok(Val::Thumbnail);
    }
    ar.read_object(r, Some("CCamera"))?;
    if ar.ver < 6 {
        return Ok(Val::Thumbnail);
    }
    ar.read_object(r, Some("CDib"))?;
    Ok(Val::Thumbnail)
}

/// An Image entity: placed like a component, its definition holding the
/// image's face and texture.
fn read_image(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    drawbase(ar, r)?;
    let def = ar.read_object(r, None)?.slot;
    let xf = r.f64s::<12>()?;
    r.f64()?;
    r.utf16()?;
    // A GUID follows from 2014 on, as for groups and components.
    if ar.ver >= 14 {
        r.raw(16);
    }
    Ok(Val::Image { def, xf })
}

fn read_relationship(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    entity_ref(ar, r)?;
    entity_ref(ar, r)?;
    Ok(Val::Relationship)
}

fn read_constructionline(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let point = r.f64s::<3>()?;
    let direction = r.f64s::<3>()?;
    let [start_param, end_param] = r.f64s::<2>()?;
    // The trailing block varies by the writing build; it is found on the
    // first guide line of the file and kept for the rest.
    let k = match ar.cline_tail {
        Some(k) => k,
        None => {
            let default = if ar.ver == 17 { 7 } else { 4 };
            let order: Vec<usize> = std::iter::once(default)
                .chain([0, 4, 7].into_iter().filter(|c| *c != default))
                .collect();
            let k = order
                .iter()
                .copied()
                .find(|c| strict_next_tag(ar, r.pos + c, false))
                .or_else(|| {
                    [4, 0, 7]
                        .into_iter()
                        .find(|c| strict_next_tag(ar, r.pos + c, true))
                })
                .unwrap_or(4);
            ar.cline_tail = Some(k);
            k
        }
    };
    r.raw(k);
    let huge = 1e20;
    let at = |t: f64| {
        (t.abs() < huge).then(|| {
            [
                point[0] + direction[0] * t,
                point[1] + direction[1] * t,
                point[2] + direction[2] * t,
            ]
        })
    };
    Ok(Val::CLine {
        db,
        point,
        direction,
        start: at(start_param),
        end: at(end_param),
    })
}

fn read_constructionpoint(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let pos = r.f64s::<3>()?;
    r.f64s::<3>()?;
    r.u8()?;
    Ok(Val::CPoint { db, pos })
}

fn read_sectionplane(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    // An optional object pointer before the plane: a real plane starts with
    // a unit normal's component, which a tag word does not decode as.
    let first = f64_at(r.data, r.pos)?;
    let unit_component = first.abs() <= 1.0001;
    if !unit_component {
        ar.read_object(r, None)?;
    }
    let plane = r.f64s::<4>()?;
    let mut name = String::new();
    if r.peek(3) == STR_MARKER {
        name = r.utf16()?;
        r.utf16()?;
    }
    Ok(Val::SectionPlane { db, plane, name })
}

fn read_skfont(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    ar.read_object(r, Some("CAttributeContainer"))?;
    if ar.has_pid {
        let mask = r.u8()?;
        r.raw(mask.count_ones() as usize);
    }
    r.utf16()?;
    r.raw(15);
    Ok(Val::Font)
}

/// A reference to an entity that may point forward (dimension connection
/// points, leader attachments); `None` for a null. An entity not yet written
/// is written in full here, and read as such.
fn entity_ref(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Option<usize>> {
    let tag = r.peek_u16()?;
    let new = tag == 0xFFFF
        || (tag != 0x7FFF && tag & 0x8000 != 0)
        || (tag == 0x7FFF && u32_at(r.data, r.pos + 2)? & 0x8000_0000 != 0);
    if new {
        return Ok(ar.read_object(r, None)?.slot);
    }
    r.pos += 2;
    if tag == 0 {
        return Ok(None);
    }
    if tag == 0x7FFF {
        return Ok(Some(r.u32()? as usize));
    }
    Ok(Some(usize::from(tag)))
}

/// What follows a dimension connection ref: an entity ref and two lists of
/// refs (the groups the anchored entity lives in).
fn connection_paths(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<()> {
    entity_ref(ar, r)?;
    for _ in 0..2 {
        let count = r.u32()?;
        if count > 1000 {
            return Err(Fault::at(r, "implausible dimension path length"));
        }
        for _ in 0..count {
            path_ref(ar, r)?;
        }
    }
    Ok(())
}

/// One group on a dimension's path: usually a reference, but written in
/// full when the dimension comes before the group it is anchored in.
fn path_ref(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Option<usize>> {
    let tag = r.peek_u16()?;
    let mut new = tag == 0xFFFF || (tag != 0x7FFF && tag & 0x8000 != 0);
    if tag == 0x7FFF {
        let big = u32_at(r.data, r.pos + 2)?;
        new = big & 0x8000_0000 != 0;
    }
    if new {
        return Ok(ar.read_object(r, None)?.slot);
    }
    entity_ref(ar, r)
}

fn point_at(b: &[u8], at: usize) -> Res<[f64; 3]> {
    Ok([f64_at(b, at)?, f64_at(b, at + 8)?, f64_at(b, at + 16)?])
}

fn read_dimlinear(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let text = r.utf16()?;
    if ar.ver >= 6 {
        ar.read_object(r, Some("CSkFont"))?;
    }
    // SketchUp 4 and 5 write neither the font nor the 5 bytes before the
    // first connection block: supply them as zeros so offsets agree.
    let mut b37 = if ar.ver < 6 { vec![0u8; 5] } else { Vec::new() };
    b37.extend_from_slice(r.raw(if ar.ver < 6 { 32 } else { 37 }));
    let c1 = entity_ref(ar, r)?;
    connection_paths(ar, r)?;
    let b32 = r.raw(32).to_vec();
    let c2 = entity_ref(ar, r)?;
    connection_paths(ar, r)?;
    let b72 = r.raw(if ar.ver >= 6 { 72 } else { 68 }).to_vec();
    let offset = f64_at(&b72, 52)?;
    // Connection blocks: type 1 is a free point stored inline.
    let a = if u32_at(&b37, 5)? == 1 {
        Some(point_at(&b37, 13)?)
    } else {
        None
    };
    let b = if u32_at(&b32, 0)? == 1 {
        Some(point_at(&b32, 8)?)
    } else {
        None
    };
    Ok(Val::Dimension {
        db,
        text,
        connect: [c1, c2],
        offset,
        free: [a, b],
    })
}

/// A radial dimension, read through and left out: its text, its font, the
/// arc it measures, two placement values, and whether it shows the
/// diameter; a radius names one more object.
fn read_dimradial(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    drawbase(ar, r)?;
    r.utf16()?;
    ar.read_object(r, Some("CSkFont"))?;
    r.raw(5);
    ar.read_object(r, None)?;
    r.f64s::<2>()?;
    if r.u8()? == 0 {
        ar.read_object(r, None)?;
    }
    Ok(Val::Font)
}

/// A text's middle, read field by field: where screen text sits, the
/// connection block (its type; a free point or the attachment's parameters;
/// the entity it is attached to and the groups on the way, any of them
/// written in full here), the label's offset from the anchor and a
/// direction the file keeps (the view's), then the block before the string.
struct TextMiddle {
    kind: u32,
    first: [f64; 3],
    target: Option<usize>,
    path: Vec<usize>,
    offset: Option<[f64; 3]>,
    screen: Option<[f64; 2]>,
}

/// An entity reference in the walk's own numbering: a back-reference
/// translated through the burned bands (it may point ahead, to an entity
/// not read yet), or an entity written in full here, read.
fn walk_ref(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Option<usize>> {
    let tag = r.peek_u16()?;
    let new = tag == 0xFFFF
        || (tag != 0x7FFF && tag & 0x8000 != 0)
        || (tag == 0x7FFF && u32_at(r.data, r.pos + 2)? & 0x8000_0000 != 0);
    if new {
        return Ok(ar.read_object(r, None)?.slot);
    }
    Ok(entity_ref(ar, r)?.and_then(|s| ar.translate_ref(s)))
}

fn text_middle(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<TextMiddle> {
    let nonzero = |v: [f64; 3]| v.iter().any(|c| c.abs() > 1e-12).then_some(v);
    let screen = r.f64s::<2>()?;
    let kind = r.u32()?;
    if kind > 16 || r.u32()? != 4 {
        return Err(Fault::at(r, "text connection block"));
    }
    let first = r.f64s::<3>()?;
    let target = walk_ref(ar, r)?;
    entity_ref(ar, r)?;
    let mut path = Vec::new();
    for list in 0..2 {
        let count = r.u32()?;
        if count > 1000 {
            return Err(Fault::at(r, "implausible text path length"));
        }
        for _ in 0..count {
            let step = walk_ref(ar, r)?;
            if list == 0 {
                path.extend(step);
            }
        }
    }
    let offset = r.f64s::<3>()?;
    r.f64s::<3>()?;
    r.raw(15);
    if r.peek(3) != STR_MARKER {
        return Err(Fault::at(r, "text string not where expected"));
    }
    Ok(TextMiddle {
        kind,
        first,
        target,
        path,
        offset: nonzero(offset),
        screen: (kind == 0).then_some(screen),
    })
}

fn read_text(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    ar.read_object(r, Some("CSkFont"))?;
    let start = r.pos;
    let middle = match text_middle(ar, r) {
        Ok(m) => m,
        Err(Fault::Parse { .. } | Fault::Bounds) => {
            r.pos = start;
            text_middle_by_search(r)?
        }
        Err(e) => return Err(e),
    };
    let text = r.utf16()?;
    r.raw(5);
    // Leader attachments in the escaped form only.
    while r.peek(2) == b"\xff\x7f" {
        let val = u32_at(r.data, r.pos + 2)?;
        if val & 0x8000_0000 != 0 {
            break;
        }
        r.raw(6);
    }
    Ok(Val::Text {
        db,
        text,
        kind: middle.kind,
        first: middle.first,
        target: middle.target,
        path: middle.path,
        offset: middle.offset,
        screen: middle.screen,
    })
}

/// A text's middle found the old way, when it does not read field by field:
/// the string's marker after an 11-byte block, and a free point if one is
/// there. Leaves `r` at the string.
fn text_middle_by_search(r: &mut R<'_>) -> Res<TextMiddle> {
    let mut p = r.pos;
    let idx = loop {
        let Some(idx) = find(r.data, STR_MARKER, p, r.pos + 512) else {
            return Err(Fault::at(r, "text delimiter not found"));
        };
        if idx >= 11 {
            let blk = &r.data[idx - 11..idx];
            let arrow = u32_at(blk, 6)?;
            if blk[..4] == [1, 0, 0, 0] && arrow <= 8 && blk[10] == 1 {
                break idx;
            }
        }
        p = idx + 3;
    };
    let mut point = None;
    let mut label = None;
    let mid = clip(r.data, r.pos, idx);
    if let Some(off) = mid.windows(8).position(|w| w == [1, 0, 0, 0, 4, 0, 0, 0])
        && off + 32 <= mid.len()
    {
        point = Some(point_at(mid, off + 8)?);
        let lo = off + 32 + 12;
        if lo + 24 <= mid.len() {
            let lp = point_at(mid, lo)?;
            if lp.iter().any(|c| c.abs() > 1e-12) {
                label = Some(lp);
            }
        }
    }
    r.raw(idx - r.pos);
    Ok(TextMiddle {
        kind: u32::from(point.is_some()),
        first: point.unwrap_or_default(),
        target: None,
        path: Vec::new(),
        offset: label,
        screen: None,
    })
}

/// An entity list of `count` items owned by a definition (`def`) or the
/// model root.
pub(crate) fn read_entity_list(
    ar: &mut Archive<'_>,
    r: &mut R<'_>,
    count: usize,
    root: bool,
) -> Res<Vec<Item>> {
    ar.burn_stack.push(0);
    let out = read_entity_list_inner(ar, r, count, root);
    ar.burn_stack.pop();
    out
}

fn read_entity_list_inner(
    ar: &mut Archive<'_>,
    r: &mut R<'_>,
    count: usize,
    root: bool,
) -> Res<Vec<Item>> {
    let mut ents = Vec::new();
    let burned = |ar: &Archive<'_>| ar.burn_stack.last().is_some_and(|b| *b > 0);
    while ents.len() < count {
        let p = r.pos;
        // A definition's count can run past what it holds (burned indices
        // count phantom entities; 2013 can declare entities it never
        // writes): the definition's tail marks the list's real end.
        if !root && (burned(ar) || !ents.is_empty()) && definition_tail_at(r.data, p)? {
            break;
        }
        match ar.read_object(r, None) {
            Ok(it) => ents.push(it),
            Err(Fault::Parse { .. }) if root => {
                // An over-declared root count runs into the document tail.
                r.pos = p;
                break;
            }
            Err(Fault::Parse { .. }) if burned(ar) => {
                r.pos = p;
                break;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(ents)
}

/// Whether a definition's tail starts at `p`: a small count, zero bytes
/// (its null relationships and a word), the GUID, then the name's string
/// marker.
fn definition_tail_at(data: &[u8], p: usize) -> Res<bool> {
    let n = u32_at(data, p)? as usize;
    if n > 16 {
        return Ok(false);
    }
    Ok((0..=2 * n + 2).step_by(2).any(|z| {
        let zeros = clip(data, p + 4, p + 4 + z);
        zeros.len() == z
            && zeros.iter().all(|b| *b == 0)
            && clip(data, p + 4 + z + 16, p + 4 + z + 19) == STR_MARKER
    }))
}

/// Where a definition's GUID and name start, when the fixed-width prefix
/// before them runs a few bytes long or short.
fn reanchor_on_string_marker(data: &[u8], pos: usize) -> usize {
    let at_guid = |at: usize| clip(data, at + 16, at + 19) == STR_MARKER;
    if at_guid(pos) {
        return pos;
    }
    for skip in [-1i64, 1, -2, 2, -3, 3, -4, 4] {
        let at = pos as i64 + skip;
        if at >= 0 && at_guid(at as usize) {
            return at as usize;
        }
    }
    pos
}

/// SketchUp 2020 writes an empty string and zero padding ahead of some
/// counts. When a count came back implausible (or zero), find that filler
/// just after `count_pos` and take the first plausible count behind it that
/// is followed by a legitimate list tag. `None` when this is not that
/// layout.
pub(crate) fn retry_count_after_v20_filler(
    ar: &Archive<'_>,
    r: &mut R<'_>,
    count_pos: usize,
    limit: u32,
) -> Option<u32> {
    let data = r.data;
    let end = data.len().saturating_sub(3).min(count_pos + 12);
    let marker_at = (count_pos..end).find(|&i| &data[i..i + 3] == STR_MARKER)?;
    if data.get(marker_at + 3) != Some(&0) {
        return None;
    }
    let mut at = marker_at + 4;
    while at < data.len() && data[at] == 0 {
        at += 1;
    }
    for back in 0..4 {
        let Some(at2) = at.checked_sub(back) else {
            continue;
        };
        if at2 < marker_at + 4 || at2 + 4 > data.len() {
            continue;
        }
        let Ok(count) = u32_at(data, at2) else {
            continue;
        };
        if !(0 < count && count <= limit) {
            continue;
        }
        if !crate::archive::plausible_list_tag(ar, at2 + 4) {
            continue;
        }
        r.pos = at2 + 4;
        return Some(count);
    }
    None
}

fn read_definition(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    r.raw(if ar.ver >= 17 {
        22
    } else if ar.ver >= 5 {
        20
    } else if ar.ver >= FIRST_V4 {
        19
    } else {
        13
    });
    let nlayers = r.u32()?;
    if nlayers > 10_000 {
        return Err(Fault::at(r, "implausible def layer count"));
    }
    // Real layers (records or back-refs); 2020 puts nulls between them.
    let mut got = 0;
    while got < nlayers {
        if r.peek_u16()? == 0 {
            r.pos += 2;
            continue;
        }
        ar.read_object(r, Some("CLayer"))?;
        got += 1;
    }
    if ar.ver == 19 && nlayers > 0 && r.peek_u16()? == 0 {
        // 2019 follows the last layer with a null too.
        r.pos += 2;
    }
    let decl = r.u16()?;
    if decl == 0x7FFF {
        r.u32()?;
    }
    let mut count = None;
    if ar.ver >= 20 {
        let at = r.pos;
        count = retry_count_after_v20_filler(ar, r, at, 5_000_000);
    }
    let mut count = match count {
        Some(c) => c,
        None => {
            r.u32()?;
            r.u32()?
        }
    };
    if count > 5_000_000 || count == 0 {
        let at = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(ar, r, at, 5_000_000) {
            count = retry;
        }
    }
    if count > 5_000_000 {
        return Err(Fault::at(r, "implausible def entity count"));
    }
    let ents = read_entity_list(ar, r, count as usize, false)?;
    let mut nrel = r.u32()?;
    if nrel > 100_000 {
        let at = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(ar, r, at, 100_000) {
            nrel = retry;
        }
    }
    if nrel > 100_000 {
        return Err(Fault::at(r, "definition list misaligned"));
    }
    for _ in 0..nrel {
        ar.read_object(r, Some("CRelationship"))?;
    }
    r.u16()?;
    r.pos = reanchor_on_string_marker(r.data, r.pos);
    let guid = r.raw(16).iter().map(|b| format!("{b:02X}")).collect();
    let name = r.utf16()?;
    r.utf16()?;
    r.utf16()?;
    r.u32()?;
    // An undecoded block, then the thumbnail.
    let thumb_slot = ar.class_slot.get("CThumbnail").copied();
    let data = r.data;
    let tpos = (0..96).map(|off| r.pos + off).find(|&p| {
        (clip(data, p, p + 2) == b"\xff\xff"
            && clip(data, p + 4, p + 6) == b"\x0a\x00"
            && clip(data, p + 6, p + 16) == b"CThumbnail")
            || thumb_slot.is_some_and(|s| is_class_ref(data, p, s))
    });
    let Some(tpos) = tpos else {
        return Err(Fault::at(r, "definition tail: thumbnail not found"));
    };
    r.raw(tpos - r.pos);
    ar.read_object(r, Some("CThumbnail"))?;
    Ok(Val::Definition { name, guid, ents })
}

fn read_instance(ar: &mut Archive<'_>, r: &mut R<'_>) -> Res<Val> {
    preamble(ar, r)?;
    let db = drawbase(ar, r)?;
    let d = ar.read_object(r, Some("CComponentDefinition"))?;
    if !d.is("CComponentDefinition") {
        let n = d.name.as_deref().unwrap_or("None");
        return Err(Fault::at(r, format!("instance definition ref is {n}")));
    }
    let xf = r.f64s::<13>()?;
    // SketchUp 3 and 4 write no name; where it is optional, take one only
    // if a string is there.
    let name = if ar.ver >= FIRST_NAMED_INSTANCE || r.peek(3) == STR_MARKER {
        r.utf16()?
    } else {
        String::new()
    };
    // A GUID follows from 2014 on, for groups and components alike.
    if ar.ver >= 14 {
        r.raw(16);
    }
    Ok(Val::Instance {
        db,
        def: d.slot,
        xf,
        name,
    })
}

/// Whether `at` begins a definition record: its class declaration, or a
/// class-ref to the known definition class.
pub(crate) fn definition_starts_at(ar: &Archive<'_>, at: usize) -> bool {
    let data = ar.data;
    if clip(data, at, at + 2) == b"\xff\xff"
        && clip(data, at + 6, at + 26) == b"CComponentDefinition"
    {
        return true;
    }
    ar.class_slot
        .get("CComponentDefinition")
        .is_some_and(|&c| is_class_ref(data, at, c))
}

/// Whether `it` is an opaque reference into the part never walked.
pub(crate) fn is_premodel(it: &Item) -> bool {
    it.is(PREMODEL)
}

/// The u16 tag at the cursor, for the walk's lookahead.
pub(crate) fn tag_at(r: &R<'_>) -> Res<u16> {
    u16_at(r.data, r.pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A class declared in full: its tag, schema, name.
    fn class(name: &str) -> Vec<u8> {
        let mut d = vec![0xFF, 0xFF, 0, 0];
        d.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        d.extend_from_slice(name.as_bytes());
        d
    }

    /// A null attribute pointer and a plain draw block (no material, the
    /// default layer).
    fn header() -> Vec<u8> {
        vec![0; 2 + 8 + 2]
    }

    fn string(s: &str) -> Vec<u8> {
        let mut d = STR_MARKER.to_vec();
        d.push(u8::try_from(s.len()).unwrap());
        for c in s.encode_utf16() {
            d.extend_from_slice(&c.to_le_bytes());
        }
        d
    }

    fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    /// Reads one object from `d` (with more bytes after it, as in a file)
    /// as version `ver`; returns where it ended.
    fn read_one(d: &[u8], ver: u32) -> Res<usize> {
        let mut file = d.to_vec();
        file.extend([0xEE; 32]);
        let mut ar = Archive::new(&file, ver);
        let mut r = R::new(&file, 0);
        ar.read_object(&mut r, None)?;
        Ok(r.pos)
    }

    fn radial(diameter: bool) -> Vec<u8> {
        let mut d = class("CDimensionRadial");
        d.extend(header());
        d.extend(string("DIA 8mm"));
        d.extend([0, 0]); // font (null here)
        d.extend([0, 3, 0, 0, 0]);
        d.extend([0, 0]); // the arc measured (null here)
        d.extend(f64s(&[5.156, 1.193]));
        d.push(u8::from(diameter));
        if !diameter {
            d.extend([0, 0]); // the radius's further object
        }
        d
    }

    #[test]
    fn a_radial_dimension_reads_through_as_a_diameter_or_a_radius() {
        for diameter in [true, false] {
            let d = radial(diameter);
            assert_eq!(read_one(&d, 17 - 1), Ok(d.len()), "diameter {diameter}");
        }
    }

    fn image(ver: u32) -> Vec<u8> {
        let mut d = class("CImage");
        d.extend(header());
        d.extend([0, 0]); // its definition (null here)
        d.extend(f64s(&[0.0; 13]));
        d.extend(string("photo.png"));
        if ver >= 14 {
            d.extend([0xAB; 16]);
        }
        d
    }

    #[test]
    fn an_image_has_its_guid_from_2014_on_only() {
        for ver in [13, 14, 16] {
            let d = image(ver);
            assert_eq!(read_one(&d, ver), Ok(d.len()), "version {ver}");
        }
    }

    #[test]
    fn a_definition_tail_is_found_with_or_without_null_relationships() {
        let tail = |count: u32, zeros: usize| {
            let mut d = count.to_le_bytes().to_vec();
            d.extend(vec![0; zeros]);
            d.extend([0x6B; 16]);
            d.extend(string("Rack"));
            d
        };
        // No relationships: a word, then the GUID.
        assert_eq!(definition_tail_at(&tail(0, 2), 0), Ok(true));
        // Two null relationships and the word (2013).
        assert_eq!(definition_tail_at(&tail(2, 6), 0), Ok(true));
        // A count of three with six zero bytes (2013).
        assert_eq!(definition_tail_at(&tail(3, 6), 0), Ok(true));
        // Not a tail: a large count, or no name where it would be.
        assert_eq!(definition_tail_at(&tail(500, 2), 0), Ok(false));
        let mut edge = tail(2, 6);
        edge[26] = 0x2D;
        assert_eq!(definition_tail_at(&edge, 0), Ok(false));
    }

    #[test]
    fn an_entity_not_yet_written_is_read_where_it_is_referred_to() {
        // A vertex written in full in place of a reference.
        let mut d = class("CVertex");
        d.extend([0, 0]);
        d.extend(f64s(&[1.0, 2.0, 3.0]));
        let mut ar = Archive::new(&d, 16);
        let mut r = R::new(&d, 0);
        let slot = entity_ref(&mut ar, &mut r).unwrap();
        assert_eq!(r.pos, d.len());
        assert!(slot.is_some());
        // A plain reference is only read.
        let back = [5u8, 0];
        let mut r = R::new(&back, 0);
        assert_eq!(entity_ref(&mut ar, &mut r), Ok(Some(5)));
        assert_eq!(r.pos, 2);
    }

    /// A whole text record: class, header, font (null here), the middle
    /// (`screen`, `kind`, its first 24 bytes, `refs`, the label's offset,
    /// the view's direction), the block before the string (ending `last`),
    /// the string, five bytes.
    fn text(
        screen: [f64; 2],
        kind: u32,
        first: [f64; 3],
        refs: &[u8],
        offset: [f64; 3],
        last: u8,
    ) -> Vec<u8> {
        let mut d = class("CText");
        d.extend(header());
        d.extend([0, 0]);
        d.extend(f64s(&screen));
        d.extend(kind.to_le_bytes());
        d.extend(4u32.to_le_bytes());
        d.extend(f64s(&first));
        d.extend_from_slice(refs);
        d.extend(f64s(&offset));
        d.extend(f64s(&[0.0, -1.0, 0.0]));
        d.extend([2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 3, 0, 0, 0, last]);
        d.extend(string("T joint"));
        d.extend([0; 5]);
        d
    }

    /// No entity, no groups: two null refs and two empty lists.
    const NO_REFS: [u8; 12] = [0; 12];

    fn read_text_record(d: &[u8]) -> (usize, Option<Val>) {
        let mut file = d.to_vec();
        file.extend([0xEE; 32]);
        let mut ar = Archive::new(&file, 16);
        let mut r = R::new(&file, 0);
        let it = ar.read_object(&mut r, None).unwrap();
        (r.pos, it.value.as_deref().cloned())
    }

    #[test]
    fn a_label_on_a_vertex_keeps_its_target_and_offset() {
        let mut refs = vec![0x86, 0x0F]; // the vertex
        refs.extend([0; 10]);
        let d = text([0.0; 2], 2, [0.0; 3], &refs, [1.0, 2.0, 3.0], 1);
        let (end, v) = read_text_record(&d);
        assert_eq!(end, d.len());
        let Some(Val::Text {
            text,
            kind,
            target,
            path,
            offset,
            screen,
            ..
        }) = v
        else {
            panic!("not a text: {v:?}");
        };
        assert_eq!(text, "T joint");
        assert_eq!(kind, 2);
        assert_eq!(target, Some(0x0F86));
        assert!(path.is_empty());
        assert_eq!(offset, Some([1.0, 2.0, 3.0]));
        assert_eq!(screen, None);
    }

    #[test]
    fn screen_text_keeps_its_place_and_a_free_leader_its_point() {
        let d = text([0.6, 0.25], 0, [0.0; 3], &NO_REFS, [0.0; 3], 1);
        let (end, v) = read_text_record(&d);
        assert_eq!(end, d.len());
        assert!(matches!(
            v,
            Some(Val::Text {
                kind: 0,
                screen: Some([0.6, 0.25]),
                ..
            })
        ));

        let d = text([0.0; 2], 1, [4.0, 5.0, 6.0], &NO_REFS, [0.5; 3], 1);
        let (_, v) = read_text_record(&d);
        assert!(matches!(
            v,
            Some(Val::Text {
                kind: 1,
                first: [4.0, 5.0, 6.0],
                offset: Some([0.5, 0.5, 0.5]),
                screen: None,
                ..
            })
        ));
    }

    #[test]
    fn a_label_whose_part_is_written_inside_it_reads_through() {
        // On an edge (type 5), a group written in full on its path, and
        // the block before the string ending in 0 (which the old search
        // read past).
        let mut refs = vec![0x10, 0x00, 0x00, 0x00]; // the edge, no second ref
        refs.extend(1u32.to_le_bytes());
        refs.extend(class("CVertex"));
        refs.extend([0, 0]);
        refs.extend(f64s(&[7.0, 8.0, 9.0]));
        refs.extend(0u32.to_le_bytes());
        let d = text([0.0; 2], 5, [0.5, 0.0, 0.0], &refs, [1.0, 1.0, 1.0], 0);
        let (end, v) = read_text_record(&d);
        assert_eq!(end, d.len());
        let Some(Val::Text {
            kind: 5,
            first,
            target: Some(0x10),
            path,
            ..
        }) = v
        else {
            panic!("not an edge text: {v:?}");
        };
        assert!((first[0] - 0.5).abs() < 1e-12);
        // The object written in full is on the path.
        assert_eq!(path.len(), 1);
    }

    #[test]
    fn a_class_declared_after_burned_indices_is_found_by_its_file_index() {
        // Four indices burned before the class: the file numbers it 4.
        let mut d = class("CVertex");
        d.extend([0, 0]);
        d.extend(f64s(&[0.0; 3]));
        let first = d.len();
        d.extend([0x04, 0x80]); // a new object of class 4 (file numbering)
        d.extend([0, 0]);
        d.extend(f64s(&[1.0; 3]));
        let mut ar = Archive::new(&d, 16);
        ar.burns.push((0, 4));
        ar.cum_delta = 4;
        let mut r = R::new(&d, 0);
        ar.read_object(&mut r, None).unwrap();
        assert_eq!(r.pos, first);
        let it = ar.read_object(&mut r, None).unwrap();
        assert_eq!(it.name.as_deref(), Some("CVertex"));
        assert_eq!(r.pos, d.len());
    }
}
