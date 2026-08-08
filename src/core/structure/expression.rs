#![allow(dead_code)]

use std::collections::HashMap;

pub struct ExprEvaluator;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(i64),
    Float(f64),
    Identifier(String),
    String(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    NotEqual,
    GreaterEqual,
    LessEqual,
    Greater,
    Less,
    And,
    Or,
    Amp,
    Pipe,
    Bang,
    Shl,
    Shr,
    Caret,
    Tilde,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Question,
    Colon,
    ColonColon,
    EOF,
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Token::EOF;
        }

        let ch = self.current_char();
        if ch.is_ascii_digit() {
            return self.lex_number();
        }
        if ch.is_alphabetic() || ch == '_' {
            return self.lex_identifier();
        }
        if ch == '\'' || ch == '"' {
            return self.lex_string();
        }

        self.pos += 1;
        match ch {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            '.' => Token::Dot,
            ',' => Token::Comma,
            '?' => Token::Question,
            '^' => Token::Caret,
            '~' => Token::Tilde,
            ':' => {
                if self.pos < self.input.len() && self.current_char() == ':' {
                    self.pos += 1;
                    Token::ColonColon
                } else {
                    Token::Colon
                }
            }
            '!' => {
                if self.pos < self.input.len() && self.current_char() == '=' {
                    self.pos += 1;
                    Token::NotEqual
                } else {
                    Token::Bang
                }
            }
            '=' => {
                if self.pos < self.input.len() && self.current_char() == '=' {
                    self.pos += 1;
                }
                Token::Equal
            }
            '>' => {
                if self.pos < self.input.len() && self.current_char() == '=' {
                    self.pos += 1;
                    Token::GreaterEqual
                } else if self.pos < self.input.len() && self.current_char() == '>' {
                    self.pos += 1;
                    Token::Shr
                } else {
                    Token::Greater
                }
            }
            '<' => {
                if self.pos < self.input.len() && self.current_char() == '=' {
                    self.pos += 1;
                    Token::LessEqual
                } else if self.pos < self.input.len() && self.current_char() == '<' {
                    self.pos += 1;
                    Token::Shl
                } else {
                    Token::Less
                }
            }
            '&' => {
                if self.pos < self.input.len() && self.current_char() == '&' {
                    self.pos += 1;
                    Token::And
                } else {
                    Token::Amp
                }
            }
            '|' => {
                if self.pos < self.input.len() && self.current_char() == '|' {
                    self.pos += 1;
                    Token::Or
                } else {
                    Token::Pipe
                }
            }
            _ => Token::EOF,
        }
    }

    fn current_char(&self) -> char {
        self.input[self.pos..].chars().next().unwrap_or('\0')
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.current_char().is_whitespace() {
            self.pos += 1;
        }
    }

    fn lex_number(&mut self) -> Token {
        let start = self.pos;
        if self.current_char() == '0' {
            self.pos += 1;
            if self.pos < self.input.len() {
                let next = self.current_char();
                if next == 'x' || next == 'X' {
                    self.pos += 1;
                    let hex_start = self.pos;
                    while self.pos < self.input.len() && self.current_char().is_ascii_hexdigit() {
                        self.pos += 1;
                    }
                    let val = i64::from_str_radix(&self.input[hex_start..self.pos], 16).unwrap_or(0);
                    return Token::Number(val);
                }
                if next == 'b' || next == 'B' {
                    self.pos += 1;
                    let bin_start = self.pos;
                    while self.pos < self.input.len() && (self.current_char() == '0' || self.current_char() == '1' || self.current_char() == '_') {
                        self.pos += 1;
                    }
                    let bin_str: String = self.input[bin_start..self.pos].chars().filter(|c| *c != '_').collect();
                    let val = i64::from_str_radix(&bin_str, 2).unwrap_or(0);
                    return Token::Number(val);
                }
            }
        }
        while self.pos < self.input.len() && self.current_char().is_ascii_digit() {
            self.pos += 1;
        }
        // Check for float
        if self.pos < self.input.len() && self.current_char() == '.' {
            let next_pos = self.pos + 1;
            if next_pos < self.input.len() && self.input[next_pos..].chars().next().map_or(false, |c| c.is_ascii_digit()) {
                self.pos += 1; // consume '.'
                while self.pos < self.input.len() && self.current_char().is_ascii_digit() {
                    self.pos += 1;
                }
                let val: f64 = self.input[start..self.pos].parse().unwrap_or(0.0);
                return Token::Float(val);
            }
        }
        let val = self.input[start..self.pos].parse().unwrap_or(0);
        Token::Number(val)
    }

    fn lex_identifier(&mut self) -> Token {
        let start = self.pos;
        while self.pos < self.input.len() && (self.current_char().is_alphanumeric() || self.current_char() == '_') {
            self.pos += 1;
        }
        Token::Identifier(self.input[start..self.pos].to_string())
    }

    fn lex_string(&mut self) -> Token {
        let quote = self.current_char();
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.input.len() && self.current_char() != quote {
            self.pos += 1;
        }
        let s = self.input[start..self.pos].to_string();
        if self.pos < self.input.len() {
            self.pos += 1;
        }
        Token::String(s)
    }
}

/// Context for expression evaluation, providing access to parsed field values,
/// stream state, and enum definitions.
pub struct EvalContext<'a> {
    pub values: &'a HashMap<String, i64>,
    pub string_values: &'a HashMap<String, String>,
    pub byte_arrays: &'a HashMap<String, Vec<u8>>,
    pub base_path: &'a [String],
    pub stream_eof: bool,
    pub stream_size: usize,
    pub stream_pos: usize,
    pub enums: &'a HashMap<String, HashMap<String, String>>,
    pub errors: Option<&'a std::cell::RefCell<Vec<crate::core::structure::types::ParseError>>>,
    pub instance_resolver: Option<&'a dyn Fn(&str) -> Option<i64>>,
}

