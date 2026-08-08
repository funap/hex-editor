#[cfg(test)]
mod tests {
    use crate::core::structure::types::FieldValue;
    use crate::core::structure::{KaitaiInterpreter, KaitaiStream, KsyDefinition};

    fn parse_ksy_yaml(yaml: &str) -> KsyDefinition {
        serde_yaml::from_str(yaml).expect("Failed to parse YAML")
    }

    #[test]
    fn test_endian_explicit() {
        let yaml = r#"
meta:
  id: test_endian
seq:
  - id: val1
    type: u2le
  - id: val2
    type: u4be
"#;
        let ksy = parse_ksy_yaml(yaml);
        let data = vec![0x01, 0x02, 0x00, 0x00, 0x00, 0x05];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[0].id, "val1");
        if let FieldValue::U16(v) = result.fields[0].value {
            assert_eq!(v, 0x0201); // little endian
        } else {
            panic!("Wrong type");
        }

        assert_eq!(result.fields[1].id, "val2");
        if let FieldValue::U32(v) = result.fields[1].value {
            assert_eq!(v, 5); // big endian
        } else {
            panic!("Wrong type");
        }
    }

    #[test]
    fn test_switch_on() {
        let yaml = r#"
meta:
  id: test_switch
seq:
  - id: tag
    type: u1
  - id: body
    type:
      switch-on: tag
      cases:
        1: u2le
        2: u4le
"#;
        let ksy = parse_ksy_yaml(yaml);
        let data = vec![0x01, 0xFF, 0x00];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy.clone());
        let result1 = interpreter.parse(&mut stream);

        assert_eq!(result1.fields.len(), 2);
        if let FieldValue::U16(v) = result1.fields[1].value {
            assert_eq!(v, 0xFF);
        } else {
            panic!("Expected U16");
        }

        let data2 = vec![0x02, 0x11, 0x22, 0x33, 0x44];
        let mut stream2 = KaitaiStream::new(&data2);
        let interpreter2 = KaitaiInterpreter::new(ksy);
        let result2 = interpreter2.parse(&mut stream2);

        assert_eq!(result2.fields.len(), 2);
        if let FieldValue::U32(v) = result2.fields[1].value {
            assert_eq!(v, 0x44332211);
        } else {
            panic!("Expected U32");
        }
    }

    #[test]
    fn test_size_eos() {
        let yaml = r#"
meta:
  id: test_eos
seq:
  - id: first
    type: u1
  - id: rest
    size-eos: true
"#;
        let ksy = parse_ksy_yaml(yaml);
        let data = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[1].size, 3);
        if let FieldValue::Bytes(b) = &result.fields[1].value {
            assert_eq!(b, &[0xBB, 0xCC, 0xDD]);
        } else {
            panic!("Expected Bytes");
        }
    }

    #[test]
    fn test_bit_fields_parsing() {
        let yaml = r#"
meta:
  id: test_bits
seq:
  - id: part1
    type: b4
  - id: part2
    type: b4
  - id: part3
    type: b8
"#;
        let ksy = parse_ksy_yaml(yaml);
        // 0b1011_0011 (0xB3), 0b0101_1010 (0x5A)
        // part1: 4 bits -> 0b1011 (11)
        // part2: 4 bits -> 0b0011 (3)
        // part3: 8 bits -> 0x5A (90)
        let data = vec![0xB3, 0x5A];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert_eq!(result.fields.len(), 3);

        assert_eq!(result.fields[0].id, "part1");
        if let FieldValue::U64(v) = result.fields[0].value {
            assert_eq!(v, 11);
        } else {
            panic!("Expected U64");
        }

        assert_eq!(result.fields[1].id, "part2");
        if let FieldValue::U64(v) = result.fields[1].value {
            assert_eq!(v, 3);
        } else {
            panic!("Expected U64");
        }

        assert_eq!(result.fields[2].id, "part3");
        if let FieldValue::U64(v) = result.fields[2].value {
            assert_eq!(v, 90);
        } else {
            panic!("Expected U64");
        }
    }

    #[test]
    fn test_process_xor() {
        let yaml = r#"
meta:
  id: test_xor
seq:
  - id: key
    type: u1
  - id: body
    size: 4
    process: xor(key)
"#;
        let ksy = parse_ksy_yaml(yaml);
        let data = vec![0x55, 0x11 ^ 0x55, 0x22 ^ 0x55, 0x33 ^ 0x55, 0x44 ^ 0x55];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[1].id, "body");
        if let FieldValue::Bytes(ref b) = result.fields[1].value {
            assert_eq!(b, &[0x11, 0x22, 0x33, 0x44]);
        } else {
            panic!("Expected Bytes");
        }
    }

    #[test]
    fn test_process_zlib() {
        // Zlib compressed "Hello"
        let compressed = vec![120, 156, 243, 72, 205, 201, 201, 7, 0, 5, 140, 1, 245];

        let yaml = r#"
meta:
  id: test_zlib
seq:
  - id: body
    size: 13
    process: zlib
"#;
        let ksy = parse_ksy_yaml(yaml);
        let mut stream = KaitaiStream::new(&compressed);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert_eq!(result.fields.len(), 1);
        if let FieldValue::Bytes(ref b) = result.fields[0].value {
            assert_eq!(std::str::from_utf8(b).unwrap(), "Hello");
        } else {
            panic!("Expected Bytes");
        }
    }

    #[test]
    fn test_ensure_fixed_contents() {
        let yaml = r#"
meta:
  id: test_fixed
seq:
  - id: magic
    contents: [0x89, "PNG"]
  - id: rest
    size: 1
"#;
        let ksy = parse_ksy_yaml(yaml);
        let data = vec![0x89, b'P', b'N', b'G', 0xFF];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert_eq!(result.errors.len(), 0);
        assert_eq!(result.fields.len(), 2);

        // Test invalid magic
        let invalid_data = vec![0x89, b'P', b'D', b'G', 0xFF];
        let mut stream2 = KaitaiStream::new(&invalid_data);
        let interpreter2 = KaitaiInterpreter::new(parse_ksy_yaml(yaml));
        let result2 = interpreter2.parse(&mut stream2);
        assert_eq!(result2.errors.len(), 1);
        assert_eq!(result2.errors[0].message, "contents mismatch");
    }

    #[test]
    fn test_expression_bitwise() {
        use crate::core::structure::expression::{EvalContext, ExprEvaluator};
        use std::collections::HashMap;
        let mut values = HashMap::new();
        values.insert("flags".to_string(), 0b1010_1100);

        let string_values = HashMap::new();
        let base_path = vec![];
        let enums = HashMap::new();

        let ctx = EvalContext {
            values: &values,
            string_values: &string_values,
            base_path: &base_path,
            stream_eof: false,
            stream_size: 0,
            stream_pos: 0,
            enums: &enums,
            errors: None,
        };

        assert_eq!(ExprEvaluator::eval_i64("flags & 0b1000_0000", &ctx), 128);
        assert_eq!(ExprEvaluator::eval_i64("flags | 0x03", &ctx), 0b1010_1111);
        assert_eq!(ExprEvaluator::eval_i64("1 << 3", &ctx), 8);
    }

    #[test]
    fn test_term_multi_backtrack() {
        let data = vec![0xAA, 0xAA, 0xBB, 0xCC];
        let mut stream = KaitaiStream::new(&data);
        let terminator = vec![0xAA, 0xBB];
        let res = stream.read_bytes_term_multi(&terminator, false, true, true);
        assert_eq!(res, Some(vec![0xAA])); // should consume AA, AA, BB and return AA
        assert_eq!(stream.pos(), 3);
    }

    #[test]
    fn test_term_eos_error() {
        let data = vec![0x11, 0x22, 0x33];
        let mut stream = KaitaiStream::new(&data);
        let res = stream.read_bytes_term(0x00, false, true, true);
        assert_eq!(res, None); // terminator not found, and eos_error is true
    }

    #[test]
    fn test_expression_zero_division() {
        use crate::core::structure::expression::{EvalContext, ExprEvaluator};
        use std::collections::HashMap;

        let values = HashMap::new();
        let string_values = HashMap::new();
        let base_path = vec![];
        let enums = HashMap::new();
        let errors = std::cell::RefCell::new(Vec::new());

        let ctx = EvalContext {
            values: &values,
            string_values: &string_values,
            base_path: &base_path,
            stream_eof: false,
            stream_size: 0,
            stream_pos: 0,
            enums: &enums,
            errors: Some(&errors),
        };

        let res = ExprEvaluator::eval_i64("10 / 0", &ctx);
        assert_eq!(res, 0);
        assert!(!errors.borrow().is_empty());
    }

    #[test]
    fn test_expr_ast_compilation_and_evaluation() {
        use crate::core::structure::expression::{EvalContext, ExprAST, ExprEvaluator};
        use std::collections::HashMap;

        let mut values = HashMap::new();
        values.insert("header.len".to_string(), 16);
        values.insert("flags".to_string(), 0x05);

        let string_values = HashMap::new();
        let base_path = vec!["header".to_string()];
        let enums = HashMap::new();
        let errors = std::cell::RefCell::new(Vec::new());

        let ctx = EvalContext {
            values: &values,
            string_values: &string_values,
            base_path: &base_path,
            stream_eof: false,
            stream_size: 100,
            stream_pos: 0,
            enums: &enums,
            errors: Some(&errors),
        };

        let ast = ExprAST::compile("flags & 0x01 != 0 ? len * 2 : 0").expect("Failed to compile AST");
        let val = ExprEvaluator::eval_ast_i64(&ast, &ctx);
        assert_eq!(val, 32);
    }

    #[test]
    fn test_ast_member_access_with_base_path() {
        use crate::core::structure::expression::{EvalContext, ExprAST, ExprEvaluator};
        use std::collections::HashMap;

        let mut values = HashMap::new();
        values.insert("header.sub.val".to_string(), 42);

        let string_values = HashMap::new();
        let base_path = vec!["header".to_string()];
        let enums = HashMap::new();

        let ctx = EvalContext {
            values: &values,
            string_values: &string_values,
            base_path: &base_path,
            stream_eof: false,
            stream_size: 100,
            stream_pos: 0,
            enums: &enums,
            errors: None,
        };

        let ast = ExprAST::compile("sub.val").expect("Failed to compile AST");
        let val = ExprEvaluator::eval_ast_i64(&ast, &ctx);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_editor_reparse_structure_on_command() {
        use crate::core::command::InsertCharCommand;
        use crate::core::document::Document;
        use crate::core::editor::Editor;
        use std::sync::{Arc, RwLock};

        let ksy_yaml = r#"
meta:
  id: test_reparse
seq:
  - id: len
    type: u1
  - id: data
    size: len
"#;
        let ksy = Arc::new(parse_ksy_yaml(ksy_yaml));
        let buffer = crate::core::buffer::Buffer::new(vec![0x02, 0xAA, 0xBB]);
        let doc = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test.bin"), buffer)));
        let mut editor = Editor::new(doc);
        editor.set_kaitai_definition(ksy);

        assert_eq!(editor.parse_result.as_ref().unwrap().fields.len(), 2);
        assert_eq!(editor.parse_result.as_ref().unwrap().fields[1].size, 2);

        // Execute command that changes `len` from 2 to 1
        editor.set_cursor_offset(0);
        editor.execute_command(Box::new(InsertCharCommand::new(0, 0x01)));

        assert_eq!(editor.parse_result.as_ref().unwrap().fields[1].size, 1);

        // Undo command
        editor.undo();
        assert_eq!(editor.parse_result.as_ref().unwrap().fields[1].size, 2);
    }

    #[test]
    fn test_parse_zip_ksy() {
        let zip_ksy_content = match std::fs::read_to_string("/Users/af/Downloads/zip.ksy") {
            Ok(c) => c,
            Err(_) => return,
        };

        let ksy: KsyDefinition = serde_yaml::from_str(&zip_ksy_content).expect("zip.ksy YAML deserialization failed");

        // Construct a minimal valid ZIP binary in memory:
        // Local File Header + File name ("test.txt") + Body ("hello") + Central Dir + End of Central Dir
        let mut sample_zip = Vec::new();
        // Local File Header magic: PK\x03\x04
        sample_zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
        // version needed (2 bytes): 20
        sample_zip.extend_from_slice(&[20, 0]);
        // flags (2 bytes): 0
        sample_zip.extend_from_slice(&[0, 0]);
        // compression method (2 bytes): 0 (store)
        sample_zip.extend_from_slice(&[0, 0]);
        // mod time (4 bytes): 0
        sample_zip.extend_from_slice(&[0, 0, 0, 0]);
        // crc32 (4 bytes): 0
        sample_zip.extend_from_slice(&[0, 0, 0, 0]);
        // compressed size (4 bytes): 5
        sample_zip.extend_from_slice(&[5, 0, 0, 0]);
        // uncompressed size (4 bytes): 5
        sample_zip.extend_from_slice(&[5, 0, 0, 0]);
        // len_file_name (2 bytes): 8 ("test.txt")
        sample_zip.extend_from_slice(&[8, 0]);
        // len_extra (2 bytes): 0
        sample_zip.extend_from_slice(&[0, 0]);
        // file_name: "test.txt"
        sample_zip.extend_from_slice(b"test.txt");
        // body: "hello"
        sample_zip.extend_from_slice(b"hello");

        let mut stream = KaitaiStream::new(&sample_zip);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert!(!result.fields.is_empty(), "Parsed fields should not be empty");
        assert_eq!(result.fields[0].id, "sections[0]");
        assert_eq!(result.fields[0].children.len(), 3); // magic, section_type, body
    }

    #[test]
    fn test_infinite_loop_prevention() {
        let yaml = r#"
meta:
  id: test_zero_byte_repeat
seq:
  - id: empty_items
    type: empty_type
    repeat: eos
types:
  empty_type:
    seq: []
"#;
        let ksy = parse_ksy_yaml(yaml);
        let data = vec![0x01, 0x02, 0x03];
        let mut stream = KaitaiStream::new(&data);
        let interpreter = KaitaiInterpreter::new(ksy);
        let result = interpreter.parse(&mut stream);

        assert!(result.fields.len() <= 1, "Should terminate loop when stream position does not advance");
    }

    #[test]
    fn test_parse_result_inline_helpers() {
        use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};

        let leaf1 = ParsedField {
            id: "e_magic".into(),
            field_type: "u2".into(),
            offset: 0,
            size: 2,
            value: FieldValue::U16(0x5A4D),
            color: gpui::Hsla::default(),
            description: Some("Magic number".into()),
            children: vec![],
            enum_label: None,
        };

        let leaf2 = ParsedField {
            id: "e_cblp".into(),
            field_type: "u2".into(),
            offset: 2,
            size: 2,
            value: FieldValue::U16(50),
            color: gpui::Hsla::default(),
            description: Some("Bytes on last page".into()),
            children: vec![],
            enum_label: None,
        };

        let container = ParsedField {
            id: "IMAGE_DOS_HEADER".into(),
            field_type: "dos_header".into(),
            offset: 0,
            size: 64,
            value: FieldValue::Struct,
            color: gpui::Hsla::default(),
            description: None,
            children: vec![leaf1.clone(), leaf2.clone()],
            enum_label: None,
        };

        let result = ParseResult {
            definition_id: "pe".into(),
            fields: vec![container],
            total_parsed_bytes: 64,
            errors: vec![],
        };

        let containers = result.find_container_structs_starting_at(0, 16);
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "IMAGE_DOS_HEADER");

        let leaves = result.find_leaf_fields_starting_at(0, 16);
        assert_eq!(leaves.len(), 2);
        assert_eq!(leaves[0].id, "e_magic");
        assert_eq!(leaves[1].id, "e_cblp");

        let active_ranges = result.find_active_struct_ranges(0, 16);
        assert_eq!(active_ranges.len(), 1);
        assert_eq!(active_ranges[0].3, "IMAGE_DOS_HEADER");

        assert_eq!(leaf1.format_expression(), "e_magic = 5A4Dh (23117)");
        assert_eq!(leaf1.format_comment(), Some("Magic number".into()));
    }

    #[test]
    fn test_editor_line_starts_breaks_per_field() {
        use std::sync::{Arc, RwLock};
        use crate::core::document::Document;
        use crate::core::editor::Editor;

        let ksy_yaml = r#"
meta:
  id: test_header
seq:
  - id: field_a
    type: u2
  - id: field_b
    type: u2
  - id: field_c
    type: u4
"#;
        let ksy = Arc::new(parse_ksy_yaml(ksy_yaml));
        let buffer = crate::core::buffer::Buffer::new(vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A]);
        let doc = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test.bin"), buffer)));
        let mut editor = Editor::new(doc);
        editor.set_kaitai_definition(ksy);

        let line_map = editor.line_starts();
        assert_eq!(line_map.get(0), Some(0)); // field_a (0..2)
        assert_eq!(line_map.get(1), Some(2)); // field_b (2..4)
        assert_eq!(line_map.get(2), Some(4)); // field_c (4..8)
        assert_eq!(line_map.get(3), Some(8)); // unparsed tail (8..10)
    }

    #[test]
    fn test_editor_toggle_inline_structure_view_and_collapse() {
        use std::sync::{Arc, RwLock};
        use crate::core::document::Document;
        use crate::core::editor::Editor;

        let ksy_yaml = r#"
meta:
  id: test_header
seq:
  - id: field_a
    type: u2
  - id: field_b
    type: u2
"#;
        let ksy = Arc::new(parse_ksy_yaml(ksy_yaml));
        let buffer = crate::core::buffer::Buffer::new(vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A]);
        let doc = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test.bin"), buffer)));
        let mut editor = Editor::new(doc);
        editor.set_kaitai_definition(ksy);

        assert!(editor.has_custom_layout());

        // Toggle inline structure view off
        editor.toggle_inline_structure_view();
        assert!(!editor.show_inline_structure_view);
        assert!(!editor.has_custom_layout());

        // Toggle back on
        editor.toggle_inline_structure_view();
        assert!(editor.show_inline_structure_view);
        assert!(editor.has_custom_layout());
    }

    #[test]
    fn test_deduplicate_leaf_fields_sharing_offset() {
        use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};

        let magic1 = ParsedField {
            id: "magic".into(),
            field_type: "bytes".into(),
            offset: 0,
            size: 2,
            value: FieldValue::Bytes(vec![0x50, 0x4B]),
            color: gpui::Hsla::default(),
            description: None,
            children: Vec::new(),
            enum_label: None,
        };

        let magic2 = ParsedField {
            id: "magic".into(),
            field_type: "bytes".into(),
            offset: 0,
            size: 2,
            value: FieldValue::Bytes(vec![0x50, 0x4B]),
            color: gpui::Hsla::default(),
            description: None,
            children: Vec::new(),
            enum_label: None,
        };

        let section0 = ParsedField {
            id: "sections[0]".into(),
            field_type: "pk_section".into(),
            offset: 0,
            size: 30,
            value: FieldValue::Struct,
            color: gpui::Hsla::default(),
            description: None,
            children: vec![magic1],
            enum_label: None,
        };

        let local_header_inst = ParsedField {
            id: "local_header".into(),
            field_type: "pk_section".into(),
            offset: 0,
            size: 30,
            value: FieldValue::Struct,
            color: gpui::Hsla::default(),
            description: None,
            children: vec![magic2],
            enum_label: None,
        };

        let parse_result = ParseResult {
            definition_id: "zip".into(),
            fields: vec![section0, local_header_inst],
            total_parsed_bytes: 30,
            errors: Vec::new(),
        };

        let leaves = parse_result.find_leaf_fields_starting_at(0, 2);
        assert_eq!(leaves.len(), 1, "Duplicate leaf fields with same offset and ID must be deduplicated");
        assert_eq!(leaves[0].id, "magic");
    }
}
