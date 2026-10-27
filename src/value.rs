//! Runtime values.
//!
//! Values are dynamically typed. Compound data lives in reference-counted
//! containers; Rust's value semantics are recovered by copying `Copy`
//! containers when they are read in a value context (see `Interp::copy_val`).
//! References are places: a stack slot, an element of a container, or a cell.

use crate::ast::FnDecl;
use crate::iter::IterState;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::rc::Rc;

pub type List = Rc<RefCell<Vec<Value>>>;
pub type VCell = Rc<RefCell<Value>>;

#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Char(char),
    /// Immutable string slice (`&str`, string literals).
    Str(Rc<String>),
    /// Owned, growable `String`.
    String(Rc<RefCell<String>>),
    Tuple(List),
    Array(List),
    Vec(List),
    /// A borrowed sub-slice view `&v[a..b]` of a list.
    Slice(Rc<SliceV>),
    /// Struct instance: definition id + fields.
    Struct(u32, List),
    /// Enum instance: global variant id + payload (None for unit variants).
    Enum(u32, Option<List>),
    Map(Rc<RefCell<MapData>>),
    Set(Rc<RefCell<SetData>>),
    Deque(Rc<RefCell<VecDeque<Value>>>),
    /// BinaryHeap (max-heap ordered by `Interp::compare`).
    Heap(List),
    Range(Rc<RangeVal>),
    /// A function item (index into the program's function table).
    Fn(u32),
    /// A tuple-struct / tuple-variant constructor used as a function value.
    Ctor(u32, bool),
    /// A builtin function used as a value (e.g. `.map(String::from)`).
    BuiltinFn(u32),
    Closure(Rc<Closure>),
    Iter(Rc<RefCell<IterState>>),
    /// `Rc<T>` / `Arc<T>`: clone shares.
    RcBox(VCell),
    /// `RefCell<T>` / `Cell<T>` / `Mutex<T>`: clone copies.
    CellBox(VCell),
    /// References.
    RefSlot(u32),
    RefElem(List, u32),
    RefBox(VCell),
    Formatter(Rc<FmtTarget>),
    Entry(Rc<(Value, Value)>),
    /// A value whose type is not known yet (e.g. `or_default()` on a map
    /// whose value type could not be inferred). Materialized on first use.
    Lazy,
    Instant(f64),
    Stdin,
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::Int(i) => write!(f, "Int({})", i),
            Value::Float(x) => write!(f, "Float({})", x),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::Char(c) => write!(f, "Char({:?})", c),
            Value::Str(s) => write!(f, "Str({:?})", s),
            Value::String(s) => write!(f, "String({:?})", s.borrow()),
            other => write!(f, "<{}>", other.type_name()),
        }
    }
}

pub struct FmtTarget {
    pub buf: RefCell<String>,
    pub spec: crate::format::Spec,
}

pub struct Closure {
    pub fd: Rc<FnDecl>,
    pub caps: List,
}

pub struct SliceV {
    pub list: List,
    pub start: usize,
    pub end: usize,
}

pub struct RangeVal {
    pub lo: Option<Value>,
    pub hi: Option<Value>,
    pub inclusive: bool,
}

pub enum MapData {
    Hash(HashMap<Key, VCell, FxBuild>),
    Tree(BTreeMap<Key, VCell>),
}

pub enum SetData {
    Hash(HashSet<Key, FxBuild>),
    Tree(BTreeSet<Key>),
}

impl MapData {
    pub fn len(&self) -> usize {
        match self {
            MapData::Hash(m) => m.len(),
            MapData::Tree(m) => m.len(),
        }
    }
    pub fn get(&self, k: &Key) -> Option<&VCell> {
        match self {
            MapData::Hash(m) => m.get(k),
            MapData::Tree(m) => m.get(k),
        }
    }
    pub fn insert(&mut self, k: Key, v: VCell) -> Option<VCell> {
        match self {
            MapData::Hash(m) => m.insert(k, v),
            MapData::Tree(m) => m.insert(k, v),
        }
    }
    pub fn remove(&mut self, k: &Key) -> Option<VCell> {
        match self {
            MapData::Hash(m) => m.remove(k),
            MapData::Tree(m) => m.remove(k),
        }
    }
    pub fn entries(&self) -> Vec<(Value, VCell)> {
        match self {
            MapData::Hash(m) => m.iter().map(|(k, v)| (k.0.clone(), v.clone())).collect(),
            MapData::Tree(m) => m.iter().map(|(k, v)| (k.0.clone(), v.clone())).collect(),
        }
    }
    pub fn clear(&mut self) {
        match self {
            MapData::Hash(m) => m.clear(),
            MapData::Tree(m) => m.clear(),
        }
    }
    pub fn is_tree(&self) -> bool {
        matches!(self, MapData::Tree(_))
    }
}

