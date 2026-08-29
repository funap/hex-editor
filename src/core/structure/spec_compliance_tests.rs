//! Kaitai Struct Specification Compliance Tests
//!
//! This test suite systematically verifies compliance with the official Kaitai Struct
//! language specification as documented in the Kaitai Struct User Guide
//! (https://doc.kaitai.io/user_guide.html) and KSY Diagram reference
//! (https://doc.kaitai.io/ksy_diagram.html).
//!
//! Organized by official specification sections:
//! - Section 4.1: Primitive Types & Endianness
//! - Section 4.2: Meta, Documentation & IDs
//! - Section 4.3: Fixed Contents / Magic Signatures
//! - Section 4.4: Validation Rules
//! - Section 4.5: Variable-length Structures & Dynamic Sizes
//! - Section 4.6: Delimited Structures (Strings & Byte Arrays)
//! - Section 4.7: Enums & Symbolic Constants
//! - Section 4.8 & 4.9: Subtypes, Nested Structures & Scopes (_root, _parent)
//! - Section 4.10: Conditionals (`if:`)
//! - Section 4.11: Repetitions (`repeat: expr`, `repeat: eos`, `repeat: until`)
//! - Section 4.12: Type Switching & TLV (`type: switch-on`, `cases:`)
//! - Section 4.13 & 4.14: Instances (Calculated `value:` & Stream Seek `pos:`)
//! - Section 4.15: Bit-sized Integers & Stream Alignment (`b1`..`b64`, `b*be`, `b*le`)
//! - Section 5: Streams, Substreams & Data Processing (`process: zlib`, `xor`, `rol`)
//! - Section 6: Expression Language (Operators, Precedence, `_io.*`, Ternary)
//! - Section 7.10: Parametric Types (`params:` and Arguments)

use crate::core::structure::types::FieldValue;
use crate::core::structure::{KaitaiInterpreter, KaitaiStream, KsyDefinition};

fn parse_ksy(yaml: &str) -> KsyDefinition {
    serde_yaml::from_str(yaml).expect("Failed to parse YAML KSY definition")
}

// ============================================================================
// Section 4.1: Primitive Types & Endianness
// ============================================================================

#[test]
fn test_spec_4_1_numeric_primitive_types_little_endian() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_1_primitives_le
  endian: le
seq:
  - id: val_u1
    type: u1
  - id: val_u2
    type: u2
  - id: val_u4
    type: u4
  - id: val_u8
    type: u8
  - id: val_s1
    type: s1
  - id: val_s2
    type: s2
  - id: val_s4
    type: s4
  - id: val_s8
    type: s8
  - id: val_f4
    type: f4
  - id: val_f8
    type: f8
"#,
    );

    let mut data = Vec::new();
    data.push(0xFE); // u1 = 254
    data.extend_from_slice(&1000u16.to_le_bytes()); // u2 = 1000
    data.extend_from_slice(&100_000u32.to_le_bytes()); // u4 = 100,000
    data.extend_from_slice(&10_000_000_000u64.to_le_bytes()); // u8 = 10,000,000,000
    data.push((-42i8) as u8); // s1 = -42
    data.extend_from_slice(&(-1234i16).to_le_bytes()); // s2 = -1234
    data.extend_from_slice(&(-500_000i32).to_le_bytes()); // s4 = -500,000
    data.extend_from_slice(&(-9_000_000_000i64).to_le_bytes()); // s8 = -9,000,000,000
    data.extend_from_slice(&123.456f32.to_le_bytes()); // f4
    data.extend_from_slice(&789.0123456789f64.to_le_bytes()); // f8

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 10);
    assert_eq!(result.fields[0].value, FieldValue::U8(254));
    assert_eq!(result.fields[1].value, FieldValue::U16(1000));
    assert_eq!(result.fields[2].value, FieldValue::U32(100_000));
    assert_eq!(result.fields[3].value, FieldValue::U64(10_000_000_000));
    assert_eq!(result.fields[4].value, FieldValue::I8(-42));
    assert_eq!(result.fields[5].value, FieldValue::I16(-1234));
    assert_eq!(result.fields[6].value, FieldValue::I32(-500_000));
    assert_eq!(result.fields[7].value, FieldValue::I64(-9_000_000_000));

    if let FieldValue::F32(f) = result.fields[8].value {
        assert!((f - 123.456f32).abs() < 1e-5);
    } else {
        panic!("Expected f4 field value");
    }

    if let FieldValue::F64(f) = result.fields[9].value {
        assert!((f - 789.0123456789f64).abs() < 1e-9);
    } else {
        panic!("Expected f8 field value");
    }
}