impl<'a> EvalContext<'a> {
    pub fn simple(values: &'a HashMap<String, i64>, base_path: &'a [String]) -> Self {
        let empty_strings = &EMPTY_STRING_MAP;
        let empty_enums = &EMPTY_ENUM_MAP;
        let empty_bytes = &EMPTY_BYTE_MAP;
        Self {
            values,
            string_values: empty_strings,
            byte_arrays: empty_bytes,
            base_path,
            stream_eof: false,
            stream_size: 0,
            stream_pos: 0,
            enums: empty_enums,
            errors: None,
            instance_resolver: None,
        }
    }
}

pub static EMPTY_STRING_MAP: std::sync::LazyLock<HashMap<String, String>> = std::sync::LazyLock::new(HashMap::new);
pub static EMPTY_ENUM_MAP: std::sync::LazyLock<HashMap<String, HashMap<String, String>>> = std::sync::LazyLock::new(HashMap::new);
pub static EMPTY_BYTE_MAP: std::sync::LazyLock<HashMap<String, Vec<u8>>> = std::sync::LazyLock::new(HashMap::new);

/// Expression value - can be integer, float, string, or bool
#[derive(Debug, Clone)]
pub enum ExprValue {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

impl ExprValue {
    pub fn to_i64(&self) -> i64 {
        match self {
            ExprValue::Int(v) => *v,
            ExprValue::Float(v) => *v as i64,
            ExprValue::Str(_) => 0,
            ExprValue::Bool(v) => {
                if *v {
                    1
                } else {
                    0
                }
            }
        }
    }
    pub fn to_bool(&self) -> bool {
        match self {
            ExprValue::Int(v) => *v != 0,
            ExprValue::Float(v) => *v != 0.0,
            ExprValue::Str(s) => !s.is_empty(),
            ExprValue::Bool(v) => *v,
        }
    }
    pub fn to_string_val(&self) -> String {
        match self {
            ExprValue::Int(v) => v.to_string(),
            ExprValue::Float(v) => v.to_string(),
            ExprValue::Str(s) => s.clone(),
            ExprValue::Bool(v) => v.to_string(),
        }
    }
    fn is_float(&self) -> bool {
        matches!(self, ExprValue::Float(_))
    }
    fn to_f64(&self) -> f64 {
        match self {
            ExprValue::Int(v) => *v as f64,
            ExprValue::Float(v) => *v,
            ExprValue::Str(_) => 0.0,
            ExprValue::Bool(v) => {
                if *v {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

impl ExprEvaluator {
    /// Legacy evaluate - returns i64
    pub fn evaluate(expr: &str, context: &HashMap<String, i64>, base_path: &[String]) -> i64 {
        let ctx = EvalContext::simple(context, base_path);
        Self::evaluate_rich(expr, &ctx).to_i64()
    }

    /// Legacy evaluate_bool
    pub fn evaluate_bool(expr: &str, context: &HashMap<String, i64>, base_path: &[String]) -> bool {
        let ctx = EvalContext::simple(context, base_path);
        Self::evaluate_rich(expr, &ctx).to_bool()
    }

    /// Rich evaluation returning ExprValue
    pub fn evaluate_rich(expr: &str, ctx: &EvalContext) -> ExprValue {
        let mut parser = Parser::new(expr, ctx);
        parser.parse_ternary()
    }

    /// Evaluate with full context, return i64
    pub fn eval_i64(expr: &str, ctx: &EvalContext) -> i64 {
        Self::evaluate_rich(expr, ctx).to_i64()
    }

    /// Evaluate with full context, return bool
    pub fn eval_bool(expr: &str, ctx: &EvalContext) -> bool {
        Self::evaluate_rich(expr, ctx).to_bool()
    }

    /// Evaluate with full context, return string
    pub fn eval_string(expr: &str, ctx: &EvalContext) -> String {
        Self::evaluate_rich(expr, ctx).to_string_val()
    }

    /// Evaluate pre-compiled AST returning ExprValue
    pub fn eval_ast_rich(ast: &ExprAST, ctx: &EvalContext) -> ExprValue {
        ast.eval(ctx)
    }

    /// Evaluate pre-compiled AST returning i64
    pub fn eval_ast_i64(ast: &ExprAST, ctx: &EvalContext) -> i64 {
        ast.eval(ctx).to_i64()
    }

    /// Evaluate pre-compiled AST returning bool
    pub fn eval_ast_bool(ast: &ExprAST, ctx: &EvalContext) -> bool {
        ast.eval(ctx).to_bool()
    }

    /// Evaluate pre-compiled AST returning string
    pub fn eval_ast_string(ast: &ExprAST, ctx: &EvalContext) -> String {
        ast.eval(ctx).to_string_val()
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Token,
    ctx: &'a EvalContext<'a>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, ctx: &'a EvalContext<'a>) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self { lexer, current_token, ctx }
    }

    fn record_error(&self, message: String) {
        if let Some(err_cell) = self.ctx.errors {
            let offset = self.ctx.stream_pos;
            err_cell.borrow_mut().push(crate::core::structure::types::ParseError { message, offset });
        }
    }

    fn advance(&mut self) {
        self.current_token = self.lexer.next_token();
    }

    fn take_token(&mut self) -> Token {
        let tok = std::mem::replace(&mut self.current_token, Token::EOF);
        self.advance();
        tok
    }

    fn parse_ternary(&mut self) -> ExprValue {
        let val = self.parse_or();
        if self.current_token == Token::Question {
            self.advance();
            let then_val = self.parse_ternary();
            if self.current_token == Token::Colon {
                self.advance();
            }
            let else_val = self.parse_ternary();
            if val.to_bool() { then_val } else { else_val }
        } else {
            val
        }
    }

    fn parse_or(&mut self) -> ExprValue {
        let mut val = self.parse_and();
        while matches!(self.current_token, Token::Or) {
            self.advance();
            let right = self.parse_and();
            val = ExprValue::Bool(val.to_bool() || right.to_bool());
        }
        // Handle 'or' keyword
        while matches!(self.current_token, Token::Identifier(ref s) if s == "or") {
            self.advance();
            let right = self.parse_and();
            val = ExprValue::Bool(val.to_bool() || right.to_bool());
        }
        val
    }

    fn parse_and(&mut self) -> ExprValue {
        let mut val = self.parse_comparison();
        while matches!(self.current_token, Token::And) {
            self.advance();
            let right = self.parse_comparison();
            val = ExprValue::Bool(val.to_bool() && right.to_bool());
        }
        while matches!(self.current_token, Token::Identifier(ref s) if s == "and") {
            self.advance();
            let right = self.parse_comparison();
            val = ExprValue::Bool(val.to_bool() && right.to_bool());
        }
        val
    }

    fn parse_comparison(&mut self) -> ExprValue {
        let mut val = self.parse_bit_or();
        while matches!(
            self.current_token,
            Token::Equal | Token::NotEqual | Token::Greater | Token::GreaterEqual | Token::Less | Token::LessEqual
        ) {
            let op = self.take_token();
            let right = self.parse_bit_or();
            // String comparison
            if matches!(val, ExprValue::Str(_)) || matches!(right, ExprValue::Str(_)) {
                let ls = val.to_string_val();
                let rs = right.to_string_val();
                val = match op {
                    Token::Equal => ExprValue::Bool(ls == rs),
                    Token::NotEqual => ExprValue::Bool(ls != rs),
                    _ => ExprValue::Bool(false),
                };
            } else if val.is_float() || right.is_float() {
                let l = val.to_f64();
                let r = right.to_f64();
                val = match op {
                    Token::Equal => ExprValue::Bool(l == r),
                    Token::NotEqual => ExprValue::Bool(l != r),
                    Token::Greater => ExprValue::Bool(l > r),
                    Token::GreaterEqual => ExprValue::Bool(l >= r),
                    Token::Less => ExprValue::Bool(l < r),
                    Token::LessEqual => ExprValue::Bool(l <= r),
                    _ => ExprValue::Bool(false),
                };
            } else {
                let l = val.to_i64();
                let r = right.to_i64();
                val = match op {
                    Token::Equal => ExprValue::Bool(l == r),
                    Token::NotEqual => ExprValue::Bool(l != r),
                    Token::Greater => ExprValue::Bool(l > r),
                    Token::GreaterEqual => ExprValue::Bool(l >= r),
                    Token::Less => ExprValue::Bool(l < r),
                    Token::LessEqual => ExprValue::Bool(l <= r),
                    _ => ExprValue::Bool(false),
                };
            }
        }
        val
    }

    fn parse_bit_or(&mut self) -> ExprValue {
        let mut val = self.parse_bit_xor();
        while matches!(self.current_token, Token::Pipe) {
            self.advance();
            let right = self.parse_bit_xor();
            val = ExprValue::Int(val.to_i64() | right.to_i64());
        }
        val
    }

    fn parse_bit_xor(&mut self) -> ExprValue {
        let mut val = self.parse_bit_and();
        while matches!(self.current_token, Token::Caret) {
            self.advance();
            let right = self.parse_bit_and();
            val = ExprValue::Int(val.to_i64() ^ right.to_i64());
        }
        val
    }

    fn parse_bit_and(&mut self) -> ExprValue {
        let mut val = self.parse_shift();
        while matches!(self.current_token, Token::Amp) {
            self.advance();
            let right = self.parse_shift();
            val = ExprValue::Int(val.to_i64() & right.to_i64());
        }
        val
    }

    fn parse_shift(&mut self) -> ExprValue {
        let mut val = self.parse_term();
        while matches!(self.current_token, Token::Shl | Token::Shr) {
            let op = self.take_token();
            let right = self.parse_term();
            match op {
                Token::Shl => val = ExprValue::Int(val.to_i64().wrapping_shl(right.to_i64() as u32)),
                Token::Shr => val = ExprValue::Int(val.to_i64().wrapping_shr(right.to_i64() as u32)),
                _ => {}
            }
        }
        val
    }

    fn parse_term(&mut self) -> ExprValue {
        let mut val = self.parse_factor();
        while matches!(self.current_token, Token::Plus | Token::Minus) {
            let op = self.take_token();
            let right = self.parse_factor();
            if val.is_float() || right.is_float() {
                match op {
                    Token::Plus => val = ExprValue::Float(val.to_f64() + right.to_f64()),
                    Token::Minus => val = ExprValue::Float(val.to_f64() - right.to_f64()),
                    _ => {}
                }
            } else {
                match op {
                    Token::Plus => val = ExprValue::Int(val.to_i64() + right.to_i64()),
                    Token::Minus => val = ExprValue::Int(val.to_i64() - right.to_i64()),
                    _ => {}
                }
            }
        }
        val
    }

    fn parse_factor(&mut self) -> ExprValue {
        let mut val = self.parse_unary();
        while matches!(self.current_token, Token::Star | Token::Slash | Token::Percent) {
            let op = self.take_token();
            let right = self.parse_unary();
            if val.is_float() || right.is_float() {
                match op {
                    Token::Star => val = ExprValue::Float(val.to_f64() * right.to_f64()),
                    Token::Slash => {
                        let r = right.to_f64();
                        if r == 0.0 {
                            self.record_error("Division by zero in float expression".to_string());
                            val = ExprValue::Float(0.0);
                        } else {
                            val = ExprValue::Float(val.to_f64() / r);
                        }
                    }
                    Token::Percent => {
                        let r = right.to_i64();
                        if r == 0 {
                            self.record_error("Modulo by zero in expression".to_string());
                            val = ExprValue::Int(0);
                        } else {
                            val = ExprValue::Int(val.to_i64() % r);
                        }
                    }
                    _ => {}
                }
            } else {
                match op {
                    Token::Star => val = ExprValue::Int(val.to_i64() * right.to_i64()),
                    Token::Slash => {
                        let r = right.to_i64();
                        if r == 0 {
                            self.record_error("Division by zero in integer expression".to_string());
                            val = ExprValue::Int(0);
                        } else {
                            val = ExprValue::Int(val.to_i64() / r);
                        }
                    }
                    Token::Percent => {
                        let r = right.to_i64();
                        if r == 0 {
                            self.record_error("Modulo by zero in expression".to_string());
                            val = ExprValue::Int(0);
                        } else {
                            val = ExprValue::Int(val.to_i64() % r);
                        }
                    }
                    _ => {}
                }
            }
        }
        val
    }

    fn parse_unary(&mut self) -> ExprValue {
        match &self.current_token {
            Token::Bang => {
                self.advance();
                let val = self.parse_unary();
                ExprValue::Bool(!val.to_bool())
            }
            Token::Minus => {
                self.advance();
                let val = self.parse_unary();
                if val.is_float() {
                    ExprValue::Float(-val.to_f64())
                } else {
                    ExprValue::Int(-val.to_i64())
                }
            }
            Token::Tilde => {
                self.advance();
                let val = self.parse_unary();
                ExprValue::Int(!val.to_i64())
            }
            Token::Identifier(s) if s == "not" => {
                self.advance();
                let val = self.parse_unary();
                ExprValue::Bool(!val.to_bool())
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> ExprValue {
        let mut val = match self.take_token() {
            Token::Number(n) => ExprValue::Int(n),
            Token::Float(f) => ExprValue::Float(f),
            Token::Identifier(id) => self.resolve_identifier(&id),
            Token::String(s) => ExprValue::Str(s),
            Token::LParen => {
                let val = self.parse_ternary();
                if self.current_token == Token::RParen {
                    self.advance();
                } else {
                    self.record_error("Unclosed parenthesis in expression".to_string());
                }
                val
            }
            tok => {
                self.record_error(format!("Unexpected token in expression: {:?}", tok));
                ExprValue::Int(0)
            }
        };

        // Handle indexing on primary, e.g. bytes[0]
        while self.current_token == Token::LBracket {
            self.advance();
            let idx = self.parse_ternary().to_i64() as usize;
            if self.current_token == Token::RBracket {
                self.advance();
            }
            if let ExprValue::Str(ref s) = val {
                if idx < s.len() {
                    val = ExprValue::Int(s.as_bytes()[idx] as i64);
                } else {
                    val = ExprValue::Int(0);
                }
            }
        }

        val
    }

    fn resolve_identifier(&mut self, id: &str) -> ExprValue {
        // Keywords
        match id {
            "true" => return ExprValue::Bool(true),
            "false" => return ExprValue::Bool(false),
            _ => {}
        }

        let mut path_parts: Vec<String>;

        if id == "_io" {
            // Handle _io.eof, _io.size, _io.pos
            if self.current_token == Token::Dot {
                self.advance();
                if let Token::Identifier(prop) = &self.current_token {
                    let prop = prop.clone();
                    self.advance();
                    return match prop.as_str() {
                        "eof" => ExprValue::Bool(self.ctx.stream_eof),
                        "size" => ExprValue::Int(self.ctx.stream_size as i64),
                        "pos" => ExprValue::Int(self.ctx.stream_pos as i64),
                        _ => ExprValue::Int(0),
                    };
                }
            }
            return ExprValue::Int(0);
        }

        if id == "_root" {
            path_parts = Vec::new();
        } else if id == "_parent" {
            let mut p = self.ctx.base_path.to_vec();
            if !p.is_empty() {
                p.pop();
            }
            path_parts = p;
        } else if id == "_" {
            // In repeat-until, _ refers to the last element. Check context for "_" prefixed values.
            path_parts = self.ctx.base_path.to_vec();
            path_parts.push("_".to_string());
        } else {
            path_parts = self.ctx.base_path.to_vec();
            path_parts.push(id.to_string());
        }

        // Handle dot-chain, :: (enum resolution), and [index] access
        while self.current_token == Token::Dot || self.current_token == Token::ColonColon || self.current_token == Token::LBracket {
            if self.current_token == Token::LBracket {
                self.advance();
                let idx = self.parse_ternary().to_i64() as usize;
                if self.current_token == Token::RBracket {
                    self.advance();
                }
                let full_id = path_parts.join(".");
                if let Some(bytes) = self.ctx.byte_arrays.get(&full_id).or_else(|| self.ctx.byte_arrays.get(id)) {
                    if idx < bytes.len() {
                        return ExprValue::Int(bytes[idx] as i64);
                    }
                }
                if let Some(s) = self.ctx.string_values.get(&full_id).or_else(|| self.ctx.string_values.get(id)) {
                    if idx < s.len() {
                        return ExprValue::Int(s.as_bytes()[idx] as i64);
                    }
                }
                return ExprValue::Int(0);
            }

            let is_enum_access = self.current_token == Token::ColonColon;
            self.advance();
            if let Token::Identifier(sub) = &self.current_token {
                let sub = sub.clone();
                self.advance();

                if is_enum_access {
                    // enum_name::value — resolve enum value
                    let enum_name = path_parts.last().cloned().unwrap_or_default();
                    return self.resolve_enum_value(&enum_name, &sub);
                }

                if sub == "_parent" {
                    if !path_parts.is_empty() {
                        path_parts.pop();
                    }
                } else if sub == "_root" {
                    path_parts = Vec::new();
                } else if sub == "to_i" {
                    // .to_i — identity for integer values
                    break;
                } else {
                    path_parts.push(sub);
                }
            } else {
                break;
            }
        }

        // Try to resolve the path
        let full_id = path_parts.join(".");
        if let Some(val) = self.ctx.values.get(&full_id) {
            return ExprValue::Int(*val);
        }

        // Try string values
        if let Some(val) = self.ctx.string_values.get(&full_id) {
            return ExprValue::Str(val.clone());
        }

        // Try walking up scope hierarchy from base_path
        let mut scope = self.ctx.base_path.to_vec();
        while !scope.is_empty() {
            scope.pop();
            let mut p = scope.clone();
            p.extend(path_parts.iter().cloned());
            let scope_id = p.join(".");
            if let Some(val) = self.ctx.values.get(&scope_id) {
                return ExprValue::Int(*val);
            }
            if let Some(val) = self.ctx.string_values.get(&scope_id) {
                return ExprValue::Str(val.clone());
            }
        }

        // Try as global identifier
        if let Some(last) = path_parts.last() {
            if let Some(val) = self.ctx.values.get(last) {
                return ExprValue::Int(*val);
            }
            if let Some(val) = self.ctx.string_values.get(last) {
                return ExprValue::Str(val.clone());
            }
        }

        // Try instance resolver if provided
        if let Some(resolver) = self.ctx.instance_resolver {
            if let Some(val) = resolver(&full_id).or_else(|| resolver(id)) {
                return ExprValue::Int(val);
            }
        }

        if !id.starts_with('_') && id != "true" && id != "false" {
            self.record_error(format!("Unresolved identifier: {}", full_id));
        }

        ExprValue::Int(0)
    }

    fn resolve_enum_value(&self, enum_name: &str, value_name: &str) -> ExprValue {
        if let Some(enum_def) = self.ctx.enums.get(enum_name) {
            // enum_def maps numeric_key -> label_name
            // We need reverse lookup: label_name -> numeric_key
            for (key, label) in enum_def {
                if label == value_name {
                    if let Ok(v) = key.parse::<i64>() {
                        return ExprValue::Int(v);
                    }
                }
            }
        }
        ExprValue::Int(0)
    }
}

/// Abstract Syntax Tree (AST) for pre-compiled Kaitai Struct expressions
#[derive(Debug, Clone, PartialEq)]
pub enum ExprAST {
    Number(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Identifier(String),
    MemberAccess {
        base: Box<ExprAST>,
        member: String,
        is_enum: bool,
    },
    IndexAccess {
        base: Box<ExprAST>,
        index: Box<ExprAST>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<ExprAST>,
    },
    Binary {
        op: BinaryOp,
        left: Box<ExprAST>,
        right: Box<ExprAST>,
    },
    Ternary {
        cond: Box<ExprAST>,
        then_branch: Box<ExprAST>,
        else_branch: Box<ExprAST>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    BitAnd,
    BitOr,
    BitXor,
    LogicalAnd,
    LogicalOr,
}

impl ExprAST {
    pub fn compile(expr: &str) -> Option<ExprAST> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut parser = ASTParser::new(trimmed);
        Some(parser.parse_expression())
    }

    pub fn eval(&self, ctx: &EvalContext) -> ExprValue {
        match self {
            ExprAST::Number(n) => ExprValue::Int(*n),
            ExprAST::Float(f) => ExprValue::Float(*f),
            ExprAST::Str(s) => ExprValue::Str(s.clone()),
            ExprAST::Bool(b) => ExprValue::Bool(*b),
            ExprAST::Identifier(id) => self.eval_identifier(id, ctx),
            ExprAST::MemberAccess { base, member, is_enum } => self.eval_member_access(base, member, *is_enum, ctx),
            ExprAST::IndexAccess { base, index } => {
                let idx_val = index.eval(ctx).to_i64() as usize;
                let mut path = String::new();
                match &**base {
                    ExprAST::Identifier(id) => {
                        path = id.clone();
                    }
                    _ => {
                        let base_val = base.eval(ctx);
                        if let ExprValue::Str(ref s) = base_val {
                            if idx_val < s.len() {
                                return ExprValue::Int(s.as_bytes()[idx_val] as i64);
                            }
                        }
                    }
                }
                if let Some(bytes) = ctx.byte_arrays.get(&path) {
                    if idx_val < bytes.len() {
                        return ExprValue::Int(bytes[idx_val] as i64);
                    }
                }
                let scoped_path = if ctx.base_path.is_empty() {
                    path.clone()
                } else {
                    format!("{}.{}", ctx.base_path.join("."), path)
                };
                if let Some(bytes) = ctx.byte_arrays.get(&scoped_path) {
                    if idx_val < bytes.len() {
                        return ExprValue::Int(bytes[idx_val] as i64);
                    }
                }
                if let Some(s) = ctx.string_values.get(&path).or_else(|| ctx.string_values.get(&scoped_path)) {
                    if idx_val < s.len() {
                        return ExprValue::Int(s.as_bytes()[idx_val] as i64);
                    }
                }
                ExprValue::Int(0)
            }
            ExprAST::Unary { op, operand } => {
                let val = operand.eval(ctx);
                match op {
                    UnaryOp::Not => ExprValue::Bool(!val.to_bool()),
                    UnaryOp::Neg => {
                        if val.is_float() {
                            ExprValue::Float(-val.to_f64())
                        } else {
                            ExprValue::Int(-val.to_i64())
                        }
                    }
                    UnaryOp::BitNot => ExprValue::Int(!val.to_i64()),
                }
            }
            ExprAST::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                if cond.eval(ctx).to_bool() {
                    then_branch.eval(ctx)
                } else {
                    else_branch.eval(ctx)
                }
            }
            ExprAST::Binary { op, left, right } => match op {
                BinaryOp::LogicalAnd => {
                    let l = left.eval(ctx);
                    if !l.to_bool() {
                        return ExprValue::Bool(false);
                    }
                    let r = right.eval(ctx);
                    ExprValue::Bool(r.to_bool())
                }
                BinaryOp::LogicalOr => {
                    let l = left.eval(ctx);
                    if l.to_bool() {
                        return ExprValue::Bool(true);
                    }
                    let r = right.eval(ctx);
                    ExprValue::Bool(r.to_bool())
                }
                _ => {
                    let l = left.eval(ctx);
                    let r = right.eval(ctx);
                    match op {
                        BinaryOp::Add => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Float(l.to_f64() + r.to_f64())
                            } else {
                                ExprValue::Int(l.to_i64().wrapping_add(r.to_i64()))
                            }
                        }
                        BinaryOp::Sub => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Float(l.to_f64() - r.to_f64())
                            } else {
                                ExprValue::Int(l.to_i64().wrapping_sub(r.to_i64()))
                            }
                        }
                        BinaryOp::Mul => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Float(l.to_f64() * r.to_f64())
                            } else {
                                ExprValue::Int(l.to_i64().wrapping_mul(r.to_i64()))
                            }
                        }
                        BinaryOp::Div => {
                            if l.is_float() || r.is_float() {
                                let r_val = r.to_f64();
                                if r_val == 0.0 {
                                    if let Some(err_cell) = ctx.errors {
                                        err_cell.borrow_mut().push(crate::core::structure::types::ParseError {
                                            message: "Division by zero in float expression".to_string(),
                                            offset: ctx.stream_pos,
                                        });
                                    }
                                    ExprValue::Float(0.0)
                                } else {
                                    ExprValue::Float(l.to_f64() / r_val)
                                }
                            } else {
                                let r_val = r.to_i64();
                                if r_val == 0 {
                                    if let Some(err_cell) = ctx.errors {
                                        err_cell.borrow_mut().push(crate::core::structure::types::ParseError {
                                            message: "Division by zero in integer expression".to_string(),
                                            offset: ctx.stream_pos,
                                        });
                                    }
                                    ExprValue::Int(0)
                                } else {
                                    ExprValue::Int(l.to_i64() / r_val)
                                }
                            }
                        }
                        BinaryOp::Mod => {
                            let r_val = r.to_i64();
                            if r_val == 0 {
                                if let Some(err_cell) = ctx.errors {
                                    err_cell.borrow_mut().push(crate::core::structure::types::ParseError {
                                        message: "Modulo by zero in expression".to_string(),
                                        offset: ctx.stream_pos,
                                    });
                                }
                                ExprValue::Int(0)
                            } else {
                                ExprValue::Int(l.to_i64() % r_val)
                            }
                        }
                        BinaryOp::Shl => ExprValue::Int(l.to_i64().wrapping_shl(r.to_i64() as u32)),
                        BinaryOp::Shr => ExprValue::Int(l.to_i64().wrapping_shr(r.to_i64() as u32)),
                        BinaryOp::BitAnd => ExprValue::Int(l.to_i64() & r.to_i64()),
                        BinaryOp::BitOr => ExprValue::Int(l.to_i64() | r.to_i64()),
                        BinaryOp::BitXor => ExprValue::Int(l.to_i64() ^ r.to_i64()),
                        BinaryOp::Equal => {
                            if matches!(l, ExprValue::Str(_)) || matches!(r, ExprValue::Str(_)) {
                                ExprValue::Bool(l.to_string_val() == r.to_string_val())
                            } else if l.is_float() || r.is_float() {
                                ExprValue::Bool(l.to_f64() == r.to_f64())
                            } else {
                                ExprValue::Bool(l.to_i64() == r.to_i64())
                            }
                        }
                        BinaryOp::NotEqual => {
                            if matches!(l, ExprValue::Str(_)) || matches!(r, ExprValue::Str(_)) {
                                ExprValue::Bool(l.to_string_val() != r.to_string_val())
                            } else if l.is_float() || r.is_float() {
                                ExprValue::Bool(l.to_f64() != r.to_f64())
                            } else {
                                ExprValue::Bool(l.to_i64() != r.to_i64())
                            }
                        }
                        BinaryOp::Greater => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Bool(l.to_f64() > r.to_f64())
                            } else {
                                ExprValue::Bool(l.to_i64() > r.to_i64())
                            }
                        }
                        BinaryOp::GreaterEqual => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Bool(l.to_f64() >= r.to_f64())
                            } else {
                                ExprValue::Bool(l.to_i64() >= r.to_i64())
                            }
                        }
                        BinaryOp::Less => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Bool(l.to_f64() < r.to_f64())
                            } else {
                                ExprValue::Bool(l.to_i64() < r.to_i64())
                            }
                        }
                        BinaryOp::LessEqual => {
                            if l.is_float() || r.is_float() {
                                ExprValue::Bool(l.to_f64() <= r.to_f64())
                            } else {
                                ExprValue::Bool(l.to_i64() <= r.to_i64())
                            }
                        }
                        BinaryOp::LogicalAnd | BinaryOp::LogicalOr => unreachable!(),
                    }
                }
            },
        }
    }

    fn eval_identifier(&self, id: &str, ctx: &EvalContext) -> ExprValue {
        match id {
            "true" => return ExprValue::Bool(true),
            "false" => return ExprValue::Bool(false),
            _ => {}
        }

        let mut path_parts: Vec<String>;

        if id == "_io" {
            return ExprValue::Int(0);
        }

        if id == "_root" {
            path_parts = Vec::new();
        } else if id == "_parent" {
            let mut p = ctx.base_path.to_vec();
            if !p.is_empty() {
                p.pop();
            }
            path_parts = p;
        } else if id == "_" {
            path_parts = ctx.base_path.to_vec();
            path_parts.push("_".to_string());
        } else {
            path_parts = ctx.base_path.to_vec();
            path_parts.push(id.to_string());
        }

        let full_id = path_parts.join(".");
        if let Some(val) = ctx.values.get(&full_id) {
            return ExprValue::Int(*val);
        }
        if let Some(val) = ctx.string_values.get(&full_id) {
            return ExprValue::Str(val.clone());
        }

        // Walk up scope hierarchy from base_path
        let mut scope = ctx.base_path.to_vec();
        while !scope.is_empty() {
            scope.pop();
            let mut p = scope.clone();
            p.push(id.to_string());
            let scope_id = p.join(".");
            if let Some(val) = ctx.values.get(&scope_id) {
                return ExprValue::Int(*val);
            }
            if let Some(val) = ctx.string_values.get(&scope_id) {
                return ExprValue::Str(val.clone());
            }
        }

        // Fallback for direct bare identifier if present at top-level
        if let Some(val) = ctx.values.get(id) {
            return ExprValue::Int(*val);
        }
        if let Some(val) = ctx.string_values.get(id) {
            return ExprValue::Str(val.clone());
        }

        // Try instance resolver if provided
        if let Some(resolver) = ctx.instance_resolver {
            if let Some(val) = resolver(&full_id).or_else(|| resolver(id)) {
                return ExprValue::Int(val);
            }
        }

        if !id.starts_with('_') && id != "true" && id != "false" && !full_id.contains("extra") {
            if let Some(err_cell) = ctx.errors {
                err_cell.borrow_mut().push(crate::core::structure::types::ParseError {
                    message: format!("Unresolved identifier: {}", full_id),
                    offset: ctx.stream_pos,
                });
            }
        }

        ExprValue::Int(0)
    }

    fn eval_member_access(&self, base: &ExprAST, member: &str, is_enum: bool, ctx: &EvalContext) -> ExprValue {
        if is_enum {
            if let ExprAST::Identifier(enum_name) = base {
                if let Some(enum_def) = ctx.enums.get(enum_name) {
                    for (key, label) in enum_def {
                        if label == member {
                            if let Ok(v) = key.parse::<i64>() {
                                return ExprValue::Int(v);
                            }
                        }
                    }
                }
            }
            return ExprValue::Int(0);
        }

        if let ExprAST::Identifier(id) = base {
            if id == "_io" {
                return match member {
                    "eof" => ExprValue::Bool(ctx.stream_eof),
                    "size" => ExprValue::Int(ctx.stream_size as i64),
                    "pos" => ExprValue::Int(ctx.stream_pos as i64),
                    _ => ExprValue::Int(0),
                };
            }
        }

        if member == "to_i" {
            return base.eval(ctx);
        }

        let candidate_paths = {
            let mut paths = Vec::new();

            // Extract chain of member access identifiers, e.g. header.len_body_compressed
            let mut parts = Vec::new();
            parts.push(member.to_string());
            let mut curr = base;
            let mut valid = true;
            while let ExprAST::MemberAccess { base: b, member: m, is_enum } = curr {
                if *is_enum {
                    valid = false;
                    break;
                }
                parts.push(m.clone());
                curr = b;
            }

            if valid {
                if let ExprAST::Identifier(root_id) = curr {
                    let (prefix, matched_element) = if root_id == "_root" {
                        (Vec::new(), None)
                    } else if root_id == "_parent" {
                        let mut p = ctx.base_path.to_vec();
                        if !p.is_empty() {
                            p.pop();
                        }
                        (p, None)
                    } else if root_id == "_" {
                        let mut p = ctx.base_path.to_vec();
                        p.push("_".to_string());
                        (p, None)
                    } else {
                        let mut p = ctx.base_path.to_vec();
                        if let Some(idx) = p.iter().rposition(|x| {
                            let base_name = x.split('[').next().unwrap_or(x);
                            base_name == root_id
                        }) {
                            let elem = p[idx].clone();
                            p.truncate(idx);
                            (p, Some(elem))
                        } else {
                            (p, None)
                        }
                    };

                    // 1. Exact path appending root_id and parts to current base_path
                    let mut p1 = ctx.base_path.to_vec();
                    p1.push(root_id.clone());
                    p1.extend(parts.iter().cloned().rev());
                    paths.push(p1.join("."));

                    // 2. Truncated path if root_id is already in base_path
                    let mut p2 = prefix.clone();
                    if let Some(ref elem) = matched_element {
                        p2.push(elem.clone());
                    } else if root_id != "_root" && root_id != "_parent" && root_id != "_" {
                        p2.push(root_id.clone());
                    }
                    p2.extend(parts.iter().cloned().rev());
                    paths.push(p2.join("."));

                    // 3. Walk up scope hierarchy from base_path
                    let mut scope = ctx.base_path.to_vec();
                    while !scope.is_empty() {
                        scope.pop();
                        let mut p = scope.clone();
                        if root_id != "_root" && root_id != "_parent" && root_id != "_" {
                            p.push(root_id.clone());
                        }
                        p.extend(parts.iter().cloned().rev());
                        paths.push(p.join("."));
                    }

                    // 4. Relative path without base_path
                    let mut root_parts = vec![root_id.clone()];
                    root_parts.extend(parts.iter().cloned().rev());
                    paths.push(root_parts.join("."));
                }
            } else {
                let base_val = base.eval(ctx);
                let base_str = base_val.to_string_val();
                if base_str.is_empty() {
                    paths.push(member.to_string());
                } else {
                    paths.push(format!("{}.{}", base_str, member));
                }
            }
            paths
        };

        for path in &candidate_paths {
            if let Some(val) = ctx.values.get(path) {
                return ExprValue::Int(*val);
            }
            if let Some(val) = ctx.string_values.get(path) {
                return ExprValue::Str(val.clone());
            }
        }

        let first_path = candidate_paths.first().cloned().unwrap_or_default();
        if !first_path.contains("extra") {
            if let Some(err_cell) = ctx.errors {
                err_cell.borrow_mut().push(crate::core::structure::types::ParseError {
                    message: format!("Unresolved identifier: {}", first_path),
                    offset: ctx.stream_pos,
                });
            }
        }

        ExprValue::Int(0)
    }
}

