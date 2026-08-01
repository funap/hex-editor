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
}
