#![allow(dead_code)]

use gpui::Hsla;
use std::sync::Arc;

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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Default)]
pub struct StructureIndex {
    pub container_structs: Vec<ParsedField>,
    pub leaf_fields: Vec<ParsedField>,
    pub active_ranges: Vec<ActiveStructRange>,
    pub highlights: Arc<Vec<(std::ops::Range<usize>, gpui::Hsla)>>,
    /// Maximum weighted character count used by the structure description column.
    pub max_container_id_chars: f32,
    pub max_leaf_expression_chars: f32,
    active_range_tree_base: usize,
    active_range_max_tree: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct ParseProgress {
    pub definition_id: String,
    pub fields: Vec<ParsedField>,
    pub parsed_offset: usize,
    pub total_bytes: usize,
    pub is_done: bool,
    pub errors: Vec<ParseError>,
    pub parse_result: Option<Arc<ParseResult>>,
}

#[derive(Debug, Clone)]
pub struct ParseResult {
    pub definition_id: String,
    pub fields: Vec<ParsedField>,
    pub total_parsed_bytes: usize,
    pub errors: Vec<ParseError>,
    pub index: Arc<StructureIndex>,
}

impl ParseResult {
    pub fn new(definition_id: String, fields: Vec<ParsedField>, total_parsed_bytes: usize, errors: Vec<ParseError>) -> Self {
        let index = Arc::new(Self::build_structure_index(&fields));
        Self {
            definition_id,
            fields,
            total_parsed_bytes,
            errors,
            index,
        }
    }

    pub fn build_index(&mut self) {
        self.index = Arc::new(Self::build_structure_index(&self.fields));
    }