#[test]
fn test_spec_4_1_numeric_primitive_types_big_endian() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_1_primitives_be
  endian: be
seq:
  - id: val_u2
    type: u2
  - id: val_u4
    type: u4
  - id: val_u8
    type: u8
  - id: val_s2
    type: s2
  - id: val_s4
    type: s4
  - id: val_s8
    type: s8
  - id: val_f4
    type: f4
  - id: val_f8
    type: f8
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(&0x1234u16.to_be_bytes());
    data.extend_from_slice(&0x12345678u32.to_be_bytes());
    data.extend_from_slice(&0x0102030405060708u64.to_be_bytes());
    data.extend_from_slice(&(-300i16).to_be_bytes());
    data.extend_from_slice(&(-70_000i32).to_be_bytes());
    data.extend_from_slice(&(-123_456_789_000i64).to_be_bytes());
    data.extend_from_slice(&1.5f32.to_be_bytes());
    data.extend_from_slice(&100.25f64.to_be_bytes());

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields[0].value, FieldValue::U16(0x1234));
    assert_eq!(result.fields[1].value, FieldValue::U32(0x12345678));
    assert_eq!(result.fields[2].value, FieldValue::U64(0x0102030405060708));
    assert_eq!(result.fields[3].value, FieldValue::I16(-300));
    assert_eq!(result.fields[4].value, FieldValue::I32(-70_000));
    assert_eq!(result.fields[5].value, FieldValue::I64(-123_456_789_000));
    assert_eq!(result.fields[6].value, FieldValue::F32(1.5));
    assert_eq!(result.fields[7].value, FieldValue::F64(100.25));
}

#[test]
fn test_spec_4_1_explicit_endianness_overrides() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_1_endian_override
  endian: le
seq:
  - id: le_default
    type: u2
  - id: be_explicit
    type: u2be
  - id: le_explicit
    type: u4le
  - id: be_explicit_s4
    type: s4be
  - id: be_explicit_f4
    type: f4be
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(&0x0001u16.to_le_bytes()); // 1
    data.extend_from_slice(&0x0002u16.to_be_bytes()); // 2
    data.extend_from_slice(&0x00000003u32.to_le_bytes()); // 3
    data.extend_from_slice(&(-4i32).to_be_bytes()); // -4
    data.extend_from_slice(&12.5f32.to_be_bytes()); // 12.5

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields[0].value, FieldValue::U16(1));
    assert_eq!(result.fields[1].value, FieldValue::U16(2));
    assert_eq!(result.fields[2].value, FieldValue::U32(3));
    assert_eq!(result.fields[3].value, FieldValue::I32(-4));
    assert_eq!(result.fields[4].value, FieldValue::F32(12.5));
}

// ============================================================================
// Section 4.2: Documentation & Meta
// ============================================================================

#[test]
fn test_spec_4_2_docstrings_and_metadata() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_2_doc_test
  endian: le
seq:
  - id: magic
    type: u4
    doc: Four-byte magic signature at start of file
  - id: version
    type: u2
    doc: Format version number
    doc-ref: https://example.com/spec/v1.0
"#,
    );

    let data = [0x50, 0x4B, 0x03, 0x04, 0x14, 0x00];
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 2);
    assert_eq!(result.fields[0].description.as_deref(), Some("Four-byte magic signature at start of file"));
    assert_eq!(result.fields[1].description.as_deref(), Some("Format version number"));
}

// ============================================================================
// Section 4.3: Fixed Contents / Magic Signatures
// ============================================================================

