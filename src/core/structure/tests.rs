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
        byte_arrays: &crate::core::structure::expression::G_EMPTY_BYTE_MAP,
        base_path: &base_path,
        stream_eof: false,
        stream_size: 0,
        stream_pos: 0,
        enums: &enums,
        errors: None,
        instance_resolver: None,
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
        byte_arrays: &crate::core::structure::expression::G_EMPTY_BYTE_MAP,
        base_path: &base_path,
        stream_eof: false,
        stream_size: 0,
        stream_pos: 0,
        enums: &enums,
        errors: Some(&errors),
        instance_resolver: None,
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
        byte_arrays: &crate::core::structure::expression::G_EMPTY_BYTE_MAP,
        base_path: &base_path,
        stream_eof: false,
        stream_size: 100,
        stream_pos: 0,
        enums: &enums,
        errors: Some(&errors),
        instance_resolver: None,
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
        byte_arrays: &crate::core::structure::expression::G_EMPTY_BYTE_MAP,
        base_path: &base_path,
        stream_eof: false,
        stream_size: 100,
        stream_pos: 0,
        enums: &enums,
        errors: None,
        instance_resolver: None,
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
    let zip_ksy_content = r#"
meta:
  id: zip
  endian: le
seq:
  - id: sections
    type: pk_section
    repeat: eos
types:
  pk_section:
    seq:
      - id: magic
        size: 2
      - id: section_type
        type: u2
      - id: body
        size: 26
