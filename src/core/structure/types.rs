#![allow(dead_code)]

use crate::core::layout::{LineMap, build_line_map_from_sorted_events};
use crate::core::radix::DisplayRadix;
use gpui::Hsla;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Debug, Clone)]
pub enum FieldValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Struct,
}

impl FieldValue {
    pub fn to_i64(&self) -> i64 {
        match self {
            FieldValue::U8(v) => *v as i64,
            FieldValue::U16(v) => *v as i64,
            FieldValue::U32(v) => *v as i64,
            FieldValue::U64(v) => *v as i64,
            FieldValue::I8(v) => *v as i64,
            FieldValue::I16(v) => *v as i64,
            FieldValue::I32(v) => *v as i64,
            FieldValue::I64(v) => *v,
            FieldValue::F32(v) => *v as i64,
            FieldValue::F64(v) => *v as i64,
            FieldValue::Bool(v) => {
                if *v {
                    1
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub fn to_f64(&self) -> f64 {
        match self {
            FieldValue::U8(v) => *v as f64,
            FieldValue::U16(v) => *v as f64,
            FieldValue::U32(v) => *v as f64,
            FieldValue::U64(v) => *v as f64,
            FieldValue::I8(v) => *v as f64,
            FieldValue::I16(v) => *v as f64,
            FieldValue::I32(v) => *v as f64,
            FieldValue::I64(v) => *v as f64,
            FieldValue::F32(v) => *v as f64,
            FieldValue::F64(v) => *v,
            FieldValue::Bool(v) => {
                if *v {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    pub fn to_string_value(&self) -> String {
        match self {
            FieldValue::String(s) => s.clone(),
            FieldValue::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            other => format!("{}", other),
        }
    }

    /// Formats numeric values in the requested radix while preserving the
    /// regular display for strings, byte arrays, structures, and non-integer
    /// values.
    pub fn format_with_radix(&self, radix: DisplayRadix) -> String {
        match self {
            FieldValue::U8(value) => Self::format_unsigned(*value as u64, radix),
            FieldValue::U16(value) => Self::format_unsigned(*value as u64, radix),
            FieldValue::U32(value) => Self::format_unsigned(*value as u64, radix),
            FieldValue::U64(value) => Self::format_unsigned(*value, radix),
            FieldValue::I8(value) => Self::format_signed(*value as i64, radix),
            FieldValue::I16(value) => Self::format_signed(*value as i64, radix),
            FieldValue::I32(value) => Self::format_signed(*value as i64, radix),
            FieldValue::I64(value) => Self::format_signed(*value, radix),
            other => other.to_string(),
        }
    }

    fn format_unsigned(value: u64, radix: DisplayRadix) -> String {
        match radix {
            DisplayRadix::Hexadecimal => format!("0x{value:X}"),
            DisplayRadix::Decimal => value.to_string(),
            DisplayRadix::Octal => format!("0o{value:o}"),
            DisplayRadix::Binary => format!("0b{value:b}"),
        }
    }

    fn format_signed(value: i64, radix: DisplayRadix) -> String {
        if radix == DisplayRadix::Decimal {
            return value.to_string();
        }

        let sign = if value.is_negative() { "-" } else { "" };
        let magnitude = value.unsigned_abs();
        match radix {
            DisplayRadix::Hexadecimal => format!("{sign}0x{magnitude:X}"),
            DisplayRadix::Octal => format!("{sign}0o{magnitude:o}"),
            DisplayRadix::Binary => format!("{sign}0b{magnitude:b}"),
            DisplayRadix::Decimal => unreachable!("decimal values return before radix formatting"),
        }
    }
}

impl std::fmt::Display for FieldValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldValue::U8(v) => write!(f, "{}", v),
            FieldValue::U16(v) => write!(f, "{}", v),
            FieldValue::U32(v) => write!(f, "{}", v),
            FieldValue::U64(v) => write!(f, "{}", v),
            FieldValue::I8(v) => write!(f, "{}", v),
            FieldValue::I16(v) => write!(f, "{}", v),
            FieldValue::I32(v) => write!(f, "{}", v),
            FieldValue::I64(v) => write!(f, "{}", v),
            FieldValue::F32(v) => write!(f, "{}", v),
            FieldValue::F64(v) => write!(f, "{}", v),
            FieldValue::Bool(v) => write!(f, "{}", v),
            FieldValue::String(v) => write!(f, "\"{}\"", v),
            FieldValue::Bytes(v) => write!(f, "[{} bytes]", v.len()),
            FieldValue::Struct => write!(f, "{{...}}"),
        }
    }
}

#[derive(Debug)]
pub struct ParsedField {
    pub id: String,
    pub field_type: String,
    pub offset: usize,
    pub size: usize,
    pub value: FieldValue,
    pub color: Hsla,
    pub description: Option<String>,
    pub children: Vec<ParsedField>,
    pub enum_label: Option<String>,
    pub is_instance: bool,
}

impl Clone for ParsedField {
    fn clone(&self) -> Self {
        struct Frame<'a> {
            source: &'a ParsedField,
            next_child: usize,
            cloned: ParsedField,
        }

        let mut frames = vec![Frame {
            source: self,
            next_child: 0,
            cloned: Self::clone_shallow(self),
        }];

        loop {
            let next_child = {
                let frame = frames.last_mut().expect("parsed-field clone stack must not be empty");
                if frame.next_child < frame.source.children.len() {
                    let index = frame.next_child;
                    frame.next_child += 1;
                    Some(index)
                } else {
                    None
                }
            };

            if let Some(index) = next_child {
                let child = {
                    let frame = frames.last().expect("parsed-field clone parent must exist");
                    &frame.source.children[index]
                };
                frames.push(Frame {
                    source: child,
                    next_child: 0,
                    cloned: Self::clone_shallow(child),
                });
                continue;
            }

            let completed = frames.pop().expect("parsed-field clone frame must exist").cloned;
            if let Some(parent) = frames.last_mut() {
                parent.cloned.children.push(completed);
            } else {
                return completed;
            }
        }
    }
}

impl ParsedField {
    fn clone_shallow(field: &Self) -> Self {
        Self {
            id: field.id.clone(),
            field_type: field.field_type.clone(),
            offset: field.offset,
            size: field.size,
            value: field.value.clone(),
            color: field.color,
            description: field.description.clone(),
            children: Vec::with_capacity(field.children.len()),
            enum_label: field.enum_label.clone(),
            is_instance: field.is_instance,
        }
    }
}

impl Drop for ParsedField {
    fn drop(&mut self) {
        // ParsedField is a recursive logical tree. Drain descendants on an
        // explicit heap-backed worklist so dropping a deeply nested Kaitai
        // result never consumes one stack frame per structure level.
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut field) = pending.pop() {
            pending.append(&mut field.children);
        }
    }
}

impl ParsedField {
    pub fn is_struct(&self) -> bool {
        !self.children.is_empty() || matches!(self.value, FieldValue::Struct)
    }

    pub fn format_expression(&self) -> String {
        if self.is_struct() {
            return self.id.clone();
        }
        if let Some(label) = &self.enum_label {
            return format!("{} = {} ({})", self.id, self.value, label);
        }
        match &self.value {
            FieldValue::String(s) => format!("{} = \"{}\"", self.id, s),
            FieldValue::Bytes(b) => format!("{} = [{} bytes]", self.id, b.len()),
            FieldValue::U8(v) => format!("{} = {:X}h ({})", self.id, v, v),
            FieldValue::U16(v) => format!("{} = {:X}h ({})", self.id, v, v),
            FieldValue::U32(v) => format!("{} = {:X}h ({})", self.id, v, v),
            FieldValue::U64(v) => format!("{} = {:X}h ({})", self.id, v, v),
            other => format!("{} = {}", self.id, other),
        }
    }

    pub fn format_comment(&self) -> Option<String> {
        if let Some(desc) = &self.description
            && !desc.is_empty()
        {
            return Some(desc.clone());
        }
        if let Some(label) = &self.enum_label {
            return Some(label.clone());
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct ActiveStructRange {
    pub start: usize,
    pub end: usize,
    pub depth: usize,
    pub id: String,
}

/// A node in the persistent, append-only root field collection.
///
/// The collection uses a binary-counter forest. Appending a chunk copies only
/// the small forest of roots (`O(log n)`) and shares all existing field data.
/// This avoids copying an ever-growing `Vec<Arc<[ParsedField]>>` on every
/// progress update.
#[derive(Debug)]
enum FieldCollectionNode {
    Chunk(Arc<[ParsedField]>),
    Concat {
        left: Arc<FieldCollectionNode>,
        right: Arc<FieldCollectionNode>,
        len: usize,
    },
}

impl FieldCollectionNode {
    fn len(&self) -> usize {
        match self {
            Self::Chunk(fields) => fields.len(),
            Self::Concat { len, .. } => *len,
        }
    }
}

/// Append-only collection used for parse snapshots.
///
/// Each snapshot shares the already parsed chunks instead of cloning the
/// complete root field vector. Snapshots are persistent, so append and drop
/// remain proportional to the number of chunk roots rather than the number of
/// chunks already parsed.
#[derive(Debug, Clone, Default)]
pub struct FieldCollection {
    roots: Arc<Vec<Option<Arc<FieldCollectionNode>>>>,
    len: usize,
}

impl FieldCollection {
    /// Creates a collection containing one owned chunk.
    pub fn from_vec(fields: Vec<ParsedField>) -> Self {
        if fields.is_empty() {
            return Self::default();
        }

        let chunk: Arc<[ParsedField]> = Arc::from(fields.into_boxed_slice());
        Self::from_shared_chunks(std::slice::from_ref(&chunk))
    }

    /// Returns a new collection with an additional shared chunk.
    pub fn append_chunk(&self, fields: Vec<ParsedField>) -> Self {
        if fields.is_empty() {
            return self.clone();
        }

        let chunk: Arc<[ParsedField]> = Arc::from(fields.into_boxed_slice());
        self.append_shared_chunks(std::slice::from_ref(&chunk))
    }

    /// Returns a new collection with additional shared chunks.
    pub fn append_shared_chunks(&self, chunks: &[Arc<[ParsedField]>]) -> Self {
        let mut result = self.clone();
        for chunk in chunks.iter().filter(|chunk| !chunk.is_empty()) {
            result = result.append_node(Arc::new(FieldCollectionNode::Chunk(chunk.clone())), chunk.len());
        }
        result
    }

    fn from_shared_chunks(chunks: &[Arc<[ParsedField]>]) -> Self {
        Self::default().append_shared_chunks(chunks)
    }

    fn append_node(&self, mut node: Arc<FieldCollectionNode>, node_len: usize) -> Self {
        let mut roots = (*self.roots).clone();
        let mut root_index = 0;

        loop {
            if root_index == roots.len() {
                roots.push(None);
            }

            if let Some(left) = roots[root_index].take() {
                let len = left.len() + node.len();
                node = Arc::new(FieldCollectionNode::Concat { left, right: node, len });
                root_index += 1;
            } else {
                roots[root_index] = Some(node);
                break;
            }
        }

        Self {
            roots: Arc::new(roots),
            len: self.len + node_len,
        }
    }

    /// Returns the number of root fields.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the collection has no root fields.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a root field by index.
    pub fn get(&self, index: usize) -> Option<&ParsedField> {
        if index >= self.len {
            return None;
        }

        let mut index = index;
        for root in self.roots.iter().rev().flatten() {
            if index < root.len() {
                return Self::get_from_node(root, index);
            }
            index -= root.len();
        }
        None
    }

    fn get_from_node(mut node: &FieldCollectionNode, mut index: usize) -> Option<&ParsedField> {
        loop {
            match node {
                FieldCollectionNode::Chunk(fields) => return fields.get(index),
                FieldCollectionNode::Concat { left, right, .. } => {
                    if index < left.len() {
                        node = left.as_ref();
                    } else {
                        index -= left.len();
                        node = right.as_ref();
                    }
                }
            }
        }
    }

    /// Iterates over root fields in parse order.
    pub fn iter(&self) -> FieldCollectionIter<'_> {
        self.iter_from(0)
    }

    /// Iterates from a root-field index without walking the skipped fields.
    pub fn iter_from(&self, index: usize) -> FieldCollectionIter<'_> {
        let mut stack = Vec::new();
        let mut current = None;
        if index < self.len {
            let mut remaining = index;
            let mut selected_root = None;
            for (root_index, root) in self.roots.iter().enumerate().rev() {
                let Some(root) = root else { continue };
                if remaining < root.len() {
                    selected_root = Some(root_index);
                    break;
                }
                remaining -= root.len();
            }

            if let Some(selected_root) = selected_root {
                // Lower roots contain later fields. Push them first so the
                // selected root remains on top of the iterator stack.
                for root in self.roots[..selected_root].iter().flatten() {
                    stack.push((root.as_ref(), 0));
                }
                if let Some(root) = &self.roots[selected_root] {
                    if let FieldCollectionNode::Chunk(fields) = root.as_ref() {
                        // The common final-result representation is one flat
                        // chunk. Keep it in the iterator's current slot so
                        // iterating it does not allocate a traversal stack.
                        current = Some((fields.as_ref(), remaining));
                    } else {
                        Self::push_from_node(root.as_ref(), remaining, &mut stack);
                    }
                }
            }
        }

        FieldCollectionIter { stack, current }
    }

    fn push_from_node<'a>(mut node: &'a FieldCollectionNode, mut index: usize, stack: &mut Vec<(&'a FieldCollectionNode, usize)>) {
        loop {
            match node {
                FieldCollectionNode::Chunk(_) => {
                    stack.push((node, index));
                    return;
                }
                FieldCollectionNode::Concat { left, right, .. } => {
                    if index < left.len() {
                        stack.push((right.as_ref(), 0));
                        node = left.as_ref();
                    } else {
                        index -= left.len();
                        node = right.as_ref();
                    }
                }
            }
        }
    }
}

/// Iterator over the shared chunks in a [`FieldCollection`].
pub struct FieldCollectionIter<'a> {
    stack: Vec<(&'a FieldCollectionNode, usize)>,
    current: Option<(&'a [ParsedField], usize)>,
}

impl<'a> Iterator for FieldCollectionIter<'a> {
    type Item = &'a ParsedField;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((fields, index)) = self.current.as_mut() {
                if let Some(field) = fields.get(*index) {
                    *index += 1;
                    return Some(field);
                }
                self.current = None;
            }

            let (node, index) = self.stack.pop()?;
            match node {
                FieldCollectionNode::Chunk(fields) => {
                    self.current = Some((fields.as_ref(), index));
                }
                FieldCollectionNode::Concat { left, right, .. } => {
                    self.stack.push((right.as_ref(), 0));
                    self.stack.push((left.as_ref(), index));
                }
            }
        }
    }
}

impl std::ops::Index<usize> for FieldCollection {
    type Output = ParsedField;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap_or_else(|| panic!("field index {index} out of bounds"))
    }
}

/// Lightweight field data used by byte-range queries.
///
/// The index intentionally does not retain `children` or raw byte buffers.
/// Those values remain in `ParseResult::fields` and are only cloned when a
/// snapshot itself is created.
#[derive(Debug, Clone)]
pub struct IndexedField {
    pub id: String,
    pub offset: usize,
    pub size: usize,
    /// Nesting depth in the parsed structure tree.
    pub depth: usize,
    /// Whether this field is a computed/positioned instance rather than a physical field.
    pub is_instance: bool,
    type_name: String,
    expression: String,
}

impl IndexedField {
    fn container(field: &ParsedField, depth: usize) -> Self {
        Self {
            id: field.id.clone(),
            offset: field.offset,
            size: field.size,
            depth,
            is_instance: field.is_instance,
            type_name: field.field_type.clone(),
            expression: String::new(),
        }
    }