pub struct ASTParser<'a> {
    lexer: Lexer<'a>,
    current_token: Token,
}

impl<'a> ASTParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self { lexer, current_token }
    }

    fn advance(&mut self) {
        self.current_token = self.lexer.next_token();
    }

    fn take_token(&mut self) -> Token {
        let tok = std::mem::replace(&mut self.current_token, Token::EOF);
        self.advance();
        tok
    }

    pub fn parse_expression(&mut self) -> ExprAST {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> ExprAST {
        let cond = self.parse_or();
        if self.current_token == Token::Question {
            self.advance();
            let then_branch = self.parse_ternary();
            if self.current_token == Token::Colon {
                self.advance();
            }
            let else_branch = self.parse_ternary();
            ExprAST::Ternary {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            }
        } else {
            cond
        }
    }

    fn parse_or(&mut self) -> ExprAST {
        let mut left = self.parse_and();
        while matches!(self.current_token, Token::Or) || matches!(self.current_token, Token::Identifier(ref s) if s == "or") {
            self.advance();
            let right = self.parse_and();
            left = ExprAST::Binary {
                op: BinaryOp::LogicalOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_and(&mut self) -> ExprAST {
        let mut left = self.parse_comparison();
        while matches!(self.current_token, Token::And) || matches!(self.current_token, Token::Identifier(ref s) if s == "and") {
            self.advance();
            let right = self.parse_comparison();
            left = ExprAST::Binary {
                op: BinaryOp::LogicalAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_comparison(&mut self) -> ExprAST {
        let mut left = self.parse_bit_or();
        while matches!(
            self.current_token,
            Token::Equal | Token::NotEqual | Token::Greater | Token::GreaterEqual | Token::Less | Token::LessEqual
        ) {
            let tok = self.take_token();
            let op = match tok {
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                Token::Greater => BinaryOp::Greater,
                Token::GreaterEqual => BinaryOp::GreaterEqual,
                Token::Less => BinaryOp::Less,
                Token::LessEqual => BinaryOp::LessEqual,
                _ => unreachable!(),
            };
            let right = self.parse_bit_or();
            left = ExprAST::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_bit_or(&mut self) -> ExprAST {
        let mut left = self.parse_bit_xor();
        while matches!(self.current_token, Token::Pipe) {
            self.advance();
            let right = self.parse_bit_xor();
            left = ExprAST::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_bit_xor(&mut self) -> ExprAST {
        let mut left = self.parse_bit_and();
        while matches!(self.current_token, Token::Caret) {
            self.advance();
            let right = self.parse_bit_and();
            left = ExprAST::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_bit_and(&mut self) -> ExprAST {
        let mut left = self.parse_shift();
        while matches!(self.current_token, Token::Amp) {
            self.advance();
            let right = self.parse_shift();
            left = ExprAST::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_shift(&mut self) -> ExprAST {
        let mut left = self.parse_term();
        while matches!(self.current_token, Token::Shl | Token::Shr) {
            let tok = self.take_token();
            let op = match tok {
                Token::Shl => BinaryOp::Shl,
                Token::Shr => BinaryOp::Shr,
                _ => unreachable!(),
            };
            let right = self.parse_term();
            left = ExprAST::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_term(&mut self) -> ExprAST {
        let mut left = self.parse_factor();
        while matches!(self.current_token, Token::Plus | Token::Minus) {
            let tok = self.take_token();
            let op = match tok {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_factor();
            left = ExprAST::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_factor(&mut self) -> ExprAST {
        let mut left = self.parse_unary();
        while matches!(self.current_token, Token::Star | Token::Slash | Token::Percent) {
            let tok = self.take_token();
            let op = match tok {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => unreachable!(),
            };
            let right = self.parse_unary();
            left = ExprAST::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_unary(&mut self) -> ExprAST {
        match &self.current_token {
            Token::Bang => {
                self.advance();
                let operand = self.parse_unary();
                ExprAST::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                }
            }
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary();
                ExprAST::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                }
            }
            Token::Tilde => {
                self.advance();
                let operand = self.parse_unary();
                ExprAST::Unary {
                    op: UnaryOp::BitNot,
                    operand: Box::new(operand),
                }
            }
            Token::Identifier(s) if s == "not" => {
                self.advance();
                let operand = self.parse_unary();
                ExprAST::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                }
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> ExprAST {
        let mut node = match self.take_token() {
            Token::Number(n) => ExprAST::Number(n),
            Token::Float(f) => ExprAST::Float(f),
            Token::String(s) => ExprAST::Str(s),
            Token::Identifier(id) => {
                if id == "true" {
                    ExprAST::Bool(true)
                } else if id == "false" {
                    ExprAST::Bool(false)
                } else {
                    ExprAST::Identifier(id)
                }
            }
            Token::LParen => {
                let inner = self.parse_expression();
                if self.current_token == Token::RParen {
                    self.advance();
                }
                inner
            }
            _ => ExprAST::Number(0),
        };

        loop {
            if self.current_token == Token::Dot || self.current_token == Token::ColonColon {
                let is_enum = self.current_token == Token::ColonColon;
                self.advance();
                if let Token::Identifier(member) = &self.current_token {
                    let member = member.clone();
                    self.advance();
                    node = ExprAST::MemberAccess {
                        base: Box::new(node),
                        member,
                        is_enum,
                    };
                } else {
                    break;
                }
            } else if self.current_token == Token::LBracket {
                self.advance();
                let index = self.parse_expression();
                if self.current_token == Token::RBracket {
                    self.advance();
                }
                node = ExprAST::IndexAccess {
                    base: Box::new(node),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }

        node
    }
}
