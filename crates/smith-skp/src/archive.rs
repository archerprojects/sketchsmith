//! SketchSmith — SKP object archive | v1.0.0 | Thursday 12 November 2026
//!
//! A 2013–2020 `.skp` body is one MFC `CArchive` object stream with a single
//! store map numbered from 1. Each object read is one of: a null; a new
//! class (its name and schema, then a new object of it); a reference to a
//! class already seen (a new object of it); or a back-reference to an object
//! already read. Every class and every object takes the next slot. The
//! [`Archive`] keeps that numbering, reads each object with its class's
//! reader (see `records`), and remembers what it read for back-references.
//!
//! Two quirks of the writer are followed exactly as the original reader
//! does:
//!
//! * the walk starts part-way into the file, so slots below its first one
//!   belong to objects it never read ("premodel" references, kept opaque);
//! * annotations attached to geometry map store-map indices without writing
//!   any bytes ("burns"), so later references run ahead of the walk's own
//!   numbering and are translated back through the recorded bands.

use std::collections::HashMap;
use std::rc::Rc;

use crate::cursor::{Fault, R, Res, u16_at, u32_at};
use crate::records;

/// How deep objects may nest inside each other before the file is taken to
/// be broken (real files stay far below it).
const MAX_DEPTH: usize = 1000;

/// What a slot holds.
#[derive(Debug, Clone)]
pub(crate) enum Entry {
    /// A class declaration.
    Class(Rc<str>),
    /// An object of a class; `None` until its reader has finished.
    Obj(Rc<str>, Option<Rc<Val>>),
}

/// The draw block most entities carry: front material, flags and layer.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Db {
    pub(crate) mat: u16,
    pub(crate) hidden: bool,
    pub(crate) soft: bool,
    pub(crate) smooth: bool,
    pub(crate) layer: u16,
}

/// An arc's frame: `P(t) = center + cos t · x_axis + sin t · y_axis` for `t`
/// from `start` to `end` (a full circle stores `start == end`). Inches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ArcFrame {
    pub(crate) center: [f64; 3],
    pub(crate) x_axis: [f64; 3],
    pub(crate) y_axis: [f64; 3],
    pub(crate) start: f64,
    pub(crate) end: f64,
}

/// A texture as a material or layer carries it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextureRec {
    pub(crate) rgba: [u8; 4],
    pub(crate) opacity: f64,
    pub(crate) use_opacity: bool,
    /// The image object's slot, when it is one.
    pub(crate) dib: Option<usize>,
    /// The image's bytes, when stored inline (very old files).
    pub(crate) data: Option<Rc<[u8]>>,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) file: String,
}

/// One object read.
#[derive(Debug, Clone)]
pub(crate) enum Val {
    Vertex([f64; 3]),
    Edge {
        db: Db,
        v1: Option<usize>,
        v2: Option<usize>,
        curve: Option<usize>,
        curve_value: Option<Rc<Val>>,
    },
    Curve,
    ArcCurve(ArcFrame),
    EdgeUse {
        edge: Option<usize>,
        reversed: bool,
    },
    Loop(Vec<Option<Rc<Val>>>),
    Face {
        db: Db,
        normal: [f64; 3],
        loops: Vec<Option<Rc<Val>>>,
        back_mat: u16,
        attrs: Option<Rc<Val>>,
    },
    Attrs(Vec<Option<Rc<Val>>>),
    Dict,
    Layer {
        name: String,
        hidden: bool,
        rgba: [u8; 4],
    },
    Material {
        name: String,
        rgba: [u8; 4],
        opacity: f64,
        use_opacity: bool,
        texture: Option<TextureRec>,
    },
    Dib(Rc<[u8]>),
    Ftc {
        front: [f64; 9],
        back: [f64; 9],
    },
    Camera,
    Thumbnail,
    Image {
        def: Option<usize>,
        xf: [f64; 12],
    },
    Relationship,
    CLine {
        db: Db,
        point: [f64; 3],
        direction: [f64; 3],
        start: Option<[f64; 3]>,
        end: Option<[f64; 3]>,
    },
    CPoint {
        db: Db,
        pos: [f64; 3],
    },
    SectionPlane {
        db: Db,
        plane: [f64; 4],
        name: String,
    },
    Font,
    Dimension {
        db: Db,
        text: String,
        connect: [Option<usize>; 2],
        offset: f64,
        free: [Option<[f64; 3]>; 2],
    },
    Text {
        db: Db,
        text: String,
        /// The connection type: 0 screen text, 1 a point in space, 2 a
        /// vertex, 4 an edge's middle, 5 a point along an edge.
        kind: u32,
        /// A point in space (type 1), or the place along an edge (type 5,
        /// its first value).
        first: [f64; 3],
        /// The entity attached to, in the walk's numbering.
        target: Option<usize>,
        /// The groups and copies from the text's context down to it.
        path: Vec<usize>,
        /// The label's place from the anchor.
        offset: Option<[f64; 3]>,
        screen: Option<[f64; 2]>,
    },
    Definition {
        name: String,
        guid: String,
        ents: Vec<Item>,
    },
    Instance {
        db: Db,
        def: Option<usize>,
        xf: [f64; 13],
        name: String,
    },
}