    fn leaf(field: &ParsedField, depth: usize) -> Self {
        Self {
            id: field.id.clone(),
            offset: field.offset,
            size: field.size,
            depth,
            is_instance: field.is_instance,
            type_name: String::new(),
            expression: field.format_expression(),
        }
    }

    /// Returns the field label used for a structure description.
    ///
    /// A switched custom type is stored as the resolved type name in
    /// `ParsedField.field_type`, while `id` remains the field name (`body` in
    /// a ZIP section). Keeping both makes the selected case visible without
    /// changing IDs used by collapse and range lookup logic.
    pub fn format_container_label(&self) -> String {
        if self.type_name.is_empty() || self.type_name == self.id {
            self.id.clone()
        } else {
            format!("{}: {}", self.id, self.type_name)
        }
    }

    /// Returns the preformatted expression used by the description column.
    pub fn format_expression(&self) -> &str {
        &self.expression
    }
}

#[derive(Debug, Clone, Default)]
pub struct StructureIndex {
    pub container_structs: Vec<IndexedField>,
    pub leaf_fields: Vec<IndexedField>,
    pub active_ranges: Vec<ActiveStructRange>,
    pub highlights: Arc<Vec<(std::ops::Range<usize>, gpui::Hsla)>>,
    /// Sorted physical field boundaries used by the inline line layout.
    pub field_breaks: Arc<Vec<usize>>,
    active_range_tree_base: usize,
    active_range_max_tree: Vec<usize>,
}

/// Incrementally collects the byte-range data needed by structure rendering.
///
/// The parser receives completed fields in batches. Collecting each batch
/// here avoids traversing the complete field tree again when the final parse
/// result is published. Ordering and deduplication are intentionally deferred
/// to [`Self::finish`] because instance fields may point backwards in the
/// stream.
#[derive(Debug, Default)]
pub(crate) struct StructureIndexBuilder {
    highlights: Vec<(std::ops::Range<usize>, gpui::Hsla)>,
    container_structs: Vec<IndexedField>,
    leaf_fields: Vec<IndexedField>,
    active_ranges: Vec<ActiveStructRange>,
    field_breaks: Vec<usize>,
    /// Interns IDs so duplicate-detection keys do not clone the full string
    /// for every parsed field.
    id_keys: HashMap<String, usize>,
    container_seen: HashSet<(usize, usize, usize)>,
    leaf_seen: HashSet<(usize, usize, usize)>,
    range_seen: HashSet<(usize, usize, usize)>,
    collect_layout: bool,
}

impl StructureIndexBuilder {
    pub(crate) fn new() -> Self {
        Self {
            collect_layout: true,
            ..Self::default()
        }
    }

