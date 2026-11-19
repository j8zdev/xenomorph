//! Reduces an XML document to its "shape": one node per distinct element
//! path, with all occurrences of that path merged. Values are kept only as
//! format statistics.

use crate::types::ValueProfile;
use std::collections::{BTreeMap, HashMap};

/// How many occurrence positions are remembered per element.
const MAX_LINES: usize = 2000;

#[derive(Debug, Default)]
pub struct ShapeNode {
    pub ns: String,
    pub local: String,
    /// Prefix as written in the file (display only).
    pub prefix: String,
    /// Number of occurrences in the document.
    pub count: usize,
    /// Line (1-based) of each occurrence's start tag.
    pub lines: Vec<u32>,
    pub children: Vec<ShapeNode>,
    index: HashMap<(String, String), usize>,
    /// In how many occurrences of the parent this element appears at least once.
    pub present_in: usize,
    /// Highest number of occurrences inside a single parent.
    pub max_per_parent: usize,
    /// Occurrences that have child elements.
    pub with_children: usize,
    /// Occurrences that have text and no child elements.
    pub with_text: usize,
    /// How often child `i` came before child `j` within one occurrence.
    pub before: HashMap<(usize, usize), u32>,
    pub attrs: BTreeMap<String, usize>,
    pub values: ValueProfile,
}

impl ShapeNode {
    pub fn child(&self, ns: &str, local: &str) -> Option<usize> {
        self.index.get(&(ns.to_string(), local.to_string())).copied()
    }

    pub fn is_container(&self) -> bool {
        !self.children.is_empty()
    }

    /// Name as written in the file, e.g. `ns2:partner`.
    pub fn qname(&self) -> String {
        if self.prefix.is_empty() {
            self.local.clone()
        } else {
            format!("{}:{}", self.prefix, self.local)
        }
    }

    pub fn ns_label(&self) -> String {
        if self.ns.is_empty() { "no namespace".into() } else { self.ns.clone() }
    }

    pub fn first_line(&self) -> Option<u32> {
        self.lines.first().copied()
    }

    /// Total number of distinct element paths below this node.
    pub fn descendant_count(&self) -> usize {
        self.children.iter().map(|c| 1 + c.descendant_count()).sum()
    }
}

pub struct Document {
    pub root: ShapeNode,
    pub lines: Vec<String>,
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    fn line(&self, offset: usize) -> u32 {
        self.starts.partition_point(|s| *s <= offset) as u32
    }
}

/// Decodes file bytes: UTF-8 (with or without BOM), UTF-16 with BOM, and
/// Latin-1 as a fallback for files that are not valid UTF-8.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|b| *b as char).collect(),
    }
}

pub fn parse(text: &str) -> Result<Document, String> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, opts).map_err(|e| e.to_string())?;
    let index = LineIndex::new(text);
    let root_el = doc.root_element();
    let mut root = new_node(root_el);
    root.present_in = 1;
    root.max_per_parent = 1;
    add_occurrence(&mut root, root_el, &index);
    let lines = text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
    Ok(Document { root, lines })
}

fn new_node(el: roxmltree::Node) -> ShapeNode {
    let tag = el.tag_name();
    let ns = tag.namespace().unwrap_or("").to_string();
    let prefix = if ns.is_empty() { None } else { el.lookup_prefix(&ns) };
    ShapeNode {
        ns,
        local: tag.name().to_string(),
        prefix: prefix.unwrap_or("").to_string(),
        ..Default::default()
    }
}

fn add_occurrence(shape: &mut ShapeNode, el: roxmltree::Node, index: &LineIndex) {
    let line = index.line(el.range().start);
    shape.count += 1;
    if shape.lines.len() < MAX_LINES {
        shape.lines.push(line);
    }
    for a in el.attributes() {
        let name = match a.namespace() {
            Some(ns) => format!("{{{}}}{}", ns, a.name()),
            None => a.name().to_string(),
        };
        *shape.attrs.entry(name).or_default() += 1;
    }

    let mut seen: Vec<(usize, usize)> = Vec::new(); // (child index, occurrences here)
    let mut has_child = false;
    for child in el.children().filter(|c| c.is_element()) {
        has_child = true;
        let tag = child.tag_name();
        let key = (tag.namespace().unwrap_or("").to_string(), tag.name().to_string());
        let i = match shape.index.get(&key) {
            Some(i) => *i,
            None => {
                shape.children.push(new_node(child));
                shape.index.insert(key, shape.children.len() - 1);
                shape.children.len() - 1
            }
        };
        match seen.iter_mut().find(|(c, _)| *c == i) {
            Some((_, n)) => *n += 1,
            None => seen.push((i, 1)),
        }
        add_occurrence(&mut shape.children[i], child, index);
    }
    for (pos, (i, n)) in seen.iter().enumerate() {
        let c = &mut shape.children[*i];
        c.present_in += 1;
        c.max_per_parent = c.max_per_parent.max(*n);
        for (j, _) in &seen[pos + 1..] {
            *shape.before.entry((*i, *j)).or_default() += 1;
        }
    }

    if has_child {
        shape.with_children += 1;
    } else {
        let text: String = el.children().filter(|c| c.is_text()).filter_map(|c| c.text()).collect();
        if !text.trim().is_empty() {
            shape.with_text += 1;
        }
        shape.values.add(&text, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_occurrences() {
        let d = parse(
            "<r xmlns:a='urn:a'>\n<a:x><y>1</y><z/></a:x>\n<a:x><z/><y>2</y><y>3</y></a:x>\n</r>",
        )
        .unwrap();
        let x = &d.root.children[0];
        assert_eq!((x.ns.as_str(), x.local.as_str(), x.prefix.as_str()), ("urn:a", "x", "a"));
        assert_eq!(x.count, 2);
        assert_eq!(x.lines, vec![2, 3]);
        let y = &x.children[x.child("", "y").unwrap()];
        assert_eq!((y.count, y.present_in, y.max_per_parent), (3, 2, 2));
    }
}