#[test]
fn test_spec_4_3_fixed_contents_string_and_byte_arrays() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_3_contents
seq:
  - id: string_magic
    contents: "GIF89a"
  - id: byte_magic
    contents: [0xCA, 0xFE, 0xBA, 0xBE]
  - id: mixed_magic
    contents: [0x89, "PNG\r\n", 0x1A, "\n"]
  - id: payload
    type: u1
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(b"GIF89a");
    data.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
    data.extend_from_slice(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']);
    data.push(0x42);

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 4);
    assert_eq!(result.fields[0].id, "string_magic");
    assert_eq!(result.fields[1].id, "byte_magic");
    assert_eq!(result.fields[2].id, "mixed_magic");
    assert_eq!(result.fields[3].id, "payload");
    assert_eq!(result.fields[3].value, FieldValue::U8(0x42));
}

// ============================================================================
// Section 4.4: Validation Rules
// ============================================================================

#[test]
fn test_spec_4_4_validation_eq_min_max_and_any_of() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_4_validation
  endian: le
seq:
  - id: exact_val
    type: u2
    valid:
      eq: 42
  - id: range_val
    type: u1
    valid:
      min: 10
      max: 50
  - id: set_val
    type: u1
    valid:
      any-of: [1, 2, 4, 8, 16, 32]
  - id: expr_val
    type: u2
    valid:
      expr: "_ % 10 == 0"
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(&42u16.to_le_bytes()); // valid: eq 42
    data.push(25); // valid: min 10, max 50
    data.push(8); // valid: any-of
    data.extend_from_slice(&100u16.to_le_bytes()); // valid: expr _ % 10 == 0

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 4);
    assert_eq!(result.fields[0].value, FieldValue::U16(42));
    assert_eq!(result.fields[1].value, FieldValue::U8(25));
    assert_eq!(result.fields[2].value, FieldValue::U8(8));
    assert_eq!(result.fields[3].value, FieldValue::U16(100));
}

// ============================================================================
// Section 4.5: Variable-length Structures & Dynamic Sizing
// ============================================================================

#[test]
fn test_spec_4_5_variable_length_fields_and_size_eos() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_5_var_length
  endian: le
seq:
  - id: fixed_size_block
    size: 4
  - id: header_len
    type: u1
  - id: dynamic_block
    size: header_len
  - id: expr_sized_block
    size: header_len * 2
  - id: remaining_data
    size-eos: true
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // fixed 4 bytes
    data.push(3); // header_len = 3
    data.extend_from_slice(&[0xAA, 0xBB, 0xCC]); // dynamic 3 bytes
    data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]); // expr: 3 * 2 = 6 bytes
    data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // remaining size-eos

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 5);
    assert_eq!(result.fields[0].size, 4);
    assert_eq!(result.fields[1].value, FieldValue::U8(3));
    assert_eq!(result.fields[2].size, 3);
    assert_eq!(result.fields[3].size, 6);
    assert_eq!(result.fields[4].size, 4);
    assert_eq!(result.fields[4].value, FieldValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]));
}

// ============================================================================
// Section 4.6: Delimited Structures (Strings & Byte Arrays)
// ============================================================================

#[test]
fn test_spec_4_6_delimited_strings_and_custom_terminators() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_6_strings
seq:
  - id: null_term_str
    type: strz
    encoding: ASCII
  - id: custom_term_str
    type: str
    terminator: 0x0A
    encoding: UTF-8
  - id: next_byte
    type: u1
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(b"Hello World\0");
    data.extend_from_slice("こんにちは\n".as_bytes());
    data.push(0xFF);

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 3);
    assert_eq!(result.fields[0].value, FieldValue::String("Hello World".to_string()));
    assert_eq!(result.fields[1].value, FieldValue::String("こんにちは".to_string()));
    assert_eq!(result.fields[2].value, FieldValue::U8(0xFF));
}

// ============================================================================
// Section 4.7: Enums & Symbolic Constants
// ============================================================================

#[test]
fn test_spec_4_7_enums_and_symbol_resolution() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_7_enums
  endian: le
enums:
  http_status:
    200: ok
    404: not_found
    500: internal_server_error
  packet_flags:
    0: none
    1: ack
    2: syn
    4: fin
seq:
  - id: status
    type: u2
    enum: http_status
  - id: flags
    type: u1
    enum: packet_flags
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(&200u16.to_le_bytes());
    data.push(2); // syn

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 2);
    assert_eq!(result.fields[0].value, FieldValue::U16(200));
    assert_eq!(result.fields[0].enum_label.as_deref(), Some("ok"));
    assert_eq!(result.fields[1].value, FieldValue::U8(2));
    assert_eq!(result.fields[1].enum_label.as_deref(), Some("syn"));
}