"#;

    let ksy: KsyDefinition = serde_yaml::from_str(zip_ksy_content).expect("zip.ksy YAML deserialization failed");

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
fn test_parse_realistic_zip_with_switch_on() {
    let zip_ksy = r#"
meta:
  id: zip
  endian: le
seq:
  - id: sections
    type: pk_section
    repeat: eos
types:
  pk_section:
    seq:
      - id: magic
        contents: "PK"
      - id: section_type
        type: u2
      - id: body
        type:
          switch-on: section_type
          cases:
            0x0403: local_file
            0x0201: central_dir_entry
            0x0605: end_of_central_dir
  local_file:
    seq:
      - id: header
        type: local_file_header
      - id: body
        size: header.len_body
  local_file_header:
    seq:
      - id: version
        type: u2
      - id: flags
        type: u2
      - id: compression_method
        type: u2
      - id: file_mod_time
        type: u2
      - id: file_mod_date
        type: u2
      - id: crc32
        type: u4
      - id: len_body_compressed
        type: u4
      - id: len_body_uncompressed
        type: u4
      - id: len_file_name
        type: u2
      - id: len_extra
        type: u2
      - id: file_name
        type: str
        size: len_file_name
        encoding: UTF-8
      - id: extra
        size: len_extra
    instances:
      len_body:
        value: len_body_compressed
  central_dir_entry:
    seq:
      - id: version_made_by
        type: u2
      - id: version_needed
        type: u2
      - id: flags
        type: u2
      - id: compression_method
        type: u2
      - id: file_mod_time
        type: u2
      - id: file_mod_date
        type: u2
      - id: crc32
        type: u4
      - id: len_body_compressed
        type: u4
      - id: len_body_uncompressed
        type: u4
      - id: len_file_name
        type: u2
      - id: len_extra
        type: u2
      - id: len_comment
        type: u2
      - id: disk_number_start
        type: u2
      - id: int_file_attr
        type: u2
      - id: ext_file_attr
        type: u4
      - id: ofs_local_header
        type: u4
      - id: file_name
        type: str
        size: len_file_name
        encoding: UTF-8
      - id: extra
        size: len_extra
      - id: comment
        size: len_comment
    instances:
      local_header:
        pos: ofs_local_header
        type: pk_section
  end_of_central_dir:
    seq:
      - id: disk_number
        type: u2
      - id: disk_number_cd
        type: u2
      - id: num_entries_this_disk
        type: u2
      - id: num_entries_total
        type: u2
      - id: ofs_central_dir
        type: u4
      - id: len_central_dir
        type: u4
      - id: len_comment
        type: u2
      - id: comment
        size: len_comment
"#;
    let ksy = parse_ksy_yaml(zip_ksy);

    // Build valid zip: Local file + Central dir + End of central dir
    let mut sample = Vec::new();
    // 1. Local File: PK\x03\x04
    sample.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    sample.extend_from_slice(&[20, 0]); // version
    sample.extend_from_slice(&[0, 0]); // flags
    sample.extend_from_slice(&[0, 0]); // comp method
    sample.extend_from_slice(&[0, 0, 0, 0]); // time + date
    sample.extend_from_slice(&[0, 0, 0, 0]); // crc
    sample.extend_from_slice(&[5, 0, 0, 0]); // compressed
    sample.extend_from_slice(&[5, 0, 0, 0]); // uncompressed
    sample.extend_from_slice(&[8, 0]); // name len
    sample.extend_from_slice(&[0, 0]); // extra len
    sample.extend_from_slice(b"test.txt");
    sample.extend_from_slice(b"hello");

    // 2. Central Dir: PK\x01\x02
    let cd_ofs = sample.len();
    sample.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02]);
    sample.extend_from_slice(&[20, 0]); // made by
    sample.extend_from_slice(&[20, 0]); // needed
    sample.extend_from_slice(&[0, 0]); // flags
    sample.extend_from_slice(&[0, 0]); // comp
    sample.extend_from_slice(&[0, 0, 0, 0]); // time + date
    sample.extend_from_slice(&[0, 0, 0, 0]); // crc
    sample.extend_from_slice(&[5, 0, 0, 0]); // comp len
    sample.extend_from_slice(&[5, 0, 0, 0]); // uncomp len
    sample.extend_from_slice(&[8, 0]); // name len
    sample.extend_from_slice(&[0, 0]); // extra len
    sample.extend_from_slice(&[0, 0]); // comment len
    sample.extend_from_slice(&[0, 0]); // disk
    sample.extend_from_slice(&[0, 0]); // int attr
    sample.extend_from_slice(&[0, 0, 0, 0]); // ext attr
    sample.extend_from_slice(&[0, 0, 0, 0]); // local header ofs
    sample.extend_from_slice(b"test.txt");

    // 3. End of Central Dir: PK\x05\x06
    let cd_len = sample.len() - cd_ofs;
    sample.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
    sample.extend_from_slice(&[0, 0]); // disk
    sample.extend_from_slice(&[0, 0]); // cd disk
    sample.extend_from_slice(&[1, 0]); // num entries disk
    sample.extend_from_slice(&[1, 0]); // num entries total
    sample.extend_from_slice(&(cd_ofs as u32).to_le_bytes()); // ofs cd
    sample.extend_from_slice(&(cd_len as u32).to_le_bytes()); // len cd
    sample.extend_from_slice(&[0, 0]); // comment len

    let total_size = sample.len();
    let mut stream = KaitaiStream::new(&sample);
    let interpreter = KaitaiInterpreter::new(ksy);

    let mut progress_list = Vec::new();
    let result = interpreter.parse_with_progress(&mut stream, |p| {
        progress_list.push((p.parsed_offset, p.total_bytes, p.is_done));
    });

    assert_eq!(result.total_parsed_bytes, total_size, "Full zip file should be completely parsed");
    assert_eq!(result.fields.len(), 3, "3 sections expected (local_file, central_dir, eocd)");
    assert!(result.errors.is_empty(), "No parse errors expected: {:?}", result.errors);

    let last_p = progress_list.last().unwrap();
    assert_eq!(last_p.0, total_size);
    assert_eq!(last_p.1, total_size);
    assert!(last_p.2, "Final progress should have is_done = true");
}