    fn new_live() -> Self {
        Self::default()
    }

    /// Adds one newly completed root field and all of its children.
    pub(crate) fn add_field(&mut self, field: &ParsedField) {
        self.collect_index_field(field, 0);
    }

    fn id_key(&mut self, id: &str) -> usize {
        if let Some(&key) = self.id_keys.get(id) {
            return key;
        }

        let key = self.id_keys.len();
        self.id_keys.insert(id.to_owned(), key);
        key
    }

    /// Finalizes ordering and lookup metadata without revisiting the fields.
    pub(crate) fn finish(mut self) -> StructureIndex {
        self.highlights.sort_unstable_by_key(|(range, _)| range.start);
        // The lookup helpers below use `partition_point`, so both collections
        // must be ordered by file offset. Structure traversal is normally in
        // stream order, but `pos`-based instances are appended after `seq`
        // fields and can point backwards in the file.
        self.container_structs.sort_by_key(|field| field.offset);
        self.leaf_fields.sort_by_key(|field| field.offset);
        self.active_ranges.sort_unstable_by_key(|r| r.start);
        self.field_breaks.sort_unstable();
        self.field_breaks.dedup();
        let (active_range_tree_base, active_range_max_tree) = Self::build_active_range_tree(&self.active_ranges);

        StructureIndex {
            container_structs: self.container_structs,
            leaf_fields: self.leaf_fields,
            active_ranges: self.active_ranges,
            highlights: Arc::new(self.highlights),
            field_breaks: Arc::new(self.field_breaks),
            active_range_tree_base,
            active_range_max_tree,
        }
    }