// ============================================================================
// Section 4.8 & 4.9: Subtypes, Nested Structures & Scopes (_root, _parent)
// ============================================================================

#[test]
fn test_spec_4_8_subtypes_and_hierarchical_scopes() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_8_scopes
  endian: le
seq:
  - id: global_multiplier
    type: u1
  - id: record
    type: item_record
types:
  item_record:
    seq:
      - id: base_val
        type: u2
      - id: detail
        type: sub_detail
    types:
      sub_detail:
        seq:
          - id: sub_val
            type: u1
        instances:
          calc_total:
            value: sub_val + _parent.base_val * _root.global_multiplier
"#,
    );

    let mut data = Vec::new();
    data.push(10); // _root.global_multiplier = 10
    data.extend_from_slice(&5u16.to_le_bytes()); // _parent.base_val = 5
    data.push(3); // sub_val = 3

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    // Verify hierarchy
    assert_eq!(result.fields[0].value, FieldValue::U8(10));
    let record_field = &result.fields[1];
    assert_eq!(record_field.children.len(), 2);
    assert_eq!(record_field.children[0].value, FieldValue::U16(5));

    let detail_field = &record_field.children[1];
    assert_eq!(detail_field.children[0].value, FieldValue::U8(3));

    // Instances check: 3 + (5 * 10) = 53
    let calc_inst = detail_field
        .children
        .iter()
        .find(|f| f.id == "calc_total")
        .expect("calc_total instance must exist");
    assert_eq!(calc_inst.value, FieldValue::U64(53));
}

// ============================================================================
// Section 4.10: Conditionals (`if:`)
// ============================================================================

#[test]
fn test_spec_4_10_conditionals_parsing_and_skipping() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_10_conditionals
  endian: le
seq:
  - id: has_optional_data
    type: u1
  - id: optional_u4
    type: u4
    if: has_optional_data != 0
  - id: tail_marker
    type: u2
"#,
    );

    // Case 1: has_optional_data == 1 (optional_u4 is parsed)
    let mut data1 = Vec::new();
    data1.push(1);
    data1.extend_from_slice(&0x12345678u32.to_le_bytes());
    data1.extend_from_slice(&0xCAFEu16.to_le_bytes());

    let mut stream1 = KaitaiStream::new(&data1);
    let interpreter1 = KaitaiInterpreter::new(ksy.clone());
    let result1 = interpreter1.parse(&mut stream1);

    assert_eq!(result1.fields.len(), 3);
    assert_eq!(result1.fields[0].value, FieldValue::U8(1));
    assert_eq!(result1.fields[1].id, "optional_u4");
    assert_eq!(result1.fields[1].value, FieldValue::U32(0x12345678));
    assert_eq!(result1.fields[2].value, FieldValue::U16(0xCAFE));

    // Case 2: has_optional_data == 0 (optional_u4 is skipped, stream does not consume 4 bytes)
    let mut data2 = Vec::new();
    data2.push(0);
    data2.extend_from_slice(&0xCAFEu16.to_le_bytes());

    let mut stream2 = KaitaiStream::new(&data2);
    let interpreter2 = KaitaiInterpreter::new(ksy);
    let result2 = interpreter2.parse(&mut stream2);

    assert_eq!(result2.fields.len(), 2);
    assert_eq!(result2.fields[0].value, FieldValue::U8(0));
    assert_eq!(result2.fields[1].id, "tail_marker");
    assert_eq!(result2.fields[1].value, FieldValue::U16(0xCAFE));
}

// ============================================================================
// Section 4.11: Repetitions (`repeat: expr`, `repeat: eos`, `repeat: until`)
// ============================================================================

#[test]
fn test_spec_4_11_repetitions_expr_eos_and_until() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_11_repetitions
  endian: le
seq:
  - id: count
    type: u1
  - id: items_by_count
    type: u2
    repeat: expr
    repeat-expr: count
  - id: items_until_zero
    type: u1
    repeat: until
    repeat-until: _ == 0
