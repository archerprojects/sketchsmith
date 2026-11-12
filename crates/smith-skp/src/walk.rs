//! SketchSmith — SKP model walk | v1.0.0 | Thursday 12 November 2026
//!
//! The walk through a 2013–2020 file's object stream: its materials, its
//! layers, the component definitions, then the root's entities.
//!
//! The stream's store map numbers from the start of the file, but the walk
//! begins at the material list, part-way in: it must first find which slot
//! number its first object takes (the "base"). With two or more materials
//! the second one's class-ref names it; otherwise each layer read gives a
//! candidate, from the reference to the active layer that opens the
//! definition list. Every candidate but the right one fails almost at once,
//! so each is tried and the failure that got furthest is the one reported.

use std::rc::Rc;

use crate::archive::{Archive, Item, Val};
use crate::cursor::{Fault, R, Res, u32_at};
use crate::records::{
    definition_starts_at, is_premodel, read_entity_list, retry_count_after_v20_filler, tag_at,
};

/// What the walk read.
pub(crate) struct Walked<'a> {
    pub(crate) ar: Archive<'a>,
    pub(crate) root: Vec<Item>,
    pub(crate) layers: Vec<(usize, Rc<Val>)>,
    pub(crate) materials: Vec<(usize, Rc<Val>)>,
}

/// The major version in the header (`{20.1.235}` → 20).
pub(crate) fn header_version(data: &[u8]) -> Option<u32> {
    let head: Vec<u8> = data
        .iter()
        .take(0x60)
        .copied()
        .filter(|b| *b != 0)
        .collect();
    head.iter()
        .enumerate()
        .filter(|(_, b)| **b == b'{')
        .find_map(|(open, _)| {
            let digits: Vec<u8> = head[open + 1..]
                .iter()
                .copied()
                .take_while(u8::is_ascii_digit)
                .collect();
            if digits.is_empty() || head.get(open + 1 + digits.len()) != Some(&b'.') {
                return None;
            }
            std::str::from_utf8(&digits).ok()?.parse().ok()
        })
}

/// The full version string in the header (`{20.1.235}`).
pub(crate) fn header_version_text(data: &[u8]) -> String {
    let Some(second) = crate::cursor::find(data, crate::cursor::STR_MARKER, 4, data.len()) else {
        return String::from("unknown");
    };
    let raw = crate::cursor::clip(data, second + 4, second + 100);
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let text = String::from_utf16_lossy(&units);
    match (text.find('{'), text.find('}')) {
        (Some(a), Some(b)) if b > a => text[a..=b].to_string(),
        _ => String::from("unknown"),
    }
}

/// The first offset of a new-class record for `name` (`ff ff`, schema,
/// name length, name).
fn class_record(data: &[u8], name: &[u8]) -> Option<usize> {
    let len = u16::try_from(name.len()).ok()?.to_le_bytes();
    let last = data.len().checked_sub(6 + name.len())?;
    (0..=last).find(|&p| {
        data[p] == 0xFF
            && data[p + 1] == 0xFF
            && data[p + 4..p + 6] == len
            && &data[p + 6..p + 6 + name.len()] == name
    })
}

/// Walks the file.
pub(crate) fn walk(data: &[u8]) -> Res<Walked<'_>> {
    let ver = header_version(data).ok_or_else(|| Fault::plain("no version string in header"))?;
    // Anchor: the material list (its count sits right before the first
    // material record). With no material, start at the layer list before
    // the first layer record.
    let (start, mat_count) = match class_record(data, b"CMaterial") {
        Some(start) => {
            let n = u32_at(data, start.checked_sub(4).ok_or(Fault::Bounds)?)?;
            if n > 100_000 {
                return Err(Fault::plain("implausible material count"));
            }
            (start, n)
        }
        None => {
            let lm = class_record(data, b"CLayer")
                .ok_or_else(|| Fault::plain("no CMaterial or CLayer class record found"))?;
            let back = if ver >= 17 { 9 } else { 8 };
            (lm.checked_sub(back).ok_or(Fault::Bounds)?, 0)
        }
    };
    let bases = if mat_count >= 2 {
        vec![bootstrap_two_materials(data, ver, start)?]
    } else {
        probe_layer_anchor_bases(data, ver, start, mat_count)?
    };
    let mut best: Option<Fault> = None;
    for base in bases {
        match walk_model(data, ver, start, mat_count, base) {
            Ok(w) => return Ok(w),
            Err(e) => {
                if best.as_ref().is_none_or(|b| e.reach() > b.reach()) {
                    best = Some(e);
                }
            }
        }
    }
    Err(best.unwrap_or_else(|| Fault::plain("no viable slot base candidate")))
}

/// Reads material 1 with a throwaway archive: material 2's class-ref names
/// the material class's true slot.
fn bootstrap_two_materials(data: &[u8], ver: u32, mat_hdr: usize) -> Res<usize> {
    let mut boot = Archive::new(data, ver);
    boot.next_slot = 1 << 20;
    boot.walk_base = 1 << 20;
    let mut r = R::new(data, mat_hdr);
    boot.read_object(&mut r, Some("CMaterial"))?;
    let tag = r.peek_u16()?;
    if tag == 0xFFFF || tag & 0x8000 == 0 {
        return Err(Fault::plain("cannot bootstrap the slot base"));
    }
    Ok(usize::from(tag & 0x7FFF))
}