    fn finish_live(mut self) -> LiveIndexBatch {
        self.container_structs.sort_by_key(|field| field.offset);
        self.leaf_fields.sort_by_key(|field| field.offset);
        LiveIndexBatch {
            container_structs: self.container_structs,
            leaf_fields: self.leaf_fields,
        }
    }

    fn collect_index_field(&mut self, field: &ParsedField, depth: usize) {
        let mut work = vec![(field, depth)];

        while let Some((field, depth)) = work.pop() {
            let is_str = field.is_struct();
            if self.collect_layout && !field.is_instance && field.size > 0 {
                self.field_breaks.push(field.offset);
                self.field_breaks.push(field.offset + field.size);
            }
            if self.collect_layout && field.size > 0 && !is_str {
                self.highlights.push((field.offset..field.offset + field.size, field.color));
            }

            if is_str {
                let end = field.offset + field.size;
                let id_key = self.id_key(&field.id);
                if self.container_seen.insert((field.offset, field.size, id_key)) {
                    self.container_structs.push(IndexedField::container(field, depth));
                }
                if self.collect_layout && self.range_seen.insert((field.offset, end, id_key)) {
                    self.active_ranges.push(ActiveStructRange {
                        start: field.offset,
                        end,
                        depth,
                        id: field.id.clone(),
                    });
                }
            } else if field.size > 0 && field.children.is_empty() && !matches!(field.value, FieldValue::Struct) {
                let id_key = self.id_key(&field.id);
                if self.leaf_seen.insert((field.offset, field.size, id_key)) {
                    self.leaf_fields.push(IndexedField::leaf(field, depth));
                }
            }

            // LIFO traversal needs children in reverse order to retain the
            // same source order as the former recursive implementation.
            for child in field.children.iter().rev() {
                work.push((child, depth + 1));
            }
        }
    }