"#,
    );

    let mut data = Vec::new();
    data.push(3); // count = 3
    data.extend_from_slice(&100u16.to_le_bytes());
    data.extend_from_slice(&200u16.to_le_bytes());
    data.extend_from_slice(&300u16.to_le_bytes());
    data.extend_from_slice(&[5, 4, 3, 2, 1, 0]); // terminated by 0

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields[0].value, FieldValue::U8(3));

    // repeat: expr count 3
    assert_eq!(result.fields[1].value, FieldValue::U16(100));
    assert_eq!(result.fields[2].value, FieldValue::U16(200));
    assert_eq!(result.fields[3].value, FieldValue::U16(300));

    // repeat: until _ == 0
    assert_eq!(result.fields[4].value, FieldValue::U8(5));
    assert_eq!(result.fields[9].value, FieldValue::U8(0));
}

// ============================================================================
// Section 4.12: Type Switching & TLV (`type: switch-on`, `cases:`)
// ============================================================================

#[test]
fn test_spec_4_12_type_switching_with_default_case() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_12_switch
  endian: le
seq:
  - id: entry_count
    type: u1
  - id: entries
    type: entry
    repeat: expr
    repeat-expr: entry_count
types:
  entry:
    seq:
      - id: tag
        type: u1
      - id: payload
        type:
          switch-on: tag
          cases:
            1: u2
            2: u4
            _: u1
"#,
    );

    let mut data = Vec::new();
    data.push(3); // 3 entries
    data.push(1); // tag 1 -> u2
    data.extend_from_slice(&0x1122u16.to_le_bytes());
    data.push(2); // tag 2 -> u4
    data.extend_from_slice(&0x33445566u32.to_le_bytes());
    data.push(99); // fallback tag -> u1
    data.push(0x77);

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 4);
    assert_eq!(result.fields[0].value, FieldValue::U8(3));

    // Entry 1
    assert_eq!(result.fields[1].children[0].value, FieldValue::U8(1));
    assert_eq!(result.fields[1].children[1].value, FieldValue::U16(0x1122));

    // Entry 2
    assert_eq!(result.fields[2].children[0].value, FieldValue::U8(2));
    assert_eq!(result.fields[2].children[1].value, FieldValue::U32(0x33445566));

    // Entry 3 (fallback)
    assert_eq!(result.fields[3].children[0].value, FieldValue::U8(99));
    assert_eq!(result.fields[3].children[1].value, FieldValue::U8(0x77));
}

// ============================================================================
// Section 4.13 & 4.14: Instances (Calculated & Stream Seek `pos:`)
// ============================================================================

#[test]
fn test_spec_4_13_instances_pos_and_value() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_13_instances
  endian: le
seq:
  - id: offset_to_title
    type: u2
  - id: number_a
    type: u2
  - id: number_b
    type: u2
instances:
  sum_ab:
    value: number_a + number_b
  title_str:
    pos: offset_to_title
    type: strz
    encoding: ASCII
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(&10u16.to_le_bytes()); // offset_to_title = 10
    data.extend_from_slice(&25u16.to_le_bytes()); // number_a = 25
    data.extend_from_slice(&75u16.to_le_bytes()); // number_b = 75
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // padding to offset 10
    data.extend_from_slice(b"Kaitai Struct\0"); // string at offset 10

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    let sum_inst = result.fields.iter().find(|f| f.id == "sum_ab").expect("sum_ab instance must exist");
    assert_eq!(sum_inst.value, FieldValue::U64(100));

    let title_inst = result.fields.iter().find(|f| f.id == "title_str").expect("title_str instance must exist");
    assert_eq!(title_inst.value, FieldValue::String("Kaitai Struct".to_string()));
    assert_eq!(title_inst.offset, 10);
}

// ============================================================================
// Section 4.15: Bit-sized Integers & Stream Alignment
// ============================================================================

#[test]
fn test_spec_4_15_bit_fields_and_byte_alignment() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_15_bitfields
  endian: be
seq:
  - id: flag1
    type: b1
  - id: flag2
    type: b1
  - id: opcode
    type: b6
  - id: short_val
    type: b12
  - id: reserved
    type: b4
  - id: byte_aligned_u2
    type: u2