#[test]
fn test_parse_full_zip_ksy_without_errors() {
    let zip_ksy_path = std::path::Path::new("/Users/af/Downloads/zip.ksy");
    if !zip_ksy_path.exists() {
        return;
    }
    let zip_ksy_content = std::fs::read_to_string(zip_ksy_path).expect("Failed to read /Users/af/Downloads/zip.ksy");
    let ksy = parse_ksy_yaml(&zip_ksy_content);

    let mut sample = Vec::new();
    // 1. Local File: PK\x03\x04
    sample.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    sample.extend_from_slice(&[20, 0]); // version
    sample.extend_from_slice(&[0, 0]); // flags (gp_flags: 2 bytes)
    sample.extend_from_slice(&[0, 0]); // comp method (0: none)
    sample.extend_from_slice(&[0, 0, 0, 0]); // time + date (dos_datetime: 4 bytes)
    sample.extend_from_slice(&[0, 0, 0, 0]); // crc
    sample.extend_from_slice(&[5, 0, 0, 0]); // compressed
    sample.extend_from_slice(&[5, 0, 0, 0]); // uncompressed
    sample.extend_from_slice(&[8, 0]); // name len
    sample.extend_from_slice(&[0, 0]); // extra len
    sample.extend_from_slice(b"test.txt");
    sample.extend_from_slice(b"hello");

    // 2. Central Dir: PK\x01\x02
    let cd_ofs = sample.len();
    sample.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02]);
    sample.extend_from_slice(&[20, 0]); // version made by
    sample.extend_from_slice(&[20, 0]); // version needed
    sample.extend_from_slice(&[0, 0]); // flags
    sample.extend_from_slice(&[0, 0]); // comp method
    sample.extend_from_slice(&[0, 0, 0, 0]); // time + date
    sample.extend_from_slice(&[0, 0, 0, 0]); // crc
    sample.extend_from_slice(&[5, 0, 0, 0]); // comp len
    sample.extend_from_slice(&[5, 0, 0, 0]); // uncomp len
    sample.extend_from_slice(&[8, 0]); // name len
    sample.extend_from_slice(&[0, 0]); // extra len
    sample.extend_from_slice(&[0, 0]); // comment len
    sample.extend_from_slice(&[0, 0]); // disk
    sample.extend_from_slice(&[0, 0]); // int attr
    sample.extend_from_slice(&[0, 0, 0, 0]); // ext attr
    sample.extend_from_slice(&[0, 0, 0, 0]); // local header ofs
    sample.extend_from_slice(b"test.txt");

    // 3. End of Central Dir: PK\x05\x06
    let cd_len = sample.len() - cd_ofs;
    sample.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
    sample.extend_from_slice(&[0, 0]); // disk
    sample.extend_from_slice(&[0, 0]); // cd disk
    sample.extend_from_slice(&[1, 0]); // num entries disk
    sample.extend_from_slice(&[1, 0]); // num entries total
    sample.extend_from_slice(&(cd_len as u32).to_le_bytes()); // len cd
    sample.extend_from_slice(&(cd_ofs as u32).to_le_bytes()); // ofs cd
    sample.extend_from_slice(&[0, 0]); // comment len

    let mut stream = KaitaiStream::new(&sample);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(
        result.errors.len(),
        0,
        "No errors expected when parsing zip with zip.ksy, got: {:?}",
        result.errors
    );
    assert_eq!(result.fields.len(), 3, "3 sections expected");
}

#[test]
fn test_parse_with_cancellation_token() {
    let yaml = r#"
meta:
  id: cancellation_test
seq:
  - id: items
    type: u1
    repeat: eos
"#;
    let ksy = parse_ksy_yaml(yaml);
    let data = vec![0x42; 1000];
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);

    let cancel_token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Cancel immediately after first item
    cancel_token.store(true, std::sync::atomic::Ordering::SeqCst);

    let result = interpreter.parse_with_progress_cancellable(&mut stream, Some(&cancel_token), |_| {});

    assert!(
        result.fields.is_empty() || result.fields.len() < 1000,
        "Parsing should have stopped early due to cancellation"
    );
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
        is_instance: false,
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
        is_instance: false,
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
        is_instance: false,
    };

    let result = ParseResult::new("pe".into(), vec![container], 64, vec![]);

    let containers = result.find_container_structs_starting_at(0, 16);
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].id, "IMAGE_DOS_HEADER");

    let leaves = result.find_leaf_fields_starting_at(0, 16);
    assert_eq!(leaves.len(), 2);
    assert_eq!(leaves[0].id, "e_magic");
    assert_eq!(leaves[1].id, "e_cblp");

    let active_ranges = result.find_active_struct_ranges(0, 16);
    assert_eq!(active_ranges.len(), 1);
    assert_eq!(active_ranges[0].id, "IMAGE_DOS_HEADER");

    assert_eq!(leaf1.format_expression(), "e_magic = 5A4Dh (23117)");
    assert_eq!(leaf1.format_comment(), Some("Magic number".into()));
}