/// One object read, as the walk hands it back: its slot, class name and
/// value (each `None` for a null; the value `None` for an opaque reference
/// or an object still being read).
#[derive(Debug, Clone, Default)]
pub(crate) struct Item {
    pub(crate) slot: Option<usize>,
    pub(crate) name: Option<Rc<str>>,
    pub(crate) value: Option<Rc<Val>>,
}

impl Item {
    /// Whether its class is `name`.
    pub(crate) fn is(&self, name: &str) -> bool {
        self.name.as_deref() == Some(name)
    }
}

/// The class name of an opaque reference into the part of the file the walk
/// never read.
pub(crate) const PREMODEL: &str = "premodel";
/// The class name of a reference into a burned band.
pub(crate) const RESERVED: &str = "reserved";

/// Store-map bookkeeping and the object-graph walk.
pub(crate) struct Archive<'a> {
    pub(crate) data: &'a [u8],
    /// SketchUp's major version (16, 17, 18, …; 3–8 for the oldest).
    pub(crate) ver: u32,
    /// Entity preambles carry a persistent-id mask (2017 and later).
    pub(crate) has_pid: bool,
    pub(crate) slots: HashMap<usize, Entry>,
    pub(crate) class_slot: HashMap<Rc<str>, usize>,
    /// The slot of the object being read.
    pub(crate) current_obj_slot: Option<usize>,
    pub(crate) next_slot: usize,
    /// Slots below this belong to the part of the file never walked.
    pub(crate) walk_base: usize,
    /// The loop whose edge uses are being read.
    pub(crate) current_loop: Option<usize>,
    /// Burned bands: (file band start, width).
    pub(crate) burns: Vec<(usize, usize)>,
    pub(crate) cum_delta: usize,
    /// The slot right after the last annotation record.
    pub(crate) annot_watermark: Option<usize>,
    /// Burned items credited to each entity list being read.
    pub(crate) burn_stack: Vec<usize>,
    /// The trailing byte count of guide lines, found on the first one.
    pub(crate) cline_tail: Option<usize>,
    depth: usize,
}

impl<'a> Archive<'a> {
    pub(crate) fn new(data: &'a [u8], ver: u32) -> Self {
        Self {
            data,
            ver,
            has_pid: ver >= 17,
            slots: HashMap::new(),
            class_slot: HashMap::new(),
            current_obj_slot: None,
            next_slot: 0,
            walk_base: 0,
            current_loop: None,
            burns: Vec::new(),
            cum_delta: 0,
            annot_watermark: None,
            burn_stack: Vec::new(),
            cline_tail: None,
            depth: 0,
        }
    }

    fn alloc(&mut self, entry: Entry) -> usize {
        let s = self.next_slot;
        self.slots.insert(s, entry);
        self.next_slot += 1;
        s
    }

    /// The class name of slot `s`, when it is a class.
    pub(crate) fn class_at(&self, s: usize) -> Option<&Rc<str>> {
        match self.slots.get(&s) {
            Some(Entry::Class(n)) => Some(n),
            _ => None,
        }
    }

    /// The object at slot `s`: its class and value.
    pub(crate) fn obj_at(&self, s: usize) -> Option<(&Rc<str>, Option<&Rc<Val>>)> {
        match self.slots.get(&s) {
            Some(Entry::Obj(n, v)) => Some((n, v.as_ref())),
            _ => None,
        }
    }

    /// Reads one object (see the module documentation). `expect` names the
    /// class to learn when the file refers to one declared before the walk.
    pub(crate) fn read_object(&mut self, r: &mut R<'_>, expect: Option<&str>) -> Res<Item> {
        let tag = r.u16()?;
        if tag == 0 {
            return Ok(Item::default());
        }
        if tag == 0x7FFF {
            let big = r.u32()? as usize;
            if big & 0x8000_0000 != 0 {
                return self.new_of_class(r, big & 0x7FFF_FFFF, expect);
            }
            return self.backref(big, r);
        }
        if tag == 0xFFFF {
            let _schema = r.u16()?;
            let namelen = usize::from(r.u16()?);
            if namelen > 40 {
                return Err(Fault::at(r, "implausible class name length"));
            }
            let raw = r.raw(namelen);
            if raw.len() < namelen || !raw.is_ascii() {
                return Err(Fault::Text);
            }
            let name: Rc<str> = Rc::from(std::str::from_utf8(raw).map_err(|_| Fault::Text)?);
            let s = self.alloc(Entry::Class(name.clone()));
            self.class_slot.insert(name.clone(), s);
            return self.new_obj(r, name);
        }
        if tag & 0x8000 != 0 {
            return self.new_of_class(r, usize::from(tag & 0x7FFF), expect);
        }
        self.backref(usize::from(tag), r)
    }

