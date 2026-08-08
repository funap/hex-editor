#![allow(dead_code)]

use gpui::Hsla;

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
        if let Some(desc) = &self.description {
            if !desc.is_empty() {
                return Some(desc.clone());
            }
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
pub struct ParseResult {
    pub definition_id: String,
    pub fields: Vec<ParsedField>,
    pub total_parsed_bytes: usize,
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    pub fn to_highlights(&self) -> Vec<(std::ops::Range<usize>, gpui::Hsla)> {
        let mut highlights = Vec::new();
        Self::collect_highlights(&self.fields, &mut highlights);
        highlights
    }

    fn collect_highlights(fields: &[ParsedField], highlights: &mut Vec<(std::ops::Range<usize>, gpui::Hsla)>) {
        for field in fields {
            if field.size > 0 {
                highlights.push((field.offset..field.offset + field.size, field.color));
            }
            if !field.children.is_empty() {
                Self::collect_highlights(&field.children, highlights);
            }
        }
    }

    pub fn collect_field_breaks(&self, breaks: &mut Vec<usize>, collapsed_structs: &std::collections::HashSet<String>) {
        Self::collect_field_breaks_recursive(&self.fields, breaks, collapsed_structs);
    }

    fn collect_field_breaks_recursive(fields: &[ParsedField], breaks: &mut Vec<usize>, collapsed_structs: &std::collections::HashSet<String>) {
        for field in fields {
            if field.size > 0 {
                breaks.push(field.offset);
                breaks.push(field.offset + field.size);
            }
            if !field.children.is_empty() && !collapsed_structs.contains(&field.id) {
                Self::collect_field_breaks_recursive(&field.children, breaks, collapsed_structs);
            }
        }
    }

    pub fn find_container_structs_starting_at(&self, start_offset: usize, len: usize) -> Vec<&ParsedField> {
        let mut result = Vec::new();
        Self::collect_container_structs_starting(&self.fields, start_offset, len, &mut result);
        result
    }

    fn collect_container_structs_starting<'a>(
        fields: &'a [ParsedField],
        start_offset: usize,
        len: usize,
        result: &mut Vec<&'a ParsedField>,
    ) {
        let end_offset = start_offset + len;
        for field in fields {
            if field.is_struct() && field.offset >= start_offset && field.offset < end_offset {
                if !result.iter().any(|existing| existing.offset == field.offset && existing.size == field.size && existing.id == field.id) {
                    result.push(field);
                }
            }
            if !field.children.is_empty() {
                Self::collect_container_structs_starting(&field.children, start_offset, len, result);
            }
        }
    }

    pub fn find_leaf_fields_starting_at(&self, start_offset: usize, len: usize) -> Vec<&ParsedField> {
        let mut result = Vec::new();
        Self::collect_leaf_fields_starting(&self.fields, start_offset, len, &mut result);
        result
    }

    fn collect_leaf_fields_starting<'a>(
        fields: &'a [ParsedField],
        start_offset: usize,
        len: usize,
        result: &mut Vec<&'a ParsedField>,
    ) {
        let end_offset = start_offset + len;
        for field in fields {
            if field.offset >= start_offset && field.offset < end_offset {
                if field.children.is_empty() && !matches!(field.value, FieldValue::Struct) {
                    if !result.iter().any(|existing| existing.offset == field.offset && existing.size == field.size && (existing.id == field.id || existing.format_expression() == field.format_expression())) {
                        result.push(field);
                    }
                }
            }
            if !field.children.is_empty() {
                Self::collect_leaf_fields_starting(&field.children, start_offset, len, result);
            }
        }
    }

    pub fn find_active_struct_ranges(&self, start_offset: usize, len: usize) -> Vec<(usize, usize, usize, String)> {
        let mut ranges = Vec::new();
        Self::collect_active_struct_ranges(&self.fields, 0, &mut ranges);
        let row_end = start_offset + len;
        ranges
            .into_iter()
            .filter(|(s, e, _, _)| *s < row_end && *e > start_offset)
            .collect()
    }

    fn collect_active_struct_ranges(
        fields: &[ParsedField],
        depth: usize,
        ranges: &mut Vec<(usize, usize, usize, String)>,
    ) {
        for field in fields {
            if field.is_struct() {
                let end = field.offset + field.size;
                if !ranges.iter().any(|(s, e, _, id)| *s == field.offset && *e == end && *id == field.id) {
                    ranges.push((field.offset, end, depth, field.id.clone()));
                }
                if !field.children.is_empty() {
                    Self::collect_active_struct_ranges(&field.children, depth + 1, ranges);
                }
            }
        }
    }
}