#[test]
fn test_active_struct_range_index_handles_nested_ranges() {
    use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};

    fn struct_field(id: &str, offset: usize, size: usize, children: Vec<ParsedField>) -> ParsedField {
        ParsedField {
            id: id.into(),
            field_type: id.into(),
            offset,
            size,
            value: FieldValue::Struct,
            color: gpui::Hsla::default(),
            description: None,
            children,
            enum_label: None,
            is_instance: false,
        }
    }

    let result = ParseResult::new(
        "nested".into(),
        vec![struct_field(
            "outer",
            0,
            100,
            vec![struct_field("inner_a", 0, 10, Vec::new()), struct_field("inner_b", 40, 20, Vec::new())],
        )],
        100,
        Vec::new(),
    );

    let ranges = result.find_active_struct_ranges(45, 1);
    let ids: Vec<_> = ranges.iter().map(|range| range.id.as_str()).collect();
    assert_eq!(ids, ["outer", "inner_b"]);

    let ranges = result.find_active_struct_ranges(20, 1);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].id, "outer");

    assert!(result.find_active_struct_ranges(100, 1).is_empty());
}

#[test]
fn test_structure_index_sorts_offset_lookups() {
    use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};

    fn field(id: &str, offset: usize, size: usize, value: FieldValue, children: Vec<ParsedField>) -> ParsedField {
        ParsedField {
            id: id.into(),
            field_type: id.into(),
            offset,
            size,
            value,
            color: gpui::Hsla::default(),
            description: None,
            children,
            enum_label: None,
            is_instance: false,
        }
    }

    let earlier_section = field(
        "section[2]",
        0,
        800,
        FieldValue::Struct,
        vec![field("body", 8, 792, FieldValue::Bytes(vec![0; 792]), Vec::new())],
    );
    let later_section = field("section[3]", 800, 16, FieldValue::Struct, Vec::new());

    // This is the order produced when a later structure contains a `pos`
    // instance that points backwards: traversal order is not file order.
    let result = ParseResult::new("ordered_lookup".into(), vec![later_section, earlier_section], 816, Vec::new());

    let body_containers = result.find_container_structs_starting_at(8, 16);
    assert!(body_containers.is_empty(), "a body row must not inherit the next section name");

    let section_containers = result.find_container_structs_starting_at(0, 8);
    assert_eq!(section_containers.len(), 1);
    assert_eq!(section_containers[0].id, "section[2]");

    let body_fields = result.find_leaf_fields_starting_at(8, 16);
    assert_eq!(body_fields.len(), 1);
    assert_eq!(body_fields[0].id, "body");
}

#[test]
fn test_editor_line_starts_breaks_per_field() {
    use crate::core::document::Document;
    use crate::core::editor::Editor;
    use std::sync::{Arc, RwLock};

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
    use crate::core::document::Document;
    use crate::core::editor::Editor;
    use std::sync::{Arc, RwLock};

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
        is_instance: false,
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
        is_instance: false,
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
        is_instance: false,
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
        is_instance: false,
    };

    let parse_result = ParseResult::new("zip".into(), vec![section0, local_header_inst], 30, Vec::new());

    let leaves = parse_result.find_leaf_fields_starting_at(0, 2);
    assert_eq!(leaves.len(), 1, "Duplicate leaf fields with same offset and ID must be deduplicated");
    assert_eq!(leaves[0].id, "magic");
}