impl SetData {
    pub fn len(&self) -> usize {
        match self {
            SetData::Hash(m) => m.len(),
            SetData::Tree(m) => m.len(),
        }
    }
    pub fn contains(&self, k: &Key) -> bool {
        match self {
            SetData::Hash(m) => m.contains(k),
            SetData::Tree(m) => m.contains(k),
        }
    }
    pub fn insert(&mut self, k: Key) -> bool {
        match self {
            SetData::Hash(m) => m.insert(k),
            SetData::Tree(m) => m.insert(k),
        }
    }
    pub fn remove(&mut self, k: &Key) -> bool {
        match self {
            SetData::Hash(m) => m.remove(k),
            SetData::Tree(m) => m.remove(k),
        }
    }
    pub fn items(&self) -> Vec<Value> {
        match self {
            SetData::Hash(m) => m.iter().map(|k| k.0.clone()).collect(),
            SetData::Tree(m) => m.iter().map(|k| k.0.clone()).collect(),
        }
    }
    pub fn clear(&mut self) {
        match self {
            SetData::Hash(m) => m.clear(),
            SetData::Tree(m) => m.clear(),
        }
    }
    pub fn is_tree(&self) -> bool {
        matches!(self, SetData::Tree(_))
    }
}

/// Simple, fast non-cryptographic hasher (FxHash).
#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}
const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.hash = (self.hash.rotate_left(5) ^ w).wrapping_mul(SEED);
    }
}
impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(b));
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64)
    }
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64)
    }
    fn write_u64(&mut self, i: u64) {
        self.add(i)
    }
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64)
    }
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64)
    }
    fn finish(&self) -> u64 {
        self.hash
    }
}
pub type FxBuild = BuildHasherDefault<FxHasher>;

/// A fully dereferenced, owned value usable as a map key (structural
/// equality, ordering and hashing).
#[derive(Clone)]
pub struct Key(pub Value);