    fn build_structure_index(fields: &[ParsedField]) -> StructureIndex {
        let mut highlights = Vec::new();
        let mut container_structs = Vec::new();
        let mut leaf_fields = Vec::new();
        let mut active_ranges = Vec::new();

        let mut container_seen = std::collections::HashSet::new();
        let mut leaf_seen = std::collections::HashSet::new();
        let mut range_seen = std::collections::HashSet::new();

        Self::collect_index_data(
            fields,
            0,
            &mut highlights,
            &mut container_structs,
            &mut leaf_fields,
            &mut active_ranges,
            &mut container_seen,
            &mut leaf_seen,
            &mut range_seen,
        );

        highlights.sort_unstable_by_key(|(range, _)| range.start);
        // The lookup helpers below use `partition_point`, so both collections
        // must be ordered by file offset.  Structure traversal is normally in
        // stream order, but `pos`-based instances are appended after `seq`
        // fields and can point backwards in the file.
        container_structs.sort_by_key(|field| field.offset);
        leaf_fields.sort_by_key(|field| field.offset);
        active_ranges.sort_unstable_by_key(|r| r.start);
        let (active_range_tree_base, active_range_max_tree) = Self::build_active_range_tree(&active_ranges);
        let max_container_id_chars = container_structs.iter().map(|field| weighted_char_count(&field.id)).fold(0.0, f32::max);
        let max_leaf_expression_chars = leaf_fields
            .iter()
            .map(|field| weighted_char_count(&field.format_expression()))
            .fold(0.0, f32::max);

        StructureIndex {
            container_structs,
            leaf_fields,
            active_ranges,
            highlights: Arc::new(highlights),
            max_container_id_chars,
            max_leaf_expression_chars,
            active_range_tree_base,
            active_range_max_tree,
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

    #[allow(clippy::too_many_arguments)]
    fn collect_index_data<'a>(
        fields: &'a [ParsedField],
        depth: usize,
        highlights: &mut Vec<(std::ops::Range<usize>, gpui::Hsla)>,
        container_structs: &mut Vec<ParsedField>,
        leaf_fields: &mut Vec<ParsedField>,
        active_ranges: &mut Vec<ActiveStructRange>,
        container_seen: &mut std::collections::HashSet<(usize, usize, &'a str)>,
        leaf_seen: &mut std::collections::HashSet<(usize, usize, &'a str)>,
        range_seen: &mut std::collections::HashSet<(usize, usize, &'a str)>,
    ) {
        for field in fields {
            let is_str = field.is_struct();
            if field.size > 0 && !is_str {
                highlights.push((field.offset..field.offset + field.size, field.color));
            }

            if is_str {
                let end = field.offset + field.size;
                let key = (field.offset, end, field.id.as_str());
                if container_seen.insert((field.offset, field.size, field.id.as_str())) {
                    container_structs.push(field.clone());
                }
                if range_seen.insert(key) {
                    active_ranges.push(ActiveStructRange {
                        start: field.offset,
                        end,
                        depth,
                        id: field.id.clone(),
                    });
                }
            } else if field.children.is_empty() && !matches!(field.value, FieldValue::Struct) && leaf_seen.insert((field.offset, field.size, field.id.as_str()))
            {
                leaf_fields.push(field.clone());
            }

            if !field.children.is_empty() {
                Self::collect_index_data(
                    &field.children,
                    depth + 1,
                    highlights,
                    container_structs,
                    leaf_fields,
                    active_ranges,
                    container_seen,
                    leaf_seen,
                    range_seen,
                );
            }
        }
    }

    pub fn to_highlights(&self) -> Vec<(std::ops::Range<usize>, gpui::Hsla)> {
        self.index.highlights.as_ref().clone()
    }

    pub fn collect_field_breaks(&self, breaks: &mut Vec<usize>, collapsed_structs: &std::collections::HashSet<String>) {
        Self::collect_field_breaks_recursive(&self.fields, breaks, collapsed_structs);
    }

    fn collect_field_breaks_recursive(fields: &[ParsedField], breaks: &mut Vec<usize>, collapsed_structs: &std::collections::HashSet<String>) {
        for field in fields {
            // Sequence fields define physical stream boundaries and line breaks.
            // Instance fields (computed values or pos-peeks) do not break the physical stream.
            if !field.is_instance && field.size > 0 {
                breaks.push(field.offset);
                breaks.push(field.offset + field.size);
            }
            if !field.children.is_empty() && !collapsed_structs.contains(&field.id) {
                Self::collect_field_breaks_recursive(&field.children, breaks, collapsed_structs);
            }
        }
    }

    pub fn find_container_structs_starting_at(&self, start_offset: usize, len: usize) -> &[ParsedField] {
        let end_offset = start_offset.saturating_add(len);
        let containers = &self.index.container_structs;
        let start_idx = containers.partition_point(|f| f.offset < start_offset);
        let end_idx = start_idx + containers[start_idx..].partition_point(|f| f.offset < end_offset);
        &containers[start_idx..end_idx]
    }

    pub fn find_leaf_fields_starting_at(&self, start_offset: usize, len: usize) -> &[ParsedField] {
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

        Self::collect_active_struct_ranges(
            ranges,
            &self.index.active_range_max_tree,
            1,
            0,
            self.index.active_range_tree_base,
            start_offset,
            row_end,
            &mut result,
        );
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_active_struct_ranges<'a>(
        ranges: &'a [ActiveStructRange],
        tree: &[usize],
        node: usize,
        segment_start: usize,
        segment_end: usize,
        query_start: usize,
        query_end: usize,
        result: &mut Vec<&'a ActiveStructRange>,
    ) {
        if segment_start >= ranges.len() || ranges[segment_start].start >= query_end || tree[node] <= query_start {
            return;
        }

        if segment_end - segment_start == 1 {
            let range = &ranges[segment_start];
            if range.start < query_end && range.end > query_start {
                result.push(range);
            }
            return;
        }

        let middle = segment_start + (segment_end - segment_start) / 2;
        Self::collect_active_struct_ranges(ranges, tree, node * 2, segment_start, middle, query_start, query_end, result);
        Self::collect_active_struct_ranges(ranges, tree, node * 2 + 1, middle, segment_end, query_start, query_end, result);
    }
}

fn weighted_char_count(text: &str) -> f32 {
    text.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