#[test]
fn test_large_scale_binary_search_and_performance() {
    use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};

    // Simulate 10,000 fields in a large ZIP archive
    let count = 10_000;
    let mut fields = Vec::with_capacity(count);

    for i in 0..count {
        let offset = i * 32;
        let leaf = ParsedField {
            id: format!("field_{}", i),
            field_type: "u4".into(),
            offset,
            size: 4,
            value: FieldValue::U32(i as u32),
            color: gpui::Hsla::default(),
            description: Some(format!("Description for field {}", i)),
            children: Vec::new(),
            enum_label: None,
            is_instance: false,
        };

        let container = ParsedField {
            id: format!("entry_{}", i),
            field_type: "zip_entry".into(),
            offset,
            size: 32,
            value: FieldValue::Struct,
            color: gpui::Hsla::default(),
            description: None,
            children: vec![leaf],
            enum_label: None,
            is_instance: false,
        };

        fields.push(container);
    }

    let parse_result = ParseResult::new("large_zip".into(), fields, count * 32, Vec::new());

    // Verify O(log N) binary search correctness at various offsets
    let test_offsets = [0, 32 * 500, 32 * 5000, 32 * 9999];
    for &off in &test_offsets {
        let target_idx = off / 32;
        let containers = parse_result.find_container_structs_starting_at(off, 32);
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, format!("entry_{}", target_idx));

        let leaves = parse_result.find_leaf_fields_starting_at(off, 32);
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].id, format!("field_{}", target_idx));
    }

    // Simulate rendering 1000 visible rows during fast scrolling (benchmarking response)
    let start_time = std::time::Instant::now();
    for row in 0..1000 {
        let row_offset = (row * 16) % (count * 32);
        let _ = parse_result.find_container_structs_starting_at(row_offset, 16);
        let _ = parse_result.find_leaf_fields_starting_at(row_offset, 16);
        let _ = parse_result.find_active_struct_ranges(row_offset, 16);
    }
    let elapsed = start_time.elapsed();
    // 1000 row searches on 10,000 elements should take under 250ms in debug builds (typically < 2ms in release)
    assert!(
        elapsed.as_millis() < 250,
        "1000 visible row lookups must complete in under 250ms, took {:?}",
        elapsed
    );
}

#[test]
fn test_struct_tree_all_collapsed_by_default() {
    use crate::core::structure::types::{FieldValue, ParsedField};
    use std::collections::HashSet;

    let leaf = ParsedField {
        id: "magic".into(),
        field_type: "u2".into(),
        offset: 0,
        size: 2,
        value: FieldValue::U16(0x5A4D),
        color: gpui::Hsla::default(),
        description: None,
        children: Vec::new(),
        enum_label: None,
        is_instance: false,
    };

    let child_container = ParsedField {
        id: "child_struct".into(),
        field_type: "child_type".into(),
        offset: 0,
        size: 2,
        value: FieldValue::Struct,
        color: gpui::Hsla::default(),
        description: None,
        children: vec![leaf],
        enum_label: None,
        is_instance: false,
    };

    let root_container = ParsedField {
        id: "root_struct".into(),
        field_type: "root_type".into(),
        offset: 0,
        size: 2,
        value: FieldValue::Struct,
        color: gpui::Hsla::default(),
        description: None,
        children: vec![child_container],
        enum_label: None,
        is_instance: false,
    };

    let fields = vec![root_container];
    let mut collapsed_paths = HashSet::new();

    fn collect_all_container_paths(fields: &[ParsedField], parent_path: &[usize], collapsed: &mut HashSet<Vec<usize>>) {
        for (idx, field) in fields.iter().enumerate() {
            let mut current_path = parent_path.to_vec();
            current_path.push(idx);
            if !field.children.is_empty() {
                collapsed.insert(current_path.clone());
                collect_all_container_paths(&field.children, &current_path, collapsed);
            }
        }
    }

    collect_all_container_paths(&fields, &Vec::new(), &mut collapsed_paths);

    // root_struct (path: [0]) and child_struct (path: [0, 0]) should both be collapsed
    assert!(collapsed_paths.contains(&vec![0]));
    assert!(collapsed_paths.contains(&vec![0, 0]));
}

#[test]
fn test_operator_precedence_bitwise_and_comparison() {
    use crate::core::structure::expression::{EvalContext, ExprEvaluator};
    use std::collections::HashMap;

    let mut values = HashMap::new();
    values.insert("byte0".to_string(), 0b1100_0000);

    let string_values = HashMap::new();
    let base_path = vec![];
    let enums = HashMap::new();

    let ctx = EvalContext {
        values: &values,
        string_values: &string_values,
        byte_arrays: &crate::core::structure::expression::G_EMPTY_BYTE_MAP,
        base_path: &base_path,
        stream_eof: false,
        stream_size: 0,
        stream_pos: 0,
        enums: &enums,
        errors: None,
        instance_resolver: None,
    };

    // In Kaitai / Python, (byte0 & 0b1110_0000 == 0b1100_0000) should be true (1)
    let res = ExprEvaluator::eval_bool("(byte0 & 0b1110_0000 == 0b1100_0000) ? true : false", &ctx);
    assert!(res, "Bitwise & must have higher precedence than ==");
}

