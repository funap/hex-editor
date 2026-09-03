//! Export helpers for structure-analysis results.
//!
//! The parser's field tree is intentionally kept out of the UI export code.
//! These functions only read an immutable [`ParseResult`] snapshot, so callers
//! can run them on a background executor while parsing or rendering continues.

use super::types::{FieldValue, ParseResult, ParsedField};
use serde::Serialize;
use std::fmt::Write as _;

/// Formats a structure-analysis snapshot as readable, Wireshark-like indented text.
///
/// Traversal is iterative rather than recursive. A malformed or deliberately
/// deep definition therefore cannot overflow the stack while being copied.
pub fn format_parse_result_as_text(result: &ParseResult) -> String {
    let mut output = String::with_capacity(256);

    let root_fields: Vec<_> = result.fields.iter().collect();
    let mut stack = Vec::with_capacity(root_fields.len());
    for field in root_fields.into_iter().rev() {
        stack.push((field, 0usize));
    }

    while let Some((field, depth)) = stack.pop() {
        let indent = "    ".repeat(depth);
        let is_container = field.is_struct() || !field.children.is_empty();
        let instance_marker = if field.is_instance { " [instance]" } else { "" };

        if is_container {
            if !field.field_type.is_empty() && field.field_type != "struct" {
                let _ = writeln!(output, "{}{}: {}{}", indent, field.id, field.field_type, instance_marker);
            } else {
                let _ = writeln!(output, "{}{}{}", indent, field.id, instance_marker);
            }
        } else {
            let _ = writeln!(output, "{}{}: {}{}", indent, field.id, format_field_value(field), instance_marker);
        }

        if let Some(description) = non_empty(field.description.as_deref()) {
            let _ = writeln!(output, "{}    [{}]", indent, description);
        }

        for child in field.children.iter().rev() {
            stack.push((child, depth + 1));
        }
    }

    if !result.errors.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("[Parse errors]\n");
        for error in &result.errors {
            let _ = writeln!(output, "    [Offset 0x{:X}: {}]", error.offset, error.message);
        }
    }

    output
}

/// Formats a structure-analysis snapshot as a YAML document.
pub fn format_parse_result_as_yaml(result: &ParseResult) -> Result<String, serde_yaml::Error> {
    let mut field_count = 0;
    let mut fields = Vec::with_capacity(result.fields.len());
    for field in result.fields.iter() {
        fields.push(convert_field_to_yaml(field, &mut field_count));
    }

    let document = YamlStructureExport {
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
            .map(|error| YamlParseError {
                message: error.message.clone(),
                offset: error.offset,
            })
            .collect(),
    };

    serde_yaml::to_string(&document)
}

fn convert_field_to_yaml(root: &ParsedField, field_count: &mut usize) -> YamlField {
    struct Frame<'a> {
        field: &'a ParsedField,
        next_child: usize,
        converted: YamlField,
    }

    let create_yaml_field = |field: &ParsedField| -> YamlField {
        let value = if field.is_struct() && !field.children.is_empty() {
            None
        } else {
            Some(format_field_value(field))
        };

        let field_type = if field.field_type.is_empty() {
            if field.children.is_empty() {
                "value".to_string()
            } else {
                "struct".to_string()
            }
        } else {
            field.field_type.clone()
        };

        YamlField {
            id: field.id.clone(),
            field_type,
            offset: field.offset,
            size: field.size,
            value,
            is_instance: field.is_instance,
            description: field.description.clone().filter(|value| !value.is_empty()),
            enum_label: field.enum_label.clone(),
            children: Vec::with_capacity(field.children.len()),
        }
    };

    *field_count += 1;
    let mut frames = vec![Frame {
        field: root,
        next_child: 0,
        converted: create_yaml_field(root),
    }];

    loop {
        let next_child = {
            let frame = frames.last_mut().expect("yaml frame stack must not be empty");
            if frame.next_child < frame.field.children.len() {
                let idx = frame.next_child;
                frame.next_child += 1;
                Some(idx)
            } else {
                None
            }
        };

        if let Some(idx) = next_child {
            let child = {
                let frame = frames.last().expect("parent frame must exist");
                &frame.field.children[idx]
            };
            *field_count += 1;
            frames.push(Frame {
                field: child,
                next_child: 0,
                converted: create_yaml_field(child),
            });
            continue;
        }

        let completed = frames.pop().expect("frame must exist").converted;
        if let Some(parent) = frames.last_mut() {
            parent.converted.children.push(completed);
        } else {
            return completed;
        }
    }
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
pub struct YamlStructureExport {
    pub format_version: u8,
    pub definition_id: String,
    pub status: String,
    pub parsed_bytes: usize,
    pub root_field_count: usize,
    pub field_count: usize,
    pub error_count: usize,
    pub fields: Vec<YamlField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<YamlParseError>,
}

#[derive(Debug, Serialize)]
pub struct YamlField {
    pub id: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub offset: usize,
    pub size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_instance: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_label: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<YamlField>,
}

#[derive(Debug, Serialize)]
pub struct YamlParseError {
    pub message: String,
    pub offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: &str, field_type: &str, offset: usize, value: FieldValue) -> ParsedField {
        ParsedField {
            id: id.to_string(),
            field_type: field_type.to_string(),
            offset,
            size: 1,
            value,
            color: crate::core::color::RgbaColor::default(),
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

        let mut flags_field = field("flags", "u2", 2, FieldValue::U16(0x0002));
        flags_field.enum_label = Some("Don't fragment".to_string());
        header.children.push(flags_field);

        let mut instance_field = field("calculated_crc", "u4", 0, FieldValue::U32(0x12345678));
        instance_field.is_instance = true;
        header.children.push(instance_field);

        let result = ParseResult::new(
            "local_file".to_string(),
            vec![header],
            3,
            vec![crate::core::structure::types::ParseError {
                offset: 10,
                message: "unexpected EOF".to_string(),
            }],
        );
        let text = format_parse_result_as_text(&result);

        assert_eq!(
            text,
            "header: local_file_header\n    [Local header]\n    magic: 4B50h (19280)\n    flags: 2h (2) (Don't fragment)\n    calculated_crc: 12345678h (305419896) [instance]\n\n[Parse errors]\n    [Offset 0xA: unexpected EOF]\n"
        );
    }

    #[test]
    fn yaml_export_preserves_tree_structure() {
        let mut root = field("body", "local_file", 0, FieldValue::Struct);
        let mut child = field("size", "u4", 0, FieldValue::U32(8));
        child.enum_label = Some("Eight bytes".to_string());
        root.children.push(child);

        let result = ParseResult::new("pk_section".to_string(), vec![root], 4, Vec::new());

        let yaml = format_parse_result_as_yaml(&result).expect("structure YAML should serialize");
        let value: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("structure YAML should parse");
        assert_eq!(value["definition_id"].as_str(), Some("pk_section"));
        assert_eq!(value["field_count"].as_u64(), Some(2));
        assert_eq!(value["fields"][0]["id"].as_str(), Some("body"));
        assert_eq!(value["fields"][0]["type"].as_str(), Some("local_file"));
        assert_eq!(value["fields"][0]["children"][0]["id"].as_str(), Some("size"));
        assert_eq!(value["fields"][0]["children"][0]["type"].as_str(), Some("u4"));
        assert_eq!(value["fields"][0]["children"][0]["value"].as_str(), Some("8h (8) (Eight bytes)"));
        assert_eq!(value["fields"][0]["children"][0]["enum_label"].as_str(), Some("Eight bytes"));
    }
}
