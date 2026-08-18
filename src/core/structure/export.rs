//! Export helpers for structure-analysis results.
//!
//! The parser's field tree is intentionally kept out of the UI export code.
//! These functions only read an immutable [`ParseResult`] snapshot, so callers
//! can run them on a background executor while parsing or rendering continues.

use super::types::{FieldValue, ParseResult, ParsedField};
use serde::Serialize;
use std::fmt::Write as _;

/// Formats a structure-analysis snapshot as readable, indented text.
///
/// Traversal is iterative rather than recursive. A malformed or deliberately
/// deep definition therefore cannot overflow the stack while being copied.
pub fn format_parse_result_as_text(result: &ParseResult) -> String {
    let mut output = String::with_capacity(256);
    let mut field_count = 0;

    output.push_str("Structure Analysis\n");
    output.push_str("==================\n");
    let _ = writeln!(output, "Definition: {}", result.definition_id);
    let _ = writeln!(output, "Status: {}", if result.is_live() { "in progress" } else { "complete" });
    let _ = writeln!(output, "Parsed bytes: {}", result.total_parsed_bytes);
    let _ = writeln!(output, "Root fields: {}", result.fields.len());
    let _ = writeln!(output, "Errors: {}", result.errors.len());

    output.push_str("\nFields\n------\n");
    let root_fields: Vec<_> = result.fields.iter().collect();
    let mut stack = Vec::with_capacity(root_fields.len());
    for field in root_fields.into_iter().rev() {
        stack.push((field, 0usize));
    }

    while let Some((field, depth)) = stack.pop() {
        field_count += 1;
        let indent = "  ".repeat(depth);
        let type_name = if field.field_type.is_empty() { "struct" } else { field.field_type.as_str() };
        let instance_marker = if field.is_instance { " [instance]" } else { "" };
        let _ = write!(
            output,
            "{}- {} [{}] @ 0x{:X}..0x{:X} ({} B){}",
            indent,
            field.id,
            type_name,
            field.offset,
            field.offset.saturating_add(field.size),
            field.size,
            instance_marker,
        );
        if !field.is_struct() {
            let _ = write!(output, " = {}", format_field_value(field));
        }
        output.push('\n');

        if let Some(description) = non_empty(field.description.as_deref()) {
            let _ = writeln!(output, "{}  Description: {}", indent, description);
        }

        for child in field.children.iter().rev() {
            stack.push((child, depth + 1));
        }
    }

    if field_count == 0 {
        output.push_str("(no fields received yet)\n");
    }
    let _ = writeln!(output, "\nTotal fields: {field_count}");

    if !result.errors.is_empty() {
        output.push_str("\nParse errors\n------------\n");
        for error in &result.errors {
            let _ = writeln!(output, "- @ 0x{:X}: {}", error.offset, error.message);
        }
    }

    output
}

/// Formats a structure-analysis snapshot as a TOML document.
///
/// Fields are exported as a flat `[[fields]]` array. Each row carries its
/// index path and depth, which preserves the complete hierarchy without
/// forcing TOML serialization or deserialization to recurse through an
/// arbitrarily deep tree.
pub fn format_parse_result_as_toml(result: &ParseResult) -> Result<String, toml::ser::Error> {
    let (fields, field_count) = collect_toml_fields(result);
    let document = TomlStructureExport {
        format_version: 1,
        definition_id: result.definition_id.clone(),
        status: if result.is_live() { "in_progress" } else { "complete" }.to_string(),
        parsed_bytes: result.total_parsed_bytes,
        root_field_count: result.fields.len(),
        field_count,
        error_count: result.errors.len(),
        fields,
        errors: result
            .errors
            .iter()
            .map(|error| TomlParseError {
                message: error.message.clone(),
                offset: error.offset,
            })
            .collect(),
    };

    toml::to_string_pretty(&document)
}