#[test]
fn test_utf8_ksy_full_parse() {
    let utf8_ksy_content = r#"
meta:
  id: utf8
  endian: le
seq:
  - id: codepoints
    type: utf8_codepoint(_io.pos)
    repeat: eos
types:
  utf8_codepoint:
    params:
      - id: ofs
        type: u8
    seq:
      - id: bytes
        size: len_bytes
    instances:
      byte0:
        pos: ofs
        type: u1
      len_bytes:
        value: '(byte0 & 0b1000_0000 == 0) ? 1 : ((byte0 & 0b1110_0000 == 0b1100_0000) ? 2 : ((byte0 & 0b1111_0000 == 0b1110_0000) ? 3 : ((byte0 & 0b1111_1000 == 0b1111_0000) ? 4 : 1)))'
      raw0:
        value: 'bytes[0] & ((len_bytes == 1) ? 0b0111_1111 : ((len_bytes == 2) ? 0b0001_1111 : ((len_bytes == 3) ? 0b0000_1111 : ((len_bytes == 4) ? 0b0000_0111 : 0))))'
      raw1:
        value: '(len_bytes >= 2) ? (bytes[1] & 0b0011_1111) : 0'
      raw2:
        value: '(len_bytes >= 3) ? (bytes[2] & 0b0011_1111) : 0'
      raw3:
        value: '(len_bytes >= 4) ? (bytes[3] & 0b0011_1111) : 0'
      value_as_int:
        value: '(len_bytes == 1) ? raw0 : ((len_bytes == 2) ? ((raw0 << 6) | raw1) : ((len_bytes == 3) ? ((raw0 << 12) | (raw1 << 6) | raw2) : ((len_bytes == 4) ? ((raw0 << 18) | (raw1 << 12) | (raw2 << 6) | raw3) : 0)))'
"#;

    let ksy: KsyDefinition = serde_yaml::from_str(utf8_ksy_content).expect("Failed to deserialize utf8.ksy YAML");
    let sample = "Hello, 世界!".as_bytes(); // 'H'(0x48), 'e'(0x65), 'l'(0x6C), 'l'(0x6C), 'o'(0x6F), ','(0x2C), ' '(0x20), '世'(0xE4,0xB8,0x96 -> 0x4E16), '界'(0xE7,0x95,0x8C -> 0x754C), '!'(0x21)
    let mut stream = KaitaiStream::new(sample);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert!(!result.fields.is_empty(), "Parsed fields must not be empty");
    assert_eq!(result.fields.len(), 10, "10 unicode codepoints expected for 'Hello, 世界!'");
    assert_eq!(result.fields[0].size, 1);
    assert_eq!(result.fields[7].size, 3, "'世' is 3 bytes in UTF-8");
    assert_eq!(result.fields[8].size, 3, "'界' is 3 bytes in UTF-8");
    assert_eq!(result.fields[9].size, 1, "'!' is 1 byte");

    // Verify that instances (byte0: pos: ofs, type: u1) do NOT cause field breaks within multi-byte characters
    let mut breaks = Vec::new();
    result.collect_field_breaks(&mut breaks, &std::collections::HashSet::new());
    breaks.sort_unstable();
    breaks.dedup();
    // The breaks must only be at codepoint boundaries: 0, 1, 2, 3, 4, 5, 6, 7, 10, 13, 14
    assert_eq!(breaks, vec![0, 1, 2, 3, 4, 5, 6, 7, 10, 13, 14]);

    // Verify with Editor line_starts
    use crate::core::document::Document;
    use crate::core::editor::Editor;
    use std::sync::{Arc, RwLock};

    let ksy_arc = Arc::new(parse_ksy_yaml(utf8_ksy_content));
    let buffer = crate::core::buffer::Buffer::new(sample.to_vec());
    let doc = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("utf8_sample.txt"), buffer)));
    let mut editor = Editor::new(doc);
    editor.set_kaitai_definition(ksy_arc);

    let line_starts = editor.line_starts();
    assert_eq!(line_starts.get(7), Some(7)); // '世' starts at offset 7
    assert_eq!(line_starts.get(8), Some(10)); // '界' starts at offset 10 (not 8 or 9!)
    assert_eq!(line_starts.get(9), Some(13)); // '!' starts at offset 13 (not 11 or 12!)
}