    fn new_of_class(&mut self, r: &mut R<'_>, cslot: usize, expect: Option<&str>) -> Res<Item> {
        // A class declared after burned bands is referred to by its file
        // index, as objects are.
        let cslot = match self.burns.first() {
            Some(&(first, _)) if cslot >= first => self.translate_ref(cslot).unwrap_or(cslot),
            _ => cslot,
        };
        let name = match self.slots.get(&cslot) {
            None => {
                let Some(expect) = expect else {
                    return Err(Fault::at(r, format!("class-ref to unknown slot {cslot}")));
                };
                // A class declared in the part of the file never walked:
                // learn it from context.
                let name: Rc<str> = Rc::from(expect);
                self.slots.insert(cslot, Entry::Class(name.clone()));
                self.class_slot.insert(name.clone(), cslot);
                name
            }
            Some(Entry::Class(n)) => n.clone(),
            Some(Entry::Obj(n, _)) => {
                return Err(Fault::at(
                    r,
                    format!("class-ref to non-class slot {cslot} ({n})"),
                ));
            }
        };
        self.new_obj(r, name)
    }

    fn new_obj(&mut self, r: &mut R<'_>, name: Rc<str>) -> Res<Item> {
        let slot = self.alloc(Entry::Obj(name.clone(), None));
        if self.depth >= MAX_DEPTH {
            return Err(Fault::at(r, "objects nested too deep"));
        }
        let prev_slot = self.current_obj_slot;
        self.current_obj_slot = Some(slot);
        self.depth += 1;
        let value = records::read(self, r, &name);
        self.depth -= 1;
        self.current_obj_slot = prev_slot;
        let value = Rc::new(value?);
        self.slots
            .insert(slot, Entry::Obj(name.clone(), Some(value.clone())));
        if &*name == "CDimensionLinear" || &*name == "CText" {
            self.annot_watermark = Some(self.next_slot);
        }
        Ok(Item {
            slot: Some(slot),
            name: Some(name),
            value: Some(value),
        })
    }

    /// A file store-map index mapped to the walk's numbering through the
    /// burned bands; `None` when it points into a band (a connection point
    /// never written).
    pub(crate) fn translate_ref(&self, slot: usize) -> Option<usize> {
        let mut offset = 0;
        for &(start, width) in &self.burns {
            if slot < start {
                break;
            }
            if slot < start + width {
                return None;
            }
            offset += width;
        }
        Some(slot - offset)
    }

    fn backref(&mut self, mut slot: usize, r: &R<'_>) -> Res<Item> {
        if let Some(&(first, _)) = self.burns.first()
            && slot >= first
        {
            match self.translate_ref(slot) {
                None => {
                    return Ok(Item {
                        slot: Some(slot),
                        name: Some(Rc::from(RESERVED)),
                        value: None,
                    });
                }
                Some(w) => slot = w,
            }
        }
        match self.slots.get(&slot) {
            None => {
                if slot < self.walk_base {
                    return Ok(Item {
                        slot: Some(slot),
                        name: Some(Rc::from(PREMODEL)),
                        value: None,
                    });
                }
                Err(Fault::at(r, format!("back-ref to unwalked slot {slot}")))
            }
            Some(Entry::Class(_)) => Err(Fault::at(r, format!("back-ref to class slot {slot}"))),
            Some(Entry::Obj(n, v)) => Ok(Item {
                slot: Some(slot),
                name: Some(n.clone()),
                value: v.clone(),
            }),
        }
    }

    /// Whether `at` starts a `CLayer` record: its class declaration, a
    /// class-ref to the known `CLayer`, or (with `unmatched_class_ref`, for
    /// the throwaway slot-base probe only) any short class-ref.
    pub(crate) fn clayer_record_at(&self, at: usize, unmatched_class_ref: bool) -> bool {
        let data = self.data;
        let Ok(tag) = u16_at(data, at) else {
            return false;
        };
        if tag == 0xFFFF
            && at + 12 <= data.len()
            && u16_at(data, at + 4) == Ok(6)
            && &data[at + 6..at + 12] == b"CLayer"
        {
            return true;
        }
        if let Some(&cs) = self.class_slot.get("CLayer")
            && is_class_ref(data, at, cs)
        {
            return true;
        }
        unmatched_class_ref && tag & 0x8000 != 0 && tag != 0xFFFF
    }

