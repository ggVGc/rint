//! Resolved program: global tables of functions, types, constants and impls.

use crate::ast::*;
use std::collections::HashMap;
use std::rc::Rc;

// Builtin enum variants (fixed ids).
pub const V_NONE: u32 = 0;
pub const V_SOME: u32 = 1;
pub const V_OK: u32 = 2;
pub const V_ERR: u32 = 3;
pub const V_LESS: u32 = 4;
pub const V_EQUAL: u32 = 5;
pub const V_GREATER: u32 = 6;

pub const E_OPTION: u32 = 0;
pub const E_RESULT: u32 = 1;
pub const E_ORDERING: u32 = 2;

// Builtin structs (fixed ids).
pub const S_REVERSE: u32 = 0;
pub const S_WRAPPING: u32 = 1;

// Type keys for method dispatch.
pub const TK_ENUM: u32 = 1 << 20;
pub const TK_INT: u32 = 1 << 21;
pub const TK_FLOAT: u32 = TK_INT + 1;
pub const TK_BOOL: u32 = TK_INT + 2;
pub const TK_CHAR: u32 = TK_INT + 3;
pub const TK_STR: u32 = TK_INT + 4;
pub const TK_VEC: u32 = TK_INT + 5;
pub const TK_TUPLE: u32 = TK_INT + 6;
pub const TK_MAP: u32 = TK_INT + 7;
pub const TK_SET: u32 = TK_INT + 8;
pub const TK_UNIT: u32 = TK_INT + 9;
pub const TK_DEQUE: u32 = TK_INT + 10;
pub const TK_OTHER: u32 = TK_INT + 11;
/// Blanket impls (`impl<T> Trait for T`).
pub const TK_ANY: u32 = TK_INT + 12;

pub struct StructDef {
    pub name: String,
    pub kind: StructKind,
    pub fields: Vec<Sym>,
    pub field_types: Vec<Type>,
    pub copy: bool,
    pub derives: Vec<String>,
}

pub struct VariantDef {
    pub name: String,
    pub enum_id: u32,
    pub kind: StructKind,
    pub fields: Vec<Sym>,
    pub field_types: Vec<Type>,
    pub disc: i64,
}

pub struct EnumDef {
    pub name: String,
    pub variants: Vec<u32>,
    pub copy: bool,
    pub derives: Vec<String>,
}

pub struct ConstDef {
    pub name: String,
    pub init: Option<Expr>,
    pub is_static: bool,
    pub nslots: u32,
}

#[derive(Default)]
pub struct Program {
    pub fns: Vec<Rc<FnDecl>>,
    pub native_fns: Vec<crate::native::NativeBinding>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub variants: Vec<VariantDef>,
    pub consts: Vec<ConstDef>,
    /// (type key, method symbol) → function id
    pub methods: HashMap<(u32, Sym), u32>,
    /// Traits implemented per type key.
    pub impls: HashMap<u32, Vec<String>>,
    pub main: Option<u32>,
    /// Per struct/enum type key: user `eq` / `cmp` / `partial_cmp` implementations.
    pub user_eq: HashMap<u32, u32>,
    pub user_cmp: HashMap<u32, u32>,
    pub user_partial_cmp: HashMap<u32, u32>,
}

impl Program {
    pub fn implements(&self, tk: u32, tr: &str) -> bool {
        self.impls.get(&tk).is_some_and(|v| v.iter().any(|t| t == tr))
    }
    pub fn find_method(&self, tk: u32, sym: Sym) -> Option<u32> {
        self.methods.get(&(tk, sym)).or_else(|| self.methods.get(&(TK_ANY, sym))).copied()
    }
}

/// Map a type name used in an `impl` header to a dispatch type key.
pub fn builtin_type_key(name: &str) -> Option<u32> {
    Some(match name {
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => TK_INT,
        "f32" | "f64" => TK_FLOAT,
        "bool" => TK_BOOL,
        "char" => TK_CHAR,
        "str" | "String" => TK_STR,
        "Vec" | "slice" | "array" => TK_VEC,
        "tuple" => TK_TUPLE,
        "HashMap" | "BTreeMap" => TK_MAP,
        "HashSet" | "BTreeSet" => TK_SET,
        "VecDeque" => TK_DEQUE,
        _ => return None,
    })
}