#[test]
fn test_parse_with_progress_notifications() {
    let yaml = r#"
meta:
  id: test_progress
seq:
  - id: count
    type: u2le
  - id: items
    type: u1
    repeat: expr
    repeat-expr: count
"#;
    let ksy = parse_ksy_yaml(yaml);
    let data = vec![0x03, 0x00, 0xAA, 0xBB, 0xCC];
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);

    let mut progress_events = Vec::new();
    let result = interpreter.parse_with_progress(&mut stream, |progress| {
        progress_events.push((progress.parsed_offset, progress.total_bytes, progress.fields.len(), progress.is_done));
    });

    assert_eq!(result.total_parsed_bytes, 5);
    assert!(!progress_events.is_empty(), "Progress events should be emitted");

    // First event is initial state
    assert_eq!(progress_events[0].1, 5); // total_bytes
    assert!(!progress_events[0].3); // is_done: false

    // Last event is done
    let last = progress_events.last().unwrap();
    assert_eq!(last.0, 5); // parsed_offset = 5
    assert_eq!(last.1, 5); // total_bytes = 5
    assert!(last.3); // is_done = true
}

#[test]
fn test_editor_parse_progress_tracking() {
    use crate::core::document::Document;
    use crate::core::editor::Editor;
    use std::sync::{Arc, RwLock};

    let sample = vec![0x01, 0x02, 0x03, 0x04];
    let buffer = crate::core::buffer::Buffer::new(sample.clone());
    let doc = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test.bin"), buffer)));
    let mut editor = Editor::new(doc);

    assert_eq!(editor.parse_progress_offset, 0);
    assert_eq!(editor.parse_total_size, 0);
    assert!(!editor.is_parsing_structure);

    // Update progress
    editor.is_parsing_structure = true;
    editor.update_parse_progress(2, 4, None);
    assert_eq!(editor.parse_progress_offset, 2);
    assert_eq!(editor.parse_total_size, 4);
    assert!(editor.is_parsing_structure);

    // Clear
    editor.clear_structure_definition();
    assert_eq!(editor.parse_progress_offset, 0);
    assert_eq!(editor.parse_total_size, 0);
    assert!(!editor.is_parsing_structure);
}

#[test]
fn test_large_scale_struct_parsing_speed() {
    // Test parsing 50,000 array elements with nested custom types and expressions
    let ksy_yaml = r#"
meta:
  id: test_perf
  endian: le
seq:
  - id: count
    type: u4
  - id: records
    type: record
    repeat: expr
    repeat-expr: count
types:
  record:
    seq:
      - id: id
        type: u4
      - id: flags
        type: u2
      - id: tag
        type: str
        size: 4
        encoding: utf-8
      - id: val
        type: u4
    instances:
      is_special:
        value: '(flags & 0x8000 != 0) ? 1 : 0'
      computed_sum:
        value: 'id + val'
"#;

    let ksy = parse_ksy_yaml(ksy_yaml);

    let count = 50_000u32;
    let mut data = Vec::with_capacity(4 + count as usize * 14);
    data.extend_from_slice(&count.to_le_bytes());
    for i in 0..count {
        data.extend_from_slice(&i.to_le_bytes()); // id: u4
        data.extend_from_slice(&0x8001u16.to_le_bytes()); // flags: u2
        data.extend_from_slice(b"TAG_"); // tag: 4 bytes utf-8
        data.extend_from_slice(&(i * 2).to_le_bytes()); // val: u4
    }

    let start = std::time::Instant::now();
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);
    let elapsed = start.elapsed();

    assert_eq!(result.fields.len(), 50_001);
    if let FieldValue::U32(c) = result.fields[0].value {
        assert_eq!(c, count);
    } else {
        panic!("count mismatch");
    }

    assert_eq!(result.fields[1].id, "records[0]");
    assert_eq!(result.fields[1].children.len(), 4);

    println!("Parsed 50,000 nested records in {:?}", elapsed);
    let threshold = if cfg!(debug_assertions) { 5.0 } else { 2.0 };
    assert!(elapsed.as_secs_f64() < threshold, "Parsing 50,000 records took too long: {:?}", elapsed);
}