    /// A colour layer's 21-byte tail may be followed (some 2018 files with
    /// no materials) by 12 zero bytes and a u32, then another layer: skip
    /// those 16 bytes only then.
    pub(crate) fn skip_clayer_colour_ext(&self, r: &mut R<'_>) {
        let (pad, ext) = (12, 16);
        if r.pos + ext + 2 > self.data.len() {
            return;
        }
        if self.data[r.pos..r.pos + pad].iter().any(|b| *b != 0) {
            return;
        }
        if self.clayer_record_at(r.pos + ext, true) {
            r.pos += ext;
        }
    }

    /// A 2018 template `Layer0` after a custom layer writes a parent
    /// reference first: skip it only when both words ahead are layers.
    pub(crate) fn skip_clayer_parent_ref(&self, r: &mut R<'_>, self_slot: Option<usize>) {
        let Some(self_slot) = self_slot else {
            return;
        };
        if r.pos + 4 > self.data.len() {
            return;
        }
        let (Ok(a), Ok(b)) = (u16_at(self.data, r.pos), u16_at(self.data, r.pos + 2)) else {
            return;
        };
        if a == 0 || a & 0x8000 != 0 || a == 0x7FFF || usize::from(a) == self_slot {
            return;
        }
        if b == 0 || b & 0x8000 != 0 || b == 0x7FFF {
            return;
        }
        let layer = |s: u16| {
            self.obj_at(usize::from(s))
                .is_some_and(|(n, _)| &**n == "CLayer")
        };
        if layer(a) && layer(b) {
            r.pos += 2;
        }
    }

    /// Layer records past the declared count (and null separators), up to
    /// the definition-list anchor.
    pub(crate) fn collect_trailing_layers(
        &mut self,
        r: &mut R<'_>,
        mut slots_out: Option<&mut Vec<usize>>,
        mut layers_out: Option<&mut Vec<(usize, Rc<Val>)>>,
        unmatched_class_ref: bool,
    ) -> Res<()> {
        while r.pos + 2 <= self.data.len() {
            let tag = r.peek_u16()?;
            if tag == 0 {
                r.pos += 2;
                continue;
            }
            if !self.clayer_record_at(r.pos, unmatched_class_ref) {
                break;
            }
            let it = self.read_object(r, Some("CLayer"))?;
            if let (Some(s), Some(v)) = (it.slot, it.value) {
                if let Some(out) = slots_out.as_deref_mut() {
                    out.push(s);
                }
                if let Some(out) = layers_out.as_deref_mut() {
                    out.push((s, v));
                }
            }
        }
        Ok(())
    }

    /// Records that the writer burned `delta` store-map indices.
    pub(crate) fn register_burn(&mut self, delta: usize) {
        let start = self.annot_watermark.unwrap_or(0) + self.cum_delta;
        self.burns.push((start, delta));
        self.cum_delta += delta;
        self.annot_watermark = None;
        // One phantom top-level entity the list's count includes.
        if let Some(last) = self.burn_stack.last_mut() {
            *last += 1;
        }
    }
}

/// Whether the bytes at `p` are a class-ref to class `slot` (the short form,
/// or the escaped big form for slots at or past 0x7FFF).
pub(crate) fn is_class_ref(data: &[u8], p: usize, slot: usize) -> bool {
    if slot < 0x7FFF {
        return u16_at(data, p).is_ok_and(|t| usize::from(t) == 0x8000 | slot);
    }
    u16_at(data, p) == Ok(0x7FFF)
        && u32_at(data, p + 2).is_ok_and(|v| v as usize == 0x8000_0000 | slot)
}

/// Whether the u16 at `at` can start an object read: a null, an escape, a
/// class declaration, a class-ref to a known class, or a back-ref within
/// the slots handed out.
pub(crate) fn plausible_list_tag(ar: &Archive<'_>, at: usize) -> bool {
    let Ok(t) = u16_at(ar.data, at) else {
        return false;
    };
    if matches!(t, 0x0000 | 0x7FFF | 0xFFFF) {
        return true;
    }
    if t & 0x8000 != 0 {
        return ar.class_at(usize::from(t & 0x7FFF)).is_some();
    }
    usize::from(t) < ar.next_slot
}

/// Whether the u16 at `at` starts an object read in an unambiguous form:
/// null (when allowed), escape, class declaration or class-ref to a known
/// class. Plain back-refs are left out on purpose.
pub(crate) fn strict_next_tag(ar: &Archive<'_>, at: usize, allow_null: bool) -> bool {
    let Ok(t) = u16_at(ar.data, at) else {
        return false;
    };
    if t == 0 {
        return allow_null;
    }
    if t == 0x7FFF || t == 0xFFFF {
        return true;
    }
    if t & 0x8000 != 0 {
        return ar.class_at(usize::from(t & 0x7FFF)).is_some();
    }
    false
}
