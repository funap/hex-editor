use crate::core::structure::expression::ExprAST;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KsyDefinition {
    pub meta: KsyMeta,
    #[serde(default)]
    pub seq: Vec<KsyAttr>,
    #[serde(default)]
    pub types: HashMap<String, KsyType>,
    #[serde(default)]
    pub enums: HashMap<String, HashMap<String, serde_yaml::Value>>,
    #[serde(default)]
    pub instances: HashMap<String, KsyAttr>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KsyMeta {
    #[serde(default)]
    pub id: String,
    pub endian: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KsyAttr {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub attr_type: Option<serde_yaml::Value>,
    pub size: Option<KsyValue>,
    #[serde(rename = "size-eos", default)]
    pub size_eos: bool,
    #[serde(rename = "if")]
    pub condition: Option<String>,
    pub repeat: Option<String>,
    #[serde(rename = "repeat-expr")]
    pub repeat_expr: Option<String>,
    #[serde(rename = "repeat-until")]
    pub repeat_until: Option<String>,
    #[serde(rename = "pos")]
    pub pos: Option<KsyValue>,
    #[serde(rename = "enum")]
    pub enum_ref: Option<String>,
    pub io: Option<String>,
    pub contents: Option<serde_yaml::Value>,
    pub encoding: Option<String>,
    pub doc: Option<String>,
    #[serde(rename = "doc-ref")]
    pub doc_ref: Option<serde_yaml::Value>,
    pub value: Option<String>,
    pub valid: Option<serde_yaml::Value>,
    pub process: Option<serde_yaml::Value>,

    #[serde(skip)]
    pub compiled_condition: Option<ExprAST>,
    #[serde(skip)]
    pub compiled_repeat_expr: Option<ExprAST>,
    #[serde(skip)]
    pub compiled_repeat_until: Option<ExprAST>,
    #[serde(skip)]
    pub compiled_value: Option<ExprAST>,
    #[serde(skip)]
    pub compiled_size: Option<ExprAST>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum KsyValue {
    Int(usize),
    Expr(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KsyType {
    #[serde(default)]
    pub seq: Vec<KsyAttr>,
    #[serde(default)]
    pub types: HashMap<String, KsyType>,
    #[serde(default)]
    pub instances: HashMap<String, KsyAttr>,
    #[serde(default)]
    pub enums: HashMap<String, HashMap<String, serde_yaml::Value>>,
}

impl KsyAttr {
    pub fn compile_expressions(&mut self) {
        if let Some(ref cond) = self.condition {
            self.compiled_condition = ExprAST::compile(cond);
        }
        if let Some(ref expr) = self.repeat_expr {
            self.compiled_repeat_expr = ExprAST::compile(expr);
        }
        if let Some(ref until) = self.repeat_until {
            self.compiled_repeat_until = ExprAST::compile(until);
        }
        if let Some(ref val) = self.value {
            self.compiled_value = ExprAST::compile(val);
        }
        if let Some(KsyValue::Expr(ref expr)) = self.size {
            self.compiled_size = ExprAST::compile(expr);
        }
    }
}

impl KsyDefinition {
    pub fn compile_expressions(&mut self) {
        for attr in &mut self.seq {
            attr.compile_expressions();
        }
        for attr in self.instances.values_mut() {
            attr.compile_expressions();
        }
        for t in self.types.values_mut() {
            t.compile_expressions();
        }
    }
}

impl KsyType {
    pub fn compile_expressions(&mut self) {
        for attr in &mut self.seq {
            attr.compile_expressions();
        }
        for attr in self.instances.values_mut() {
            attr.compile_expressions();
        }
        for t in self.types.values_mut() {
            t.compile_expressions();
        }
    }
}