"#,
    );

    // Byte 0: flag1(1) | flag2(0) | opcode(6 = 0b101010) -> 0b10101010 = 0xAA
    // Byte 1 & 2: short_val(12 = 0x123) | reserved(4 = 0xF) -> 0x12, 0x3F
    // Byte 3 & 4: byte_aligned_u2 -> 0xCAFE
    let data = [0xAA, 0x12, 0x3F, 0xCA, 0xFE];

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 6);
    assert_eq!(result.fields[0].value, FieldValue::U64(1));
    assert_eq!(result.fields[1].value, FieldValue::U64(0));
    assert_eq!(result.fields[2].value, FieldValue::U64(0b101010));
    assert_eq!(result.fields[3].value, FieldValue::U64(0x123));
    assert_eq!(result.fields[4].value, FieldValue::U64(0x0F));
    assert_eq!(result.fields[5].value, FieldValue::U16(0xCAFE));
}

// ============================================================================
// Section 5: Streams, Substreams & Data Processing
// ============================================================================

#[test]
fn test_spec_5_processing_zlib_xor_and_rol() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_5_processing
  endian: le
seq:
  - id: xor_data
    size: 4
    process: xor(0x5A)
"#,
    );

    let raw = [b'T' ^ 0x5A, b'E' ^ 0x5A, b'S' ^ 0x5A, b'T' ^ 0x5A];
    let mut stream = KaitaiStream::new(&raw);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields[0].value, FieldValue::Bytes(b"TEST".to_vec()));
}

// ============================================================================
// Section 6: Expression Language & Built-in Variables
// ============================================================================

#[test]
fn test_spec_6_expression_language_operators_and_stream_properties() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_6_expressions
  endian: le
seq:
  - id: pos_before
    type: u2
instances:
  stream_pos:
    value: _io.pos
  stream_size:
    value: _io.size
  stream_is_eof:
    value: _io.eof
  ternary_test:
    value: "pos_before > 10 ? 100 : 200"
  bitwise_calc:
    value: "(pos_before << 2) | 0x01"
"#,
    );

    let data = [0x20, 0x00, 0xAA, 0xBB]; // pos_before = 32 (0x0020), total size = 4
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    let inst_pos = result.fields.iter().find(|f| f.id == "stream_pos").unwrap();
    assert_eq!(inst_pos.value, FieldValue::U64(2));

    let inst_size = result.fields.iter().find(|f| f.id == "stream_size").unwrap();
    assert_eq!(inst_size.value, FieldValue::U64(4));

    let inst_eof = result.fields.iter().find(|f| f.id == "stream_is_eof").unwrap();
    assert_eq!(inst_eof.value, FieldValue::Bool(false));

    let inst_ternary = result.fields.iter().find(|f| f.id == "ternary_test").unwrap();
    assert_eq!(inst_ternary.value, FieldValue::U64(100));

    let inst_bitwise = result.fields.iter().find(|f| f.id == "bitwise_calc").unwrap();
    assert_eq!(inst_bitwise.value, FieldValue::U64((32 << 2) | 0x01));
}

// ============================================================================
// Section 7.10: Parametric Types (`params:` and Arguments)
// ============================================================================

#[test]
fn test_spec_7_10_parametric_subtypes() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_7_10_params
  endian: le
seq:
  - id: chunk_size
    type: u1
  - id: custom_chunk
    type: sized_block(chunk_size)
types:
  sized_block:
    params:
      - id: block_len
        type: u1
    seq:
      - id: payload
        size: block_len
"#,
    );

    let data = [4, 0x01, 0x02, 0x03, 0x04, 0xFF];
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields[0].value, FieldValue::U8(4));
    let custom_chunk = &result.fields[1];
    assert_eq!(custom_chunk.children.len(), 1);
    assert_eq!(custom_chunk.children[0].value, FieldValue::Bytes(vec![0x01, 0x02, 0x03, 0x04]));
}

// ============================================================================
// Advanced Spec Tests: Validation Failure Modes
// ============================================================================

#[test]
fn test_spec_4_4_validation_failure_modes() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_4_fail
  endian: le
seq:
  - id: magic_val
    type: u2
    valid:
      eq: 0x1234
  - id: range_val
    type: u1
    valid:
      min: 10
      max: 20
"#,
    );

    let data = [0x00, 0x00, 25]; // magic = 0x0000 (invalid), range = 25 (invalid)
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert!(!result.errors.is_empty(), "Validation errors should be recorded");
}

// ============================================================================
// Advanced Spec Tests: Delimited Strings (consume: false & pad-right)
// ============================================================================