/// The layer list's marker: `<u32>` (2016) or `<u32><u8>` (2017 on).
fn layer_marker(r: &mut R<'_>, ver: u32) -> Res<()> {
    r.u32()?;
    if ver >= 17 {
        r.u8()?;
    }
    Ok(())
}

/// Base candidates when the two-material trick is not open: read the
/// prefix with a throwaway base; the object after the layer list is an
/// absolute back-ref to the active layer, a layer just read relatively.
fn probe_layer_anchor_bases(
    data: &[u8],
    ver: u32,
    start: usize,
    mat_count: u32,
) -> Res<Vec<usize>> {
    let mut boot = Archive::new(data, ver);
    let b0 = 1usize << 20;
    boot.next_slot = b0;
    boot.walk_base = b0;
    let mut r = R::new(data, start);
    for _ in 0..mat_count {
        boot.read_object(&mut r, Some("CMaterial"))?;
    }
    layer_marker(&mut r, ver)?;
    // SketchUp 3 writes no layer count.
    let layer_count = if ver < 4 {
        0
    } else {
        let n = r.u32()?;
        if !(1..=100_000).contains(&n) {
            return Err(Fault::plain("implausible layer count in base probe"));
        }
        n
    };
    let mut layer_slots = Vec::new();
    for _ in 0..layer_count {
        if let Some(s) = boot.read_object(&mut r, Some("CLayer"))?.slot {
            layer_slots.push(s);
        }
    }
    boot.collect_trailing_layers(&mut r, Some(&mut layer_slots), None, true)?;
    let anchor = boot.read_object(&mut r, None)?;
    if !is_premodel(&anchor) {
        let n = anchor.name.as_deref().unwrap_or("None");
        return Err(Fault::plain(format!("base probe: anchor resolved to {n}")));
    }
    let s = anchor.slot.unwrap_or(0) as i64;
    Ok(layer_slots
        .iter()
        .map(|rel| s - (*rel as i64 - b0 as i64))
        .filter(|c| 0 < *c && *c < b0 as i64)
        .map(|c| c as usize)
        .collect())
}

fn walk_model(data: &[u8], ver: u32, start: usize, mat_count: u32, base: usize) -> Res<Walked<'_>> {
    let mut ar = Archive::new(data, ver);
    ar.next_slot = base;
    ar.walk_base = base;
    let mut r = R::new(data, start);

    let mut materials = Vec::new();
    for _ in 0..mat_count {
        let it = ar.read_object(&mut r, Some("CMaterial"))?;
        if let (Some(s), Some(v)) = (it.slot, it.value) {
            materials.push((s, v));
        }
    }

    layer_marker(&mut r, ver)?;
    let layer_count = if ver < 4 { 1 << 30 } else { r.u32()? };
    if layer_count > 100_000 && ver >= 4 {
        return Err(Fault::plain("implausible layer count"));
    }
    // The count is of real layers: 2020 puts a null after each record, and
    // a back-ref (the definition-list anchor) ends the list early.
    let mut layers: Vec<(usize, Rc<Val>)> = Vec::new();
    while (layers.len() as u64) < u64::from(layer_count) {
        let tag = r.peek_u16()?;
        if tag == 0 {
            r.pos += 2;
            continue;
        }
        if tag != 0xFFFF && tag & 0x8000 == 0 {
            break;
        }
        let it = ar.read_object(&mut r, Some("CLayer"))?;
        if let (Some(s), Some(v)) = (it.slot, it.value) {
            layers.push((s, v));
        }
    }
    ar.collect_trailing_layers(&mut r, None, Some(&mut layers), false)?;

    // The definition list: a reference to the active layer, then the count.
    let anchor = ar.read_object(&mut r, None)?;
    if !anchor.is("CLayer") {
        let n = anchor.name.as_deref().unwrap_or("None");
        return Err(Fault::plain(format!(
            "definition-list anchor is {n}, not a layer"
        )));
    }
    let mut def_count = r.u32()?;
    if def_count > 1_000_000 {
        let at = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(&ar, &mut r, at, 1_000_000) {
            def_count = retry;
            if !definition_starts_at(&ar, r.pos) {
                // A model with no definitions: the count behind the filler
                // is the root's, so put it back.
                def_count = 0;
                r.pos -= 4;
            }
        }
    }
    if def_count > 1_000_000 {
        return Err(Fault::plain("implausible definition count"));
    }
    for _ in 0..def_count {
        ar.read_object(&mut r, Some("CComponentDefinition"))?;
    }
    // Further definitions, back to back.
    loop {
        let tag = tag_at(&r)?;
        let def_cls = ar.class_slot.get("CComponentDefinition").copied();
        let mut is_def = def_cls.is_some_and(|c| usize::from(tag) == (0x8000 | c));
        if !is_def
            && tag == 0xFFFF
            && crate::cursor::clip(data, r.pos + 6, r.pos + 26) == b"CComponentDefinition"
        {
            is_def = true;
        }
        if !is_def {
            break;
        }
        ar.read_object(&mut r, None)?;
    }

    let mut root_count = r.u32()?;
    if root_count > 5_000_000 {
        let at = r.pos - 4;
        if let Some(retry) = retry_count_after_v20_filler(&ar, &mut r, at, 5_000_000) {
            root_count = retry;
        }
    }
    if root_count > 5_000_000 {
        return Err(Fault::plain("implausible root entity count"));
    }
    let root = read_entity_list(&mut ar, &mut r, root_count as usize, true)?;
    Ok(Walked {
        ar,
        root,
        layers,
        materials,
    })
}
