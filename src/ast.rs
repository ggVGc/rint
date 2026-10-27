//! Abstract syntax tree. The parser produces it with unresolved paths; the
//! resolver rewrites paths into slot indices / item ids in place.

use crate::format::Spec;
use crate::value::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

pub type Sym = u32;

thread_local! {
    static INTERNER: RefCell<(HashMap<String, Sym>, Vec<String>)> = RefCell::new((HashMap::new(), Vec::new()));
}

pub fn intern(s: &str) -> Sym {
    INTERNER.with(|i| {
        let mut i = i.borrow_mut();
        if let Some(&id) = i.0.get(s) {
            return id;
        }
        let id = i.1.len() as Sym;
        i.1.push(s.to_string());
        i.0.insert(s.to_string(), id);
        id
    })
}

pub fn sym_str(s: Sym) -> String {
    INTERNER.with(|i| i.borrow().1[s as usize].clone())
}

/// A (simplified) type: `name` is the last path segment, or one of the
/// special names `&`, `&mut`, `*`, `tuple`, `array`, `slice`, `fn`, `_`, `!`,
/// `dyn`, `impl`. Types are only used as hints (casts, `collect`, `parse`,
/// `Default`), never checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub name: String,
    pub args: Vec<Type>,
}

impl Type {
    pub fn new(name: &str, args: Vec<Type>) -> Type {
        Type { name: name.to_string(), args }
    }
    /// Strip references.
    pub fn base(&self) -> &Type {
        if (self.name == "&" || self.name == "&mut" || self.name == "Box") && !self.args.is_empty() {
            self.args[0].base()
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Unresolved,
    Struct(u32),
    Variant(u32),
}

pub enum Pat {
    Wild,
    Rest,
    Bind { name: String, slot: u32, by_ref: bool, mutable: bool, sub: Option<Box<Pat>> },
    Lit(Value),
    Range(Option<Value>, Option<Value>, bool),
    /// Range pattern with constant-path bounds (e.g. `i32::MIN..=-1`).
    RangeExpr(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    Tuple(Vec<Pat>),
    TupleStruct { path: Vec<String>, target: Target, elems: Vec<Pat> },
    Struct { path: Vec<String>, target: Target, fields: Vec<(String, u32, Pat)>, rest: bool },
    /// Path pattern; resolved to a constant value to compare against.
    Path { path: Vec<String>, value: Option<Box<Expr>> },
    Or(Vec<Pat>),
    Ref(Box<Pat>),
    Slice(Vec<Pat>),
}

pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
    pub col: u32,
}

pub struct PathExpr {
    pub segs: Vec<String>,
    /// Turbofish / generic arguments on the last segments.
    pub generics: Vec<Type>,
}

pub enum Cond {
    Expr(Expr),
    Let(Pat, Expr),
}

pub struct Arm {
    pub pat: Pat,
    pub guard: Option<Vec<Cond>>,
    pub body: Expr,
}

pub struct MethodCall {
    pub recv: Expr,
    pub name: String,
    pub sym: Sym,
    /// Builtin method id (precomputed from `name`).
    pub mid: crate::builtins::M,
    pub args: Vec<Expr>,
    pub turbofish: Option<Type>,
    pub hint: Option<Rc<Type>>,
    /// Inline cache: (type key + 1, function id).
    pub cache: Cell<(u32, u32)>,
}

pub struct StructLit {
    pub path: Vec<String>,
    pub target: Target,
    pub fields: Vec<(String, u32, Expr)>,
    pub base: Option<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroKind {
    Print,
    Println,
    Eprint,
    Eprintln,
    Format,
    Panic,
    Write,
    Writeln,
    Assert,
    AssertEq,
    AssertNe,
    Unreachable,
    Todo,
    Unimplemented,
    Dbg,
}

pub enum FmtPiece {
    Lit(String),
    Arg { arg: usize, spec: Spec },
}

pub struct MacroCall {
    pub kind: MacroKind,
    /// Write target (write!/writeln!) or assertion operands.
    pub operands: Vec<Expr>,
    pub pieces: Option<Vec<FmtPiece>>,
    pub args: Vec<Expr>,
    pub text: String,
}

pub enum ExprKind {
    Lit(Value),
    Path(Box<PathExpr>),
    Local(u32),
    Cap(u32),
    Static(u32),
    Const(u32),
    Block(Box<Block>),
    If(Box<(Vec<Cond>, Block, Option<Expr>)>),
    Match(Box<(Expr, Vec<Arm>)>),
    While(Box<(Option<Sym>, Vec<Cond>, Block)>),
    Loop(Box<(Option<Sym>, Block)>),
    For(Box<(Option<Sym>, Pat, Expr, Block)>),
    Break(Option<Sym>, Option<Box<Expr>>),
    Continue(Option<Sym>),
    Return(Option<Box<Expr>>),
    Call(Box<Expr>, Vec<Expr>),
    CallFn(u32, Vec<Expr>),
    CallBuiltin(u32, Vec<Expr>, Option<Rc<Type>>),
    MakeVariant(u32, Vec<Expr>),
    MakeStruct(u32, Vec<Expr>),
    MethodCall(Box<MethodCall>),
    Field(Box<Expr>, Sym, Cell<(u32, u32)>),
    TupleField(Box<Expr>, u32),
    Index(Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(BinOp, Box<Expr>, Box<Expr>),
    Cast(Box<Expr>, Type),
    Ref(bool, Box<Expr>),
    Deref(Box<Expr>),
    StructLit(Box<StructLit>),
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    Repeat(Box<Expr>, Box<Expr>),
    VecLit(Vec<Expr>),
    VecRepeat(Box<Expr>, Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    Closure(Rc<FnDecl>),
    Try(Box<Expr>),
    Macro(Box<MacroCall>),
    Matches(Box<(Expr, Pat, Option<Expr>)>),
    // Superinstructions (introduced by the resolver for common shapes).
    /// `local <op> local`
    BinLL(BinOp, u32, u32),
    /// `local <op> int-literal`
    BinLI(BinOp, u32, i64),
    /// `local = expr`
    AssignLocal(u32, Box<Expr>),
    /// `local <op>= expr`
    OpAssignLocal(BinOp, u32, Box<Expr>),
    /// Destructuring assignment: `(a, b) = expr`, lowered to a pattern match.
    DestructAssign(Box<(Pat, Expr)>),
    Let(Box<(Pat, Expr)>),
}

pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub label: Option<Sym>,
    pub items: Vec<Item>,
}

pub enum Stmt {
    Let(Box<LetStmt>),
    Expr(Expr, bool),
}

pub struct LetStmt {
    pub pat: Pat,
    pub ty: Option<Type>,
    pub init: Option<Expr>,
    pub els: Option<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfKind {
    None,
    Value,
    Ref,
    RefMut,
}

pub struct Param {
    pub pat: Pat,
    pub ty: Option<Type>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapSrc {
    Local(u32),
    Cap(u32),
}

pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Option<Block>,
    pub self_kind: SelfKind,
    pub nslots: u32,
    pub is_closure: bool,
    pub is_move: bool,
    pub caps: Vec<CapSrc>,
    /// For each param: Some(slot) when the param is a simple binding stored
    /// directly in its argument slot.
    pub line: u32,
}

pub struct StructDecl {
    pub name: String,
    pub kind: StructKind,
    pub fields: Vec<(String, Type)>,
    pub derives: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructKind {
    Named,
    Tuple,
    Unit,
}

pub struct VariantDecl {
    pub name: String,
    pub kind: StructKind,
    pub fields: Vec<(String, Type)>,
    pub disc: Option<Expr>,
}

pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<VariantDecl>,
    pub derives: Vec<String>,
}

pub struct ImplDecl {
    pub self_ty: Type,
    pub trait_name: Option<String>,
    pub fns: Vec<FnDecl>,
    pub consts: Vec<ConstDecl>,
}

pub struct TraitDecl {
    pub name: String,
    pub fns: Vec<FnDecl>,
    pub consts: Vec<ConstDecl>,
}

pub struct ConstDecl {
    pub name: String,
    pub ty: Option<Type>,
    pub init: Option<Expr>,
    pub is_static: bool,
}

pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Impl(ImplDecl),
    Trait(TraitDecl),
    Const(ConstDecl),
    /// `use a::b::c as d;` → (alias, full path)
    Use(Vec<(String, Vec<String>)>),
    Mod(String, Vec<Item>),
    FileMod(String),
    TypeAlias(String, Type),
}

impl Expr {
    pub fn new(kind: ExprKind, line: u32, col: u32) -> Expr {
        Expr { kind, line, col }
    }
    /// Is this a syntactic place expression?
    pub fn is_place(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::Local(_)
                | ExprKind::Cap(_)
                | ExprKind::Static(_)
                | ExprKind::Field(..)
                | ExprKind::TupleField(..)
                | ExprKind::Index(..)
                | ExprKind::Deref(_)
        )
    }
}
