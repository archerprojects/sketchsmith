//! SketchSmith — reading XML | v1.0.0 | Thursday 12 November 2026
//!
//! A small element tree for the XML formats imports read (DAE, and later
//! SVG), built from `quick-xml`'s events: each element's local name
//! (namespace prefix dropped), its attributes, its children and its text
//! (character data and references, joined). Comments, declarations and
//! processing instructions are skipped. The tree is built without
//! recursion, so deep files cannot exhaust the stack.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::IoError;

/// An XML element.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Element {
    /// Its local name.
    pub name: String,
    /// Its attributes, by local name, in order.
    pub attrs: Vec<(String, String)>,
    /// Its child elements, in order.
    pub children: Vec<Element>,
    /// Its text, joined.
    pub text: String,
}

impl Element {
    /// Attribute `name`'s value.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The first child named `name`.
    pub(crate) fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Every child named `name`.
    pub(crate) fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// This element and every element inside it, depth first.
    pub(crate) fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        let mut todo = vec![self];
        while let Some(e) = todo.pop() {
            out.push(e);
            todo.extend(e.children.iter().rev());
        }
        out
    }

    /// The numbers in its text (a decimal comma, as some exporters write
    /// by mistake, is read as a point).
    pub(crate) fn numbers(&self) -> Vec<f64> {
        self.text
            .split_ascii_whitespace()
            .filter_map(|t| t.parse().ok().or_else(|| t.replace(',', ".").parse().ok()))
            .collect()
    }
}

/// `bytes` as text: UTF-16 when it begins with a UTF-16 byte-order mark,
/// else UTF-8 (a UTF-8 mark dropped).
pub(crate) fn text_of(bytes: &[u8]) -> String {
    let utf16 = |le: bool| {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(true),
        [0xFE, 0xFF, ..] => utf16(false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Reads `text` as XML; returns its root element.
///
/// # Errors
/// [`IoError::Damaged`] for malformed XML, saying where.
pub(crate) fn parse(text: &str) -> Result<Element, IoError> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Element> = vec![Element::default()];
    let damaged = |reader: &Reader<&[u8]>, what: String| {
        IoError::Damaged(format!("XML at byte {}: {what}", reader.error_position()))
    };
    let open = |e: &quick_xml::events::BytesStart<'_>| -> Element {
        let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
        let attrs = e
            .attributes()
            .flatten()
            .map(|a| {
                (
                    String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned(),
                    a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map(|v| v.into_owned())
                        .unwrap_or_default(),
                )
            })
            .collect();
        Element {
            name,
            attrs,
            ..Element::default()
        }
    };
    loop {
        let event = reader
            .read_event()
            .map_err(|e| damaged(&reader, e.to_string()))?;
        match event {
            Event::Start(e) => stack.push(open(&e)),
            Event::Empty(e) => {
                let el = open(&e);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(el);
                }
            }
            Event::End(_) => {
                let done = stack
                    .pop()
                    .ok_or_else(|| damaged(&reader, "an element closes twice".into()))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => return Err(damaged(&reader, "an element closes twice".into())),
                }
            }
            Event::Text(t) => {
                if let (Some(top), Ok(s)) = (stack.last_mut(), t.decode()) {
                    top.text.push_str(&s);
                }
            }
            Event::CData(c) => {
                if let (Some(top), Ok(s)) = (stack.last_mut(), c.decode()) {
                    top.text.push_str(&s);
                }
            }
            Event::GeneralRef(r) => {
                let c = match r.resolve_char_ref() {
                    Ok(Some(c)) => Some(c.to_string()),
                    _ => r.decode().ok().and_then(|name| {
                        quick_xml::escape::resolve_predefined_entity(&name).map(str::to_owned)
                    }),
                };
                if let (Some(top), Some(c)) = (stack.last_mut(), c) {
                    top.text.push_str(&c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err(damaged(&reader, "the file ends inside an element".into()));
    }
    stack
        .pop()
        .and_then(|doc| doc.children.into_iter().next())
        .ok_or_else(|| IoError::Damaged("XML: no root element".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_with_attributes_text_and_references() {
        let doc = parse(
            r#"<?xml version="1.0"?>
<!-- a comment -->
<c:COLLADA xmlns:c="x" version="1.4.1">
  <asset><unit meter="0.0254" name="inch"/><up_axis>Z_UP</up_axis></asset>
  <float_array id="p" count="3">1 2.5 -3</float_array>
  <name>Joe &amp; Bob&#x21;<![CDATA[ <ok> ]]></name>
</c:COLLADA>"#,
        )
        .unwrap();
        assert_eq!(doc.name, "COLLADA");
        assert_eq!(doc.attr("version"), Some("1.4.1"));
        let asset = doc.child("asset").unwrap();
        assert_eq!(asset.child("unit").unwrap().attr("meter"), Some("0.0254"));
        assert_eq!(asset.child("up_axis").unwrap().text, "Z_UP");
        assert_eq!(
            doc.child("float_array").unwrap().numbers(),
            [1.0, 2.5, -3.0]
        );
        assert_eq!(doc.child("name").unwrap().text, "Joe & Bob! <ok> ");
        assert_eq!(doc.descendants().len(), 6);
    }

    #[test]
    fn utf16_and_decimal_commas() {
        let mut le = vec![0xFF, 0xFE];
        for u in "<a>1,5 2</a>".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        let doc = parse(&text_of(&le)).unwrap();
        assert_eq!(doc.numbers(), [1.5, 2.0]);
        assert_eq!(text_of(b"\xEF\xBB\xBF<b/>"), "<b/>");
    }

    #[test]
    fn broken_xml_says_where() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a>").is_err());
        assert!(parse("").is_err());
    }
}