impl PartialEq for Key {
    fn eq(&self, o: &Key) -> bool {
        struct_eq(&self.0, &o.0)
    }
}
impl Eq for Key {}
impl Hash for Key {
    fn hash<H: Hasher>(&self, h: &mut H) {
        hash_value(&self.0, h)
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, o: &Key) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Key {
    fn cmp(&self, o: &Key) -> std::cmp::Ordering {
        struct_cmp(&self.0, &o.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

fn hash_value<H: Hasher>(v: &Value, h: &mut H) {
    if let Value::Slice(sv) = v {
        for x in sv.list.borrow()[sv.start..sv.end].iter() {
            hash_value(x, h);
        }
        return;
    }
    match v {
        Value::Unit => h.write_u8(0),
        Value::Bool(b) => h.write_u8(*b as u8 + 1),
        Value::Int(i) => h.write_i64(*i),
        Value::Float(f) => h.write_u64(f.to_bits()),
        Value::Char(c) => h.write_u32(*c as u32),
        Value::Str(s) => s.hash(h),
        Value::String(s) => s.borrow().as_str().hash(h),
        Value::Tuple(l) | Value::Array(l) | Value::Vec(l) | Value::Struct(_, l) | Value::Heap(l) => {
            for x in l.borrow().iter() {
                hash_value(x, h);
            }
        }
        Value::Enum(vid, p) => {
            h.write_u32(*vid);
            if let Some(l) = p {
                for x in l.borrow().iter() {
                    hash_value(x, h);
                }
            }
        }
        Value::RcBox(c) | Value::CellBox(c) | Value::RefBox(c) => hash_value(&c.borrow(), h),
        Value::RefElem(l, i) => hash_value(&l.borrow()[*i as usize], h),
        Value::Set(s) => {
            for k in s.borrow().items() {
                hash_value(&k, h);
            }
        }
        Value::Deque(d) => {
            for x in d.borrow().iter() {
                hash_value(x, h);
            }
        }
        _ => h.write_u8(255),
    }
}

fn list_eq(a: &List, b: &List) -> bool {
    if Rc::ptr_eq(a, b) {
        return true;
    }
    let (a, b) = (a.borrow(), b.borrow());
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| struct_eq(x, y))
}

/// Structural equality (used for map keys; `Interp::equals` additionally
/// honours user `PartialEq` impls).
pub fn struct_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    if matches!(a, Slice(_)) || matches!(b, Slice(_)) {
        let (Some(x), Some(y)) = (seq_items(a), seq_items(b)) else { return false };
        return x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| struct_eq(p, q));
    }
    match (a, b) {
        (Unit, Unit) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Int(x), Float(y)) | (Float(y), Int(x)) => (*x as f64) == *y,
        (Char(x), Char(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (String(x), String(y)) => *x.borrow() == *y.borrow(),
        (Str(x), String(y)) | (String(y), Str(x)) => **x == *y.borrow(),
        (Tuple(x), Tuple(y)) => list_eq(x, y),
        (Array(x) | Vec(x), Array(y) | Vec(y)) => list_eq(x, y),
        (Struct(d1, x), Struct(d2, y)) => d1 == d2 && list_eq(x, y),
        (Enum(v1, x), Enum(v2, y)) => {
            v1 == v2
                && match (x, y) {
                    (Some(x), Some(y)) => list_eq(x, y),
                    (None, None) => true,
                    _ => false,
                }
        }
        (Deque(x), Deque(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| struct_eq(a, b))
        }
        (Map(x), Map(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len()
                && x.entries().iter().all(|(k, v)| y.get(&Key(k.clone())).is_some_and(|w| struct_eq(&v.borrow(), &w.borrow())))
        }
        (Set(x), Set(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.items().into_iter().all(|k| y.contains(&Key(k)))
        }
        (RcBox(x), RcBox(y)) | (CellBox(x), CellBox(y)) => struct_eq(&x.borrow(), &y.borrow()),
        (Range(x), Range(y)) => {
            x.inclusive == y.inclusive
                && opt_eq(&x.lo, &y.lo)
                && opt_eq(&x.hi, &y.hi)
        }
        (Fn(x), Fn(y)) => x == y,
        (RefBox(x), _) => struct_eq(&x.borrow(), b),
        (_, RefBox(y)) => struct_eq(a, &y.borrow()),
        (RefElem(l, i), _) => struct_eq(&l.borrow()[*i as usize], b),
        (_, RefElem(l, i)) => struct_eq(a, &l.borrow()[*i as usize]),
        _ => false,
    }
}

fn opt_eq(a: &Option<Value>, b: &Option<Value>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => struct_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

fn list_cmp(a: &[Value], b: &[Value]) -> Option<std::cmp::Ordering> {
    for (x, y) in a.iter().zip(b.iter()) {
        match struct_cmp(x, y)? {
            std::cmp::Ordering::Equal => {}
            o => return Some(o),
        }
    }
    Some(a.len().cmp(&b.len()))
}

/// Structural ordering (derive(PartialOrd) semantics). Variant ids of one enum
/// are allocated consecutively in declaration order, so comparing ids orders
/// variants correctly.
pub fn struct_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use Value::*;
    if matches!(a, Slice(_)) || matches!(b, Slice(_)) {
        let (Some(x), Some(y)) = (seq_items(a), seq_items(b)) else { return None };
        return list_cmp(&x, &y);
    }
    match (a, b) {
        (Unit, Unit) => Some(std::cmp::Ordering::Equal),
        (Bool(x), Bool(y)) => x.partial_cmp(y),
        (Int(x), Int(y)) => Some(x.cmp(y)),
        (Float(x), Float(y)) => x.partial_cmp(y),
        (Int(x), Float(y)) => (*x as f64).partial_cmp(y),
        (Float(x), Int(y)) => x.partial_cmp(&(*y as f64)),
        (Char(x), Char(y)) => Some(x.cmp(y)),
        (Str(x), Str(y)) => Some(x.cmp(y)),
        (String(x), String(y)) => Some(x.borrow().as_str().cmp(y.borrow().as_str())),
        (Str(x), String(y)) => Some(x.as_str().cmp(y.borrow().as_str())),
        (String(x), Str(y)) => Some(x.borrow().as_str().cmp(y.as_str())),
        (Tuple(x), Tuple(y)) | (Array(x) | Vec(x), Array(y) | Vec(y)) | (Struct(_, x), Struct(_, y)) => {
            list_cmp(&x.borrow(), &y.borrow())
        }
        (Enum(v1, x), Enum(v2, y)) => {
            if v1 != v2 {
                return Some(v1.cmp(v2));
            }
            match (x, y) {
                (Some(x), Some(y)) => list_cmp(&x.borrow(), &y.borrow()),
                _ => Some(std::cmp::Ordering::Equal),
            }
        }
        (RcBox(x), RcBox(y)) | (CellBox(x), CellBox(y)) => struct_cmp(&x.borrow(), &y.borrow()),
        (RefBox(x), _) => struct_cmp(&x.borrow(), b),
        (_, RefBox(y)) => struct_cmp(a, &y.borrow()),
        (RefElem(l, i), _) => struct_cmp(&l.borrow()[*i as usize], b),
        (_, RefElem(l, i)) => struct_cmp(a, &l.borrow()[*i as usize]),
        (Set(x), Set(y)) => {
            let (x, y) = (x.borrow().items(), y.borrow().items());
            list_cmp(&x, &y)
        }
        (Deque(x), Deque(y)) => {
            let x: std::vec::Vec<Value> = x.borrow().iter().cloned().collect();
            let y: std::vec::Vec<Value> = y.borrow().iter().cloned().collect();
            list_cmp(&x, &y)
        }
        _ => None,
    }
}

/// Elements of a sequence value when at least one side is a slice view.
fn seq_items(v: &Value) -> Option<Vec<Value>> {
    match v {
        Value::Slice(s) => Some(s.list.borrow()[s.start..s.end].to_vec()),
        Value::Vec(l) | Value::Array(l) => Some(l.borrow().clone()),
        _ => None,
    }
}

pub fn new_list(v: Vec<Value>) -> List {
    Rc::new(RefCell::new(v))
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::new(s.to_string()))
    }
    pub fn string(s: String) -> Value {
        Value::String(Rc::new(RefCell::new(s)))
    }
    pub fn tuple(v: Vec<Value>) -> Value {
        Value::Tuple(new_list(v))
    }
    pub fn vec(v: Vec<Value>) -> Value {
        Value::Vec(new_list(v))
    }
    pub fn some(v: Value) -> Value {
        Value::Enum(crate::program::V_SOME, Some(new_list(vec![v])))
    }
    pub fn none() -> Value {
        Value::Enum(crate::program::V_NONE, None)
    }
    pub fn ok(v: Value) -> Value {
        Value::Enum(crate::program::V_OK, Some(new_list(vec![v])))
    }
    pub fn err(v: Value) -> Value {
        Value::Enum(crate::program::V_ERR, Some(new_list(vec![v])))
    }
    pub fn opt(v: Option<Value>) -> Value {
        match v {
            Some(v) => Value::some(v),
            None => Value::none(),
        }
    }
    pub fn is_ref(&self) -> bool {
        matches!(self, Value::RefSlot(_) | Value::RefElem(..) | Value::RefBox(_))
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Unit => "()",
            Value::Bool(_) => "bool",
            Value::Int(_) => "integer",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Str(_) => "&str",
            Value::String(_) => "String",
            Value::Tuple(_) => "tuple",
            Value::Array(_) => "array",
            Value::Vec(_) => "Vec",
            Value::Slice(_) => "slice",
            Value::Struct(..) => "struct",
            Value::Enum(..) => "enum",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Deque(_) => "VecDeque",
            Value::Heap(_) => "BinaryHeap",
            Value::Range(_) => "range",
            Value::Fn(_) | Value::Ctor(..) | Value::BuiltinFn(_) => "fn",
            Value::Closure(_) => "closure",
            Value::Iter(_) => "iterator",
            Value::RcBox(_) => "Rc",
            Value::CellBox(_) => "RefCell",
            Value::RefSlot(_) | Value::RefElem(..) | Value::RefBox(_) => "reference",
            Value::Formatter(_) => "Formatter",
            Value::Entry(_) => "Entry",
            Value::Lazy => "default",
            Value::Instant(_) => "Instant",
            Value::Stdin => "Stdin",
        }
    }
}