    fn build_active_range_tree(ranges: &[ActiveStructRange]) -> (usize, Vec<usize>) {
        let base = ranges.len().max(1).next_power_of_two();
        let mut tree = vec![0; base * 2];

        for (index, range) in ranges.iter().enumerate() {
            tree[base + index] = range.end;
        }
        for index in (1..base).rev() {
            tree[index] = tree[index * 2].max(tree[index * 2 + 1]);
        }

        (base, tree)
    }
}

/// Indexes newly parsed chunks for the live structure view.
///
/// The final parser index is intentionally built once, on the parser thread.
/// During parsing the UI only needs descriptions for the chunks that have
/// arrived so far. Keeping one small, sorted index per received chunk avoids
/// rebuilding or cloning the complete index on every progress update.
#[derive(Debug)]
struct LiveIndexBatch {
    container_structs: Vec<IndexedField>,
    leaf_fields: Vec<IndexedField>,
}

#[derive(Debug, Default)]
struct LiveStructureIndex {
    batches: RwLock<Vec<Arc<LiveIndexBatch>>>,
}

impl LiveStructureIndex {
    fn append_chunks(&self, chunks: &[Arc<[ParsedField]>]) {
        let mut batches = Vec::with_capacity(chunks.len());
        for chunk in chunks.iter().filter(|chunk| !chunk.is_empty()) {
            let mut builder = StructureIndexBuilder::new_live();
            for field in chunk.iter() {
                builder.add_field(field);
            }
            batches.push(Arc::new(builder.finish_live()));
        }

        if !batches.is_empty() {
            self.batches.write().expect("live structure index write lock").extend(batches);
        }
    }

    fn fields_starting_at(&self, start_offset: usize, len: usize, select: fn(&LiveIndexBatch) -> &[IndexedField]) -> Vec<IndexedField> {
        if len == 0 {
            return Vec::new();
        }

        let end_offset = start_offset.saturating_add(len);
        let batches = self.batches.read().expect("live structure index read lock");
        let mut fields = Vec::new();
        for batch in batches.iter() {
            let fields_in_batch = select(batch);
            let start_idx = fields_in_batch.partition_point(|field| field.offset < start_offset);
            let end_idx = start_idx + fields_in_batch[start_idx..].partition_point(|field| field.offset < end_offset);
            fields.extend(fields_in_batch[start_idx..end_idx].iter().cloned());
        }

        fields.sort_by(|left, right| (left.offset, left.depth, left.size, &left.id).cmp(&(right.offset, right.depth, right.size, &right.id)));
        fields.dedup_by(|right, left| right.offset == left.offset && right.size == left.size && right.id == left.id);
        fields
    }

    fn find_container_structs_starting_at(&self, start_offset: usize, len: usize) -> Vec<IndexedField> {
        self.fields_starting_at(start_offset, len, |index| &index.container_structs)
    }

    fn find_leaf_fields_starting_at(&self, start_offset: usize, len: usize) -> Vec<IndexedField> {
        self.fields_starting_at(start_offset, len, |index| &index.leaf_fields)
    }
}

#[derive(Debug, Clone)]
pub struct ParseProgress {
    pub definition_id: String,
    /// Newly completed root fields since the previous progress notification.
    /// This is a delta, not a cumulative snapshot.
    pub fields: Arc<[ParsedField]>,
    pub parsed_offset: usize,
    pub total_bytes: usize,
    pub is_done: bool,
    /// True while the parser has reached the byte end but is preparing the
    /// final display index and line map.
    pub is_finalizing: bool,
    pub errors: Vec<ParseError>,
    pub parse_result: Option<Arc<ParseResult>>,
}

#[derive(Debug, Clone)]
pub struct ParseResult {
    pub definition_id: String,
    pub fields: FieldCollection,
    pub total_parsed_bytes: usize,
    pub errors: Vec<ParseError>,
    pub index: Arc<StructureIndex>,
    /// Background-prepared layout for the default expanded structure view.
    ///
    /// Custom joins/breaks and collapsed structures are intentionally not
    /// included. The editor falls back to the existing dynamic layout for
    /// those cases, so this cache cannot change their display semantics.
    pub structure_line_map: Option<Arc<LineMap>>,
    live_index: Option<Arc<LiveStructureIndex>>,
}

impl ParseResult {
    pub fn new(definition_id: String, fields: Vec<ParsedField>, total_parsed_bytes: usize, errors: Vec<ParseError>) -> Self {
        let fields = FieldCollection::from_vec(fields);
        let mut index_builder = StructureIndexBuilder::new();
        for field in fields.iter() {
            index_builder.add_field(field);
        }
        Self::new_with_index(definition_id, fields, total_parsed_bytes, errors, index_builder.finish())
    }