fn collect_toml_fields(result: &ParseResult) -> (Vec<TomlField>, usize) {
    let mut fields = Vec::with_capacity(result.fields.len().saturating_mul(2));
    let root_fields: Vec<_> = result.fields.iter().collect();
    let mut stack = Vec::with_capacity(root_fields.len());
    for (index, field) in root_fields.into_iter().enumerate().rev() {
        stack.push((field, vec![index], 0usize));
    }

    while let Some((field, path, depth)) = stack.pop() {
        fields.push(TomlField {
            path,
            depth,
            id: field.id.clone(),
            field_type: field.field_type.clone(),
            offset: field.offset,
            size: field.size,
            value: format_field_value(field),
            is_struct: field.is_struct(),
            has_children: !field.children.is_empty(),
            is_instance: field.is_instance,
            description: field.description.clone().filter(|value| !value.is_empty()),
            enum_label: field.enum_label.clone(),
        });

        for (child_index, child) in field.children.iter().enumerate().rev() {
            let mut child_path = fields.last().map(|exported| exported.path.clone()).unwrap_or_default();
            child_path.push(child_index);
            stack.push((child, child_path, depth + 1));
        }
    }

    let field_count = fields.len();
    (fields, field_count)
}

fn format_field_value(field: &ParsedField) -> String {
    let mut value = match &field.value {
        FieldValue::U8(value) => format!("{:X}h ({value})", value),
        FieldValue::U16(value) => format!("{:X}h ({value})", value),
        FieldValue::U32(value) => format!("{:X}h ({value})", value),
        FieldValue::U64(value) => format!("{:X}h ({value})", value),
        FieldValue::I8(value) => value.to_string(),
        FieldValue::I16(value) => value.to_string(),
        FieldValue::I32(value) => value.to_string(),
        FieldValue::I64(value) => value.to_string(),
        FieldValue::F32(value) => value.to_string(),
        FieldValue::F64(value) => value.to_string(),
        FieldValue::Bool(value) => value.to_string(),
        FieldValue::String(value) => format!("{value:?}"),
        FieldValue::Bytes(value) => format!("[{} bytes]", value.len()),
        FieldValue::Struct => "{...}".to_string(),
    };

    if let Some(label) = &field.enum_label {
        let _ = write!(value, " ({label})");
    }
    value
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

#[derive(Debug, Serialize)]
struct TomlStructureExport {
    format_version: u8,
    definition_id: String,
    status: String,
    parsed_bytes: usize,
    root_field_count: usize,
    field_count: usize,
    error_count: usize,
    fields: Vec<TomlField>,
    errors: Vec<TomlParseError>,
}

#[derive(Debug, Serialize)]
struct TomlField {
    path: Vec<usize>,
    depth: usize,
    id: String,
    #[serde(rename = "type")]
    field_type: String,
    offset: usize,
    size: usize,
    value: String,
    is_struct: bool,
    has_children: bool,
    is_instance: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enum_label: Option<String>,
}

#[derive(Debug, Serialize)]
struct TomlParseError {
    message: String,
    offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Hsla;

    fn field(id: &str, field_type: &str, offset: usize, value: FieldValue) -> ParsedField {
        ParsedField {
            id: id.to_string(),
            field_type: field_type.to_string(),
            offset,
            size: 1,
            value,
            color: Hsla::default(),
            description: None,
            children: Vec::new(),
            enum_label: None,
            is_instance: false,
        }
    }

    #[test]
    fn text_export_preserves_hierarchy_and_metadata() {
        let mut header = field("header", "local_file_header", 0, FieldValue::Struct);
        header.size = 3;
        header.description = Some("Local header".to_string());
        header.children.push(field("magic", "u2", 0, FieldValue::U16(0x4B50)));

        let result = ParseResult::new("local_file".to_string(), vec![header], 3, Vec::new());
        let text = format_parse_result_as_text(&result);

        assert!(text.contains("Definition: local_file"));
        assert!(text.contains("- header [local_file_header]"));
        assert!(text.contains("Description: Local header"));
        assert!(text.contains("  - magic [u2]"));
        assert!(text.contains("4B50h (19280)"));
    }

    #[test]
    fn toml_export_is_flat_but_keeps_index_paths() {
        let mut root = field("body", "local_file", 0, FieldValue::Struct);
        root.children.push(field("size", "u4", 0, FieldValue::U32(8)));
        let result = ParseResult::new("pk_section".to_string(), vec![root], 4, Vec::new());

        let toml = format_parse_result_as_toml(&result).expect("structure TOML should serialize");
        let value: toml::Value = toml::from_str(&toml).expect("structure TOML should parse");
        assert_eq!(value["definition_id"].as_str(), Some("pk_section"));
        assert_eq!(value["field_count"].as_integer(), Some(2));
        assert_eq!(value["fields"][0]["path"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["fields"][1]["path"].as_array().map(Vec::len), Some(2));
        assert_eq!(value["fields"][1]["type"].as_str(), Some("u4"));
    }
}