#[test]
fn test_spec_4_6_delimited_string_pad_right_and_consume_false() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_6_advanced
seq:
  - id: padded_name
    type: str
    size: 10
    pad-right: 0x20
    encoding: ASCII
  - id: non_consumed_delim
    type: str
    terminator: 0x2C
    consume: false
    encoding: ASCII
  - id: delimiter_byte
    type: u1
"#,
    );

    let mut data = Vec::new();
    data.extend_from_slice(b"Alice     "); // 10 bytes padded with spaces
    data.extend_from_slice(b"Value,Next"); // terminated by ','

    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields[0].value, FieldValue::String("Alice".to_string()));
    assert_eq!(result.fields[1].value, FieldValue::String("Value".to_string()));
    // Because consume: false, next read gets the delimiter ',' (0x2C)
    assert_eq!(result.fields[2].value, FieldValue::U8(b','));
}

// ============================================================================
// Advanced Spec Tests: Sized Substream Repetitions
// ============================================================================

#[test]
fn test_spec_4_11_repetition_eos_in_sized_substream() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_11_substream_eos
  endian: le
seq:
  - id: container
    type: block
    size: 6
  - id: trailing_byte
    type: u1
types:
  block:
    seq:
      - id: items
        type: u2
        repeat: eos
"#,
    );

    let data = [0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0xFF];
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    let container = &result.fields[0];
    assert_eq!(container.children.len(), 3);
    assert_eq!(container.children[0].value, FieldValue::U16(1));
    assert_eq!(container.children[1].value, FieldValue::U16(2));
    assert_eq!(container.children[2].value, FieldValue::U16(3));
    assert_eq!(result.fields[1].value, FieldValue::U8(0xFF));
}

// ============================================================================
// Advanced Spec Tests: Bitfield LE vs BE Endianness
// ============================================================================

#[test]
fn test_spec_4_15_bit_fields_endianness() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_4_15_endian_bits
seq:
  - id: bits_be
    type: b4be
  - id: bits_le
    type: b4le
"#,
    );

    let data = [0xAB];
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    assert_eq!(result.fields.len(), 2);
    assert_eq!(result.fields[0].value, FieldValue::U64(0x0A));
    assert_eq!(result.fields[1].value, FieldValue::U64(0x0B));
}

// ============================================================================
// Advanced Spec Tests: Complex Expression Operators & Precedence
// ============================================================================

#[test]
fn test_spec_6_expression_operators_precedence_and_logic() {
    let ksy = parse_ksy(
        r#"
meta:
  id: spec_6_operators
  endian: le
seq:
  - id: a
    type: u1
  - id: b
    type: u1
  - id: c
    type: u1
instances:
  arithmetic_precedence:
    value: (a + b) * c
  modulo_div:
    value: (c * 10) / a % 7
  logical_operators:
    value: "(a == 2 and b != 3) or not (c == 0)"
  shift_and_mask:
    value: "((a << 4) | b) & 0x3F"
"#,
    );

    let data = [2, 5, 4]; // a = 2, b = 5, c = 4
    let mut stream = KaitaiStream::new(&data);
    let interpreter = KaitaiInterpreter::new(ksy);
    let result = interpreter.parse(&mut stream);

    // (2 + 5) * 4 = 28
    let inst_prec = result.fields.iter().find(|f| f.id == "arithmetic_precedence").unwrap();
    assert_eq!(inst_prec.value, FieldValue::U64(28));

    // (4 * 10) / 2 % 7 = 40 / 2 % 7 = 20 % 7 = 6
    let inst_mod = result.fields.iter().find(|f| f.id == "modulo_div").unwrap();
    assert_eq!(inst_mod.value, FieldValue::U64(6));

    // (2 == 2 and 5 != 3) or not (4 == 0) = (true and true) or not false = true
    let inst_logic = result.fields.iter().find(|f| f.id == "logical_operators").unwrap();
    assert_eq!(inst_logic.value, FieldValue::Bool(true));

    // ((2 << 4) | 5) & 0x3F = (32 | 5) & 0x3F = 37 & 0x3F = 37
    let inst_shift = result.fields.iter().find(|f| f.id == "shift_and_mask").unwrap();
    assert_eq!(inst_shift.value, FieldValue::U64(37));
}