    pub(crate) fn new_with_index(
        definition_id: String,
        fields: FieldCollection,
        total_parsed_bytes: usize,
        errors: Vec<ParseError>,
        index: StructureIndex,
    ) -> Self {
        Self {
            definition_id,
            fields,
            total_parsed_bytes,
            errors,
            index: Arc::new(index),
            structure_line_map: None,
            live_index: None,
        }
    }

    /// Creates an empty result that can receive incremental parse batches.
    pub fn empty(definition_id: String) -> Self {
        Self {
            definition_id,
            fields: FieldCollection::default(),
            total_parsed_bytes: 0,
            errors: Vec::new(),
            index: Arc::new(StructureIndex::default()),
            structure_line_map: None,
            live_index: Some(Arc::new(LiveStructureIndex::default())),
        }
    }

    /// Prepares the default expanded structure line map off the UI thread.
    pub fn with_structure_line_map(mut self, total_size: usize) -> Self {
        let field_breaks = self.index.field_breaks.as_ref();
        let mut structure_headers = BTreeMap::new();
        self.collect_structure_header_lines(&mut structure_headers, &HashSet::new());
        self.structure_line_map = Some(Arc::new(build_line_map_from_sorted_events(
            total_size,
            field_breaks,
            &Default::default(),
            &structure_headers,
        )));
        self
    }

    /// Appends a field batch while sharing all previously parsed field chunks.
    pub fn append_fields(&self, fields: Vec<ParsedField>, total_parsed_bytes: usize) -> Self {
        let fields = self.fields.append_chunk(fields);
        let mut index_builder = StructureIndexBuilder::new();
        for field in fields.iter() {
            index_builder.add_field(field);
        }
        Self::new_with_index(
            self.definition_id.clone(),
            fields,
            total_parsed_bytes,
            self.errors.clone(),
            index_builder.finish(),
        )
    }

    /// Appends a field batch to a live parse snapshot.
    ///
    /// Only the new chunk is indexed. Previously received chunks keep their
    /// own small index, so the UI never rebuilds the complete parse result.
    pub fn append_fields_without_index(&self, fields: Vec<ParsedField>, total_parsed_bytes: usize) -> Self {
        let chunk: Arc<[ParsedField]> = Arc::from(fields.into_boxed_slice());
        self.append_shared_chunks_without_index(std::slice::from_ref(&chunk), total_parsed_bytes)
    }

    /// Appends shared parse chunks without rebuilding the complete byte-range index.
    pub fn append_shared_chunks_without_index(&self, chunks: &[Arc<[ParsedField]>], total_parsed_bytes: usize) -> Self {
        let fields = self.fields.append_shared_chunks(chunks);
        let live_index = self.live_index.clone().unwrap_or_else(|| Arc::new(LiveStructureIndex::default()));
        live_index.append_chunks(chunks);
        Self {
            definition_id: self.definition_id.clone(),
            fields,
            total_parsed_bytes,
            errors: self.errors.clone(),
            index: self.index.clone(),
            structure_line_map: None,
            live_index: Some(live_index),
        }
    }

    /// Returns whether this result is an incremental snapshot.
    pub fn is_live(&self) -> bool {
        self.live_index.is_some()
    }

    /// Finds container descriptions in an incremental snapshot.
    pub fn find_live_container_structs_starting_at(&self, start_offset: usize, len: usize) -> Vec<IndexedField> {
        self.live_index
            .as_ref()
            .map(|index| index.find_container_structs_starting_at(start_offset, len))
            .unwrap_or_default()
    }

    /// Finds leaf descriptions in an incremental snapshot.
    pub fn find_live_leaf_fields_starting_at(&self, start_offset: usize, len: usize) -> Vec<IndexedField> {
        self.live_index
            .as_ref()
            .map(|index| index.find_leaf_fields_starting_at(start_offset, len))
            .unwrap_or_default()
    }

    pub fn build_index(&mut self) {
        let mut index_builder = StructureIndexBuilder::new();
        for field in self.fields.iter() {
            index_builder.add_field(field);
        }
        self.index = Arc::new(index_builder.finish());
        self.structure_line_map = None;
        self.live_index = None;
    }

    pub fn to_highlights(&self) -> Vec<(std::ops::Range<usize>, gpui::Hsla)> {
        self.index.highlights.as_ref().clone()
    }

    pub fn collect_field_breaks(&self, breaks: &mut Vec<usize>, collapsed_structs: &std::collections::HashSet<String>) {
        if collapsed_structs.is_empty() {
            breaks.extend(self.index.field_breaks.iter().copied());
            return;
        }

        let mut work = Vec::new();
        for field in self.fields.iter() {
            work.push(field);
            while let Some(field) = work.pop() {
                // Sequence fields define physical stream boundaries and line breaks.
                // Instance fields (computed values or pos-peeks) do not break the physical stream.
                if !field.is_instance && field.size > 0 {
                    breaks.push(field.offset);
                    breaks.push(field.offset + field.size);
                }
                if !field.children.is_empty() && !collapsed_structs.contains(&field.id) {
                    for child in field.children.iter().rev() {
                        work.push(child);
                    }
                }
            }
        }
    }

    /// Adds one description-only row for each visible physical structure.
    pub fn collect_structure_header_lines(&self, empty_lines: &mut BTreeMap<usize, usize>, collapsed_structs: &HashSet<String>) {
        if collapsed_structs.is_empty() {
            for header in self.index.container_structs.iter().filter(|field| !field.is_instance && field.size > 0) {
                *empty_lines.entry(header.offset).or_default() += 1;
            }
            return;
        }

        let mut work = self.fields.iter().map(|field| (field, false)).collect::<Vec<_>>();
        while let Some((field, hidden_by_collapsed_parent)) = work.pop() {
            let is_collapsed = collapsed_structs.contains(&field.id);
            if field.is_struct() && !field.is_instance && field.size > 0 && !hidden_by_collapsed_parent && !is_collapsed {
                *empty_lines.entry(field.offset).or_default() += 1;
            }

            if !hidden_by_collapsed_parent && !is_collapsed {
                for child in field.children.iter().rev() {
                    work.push((child, false));
                }
            }
        }
    }

    /// Returns whether a visible structure starts at `offset`.
    pub fn has_structure_header_at(&self, offset: usize, collapsed_structs: &HashSet<String>) -> bool {
        self.index
            .container_structs
            .iter()
            .any(|header| header.offset == offset && header.size > 0 && !header.is_instance && !collapsed_structs.contains(&header.id))
    }

    pub fn find_container_structs_starting_at(&self, start_offset: usize, len: usize) -> &[IndexedField] {
        let end_offset = start_offset.saturating_add(len);
        let containers = &self.index.container_structs;
        let start_idx = containers.partition_point(|f| f.offset < start_offset);
        let end_idx = start_idx + containers[start_idx..].partition_point(|f| f.offset < end_offset);
        &containers[start_idx..end_idx]
    }

    pub fn find_leaf_fields_starting_at(&self, start_offset: usize, len: usize) -> &[IndexedField] {
        let end_offset = start_offset.saturating_add(len);
        let leaves = &self.index.leaf_fields;
        let start_idx = leaves.partition_point(|f| f.offset < start_offset);
        let end_idx = start_idx + leaves[start_idx..].partition_point(|f| f.offset < end_offset);
        &leaves[start_idx..end_idx]
    }

    pub fn find_active_struct_ranges(&self, start_offset: usize, len: usize) -> Vec<&ActiveStructRange> {
        let row_end = start_offset.saturating_add(len);
        let ranges = &self.index.active_ranges;
        let mut result = Vec::with_capacity(8);
        if ranges.is_empty() {
            return result;
        }

        let tree = &self.index.active_range_max_tree;
        let mut work = vec![(1, 0, self.index.active_range_tree_base)];
        while let Some((node, segment_start, segment_end)) = work.pop() {
            if segment_start >= ranges.len() || ranges[segment_start].start >= row_end || tree[node] <= start_offset {
                continue;
            }

            if segment_end - segment_start == 1 {
                let range = &ranges[segment_start];
                if range.start < row_end && range.end > start_offset {
                    result.push(range);
                }
                continue;
            }

            let middle = segment_start + (segment_end - segment_start) / 2;
            // Push right first so results remain sorted by the range start.
            work.push((node * 2 + 1, middle, segment_end));
            work.push((node * 2, segment_start, middle));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_field(id: &str, offset: usize) -> ParsedField {
        ParsedField {
            id: id.into(),
            field_type: "u1".into(),
            offset,
            size: 1,
            value: FieldValue::U8(offset as u8),
            color: Hsla::default(),
            description: None,
            children: Vec::new(),
            enum_label: None,
            is_instance: false,
        }
    }

    #[test]
    fn field_collection_appends_shared_chunks_in_parse_order() {
        let first: Arc<[ParsedField]> = Arc::from(vec![test_field("a", 0), test_field("b", 1)].into_boxed_slice());
        let second: Arc<[ParsedField]> = Arc::from(vec![test_field("c", 2)].into_boxed_slice());
        let third: Arc<[ParsedField]> = Arc::from(vec![test_field("d", 3), test_field("e", 4), test_field("f", 5)].into_boxed_slice());

        let collection = FieldCollection::default().append_shared_chunks(&[first, second, third]);

        assert_eq!(collection.len(), 6);
        assert_eq!(collection.get(0).map(|field| field.id.as_str()), Some("a"));
        assert_eq!(collection.get(5).map(|field| field.id.as_str()), Some("f"));
        assert!(collection.get(6).is_none());

        let all_ids: Vec<_> = collection.iter().map(|field| field.id.as_str()).collect();
        assert_eq!(all_ids, ["a", "b", "c", "d", "e", "f"]);

        let tail_ids: Vec<_> = collection.iter_from(2).map(|field| field.id.as_str()).collect();
        assert_eq!(tail_ids, ["c", "d", "e", "f"]);
        assert!(collection.iter_from(collection.len()).next().is_none());

        let mut many = FieldCollection::default();
        for index in 0..97 {
            many = many.append_chunk(vec![test_field(&format!("field_{index}"), index)]);
        }
        for start in 0..=many.len() {
            let actual: Vec<_> = many.iter_from(start).map(|field| field.offset).collect();
            let expected: Vec<_> = (start..many.len()).collect();
            assert_eq!(actual, expected, "iterator start index {start}");
        }
    }

    #[test]
    fn live_parse_index_exposes_only_received_chunks() {
        let mut first_container = test_field("header", 0);
        first_container.size = 2;
        first_container.value = FieldValue::Struct;
        first_container.children = vec![test_field("magic", 0)];

        let first: Arc<[ParsedField]> = Arc::from(vec![first_container].into_boxed_slice());
        let second: Arc<[ParsedField]> = Arc::from(vec![test_field("tail", 2)].into_boxed_slice());

        let partial = ParseResult::empty("live".into()).append_shared_chunks_without_index(&[first], 2);
        assert!(partial.is_live());
        assert_eq!(partial.find_live_container_structs_starting_at(0, 2)[0].id, "header");
        assert_eq!(partial.find_live_leaf_fields_starting_at(0, 2)[0].id, "magic");
        assert!(partial.find_live_leaf_fields_starting_at(2, 1).is_empty());

        let partial = partial.append_shared_chunks_without_index(&[second], 3);
        assert_eq!(partial.find_live_leaf_fields_starting_at(2, 1)[0].id, "tail");

        let complete = ParseResult::new("complete".into(), Vec::new(), 0, Vec::new());
        assert!(!complete.is_live());
    }

    #[test]
    fn field_value_conversions_cover_numeric_text_and_struct_values() {
        assert_eq!(FieldValue::U8(0x12).to_i64(), 0x12);
        assert_eq!(FieldValue::I64(-4).to_i64(), -4);
        assert_eq!(FieldValue::F32(2.75).to_i64(), 2);
        assert_eq!(FieldValue::Bool(true).to_i64(), 1);
        assert_eq!(FieldValue::Bool(false).to_f64(), 0.0);
        assert_eq!(FieldValue::U16(12).to_f64(), 12.0);
        assert_eq!(FieldValue::String("text".into()).to_string_value(), "text");
        assert_eq!(FieldValue::Bytes(vec![b'A', b'\0']).to_string_value(), "A\0");
        assert_eq!(FieldValue::Struct.to_string_value(), "{...}");
        assert_eq!(FieldValue::U16(0x2A).format_with_radix(DisplayRadix::Binary), "0b101010");
        assert_eq!(FieldValue::U16(0x2A).format_with_radix(DisplayRadix::Octal), "0o52");
        assert_eq!(FieldValue::U16(0x2A).format_with_radix(DisplayRadix::Decimal), "42");
        assert_eq!(FieldValue::U16(0x2A).format_with_radix(DisplayRadix::Hexadecimal), "0x2A");
        assert_eq!(FieldValue::I8(-10).format_with_radix(DisplayRadix::Hexadecimal), "-0xA");
    }

    #[test]
    fn parsed_field_formatting_prefers_structure_enum_and_description_details() {
        let numeric = ParsedField {
            id: "flags".into(),
            field_type: "u1".into(),
            offset: 0,
            size: 1,
            value: FieldValue::U8(0xAB),
            color: Hsla::default(),
            description: None,
            children: Vec::new(),
            enum_label: None,
            is_instance: false,
        };
        assert_eq!(numeric.format_expression(), "flags = ABh (171)");
        assert_eq!(numeric.format_comment(), None);

        let mut enum_field = numeric.clone();
        enum_field.enum_label = Some("enabled".into());
        enum_field.description = Some("flag description".into());
        assert_eq!(enum_field.format_expression(), "flags = 171 (enabled)");
        assert_eq!(enum_field.format_comment(), Some("flag description".into()));

        let structure = ParsedField {
            id: "header".into(),
            field_type: "header".into(),
            offset: 0,
            size: 4,
            value: FieldValue::Struct,
            color: Hsla::default(),
            description: None,
            children: vec![numeric],
            enum_label: None,
            is_instance: false,
        };
        assert!(structure.is_struct());
        assert_eq!(structure.format_expression(), "header");
    }

    #[test]
    fn deep_structure_index_walks_without_call_stack_growth() {
        let depth = 4096;
        let mut field = test_field("leaf", 0);
        for level in (0..depth).rev() {
            field = ParsedField {
                id: format!("node_{level}"),
                field_type: "nested".into(),
                offset: 0,
                size: 1,
                value: FieldValue::Struct,
                color: Hsla::default(),
                description: None,
                children: vec![field],
                enum_label: None,
                is_instance: false,
            };
        }

        // The result is dropped normally at the end of the test. ParsedField's
        // custom destructor must keep that operation off the call stack too.
        let result = ParseResult::new("deep".into(), vec![field], 1, Vec::new());

        let mut collapsed = HashSet::new();
        collapsed.insert("unrelated".to_string());
        let mut breaks = Vec::new();
        result.collect_field_breaks(&mut breaks, &collapsed);
        assert_eq!(breaks.len(), (depth + 1) * 2);

        let ranges = result.find_active_struct_ranges(0, 1);
        assert_eq!(ranges.len(), depth);
        assert_eq!(ranges.first().map(|range| range.depth), Some(0));
        assert_eq!(ranges.last().map(|range| range.depth), Some(depth - 1));

        let cloned = result.fields.get(0).expect("deep root field").clone();
        assert_eq!(cloned.children.len(), 1);
    }
}
