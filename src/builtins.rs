//! Standard-library emulation: builtin functions, constants and methods.

use crate::ast::{BinOp, Expr, Sym, Type};
use crate::interp::{err, Ctrl, Interp, Place, R};
use crate::iter::IterState;
use crate::program::*;
use crate::value::*;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::io::{BufRead, Read};
use std::rc::Rc;

macro_rules! methods {
    ($($name:ident),* $(,)?) => {
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum M { $($name,)* Unknown }
        pub fn method_id(s: &str) -> M {
            match s { $(stringify!($name) => M::$name,)* _ => M::Unknown }
        }
    };
}

methods!(
    // general
    clone, to_string, to_owned, into, try_into, as_ref, as_mut, borrow, borrow_mut, eq, ne, cmp, partial_cmp, lt, le,
    gt, ge, max, min, clamp, hash, iter, iter_mut, into_iter, len, is_empty, get, get_mut, contains, push, pop,
    insert, remove, clear, extend, append, first, last, first_mut, last_mut, sort, sort_unstable, sort_by,
    sort_by_key, sort_unstable_by, sort_unstable_by_key, sort_by_cached_key, reverse, dedup, dedup_by_key, retain,
    truncate, drain, split_off, swap, swap_remove, resize, join, concat, contains_key, keys, values, values_mut,
    entry, or_insert, or_insert_with, or_default, and_modify, or_insert_with_key, key, push_back, push_front,
    pop_back, pop_front, front, back, peek, into_sorted_vec, into_vec, windows, chunks, chunks_exact, to_vec,
    binary_search, binary_search_by, binary_search_by_key, starts_with, ends_with, split_at, split_first,
    split_last, fill, rotate_left, rotate_right, repeat, capacity, reserve, shrink_to_fit, as_slice, as_mut_slice,
    copy_from_slice, clone_from_slice, iter_count, into_keys, into_values, remove_entry, first_key_value,
    last_key_value, pop_first, pop_last, range, make_contiguous, is_sorted, union, intersection, difference,
    symmetric_difference, is_subset, is_superset, is_disjoint, take, replace, extend_from_slice, position,
    // strings
    as_str, as_bytes, bytes, chars, char_indices, lines, split, split_whitespace, splitn, rsplit, rsplitn,
    split_terminator, trim, trim_start, trim_end, trim_matches, trim_start_matches, trim_end_matches,
    strip_prefix, strip_suffix, to_uppercase, to_lowercase, to_ascii_uppercase, to_ascii_lowercase, replacen,
    find, rfind, parse, push_str, is_char_boundary, matches, eq_ignore_ascii_case, is_ascii, split_once,
    rsplit_once, into_bytes, insert_str, char_at, as_string, nth_char,
    // char
    is_alphabetic, is_numeric, is_alphanumeric, is_whitespace, is_uppercase, is_lowercase, is_ascii_digit,
    is_ascii_alphabetic, is_ascii_alphanumeric, is_ascii_uppercase, is_ascii_lowercase, is_ascii_punctuation,
    is_ascii_whitespace, is_ascii_hexdigit, is_ascii_graphic, is_control, is_digit, to_digit, len_utf8,
    // ints
    abs, pow, checked_add, checked_sub, checked_mul, checked_div, checked_rem, checked_pow, checked_neg,
    wrapping_add, wrapping_sub, wrapping_mul, wrapping_neg, wrapping_pow, wrapping_div, wrapping_rem,
    saturating_add, saturating_sub, saturating_mul, saturating_pow, overflowing_add, overflowing_sub,
    overflowing_mul, rem_euclid, div_euclid, signum, is_positive, is_negative, count_ones, count_zeros,
    leading_zeros, trailing_zeros, abs_diff, isqrt, is_power_of_two, next_power_of_two, ilog2, ilog10,
    to_be_bytes, to_le_bytes, unsigned_abs, wrapping_shl, wrapping_shr, rotate_left_bits,
    // floats
    sqrt, cbrt, powi, powf, exp, exp2, ln, log10, log2, log, sin, cos, tan, asin, acos, atan, atan2, sinh, cosh,
    tanh, floor, ceil, round, trunc, fract, is_nan, is_infinite, is_finite, hypot, to_degrees, to_radians,
    mul_add, recip, total_cmp, is_sign_negative, is_sign_positive, to_bits,
    // bool
    then, then_some,
    // option / result
    is_some, is_none, is_some_and, is_none_or, is_ok, is_err, is_ok_and, is_err_and, unwrap, expect, unwrap_or,
    unwrap_or_else, unwrap_or_default, unwrap_err, expect_err, map, map_err, map_or, map_or_else, and_then,
    or_else, or, and, ok, err, ok_or, ok_or_else, filter, get_or_insert, get_or_insert_with, as_deref,
    as_deref_mut, cloned, copied, flatten, zip, xor, unwrap_unchecked, inspect,
    // ordering
    then_with, is_lt, is_le, is_gt, is_ge, is_eq, is_ne,
    // cells
    get_ref, set, into_inner, lock, unwrap_or_clone, write, read,
    // iterators
    next, next_back, next_if, filter_map, enumerate, chain, skip, step_by, take_while, skip_while, map_while,
    flat_map, peekable, scan, rev, cycle, fuse, by_ref, collect, sum, product, count, nth, for_each, fold,
    reduce, any, all, find_map, rposition, max_by_key, min_by_key, max_by, min_by, partition, unzip, size_hint,
    // formatter
    write_str, write_char, write_fmt, pad, width, precision, alternate, debug_struct, field, finish,
    // misc
    elapsed, as_secs_f64, as_secs_f32, as_millis, as_secs, as_micros, as_nanos, duration_since, read_line,
    read_to_string, start, end, flush,
);

/// `BuiltinFn` ids at or above this encode "call method `id - BASE` on the
/// first argument" (e.g. `.map(str::trim)`).
pub const METHOD_FN_BASE: u32 = 1 << 24;

pub mod bf {
    pub const VEC_NEW: u32 = 0;
    pub const VEC_FROM: u32 = 1;
    pub const STRING_NEW: u32 = 2;
    pub const STRING_FROM: u32 = 3;
    pub const STRING_FROM_UTF8: u32 = 4;
    pub const STRING_FROM_UTF8_LOSSY: u32 = 5;
    pub const HASHMAP_NEW: u32 = 6;
    pub const HASHSET_NEW: u32 = 7;
    pub const BTREEMAP_NEW: u32 = 8;
    pub const BTREESET_NEW: u32 = 9;
    pub const DEQUE_NEW: u32 = 10;
    pub const HEAP_NEW: u32 = 11;
    pub const HASHMAP_FROM: u32 = 12;
    pub const HASHSET_FROM: u32 = 13;
    pub const BTREEMAP_FROM: u32 = 14;
    pub const BTREESET_FROM: u32 = 15;
    pub const DEQUE_FROM: u32 = 16;
    pub const HEAP_FROM: u32 = 17;
    pub const IDENTITY: u32 = 18;
    pub const RC_NEW: u32 = 19;
    pub const RC_CLONE: u32 = 20;
    pub const RC_STRONG_COUNT: u32 = 21;
    pub const RC_PTR_EQ: u32 = 22;
    pub const CELL_NEW: u32 = 23;
    pub const MEM_SWAP: u32 = 24;
    pub const MEM_REPLACE: u32 = 25;
    pub const MEM_TAKE: u32 = 26;
    pub const DROP: u32 = 27;
    pub const MAX: u32 = 28;
    pub const MIN: u32 = 29;
    pub const EXIT: u32 = 30;
    pub const STDIN: u32 = 31;
    pub const CHAR_FROM_U32: u32 = 32;
    pub const CHAR_FROM_DIGIT: u32 = 33;
    pub const CHAR_FROM: u32 = 34;
    pub const INT_FROM: u32 = 35;
    pub const FLOAT_FROM: u32 = 36;
    pub const FROM_STR_RADIX: u32 = 37;
    pub const DEFAULT: u32 = 38;
    pub const ITER_REPEAT: u32 = 39;
    pub const ITER_ONCE: u32 = 40;
    pub const ITER_EMPTY: u32 = 41;
    pub const ITER_FROM_FN: u32 = 42;
    pub const ITER_SUCCESSORS: u32 = 43;
    pub const ITER_ZIP: u32 = 44;
    pub const ENV_ARGS: u32 = 45;
    pub const INSTANT_NOW: u32 = 46;
    pub const DURATION_SECS: u32 = 47;
    pub const DURATION_MILLIS: u32 = 48;
    pub const SLEEP: u32 = 49;
    pub const INT_TRY_FROM: u32 = 50;
    pub const STRING_FROM_ITER: u32 = 51;
    pub const VEC_FROM_ITER: u32 = 52;
    pub const DURATION_MICROS: u32 = 53;
    pub const DURATION_NANOS: u32 = 54;
    pub const SWAP_FN: u32 = 55;
    pub const IO_READ_TO_STRING: u32 = 56;
    pub const FLUSH: u32 = 57;
    pub const STDOUT: u32 = 58;
    pub const BLACK_BOX: u32 = 59;
}

pub enum StaticRes {
    Fn(u32),
    Value(Value),
}

const INT_TYPES: &[&str] = &["i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize"];

fn int_max(t: &str) -> i64 {
    match t {
        "i8" => i8::MAX as i64,
        "i16" => i16::MAX as i64,
        "i32" => i32::MAX as i64,
        "u8" => u8::MAX as i64,
        "u16" => u16::MAX as i64,
        "u32" => u32::MAX as i64,
        _ => i64::MAX,
    }
}

fn int_min(t: &str) -> i64 {
    match t {
        "i8" => i8::MIN as i64,
        "i16" => i16::MIN as i64,
        "i32" => i32::MIN as i64,
        t if t.starts_with('u') => 0,
        _ => i64::MIN,
    }
}

pub fn lookup_const(name: &str) -> Option<Value> {
    use std::f64::consts::*;
    Some(Value::Float(match name {
        "PI" => PI,
        "E" => E,
        "TAU" => TAU,
        "SQRT_2" => SQRT_2,
        "LN_2" => LN_2,
        "LN_10" => LN_10,
        "FRAC_PI_2" => FRAC_PI_2,
        "FRAC_PI_3" => FRAC_PI_3,
        "FRAC_PI_4" => FRAC_PI_4,
        "FRAC_PI_6" => FRAC_PI_6,
        "FRAC_PI_8" => FRAC_PI_8,
        "FRAC_1_PI" => FRAC_1_PI,
        "FRAC_2_PI" => FRAC_2_PI,
        "FRAC_1_SQRT_2" => FRAC_1_SQRT_2,
        "LOG2_E" => LOG2_E,
        "LOG10_E" => LOG10_E,
        "LOG2_10" => LOG2_10,
        "LOG10_2" => LOG10_2,
        _ => return None,
    }))
}

pub fn lookup_static(ty: &str, item: &str) -> Option<StaticRes> {
    use bf::*;
    use StaticRes::Fn as F;
    if INT_TYPES.contains(&ty) {
        return Some(match item {
            "MAX" => StaticRes::Value(Value::Int(int_max(ty))),
            "MIN" => StaticRes::Value(Value::Int(int_min(ty))),
            "BITS" => StaticRes::Value(Value::Int(match ty {
                "i8" | "u8" => 8,
                "i16" | "u16" => 16,
                "i32" | "u32" => 32,
                "i128" | "u128" => 128,
                _ => 64,
            })),
            "from" => F(INT_FROM),
            "try_from" => F(INT_TRY_FROM),
            "from_str_radix" => F(FROM_STR_RADIX),
            _ => return None,
        });
    }
    Some(match (ty, item) {
        ("f64" | "f32", c) => match c {
            "MAX" => StaticRes::Value(Value::Float(if ty == "f32" { f32::MAX as f64 } else { f64::MAX })),
            "MIN" => StaticRes::Value(Value::Float(if ty == "f32" { f32::MIN as f64 } else { f64::MIN })),
            "INFINITY" => StaticRes::Value(Value::Float(f64::INFINITY)),
            "NEG_INFINITY" => StaticRes::Value(Value::Float(f64::NEG_INFINITY)),
            "NAN" => StaticRes::Value(Value::Float(f64::NAN)),
            "EPSILON" => StaticRes::Value(Value::Float(if ty == "f32" { f32::EPSILON as f64 } else { f64::EPSILON })),
            "MIN_POSITIVE" => StaticRes::Value(Value::Float(f64::MIN_POSITIVE)),
            "from" => F(FLOAT_FROM),
            _ => return lookup_const(c).map(StaticRes::Value),
        },
        ("Vec", "new" | "with_capacity") => F(VEC_NEW),
        ("Vec", "from") => F(VEC_FROM),
        ("Vec", "from_iter") => F(VEC_FROM_ITER),
        ("String", "new" | "with_capacity") => F(STRING_NEW),
        ("String" | "str", "from") => F(STRING_FROM),
        ("String", "from_utf8") => F(STRING_FROM_UTF8),
        ("String", "from_utf8_lossy") => F(STRING_FROM_UTF8_LOSSY),
        ("String", "from_iter") => F(STRING_FROM_ITER),
        ("HashMap", "new" | "with_capacity") => F(HASHMAP_NEW),
        ("HashSet", "new" | "with_capacity") => F(HASHSET_NEW),
        ("BTreeMap", "new") => F(BTREEMAP_NEW),
        ("BTreeSet", "new") => F(BTREESET_NEW),
        ("VecDeque", "new" | "with_capacity") => F(DEQUE_NEW),
        ("BinaryHeap", "new" | "with_capacity") => F(HEAP_NEW),
        ("HashMap", "from" | "from_iter") => F(HASHMAP_FROM),
        ("HashSet", "from" | "from_iter") => F(HASHSET_FROM),
        ("BTreeMap", "from" | "from_iter") => F(BTREEMAP_FROM),
        ("BTreeSet", "from" | "from_iter") => F(BTREESET_FROM),
        ("VecDeque", "from" | "from_iter") => F(DEQUE_FROM),
        ("BinaryHeap", "from" | "from_iter") => F(HEAP_FROM),
        ("Box", "new") | ("Cow", "from") | ("Some", _) => F(IDENTITY),
        ("Rc" | "Arc", "new") => F(RC_NEW),
        ("Rc" | "Arc", "clone") => F(RC_CLONE),
        ("Rc" | "Arc", "strong_count") => F(RC_STRONG_COUNT),
        ("Rc" | "Arc", "ptr_eq") => F(RC_PTR_EQ),
        ("RefCell" | "Cell" | "Mutex" | "RwLock" | "UnsafeCell" | "OnceCell", "new") => F(CELL_NEW),
        ("mem", "swap") => F(MEM_SWAP),
        ("mem", "replace") => F(MEM_REPLACE),
        ("mem", "take") => F(MEM_TAKE),
        ("mem", "drop") => F(DROP),
        ("cmp", "max") => F(MAX),
        ("cmp", "min") => F(MIN),
        ("process", "exit") => F(EXIT),
        ("io", "stdin") => F(STDIN),
        ("io", "stdout") => F(STDOUT),
        ("io", "read_to_string") => F(IO_READ_TO_STRING),
        ("char", "from_u32") => F(CHAR_FROM_U32),
        ("char", "from_digit") => F(CHAR_FROM_DIGIT),
        ("char", "from") => F(CHAR_FROM),
        ("Default", "default") => F(DEFAULT),
        ("iter", "repeat") => F(ITER_REPEAT),
        ("iter", "once") => F(ITER_ONCE),
        ("iter", "empty") => F(ITER_EMPTY),
        ("iter", "from_fn") => F(ITER_FROM_FN),
        ("iter", "successors") => F(ITER_SUCCESSORS),
        ("iter", "zip") => F(ITER_ZIP),
        ("env", "args") => F(ENV_ARGS),
        ("Instant", "now") => F(INSTANT_NOW),
        ("Duration", "from_secs" | "from_secs_f64" | "from_secs_f32") => F(DURATION_SECS),
        ("Duration", "from_millis") => F(DURATION_MILLIS),
        ("Duration", "from_micros") => F(DURATION_MICROS),
        ("Duration", "from_nanos") => F(DURATION_NANOS),
        ("thread", "sleep") => F(SLEEP),
        ("hint", "black_box") => F(BLACK_BOX),
        _ => return None,
    })
}

pub fn lookup_free(name: &str) -> Option<u32> {
    Some(match name {
        "drop" => bf::DROP,
        _ => return None,
    })
}

// ------------------------------------------------------------------ helpers

/// (list, start, end) of a sequence value (Vec / array / slice view).
pub fn slice_view(v: &Value) -> Option<(List, usize, usize)> {
    match v {
        Value::Vec(l) | Value::Array(l) => {
            let n = l.borrow().len();
            Some((l.clone(), 0, n))
        }
        Value::Slice(s) => Some((s.list.clone(), s.start, s.end)),
        _ => None,
    }
}

pub fn slice_of(l: &List, a: usize, b: usize) -> Value {
    Value::Slice(Rc::new(SliceV { list: l.clone(), start: a, end: b }))
}

pub fn str_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        Value::String(s) => s.borrow().clone(),
        Value::Char(c) => c.to_string(),
        _ => String::new(),
    }
}

fn is_str(v: &Value) -> bool {
    matches!(v, Value::Str(_) | Value::String(_))
}

/// A zero / empty value of the same kind as `v` (for `mem::take`, `or_default`).
pub fn zero_like(v: &Value) -> Value {
    match v {
        Value::Int(_) => Value::Int(0),
        Value::Float(_) => Value::Float(0.0),
        Value::Bool(_) => Value::Bool(false),
        Value::Char(_) => Value::Char('\0'),
        Value::Str(_) | Value::String(_) => Value::string(String::new()),
        Value::Vec(_) => Value::vec(vec![]),
        Value::Map(m) => Value::Map(Rc::new(RefCell::new(if m.borrow().is_tree() {
            MapData::Tree(Default::default())
        } else {
            MapData::Hash(Default::default())
        }))),
        Value::Set(s) => Value::Set(Rc::new(RefCell::new(if s.borrow().is_tree() {
            SetData::Tree(Default::default())
        } else {
            SetData::Hash(Default::default())
        }))),
        Value::Deque(_) => Value::Deque(Rc::new(RefCell::new(VecDeque::new()))),
        Value::Heap(_) => Value::Heap(new_list(vec![])),
        Value::Enum(V_SOME | V_NONE, _) => Value::none(),
        _ => Value::Lazy,
    }
}

pub fn set_op(op: BinOp, a: &Value, b: &Value) -> R<Value> {
    let (Value::Set(x), Value::Set(y)) = (a, b) else { return err("set operator on non-sets") };
    let (x, y) = (x.borrow(), y.borrow());
    let mut out = if x.is_tree() { SetData::Tree(Default::default()) } else { SetData::Hash(Default::default()) };
    match op {
        BinOp::BitOr => {
            for k in x.items().into_iter().chain(y.items()) {
                out.insert(Key(k));
            }
        }
        BinOp::BitAnd => {
            for k in x.items() {
                if y.contains(&Key(k.clone())) {
                    out.insert(Key(k));
                }
            }
        }
        BinOp::Sub => {
            for k in x.items() {
                if !y.contains(&Key(k.clone())) {
                    out.insert(Key(k));
                }
            }
        }
        BinOp::BitXor => {
            for k in x.items() {
                if !y.contains(&Key(k.clone())) {
                    out.insert(Key(k));
                }
            }
            for k in y.items() {
                if !x.contains(&Key(k.clone())) {
                    out.insert(Key(k));
                }
            }
        }
        _ => return err("unsupported set operator"),
    }
    Ok(Value::Set(Rc::new(RefCell::new(out))))
}

fn new_map(tree: bool) -> Value {
    Value::Map(Rc::new(RefCell::new(if tree { MapData::Tree(Default::default()) } else { MapData::Hash(Default::default()) })))
}
fn new_set(tree: bool) -> Value {
    Value::Set(Rc::new(RefCell::new(if tree { SetData::Tree(Default::default()) } else { SetData::Hash(Default::default()) })))
}

fn wrap_iter(st: IterState) -> Value {
    Value::Iter(Rc::new(RefCell::new(st)))
}

fn vals_iter(v: Vec<Value>) -> Value {
    wrap_iter(IterState::Vals(v.into()))
}

fn arg(args: &[Value], i: usize) -> R<Value> {
    args.get(i).cloned().ok_or_else(|| crate::interp::ctrl_error(format!("missing argument {}", i + 1)))
}

fn parse_error_msg(kind: std::num::IntErrorKind) -> &'static str {
    use std::num::IntErrorKind::*;
    match kind {
        Empty => "cannot parse integer from empty string",
        InvalidDigit => "invalid digit found in string",
        PosOverflow => "number too large to fit in target type",
        NegOverflow => "number too small to fit in target type",
        _ => "invalid integer",
    }
}

fn parse_int_as(s: &str, ty: &str) -> Result<i64, String> {
    macro_rules! p {
        ($t:ty) => {
            s.parse::<$t>().map(|v| v as i64).map_err(|e| parse_error_msg(e.kind().clone()).to_string())
        };
    }
    match ty {
        "i8" => p!(i8),
        "i16" => p!(i16),
        "i32" => p!(i32),
        "u8" => p!(u8),
        "u16" => p!(u16),
        "u32" => p!(u32),
        "u64" | "usize" | "u128" => s.parse::<u64>().map(|v| v as i64).map_err(|e| parse_error_msg(e.kind().clone()).to_string()),
        _ => p!(i64),
    }
}

impl Interp {
    // ------------------------------------------------------- static builtins
    pub fn call_builtin(&mut self, id: u32, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Value> {
        use bf::*;
        Ok(match id {
            VEC_NEW => Value::vec(vec![]),
            VEC_FROM | VEC_FROM_ITER => {
                let a = self.deref(arg(&args, 0)?);
                match &a {
                    Value::Str(_) | Value::String(_) => Value::vec(str_of(&a).bytes().map(|b| Value::Int(b as i64)).collect()),
                    _ => {
                        let mut st = self.make_iter(a, e)?;
                        let items = self.drain_iter(&mut st)?;
                        let items = items.into_iter().map(|x| self.deref(x)).collect();
                        Value::vec(items)
                    }
                }
            }
            STRING_NEW => Value::string(String::new()),
            STRING_FROM => {
                let a = self.deref(arg(&args, 0)?);
                Value::string(str_of(&a))
            }
            STRING_FROM_ITER => {
                let a = arg(&args, 0)?;
                let mut st = self.make_iter(a, e)?;
                self.collect(&mut st, Some(&Type::new("String", vec![])), e)?
            }
            STRING_FROM_UTF8 | STRING_FROM_UTF8_LOSSY => {
                let a = self.deref(arg(&args, 0)?);
                let bytes: Vec<u8> = match slice_view(&a) {
                    Some((l, s, t)) => l.borrow()[s..t].iter().map(|v| self.as_int(v).unwrap_or(0) as u8).collect(),
                    None => vec![],
                };
                if id == STRING_FROM_UTF8 {
                    match String::from_utf8(bytes) {
                        Ok(s) => Value::ok(Value::string(s)),
                        Err(e) => Value::err(Value::string(e.to_string())),
                    }
                } else {
                    Value::string(String::from_utf8_lossy(&bytes).into_owned())
                }
            }
            HASHMAP_NEW => new_map(false),
            BTREEMAP_NEW => new_map(true),
            HASHSET_NEW => new_set(false),
            BTREESET_NEW => new_set(true),
            DEQUE_NEW => Value::Deque(Rc::new(RefCell::new(VecDeque::new()))),
            HEAP_NEW => Value::Heap(new_list(vec![])),
            HASHMAP_FROM | BTREEMAP_FROM | HASHSET_FROM | BTREESET_FROM | DEQUE_FROM | HEAP_FROM => {
                let name = match id {
                    HASHMAP_FROM => "HashMap",
                    BTREEMAP_FROM => "BTreeMap",
                    HASHSET_FROM => "HashSet",
                    BTREESET_FROM => "BTreeSet",
                    DEQUE_FROM => "VecDeque",
                    _ => "BinaryHeap",
                };
                let mut st = self.make_iter(arg(&args, 0)?, e)?;
                let mut st2 = IterState::Vals(self.drain_iter(&mut st)?.into_iter().map(|x| self.deref(x)).collect());
                self.collect(&mut st2, Some(&Type::new(name, vec![])), e)?
            }
            IDENTITY | BLACK_BOX => arg(&args, 0)?,
            RC_NEW => Value::RcBox(Interp::new_cell(arg(&args, 0)?)),
            RC_CLONE => {
                let a = arg(&args, 0)?;
                let (a, _) = self.deref_place(a, None);
                a
            }
            RC_STRONG_COUNT => {
                let (a, _) = self.deref_place(arg(&args, 0)?, None);
                match a {
                    Value::RcBox(c) => Value::Int(Rc::strong_count(&c) as i64 - 1),
                    _ => Value::Int(1),
                }
            }
            RC_PTR_EQ => {
                let (a, _) = self.deref_place(arg(&args, 0)?, None);
                let (b, _) = self.deref_place(arg(&args, 1)?, None);
                Value::Bool(match (a, b) {
                    (Value::RcBox(x), Value::RcBox(y)) => Rc::ptr_eq(&x, &y),
                    _ => false,
                })
            }
            CELL_NEW => Value::CellBox(Interp::new_cell(arg(&args, 0)?)),
            MEM_SWAP => {
                let (a, b) = (arg(&args, 0)?, arg(&args, 1)?);
                let (Some(pa), Some(pb)) = (Interp::ref_place(&a), Interp::ref_place(&b)) else {
                    return err("mem::swap requires references");
                };
                let (va, vb) = (self.load(&pa), self.load(&pb));
                self.store(&pa, vb);
                self.store(&pb, va);
                Value::Unit
            }
            MEM_REPLACE => {
                let a = arg(&args, 0)?;
                let Some(p) = Interp::ref_place(&a) else { return err("mem::replace requires a reference") };
                let old = self.load(&p);
                self.store(&p, arg(&args, 1)?);
                old
            }
            MEM_TAKE => {
                let a = arg(&args, 0)?;
                let Some(p) = Interp::ref_place(&a) else { return err("mem::take requires a reference") };
                let old = self.load(&p);
                self.store(&p, zero_like(&old));
                old
            }
            DROP => Value::Unit,
            MAX | MIN => {
                let (a, b) = (self.deref(arg(&args, 0)?), self.deref(arg(&args, 1)?));
                let c = self.cmp_total(&a, &b)?;
                if id == MAX {
                    if c == Ordering::Greater { a } else { b }
                } else if c == Ordering::Greater {
                    b
                } else {
                    a
                }
            }
            EXIT => {
                let c = self.as_int(&arg(&args, 0)?)?;
                let _ = self.out.flush();
                return Err(crate::interp::ctrl_exit(c as i32));
            }
            STDIN | STDOUT => Value::Stdin,
            IO_READ_TO_STRING => {
                let mut s = String::new();
                self.read_all_stdin(&mut s);
                Value::ok(Value::string(s))
            }
            CHAR_FROM_U32 => Value::opt(char::from_u32(self.as_int(&arg(&args, 0)?)? as u32).map(Value::Char)),
            CHAR_FROM_DIGIT => {
                let d = self.as_int(&arg(&args, 0)?)? as u32;
                let r = self.as_int(&arg(&args, 1)?)? as u32;
                Value::opt(char::from_digit(d, r).map(Value::Char))
            }
            CHAR_FROM => match self.deref(arg(&args, 0)?) {
                Value::Int(i) => Value::Char(char::from_u32(i as u32).unwrap_or('\0')),
                v => v,
            },
            INT_FROM => Value::Int(self.as_int(&arg(&args, 0)?)?),
            INT_TRY_FROM => {
                let v = self.as_int(&arg(&args, 0)?)?;
                let ty = hint.map(|t| t.base().name.clone()).unwrap_or_default();
                if INT_TYPES.contains(&ty.as_str()) && (v > int_max(&ty) || v < int_min(&ty)) {
                    Value::err(Value::str("out of range integral type conversion attempted"))
                } else {
                    Value::ok(Value::Int(v))
                }
            }
            FLOAT_FROM => match self.deref(arg(&args, 0)?) {
                Value::Int(i) => Value::Float(i as f64),
                v => v,
            },
            FROM_STR_RADIX => {
                let s = str_of(&self.deref(arg(&args, 0)?));
                let r = self.as_int(&arg(&args, 1)?)? as u32;
                match i64::from_str_radix(&s, r) {
                    Ok(v) => Value::ok(Value::Int(v)),
                    Err(e) => Value::err(Value::str(parse_error_msg(e.kind().clone()))),
                }
            }
            DEFAULT => match hint {
                Some(t) => self.default_for_type(t)?,
                None => Value::Lazy,
            },
            ITER_REPEAT => wrap_iter(IterState::Repeat(arg(&args, 0)?)),
            ITER_ONCE => vals_iter(vec![arg(&args, 0)?]),
            ITER_EMPTY => wrap_iter(IterState::Empty),
            ITER_FROM_FN => wrap_iter(IterState::FromFn(arg(&args, 0)?)),
            ITER_SUCCESSORS => {
                let first = match self.deref(arg(&args, 0)?) {
                    Value::Enum(V_SOME, Some(l)) => Some(l.borrow()[0].clone()),
                    _ => None,
                };
                wrap_iter(IterState::Successors(first, arg(&args, 1)?))
            }
            ITER_ZIP => {
                let a = self.make_iter(arg(&args, 0)?, e)?;
                let b = self.make_iter(arg(&args, 1)?, e)?;
                wrap_iter(IterState::Zip(Box::new(a), Box::new(b)))
            }
            ENV_ARGS => vals_iter(self.args.iter().map(|s| Value::string(s.clone())).collect()),
            INSTANT_NOW => Value::Instant(self.start.elapsed().as_secs_f64()),
            DURATION_SECS => Value::Instant(match self.deref(arg(&args, 0)?) {
                Value::Float(f) => f,
                v => self.as_int(&v)? as f64,
            }),
            DURATION_MILLIS => Value::Instant(self.as_int(&arg(&args, 0)?)? as f64 / 1e3),
            DURATION_MICROS => Value::Instant(self.as_int(&arg(&args, 0)?)? as f64 / 1e6),
            DURATION_NANOS => Value::Instant(self.as_int(&arg(&args, 0)?)? as f64 / 1e9),
            SLEEP => {
                if let Value::Instant(d) = self.deref(arg(&args, 0)?) {
                    std::thread::sleep(std::time::Duration::from_secs_f64(d));
                }
                Value::Unit
            }
            SWAP_FN | FLUSH => Value::Unit,
            _ => return err(format!("unknown builtin {}", id)),
        })
    }

    fn read_all_stdin(&mut self, s: &mut String) {
        match &mut self.stdin {
            Some(r) => {
                let _ = r.read_to_string(s);
            }
            None => {
                let _ = std::io::stdin().read_to_string(s);
            }
        }
    }

    fn read_stdin_line(&mut self, s: &mut String) -> usize {
        match &mut self.stdin {
            Some(r) => r.read_line(s).unwrap_or(0),
            None => std::io::stdin().lock().read_line(s).unwrap_or(0),
        }
    }

    pub fn default_for_type(&mut self, t: &Type) -> R<Value> {
        let t = t.base();
        Ok(match t.name.as_str() {
            n if INT_TYPES.contains(&n) => Value::Int(0),
            "f32" | "f64" => Value::Float(0.0),
            "bool" => Value::Bool(false),
            "char" => Value::Char('\0'),
            "String" | "str" => Value::string(String::new()),
            "Vec" => Value::vec(vec![]),
            "HashMap" => new_map(false),
            "BTreeMap" => new_map(true),
            "HashSet" => new_set(false),
            "BTreeSet" => new_set(true),
            "VecDeque" => Value::Deque(Rc::new(RefCell::new(VecDeque::new()))),
            "BinaryHeap" => Value::Heap(new_list(vec![])),
            "Option" => Value::none(),
            "tuple" => {
                let mut v = Vec::new();
                for a in &t.args {
                    v.push(self.default_for_type(a)?);
                }
                if v.is_empty() { Value::Unit } else { Value::tuple(v) }
            }
            "Rc" | "Arc" => {
                let inner = match t.args.first() {
                    Some(a) => self.default_for_type(a)?,
                    None => Value::Lazy,
                };
                Value::RcBox(Interp::new_cell(inner))
            }
            "RefCell" | "Cell" | "Mutex" => {
                let inner = match t.args.first() {
                    Some(a) => self.default_for_type(a)?,
                    None => Value::Lazy,
                };
                Value::CellBox(Interp::new_cell(inner))
            }
            name => match self.user_type_key(name) {
                Some(tk) => {
                    if let Some(fid) = self.prog.find_method(tk, intern_default()) {
                        let fd = self.prog.fns[fid as usize].clone();
                        let base = self.stack.len();
                        return self.invoke(fd, base, None);
                    }
                    if tk < TK_ENUM {
                        let sd = &self.prog.structs[tk as usize];
                        let types = sd.field_types.clone();
                        let mut fields = Vec::new();
                        for ft in &types {
                            fields.push(self.default_for_type(ft)?);
                        }
                        Value::Struct(tk, new_list(fields))
                    } else {
                        let ed = &self.prog.enums[(tk - TK_ENUM) as usize];
                        Value::Enum(ed.variants[0], None)
                    }
                }
                None => Value::Lazy,
            },
        })
    }

    // -------------------------------------------------------------- methods
    #[allow(clippy::too_many_arguments)]
    pub fn builtin_method(
        &mut self,
        recv: Value,
        place: Option<Place>,
        mid: M,
        sym: Sym,
        args: Vec<Value>,
        hint: Option<&Type>,
        e: &Expr,
    ) -> R<Value> {
        // universal methods
        match mid {
            M::clone | M::to_owned | M::cloned if !matches!(recv, Value::Iter(_) | Value::Enum(V_SOME | V_NONE, _)) => {
                return Ok(match &recv {
                    Value::Str(s) if mid == M::to_owned => Value::string(s.to_string()),
                    _ => self.clone_deep(&recv),
                });
            }
            M::to_string if !matches!(recv, Value::Iter(_)) => {
                let mut s = String::new();
                self.display_value(&recv, &crate::format::RSpec::plain(crate::format::FmtTy::Display), &mut s)?;
                return Ok(Value::string(s));
            }
            M::into => return Ok(self.convert_into(recv, hint)),
            M::eq | M::ne if !matches!(recv, Value::Iter(_)) => {
                let o = self.deref(arg(&args, 0)?);
                let r = self.equals(&recv, &o)?;
                return Ok(Value::Bool(if mid == M::eq { r } else { !r }));
            }
            M::cmp | M::partial_cmp | M::total_cmp => {
                let o = self.deref(arg(&args, 0)?);
                let c = self.compare(&recv, &o)?;
                return Ok(match (mid, c) {
                    (M::partial_cmp, c) => Value::opt(c.map(Interp::ordering_value)),
                    (_, Some(c)) => Interp::ordering_value(c),
                    (_, None) => match (&recv, &o) {
                        (Value::Float(a), Value::Float(b)) => Interp::ordering_value(a.total_cmp(b)),
                        _ => Interp::ordering_value(Ordering::Equal),
                    },
                });
            }
            M::lt | M::le | M::gt | M::ge if !matches!(recv, Value::Iter(_)) => {
                let o = self.deref(arg(&args, 0)?);
                let op = match mid {
                    M::lt => BinOp::Lt,
                    M::le => BinOp::Le,
                    M::gt => BinOp::Gt,
                    _ => BinOp::Ge,
                };
                return self.compare_op(op, recv, o);
            }
            M::max | M::min if args.len() == 1 && !matches!(recv, Value::Iter(_)) => {
                let o = self.deref(arg(&args, 0)?);
                if let (Value::Float(a), Value::Float(b)) = (&recv, &o) {
                    return Ok(Value::Float(if mid == M::max { a.max(*b) } else { a.min(*b) }));
                }
                let c = self.cmp_total(&recv, &o)?;
                return Ok(if (mid == M::max) == (c == Ordering::Greater) { recv } else if c == Ordering::Equal { if mid == M::max { o } else { recv } } else { o });
            }
            M::clamp if args.len() == 2 => {
                let lo = self.deref(arg(&args, 0)?);
                let hi = self.deref(arg(&args, 1)?);
                if self.cmp_total(&recv, &lo)? == Ordering::Less {
                    return Ok(lo);
                }
                if self.cmp_total(&recv, &hi)? == Ordering::Greater {
                    return Ok(hi);
                }
                return Ok(recv);
            }
            M::borrow | M::as_ref | M::borrow_mut | M::as_mut if !matches!(recv, Value::CellBox(_) | Value::Enum(..) | Value::RcBox(_)) => {
                return Ok(match place {
                    Some(p) => Interp::to_ref(p),
                    None => recv,
                });
            }
            M::hash => return Ok(Value::Unit),
            _ => {}
        }
        match &recv {
            Value::Int(i) => self.int_method(*i, mid, &args, e),
            Value::Float(f) => self.float_method(*f, mid, &args, e),
            Value::Char(c) => self.char_method(*c, mid, &args, e),
            Value::Bool(b) => match mid {
                M::then => {
                    if *b {
                        let f = arg(&args, 0)?;
                        Ok(Value::some(self.call_value(&f, vec![])?))
                    } else {
                        Ok(Value::none())
                    }
                }
                M::then_some => Ok(if *b { Value::some(arg(&args, 0)?) } else { Value::none() }),
                _ => self.no_method(&recv, sym, e),
            },
            Value::Str(_) | Value::String(_) => self.str_method(recv.clone(), place, mid, sym, args, hint, e),
            Value::Vec(_) | Value::Array(_) | Value::Slice(_) => self.vec_method(recv.clone(), place, mid, sym, args, hint, e),
            Value::Map(_) => self.map_method(recv.clone(), mid, sym, args, e),
            Value::Set(_) => self.set_method(recv.clone(), mid, sym, args, e),
            Value::Deque(_) => self.deque_method(recv.clone(), mid, sym, args, hint, e),
            Value::Heap(_) => self.heap_method(recv.clone(), mid, sym, args, hint, e),
            Value::Entry(_) => self.entry_method(recv.clone(), mid, sym, args, e),
            Value::Enum(V_NONE | V_SOME | V_OK | V_ERR, _) => self.option_method(recv.clone(), place, mid, sym, args, hint, e),
            Value::Enum(V_LESS..=V_GREATER, _) => self.ordering_method(recv.clone(), mid, sym, args, e),
            Value::Iter(it) if mid == M::as_str => {
                let st = it.borrow().try_clone();
                let mut st = st.unwrap_or(IterState::Empty);
                let mut s = String::new();
                while let Some(x) = self.iter_next(&mut st)? {
                    if let Value::Char(c) = self.deref(x) {
                        s.push(c);
                    }
                }
                Ok(Value::string(s))
            }
            Value::Iter(_) | Value::Range(_) => {
                if let Value::Range(r) = &recv {
                    match mid {
                        M::contains => {
                            let x = self.deref(arg(&args, 0)?);
                            let lo_ok = match &r.lo {
                                Some(lo) => self.cmp_total(&x, lo)? != Ordering::Less,
                                None => true,
                            };
                            let hi_ok = match &r.hi {
                                Some(hi) => {
                                    let c = self.cmp_total(&x, hi)?;
                                    c == Ordering::Less || (r.inclusive && c == Ordering::Equal)
                                }
                                None => true,
                            };
                            return Ok(Value::Bool(lo_ok && hi_ok));
                        }
                        M::is_empty => {
                            let st = self.range_iter(r, e)?;
                            return Ok(Value::Bool(st.len_hint() == Some(0)));
                        }
                        M::start => return Ok(r.lo.clone().unwrap_or(Value::Unit)),
                        M::end => return Ok(r.hi.clone().unwrap_or(Value::Unit)),
                        _ => {}
                    }
                }
                match self.iter_method(recv.clone(), mid, args, hint, e)? {
                    Some(v) => Ok(v),
                    None => self.no_method(&recv, sym, e),
                }
            }
            Value::RcBox(c) => {
                let inner = c.borrow().clone();
                match mid {
                    M::borrow | M::as_ref if !matches!(inner, Value::CellBox(_)) => Ok(inner),
                    _ => {
                        let p = Some(Place::Cell(c.clone()));
                        let (inner, p) = self.deref_place(inner, p);
                        self.builtin_method(inner, p, mid, sym, args, hint, e)
                    }
                }
            }
            Value::CellBox(c) => match mid {
                M::borrow | M::get | M::lock | M::read | M::get_ref => Ok(match mid {
                    M::get => c.borrow().clone(),
                    M::lock | M::read => Value::ok(Value::RefBox(c.clone())),
                    _ => Value::RefBox(c.clone()),
                }),
                M::borrow_mut | M::get_mut | M::as_mut | M::write => {
                    Ok(if mid == M::write { Value::ok(Value::RefBox(c.clone())) } else { Value::RefBox(c.clone()) })
                }
                M::set => {
                    let v = arg(&args, 0)?;
                    *c.borrow_mut() = v;
                    Ok(Value::Unit)
                }
                M::replace => {
                    let v = arg(&args, 0)?;
                    Ok(c.replace(v))
                }
                M::take => {
                    let old = c.borrow().clone();
                    let z = zero_like(&old);
                    Ok(c.replace(z))
                }
                M::into_inner => Ok(c.borrow().clone()),
                _ => self.no_method(&recv, sym, e),
            },
            Value::Formatter(f) => self.formatter_method(f.clone(), mid, sym, args, e),
            Value::Tuple(_) | Value::Unit => self.no_method(&recv, sym, e),
            Value::Struct(..) | Value::Enum(..) => {
                // user types implementing Iterator get the adapter methods
                let tk = self.type_key(&recv);
                if self.prog.find_method(tk, self.syms().next).is_some() {
                    let obj = match place {
                        Some(p) => Interp::to_ref(p),
                        None => Value::RefBox(Interp::new_cell(recv.clone())),
                    };
                    let it = wrap_iter(IterState::User(obj));
                    if let Some(v) = self.iter_method(it, mid, args, hint, e)? {
                        return Ok(v);
                    }
                }
                self.no_method(&recv, sym, e)
            }
            Value::Lazy => {
                let z = match mid {
                    M::push | M::extend | M::iter | M::len | M::is_empty | M::sort | M::last | M::first | M::extend_from_slice => {
                        Value::vec(vec![])
                    }
                    M::push_str => Value::string(String::new()),
                    M::insert if args.len() == 2 => new_map(false),
                    M::insert | M::contains => new_set(false),
                    M::entry | M::contains_key | M::get => new_map(false),
                    M::push_back | M::push_front | M::pop_front => Value::Deque(Rc::new(RefCell::new(VecDeque::new()))),
                    _ => Value::Int(0),
                };
                if let Some(p) = &place {
                    self.store(p, z.clone());
                }
                self.builtin_method(z, place, mid, sym, args, hint, e)
            }
            Value::Instant(t) => match mid {
                M::elapsed => Ok(Value::Instant(self.start.elapsed().as_secs_f64() - t)),
                M::duration_since => match self.deref(arg(&args, 0)?) {
                    Value::Instant(o) => Ok(Value::Instant(t - o)),
                    _ => err("duration_since expects an Instant"),
                },
                M::as_secs_f64 | M::as_secs_f32 => Ok(Value::Float(*t)),
                M::as_secs => Ok(Value::Int(*t as i64)),
                M::as_millis => Ok(Value::Int((*t * 1e3) as i64)),
                M::as_micros => Ok(Value::Int((*t * 1e6) as i64)),
                M::as_nanos => Ok(Value::Int((*t * 1e9) as i64)),
                _ => self.no_method(&recv, sym, e),
            },
            Value::Stdin => match mid {
                M::lock => Ok(Value::Stdin),
                M::read_line => {
                    let target = arg(&args, 0)?;
                    let mut s = String::new();
                    let n = self.read_stdin_line(&mut s);
                    match self.deref(target) {
                        Value::String(t) => t.borrow_mut().push_str(&s),
                        _ => return err("read_line expects &mut String"),
                    }
                    Ok(Value::ok(Value::Int(n as i64)))
                }
                M::read_to_string => {
                    let target = arg(&args, 0)?;
                    let mut s = String::new();
                    self.read_all_stdin(&mut s);
                    let n = s.len();
                    match self.deref(target) {
                        Value::String(t) => t.borrow_mut().push_str(&s),
                        _ => return err("read_to_string expects &mut String"),
                    }
                    Ok(Value::ok(Value::Int(n as i64)))
                }
                M::lines => {
                    let mut s = String::new();
                    self.read_all_stdin(&mut s);
                    Ok(vals_iter(s.lines().map(|l| Value::ok(Value::string(l.to_string()))).collect()))
                }
                M::flush | M::write_str | M::write_fmt => Ok(Value::ok(Value::Unit)),
                _ => self.no_method(&recv, sym, e),
            },
            _ => self.no_method(&recv, sym, e),
        }
    }

    fn no_method<T>(&self, recv: &Value, sym: Sym, e: &Expr) -> R<T> {
        err(format!("{}:{}: no method `{}` found for {}", e.line, e.col, sym_name(sym), self.value_type_name(recv)))
    }

    fn convert_into(&mut self, recv: Value, hint: Option<&Type>) -> Value {
        let name = hint.map(|t| t.base().name.as_str()).unwrap_or("");
        match (&recv, name) {
            (Value::Int(i), "f64" | "f32") => Value::Float(*i as f64),
            (Value::Str(s), _) => Value::string(s.to_string()),
            (Value::Char(c), "String") => Value::string(c.to_string()),
            (Value::Array(l), "Vec") => Value::Vec(new_list(l.borrow().clone())),
            (Value::Slice(_), _) => self.clone_deep(&self.slice_to_vec(&recv)),
            _ => recv,
        }
    }

    fn slice_to_vec(&self, v: &Value) -> Value {
        match slice_view(v) {
            Some((l, a, b)) => Value::vec(l.borrow()[a..b].to_vec()),
            None => v.clone(),
        }
    }

    // ----------------------------------------------------------------- ints
    fn int_method(&mut self, i: i64, mid: M, args: &[Value], e: &Expr) -> R<Value> {
        let a = |n: usize, me: &Self| -> R<i64> { me.as_int(&arg(args, n)?) };
        let ovf = |me: &Self, what: &str| -> R<Value> { me.panic_e(e, format!("attempt to {} with overflow", what)) };
        Ok(match mid {
            M::abs => match i.checked_abs() {
                Some(v) => Value::Int(v),
                None => return ovf(self, "negate"),
            },
            M::pow => {
                let p = a(0, self)?;
                match i.checked_pow(p as u32) {
                    Some(v) => Value::Int(v),
                    None => return ovf(self, "multiply"),
                }
            }
            M::checked_add => Value::opt(i.checked_add(a(0, self)?).map(Value::Int)),
            M::checked_sub => Value::opt(i.checked_sub(a(0, self)?).map(Value::Int)),
            M::checked_mul => Value::opt(i.checked_mul(a(0, self)?).map(Value::Int)),
            M::checked_div => Value::opt(i.checked_div(a(0, self)?).map(Value::Int)),
            M::checked_rem => Value::opt(i.checked_rem(a(0, self)?).map(Value::Int)),
            M::checked_pow => Value::opt(i.checked_pow(a(0, self)? as u32).map(Value::Int)),
            M::checked_neg => Value::opt(i.checked_neg().map(Value::Int)),
            M::wrapping_add => Value::Int(i.wrapping_add(a(0, self)?)),
            M::wrapping_sub => Value::Int(i.wrapping_sub(a(0, self)?)),
            M::wrapping_mul => Value::Int(i.wrapping_mul(a(0, self)?)),
            M::wrapping_div => Value::Int(i.wrapping_div(a(0, self)?)),
            M::wrapping_rem => Value::Int(i.wrapping_rem(a(0, self)?)),
            M::wrapping_neg => Value::Int(i.wrapping_neg()),
            M::wrapping_pow => Value::Int(i.wrapping_pow(a(0, self)? as u32)),
            M::wrapping_shl => Value::Int(i.wrapping_shl(a(0, self)? as u32)),
            M::wrapping_shr => Value::Int(i.wrapping_shr(a(0, self)? as u32)),
            M::saturating_add => Value::Int(i.saturating_add(a(0, self)?)),
            M::saturating_sub => Value::Int(i.saturating_sub(a(0, self)?)),
            M::saturating_mul => Value::Int(i.saturating_mul(a(0, self)?)),
            M::saturating_pow => Value::Int(i.saturating_pow(a(0, self)? as u32)),
            M::overflowing_add => {
                let (v, o) = i.overflowing_add(a(0, self)?);
                Value::tuple(vec![Value::Int(v), Value::Bool(o)])
            }
            M::overflowing_sub => {
                let (v, o) = i.overflowing_sub(a(0, self)?);
                Value::tuple(vec![Value::Int(v), Value::Bool(o)])
            }
            M::overflowing_mul => {
                let (v, o) = i.overflowing_mul(a(0, self)?);
                Value::tuple(vec![Value::Int(v), Value::Bool(o)])
            }
            M::rem_euclid => {
                let d = a(0, self)?;
                if d == 0 {
                    return self.panic_e(e, "attempt to calculate the remainder with a divisor of zero");
                }
                Value::Int(i.rem_euclid(d))
            }
            M::div_euclid => {
                let d = a(0, self)?;
                if d == 0 {
                    return self.panic_e(e, "attempt to divide by zero");
                }
                Value::Int(i.div_euclid(d))
            }
            M::signum => Value::Int(i.signum()),
            M::is_positive => Value::Bool(i > 0),
            M::is_negative => Value::Bool(i < 0),
            M::count_ones => Value::Int(i.count_ones() as i64),
            M::count_zeros => Value::Int(i.count_zeros() as i64),
            M::leading_zeros => Value::Int(i.leading_zeros() as i64),
            M::trailing_zeros => Value::Int(i.trailing_zeros() as i64),
            M::abs_diff => Value::Int((i - a(0, self)?).abs()),
            M::unsigned_abs => Value::Int(i.unsigned_abs() as i64),
            M::isqrt => Value::Int((i as f64).sqrt() as i64),
            M::is_power_of_two => Value::Bool(i > 0 && (i & (i - 1)) == 0),
            M::next_power_of_two => Value::Int((i as u64).next_power_of_two() as i64),
            M::ilog2 => Value::Int(i.ilog2() as i64),
            M::ilog10 => Value::Int(i.ilog10() as i64),
            M::is_ascii_digit | M::is_ascii_alphabetic | M::is_ascii_alphanumeric | M::is_ascii_uppercase
            | M::is_ascii_lowercase | M::is_ascii_whitespace | M::is_ascii_punctuation | M::is_ascii_hexdigit
            | M::is_ascii_graphic | M::to_ascii_uppercase | M::to_ascii_lowercase | M::is_ascii => {
                let r = self.char_method(i as u8 as char, mid, args, e)?;
                match r {
                    Value::Char(c) => Value::Int(c as i64),
                    v => v,
                }
            }
            M::to_be_bytes | M::to_le_bytes => {
                let bytes = if mid == M::to_be_bytes { i.to_be_bytes() } else { i.to_le_bytes() };
                Value::Array(new_list(bytes.iter().map(|b| Value::Int(*b as i64)).collect()))
            }
            M::sqrt | M::powi | M::powf | M::floor | M::ceil | M::round => self.float_method(i as f64, mid, args, e)?,
            _ => return self.no_method(&Value::Int(i), intern_m(mid), e),
        })
    }

    // --------------------------------------------------------------- floats
    fn float_method(&mut self, f: f64, mid: M, args: &[Value], e: &Expr) -> R<Value> {
        let a = |n: usize, me: &Self| -> R<f64> {
            match me.deref(arg(args, n)?) {
                Value::Float(x) => Ok(x),
                Value::Int(x) => Ok(x as f64),
                v => err(format!("expected float, found {}", v.type_name())),
            }
        };
        Ok(Value::Float(match mid {
            M::sqrt => f.sqrt(),
            M::cbrt => f.cbrt(),
            M::abs => f.abs(),
            M::powi => f.powi(self.as_int(&arg(args, 0)?)? as i32),
            M::powf => f.powf(a(0, self)?),
            M::exp => f.exp(),
            M::exp2 => f.exp2(),
            M::ln => f.ln(),
            M::log10 => f.log10(),
            M::log2 => f.log2(),
            M::log => f.log(a(0, self)?),
            M::sin => f.sin(),
            M::cos => f.cos(),
            M::tan => f.tan(),
            M::asin => f.asin(),
            M::acos => f.acos(),
            M::atan => f.atan(),
            M::atan2 => f.atan2(a(0, self)?),
            M::sinh => f.sinh(),
            M::cosh => f.cosh(),
            M::tanh => f.tanh(),
            M::floor => f.floor(),
            M::ceil => f.ceil(),
            M::round => f.round(),
            M::trunc => f.trunc(),
            M::fract => f.fract(),
            M::signum => f.signum(),
            M::hypot => f.hypot(a(0, self)?),
            M::to_degrees => f.to_degrees(),
            M::to_radians => f.to_radians(),
            M::mul_add => f.mul_add(a(0, self)?, a(1, self)?),
            M::recip => f.recip(),
            M::rem_euclid => f.rem_euclid(a(0, self)?),
            M::div_euclid => f.div_euclid(a(0, self)?),
            M::is_nan => return Ok(Value::Bool(f.is_nan())),
            M::is_infinite => return Ok(Value::Bool(f.is_infinite())),
            M::is_finite => return Ok(Value::Bool(f.is_finite())),
            M::is_sign_negative => return Ok(Value::Bool(f.is_sign_negative())),
            M::is_sign_positive => return Ok(Value::Bool(f.is_sign_positive())),
            M::to_bits => return Ok(Value::Int(f.to_bits() as i64)),
            M::abs_diff => (f - a(0, self)?).abs(),
            _ => return self.no_method(&Value::Float(f), intern_m(mid), e),
        }))
    }

    // ---------------------------------------------------------------- chars
    fn char_method(&mut self, c: char, mid: M, args: &[Value], e: &Expr) -> R<Value> {
        Ok(match mid {
            M::is_alphabetic => Value::Bool(c.is_alphabetic()),
            M::is_numeric => Value::Bool(c.is_numeric()),
            M::is_alphanumeric => Value::Bool(c.is_alphanumeric()),
            M::is_whitespace => Value::Bool(c.is_whitespace()),
            M::is_uppercase => Value::Bool(c.is_uppercase()),
            M::is_lowercase => Value::Bool(c.is_lowercase()),
            M::is_control => Value::Bool(c.is_control()),
            M::is_ascii => Value::Bool(c.is_ascii()),
            M::is_ascii_digit => Value::Bool(c.is_ascii_digit()),
            M::is_ascii_alphabetic => Value::Bool(c.is_ascii_alphabetic()),
            M::is_ascii_alphanumeric => Value::Bool(c.is_ascii_alphanumeric()),
            M::is_ascii_uppercase => Value::Bool(c.is_ascii_uppercase()),
            M::is_ascii_lowercase => Value::Bool(c.is_ascii_lowercase()),
            M::is_ascii_punctuation => Value::Bool(c.is_ascii_punctuation()),
            M::is_ascii_whitespace => Value::Bool(c.is_ascii_whitespace()),
            M::is_ascii_hexdigit => Value::Bool(c.is_ascii_hexdigit()),
            M::is_ascii_graphic => Value::Bool(c.is_ascii_graphic()),
            M::is_digit => Value::Bool(c.is_digit(self.as_int(&arg(args, 0)?)? as u32)),
            M::to_digit => Value::opt(c.to_digit(self.as_int(&arg(args, 0)?)? as u32).map(|d| Value::Int(d as i64))),
            M::to_ascii_uppercase => Value::Char(c.to_ascii_uppercase()),
            M::to_ascii_lowercase => Value::Char(c.to_ascii_lowercase()),
            M::to_uppercase => vals_iter(c.to_uppercase().map(Value::Char).collect()),
            M::to_lowercase => vals_iter(c.to_lowercase().map(Value::Char).collect()),
            M::len_utf8 => Value::Int(c.len_utf8() as i64),
            M::eq_ignore_ascii_case => match self.deref(arg(args, 0)?) {
                Value::Char(o) => Value::Bool(c.eq_ignore_ascii_case(&o)),
                _ => Value::Bool(false),
            },
            _ => return self.no_method(&Value::Char(c), intern_m(mid), e),
        })
    }

    // -------------------------------------------------------------- strings
    #[allow(clippy::too_many_arguments)]
    fn str_method(&mut self, recv: Value, place: Option<Place>, mid: M, sym: Sym, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Value> {
        // mutating methods on String
        if let Value::String(sc) = &recv {
            match mid {
                M::push_str => {
                    let a = self.deref(arg(&args, 0)?);
                    match &a {
                        Value::Str(s) => sc.borrow_mut().push_str(s),
                        Value::String(s) => {
                            let s = s.borrow().clone();
                            sc.borrow_mut().push_str(&s);
                        }
                        Value::Char(c) => sc.borrow_mut().push(*c),
                        _ => return err("push_str expects a string"),
                    }
                    return Ok(Value::Unit);
                }
                M::push => {
                    match self.deref(arg(&args, 0)?) {
                        Value::Char(c) => sc.borrow_mut().push(c),
                        v => {
                            let s = str_of(&v);
                            sc.borrow_mut().push_str(&s);
                        }
                    }
                    return Ok(Value::Unit);
                }
                M::pop => return Ok(Value::opt(sc.borrow_mut().pop().map(Value::Char))),
                M::clear => {
                    sc.borrow_mut().clear();
                    return Ok(Value::Unit);
                }
                M::insert => {
                    let i = self.as_int(&arg(&args, 0)?)? as usize;
                    if let Value::Char(c) = self.deref(arg(&args, 1)?) {
                        sc.borrow_mut().insert(i, c);
                    }
                    return Ok(Value::Unit);
                }
                M::insert_str => {
                    let i = self.as_int(&arg(&args, 0)?)? as usize;
                    let s = str_of(&self.deref(arg(&args, 1)?));
                    sc.borrow_mut().insert_str(i, &s);
                    return Ok(Value::Unit);
                }
                M::remove => {
                    let i = self.as_int(&arg(&args, 0)?)? as usize;
                    return Ok(Value::Char(sc.borrow_mut().remove(i)));
                }
                M::truncate => {
                    let i = self.as_int(&arg(&args, 0)?)? as usize;
                    sc.borrow_mut().truncate(i);
                    return Ok(Value::Unit);
                }
                M::retain => {
                    let f = arg(&args, 0)?;
                    let s = sc.borrow().clone();
                    let mut out = String::new();
                    for c in s.chars() {
                        let r = self.call_value(&f, vec![Value::Char(c)])?;
                        if matches!(self.deref(r), Value::Bool(true)) {
                            out.push(c);
                        }
                    }
                    *sc.borrow_mut() = out;
                    return Ok(Value::Unit);
                }
                M::drain => {
                    let s = sc.borrow().clone();
                    let (a, b) = match self.deref(arg(&args, 0)?) {
                        Value::Range(r) => self.range_bounds(&r, s.len(), e)?,
                        _ => (0, s.len()),
                    };
                    let drained: Vec<Value> = s[a..b].chars().map(Value::Char).collect();
                    sc.borrow_mut().replace_range(a..b, "");
                    return Ok(vals_iter(drained));
                }
                M::extend => {
                    let mut st = self.make_iter(arg(&args, 0)?, e)?;
                    while let Some(x) = self.iter_next(&mut st)? {
                        let x = self.deref(x);
                        match x {
                            Value::Char(c) => sc.borrow_mut().push(c),
                            v => {
                                let s = str_of(&v);
                                sc.borrow_mut().push_str(&s);
                            }
                        }
                    }
                    return Ok(Value::Unit);
                }
                M::as_mut_slice | M::as_mut => {
                    return Ok(match place {
                        Some(p) => Interp::to_ref(p),
                        None => recv.clone(),
                    })
                }
                M::capacity => return Ok(Value::Int(sc.borrow().capacity() as i64)),
                M::reserve | M::shrink_to_fit => return Ok(Value::Unit),
                M::into_bytes => {
                    return Ok(Value::vec(sc.borrow().bytes().map(|b| Value::Int(b as i64)).collect()));
                }
                _ => {}
            }
        }
        let s: Rc<String> = match &recv {
            Value::Str(s) => s.clone(),
            Value::String(s) => Rc::new(s.borrow().clone()),
            _ => unreachable!(),
        };
        let sarg = |me: &Self, n: usize| -> R<Value> { Ok(me.deref(arg(&args, n)?)) };
        // a string pattern argument: &str, String, char, or closure
        enum Pat {
            S(String),
            C(char),
            F(Value),
            Chars(Vec<char>),
        }
        let pat = |me: &Self, n: usize| -> R<Pat> {
            Ok(match sarg(me, n)? {
                Value::Char(c) => Pat::C(c),
                v @ (Value::Str(_) | Value::String(_)) => Pat::S(str_of(&v)),
                v @ (Value::Closure(_) | Value::Fn(_) | Value::BuiltinFn(_)) => Pat::F(v),
                v => match slice_view(&v) {
                    Some((l, a, b)) => Pat::Chars(
                        l.borrow()[a..b].iter().filter_map(|x| if let Value::Char(c) = me.deref(x.clone()) { Some(c) } else { None }).collect(),
                    ),
                    None => return err(format!("unsupported string pattern {}", v.type_name())),
                },
            })
        };
        // char predicate closure evaluation
        let mut char_matches = |me: &mut Self, p: &Pat, c: char| -> R<bool> {
            Ok(match p {
                Pat::C(x) => *x == c,
                Pat::F(f) => {
                    let r = me.call_value(f, vec![Value::Char(c)])?;
                    matches!(me.deref(r), Value::Bool(true))
                }
                Pat::Chars(cs) => cs.contains(&c),
                Pat::S(_) => false,
            })
        };
        let strs = |v: Vec<&str>| vals_iter(v.into_iter().map(Value::str).collect());
        Ok(match mid {
            M::len => Value::Int(s.len() as i64),
            M::is_empty => Value::Bool(s.is_empty()),
            M::as_str | M::as_ref | M::borrow => Value::Str(s.clone()),
            M::to_string | M::to_owned | M::into | M::as_string => Value::string(s.to_string()),
            M::chars => vals_iter(s.chars().map(Value::Char).collect()),
            M::char_indices => vals_iter(s.char_indices().map(|(i, c)| Value::tuple(vec![Value::Int(i as i64), Value::Char(c)])).collect()),
            M::bytes => vals_iter(s.bytes().map(|b| Value::Int(b as i64)).collect()),
            M::as_bytes | M::into_bytes => Value::Vec(new_list(s.bytes().map(|b| Value::Int(b as i64)).collect())),
            M::lines => strs(s.lines().collect()),
            M::split_whitespace => strs(s.split_whitespace().collect()),
            M::split | M::split_terminator | M::rsplit => {
                let p = pat(self, 0)?;
                let parts: Vec<String> = match &p {
                    Pat::S(x) => match mid {
                        M::split => s.split(x.as_str()).map(String::from).collect(),
                        M::rsplit => s.rsplit(x.as_str()).map(String::from).collect(),
                        _ => s.split_terminator(x.as_str()).map(String::from).collect(),
                    },
                    _ => {
                        let mut parts = Vec::new();
                        let mut cur = String::new();
                        for c in s.chars() {
                            if char_matches(self, &p, c)? {
                                parts.push(std::mem::take(&mut cur));
                            } else {
                                cur.push(c);
                            }
                        }
                        if !(mid == M::split_terminator && cur.is_empty()) {
                            parts.push(cur);
                        }
                        if mid == M::rsplit {
                            parts.reverse();
                        }
                        parts
                    }
                };
                vals_iter(parts.into_iter().map(|x| Value::str(&x)).collect())
            }
            M::splitn | M::rsplitn => {
                let n = self.as_int(&arg(&args, 0)?)? as usize;
                let p = match pat(self, 1)? {
                    Pat::S(x) => x,
                    Pat::C(c) => c.to_string(),
                    _ => return err("unsupported splitn pattern"),
                };
                if mid == M::splitn {
                    strs(s.splitn(n, p.as_str()).collect())
                } else {
                    strs(s.rsplitn(n, p.as_str()).collect())
                }
            }
            M::split_once | M::rsplit_once => {
                let p = match pat(self, 0)? {
                    Pat::S(x) => x,
                    Pat::C(c) => c.to_string(),
                    _ => return err("unsupported split_once pattern"),
                };
                let r = if mid == M::split_once { s.split_once(p.as_str()) } else { s.rsplit_once(p.as_str()) };
                Value::opt(r.map(|(a, b)| Value::tuple(vec![Value::str(a), Value::str(b)])))
            }
            M::split_at => {
                let i = self.as_int(&arg(&args, 0)?)? as usize;
                let (a, b) = s.split_at(i);
                Value::tuple(vec![Value::str(a), Value::str(b)])
            }
            M::trim => Value::str(s.trim()),
            M::trim_start => Value::str(s.trim_start()),
            M::trim_end => Value::str(s.trim_end()),
            M::trim_matches | M::trim_start_matches | M::trim_end_matches => {
                let p = pat(self, 0)?;
                let r: String = match &p {
                    Pat::S(x) => match mid {
                        M::trim_matches => s.trim_start_matches(x.as_str()).trim_end_matches(x.as_str()).into(),
                        M::trim_start_matches => s.trim_start_matches(x.as_str()).into(),
                        _ => s.trim_end_matches(x.as_str()).into(),
                    },
                    _ => {
                        let chars: Vec<char> = s.chars().collect();
                        let mut a = 0;
                        let mut b = chars.len();
                        if mid != M::trim_end_matches {
                            while a < b && char_matches(self, &p, chars[a])? {
                                a += 1;
                            }
                        }
                        if mid != M::trim_start_matches {
                            while b > a && char_matches(self, &p, chars[b - 1])? {
                                b -= 1;
                            }
                        }
                        chars[a..b].iter().collect()
                    }
                };
                Value::str(&r)
            }
            M::strip_prefix | M::strip_suffix => {
                let p = match pat(self, 0)? {
                    Pat::S(x) => x,
                    Pat::C(c) => c.to_string(),
                    _ => return err("unsupported strip pattern"),
                };
                let r = if mid == M::strip_prefix { s.strip_prefix(p.as_str()) } else { s.strip_suffix(p.as_str()) };
                Value::opt(r.map(Value::str))
            }
            M::starts_with | M::ends_with | M::contains | M::find | M::rfind | M::matches => {
                let p = pat(self, 0)?;
                match &p {
                    Pat::S(x) => match mid {
                        M::starts_with => Value::Bool(s.starts_with(x.as_str())),
                        M::ends_with => Value::Bool(s.ends_with(x.as_str())),
                        M::contains => Value::Bool(s.contains(x.as_str())),
                        M::find => Value::opt(s.find(x.as_str()).map(|i| Value::Int(i as i64))),
                        M::rfind => Value::opt(s.rfind(x.as_str()).map(|i| Value::Int(i as i64))),
                        _ => strs(s.matches(x.as_str()).collect()),
                    },
                    _ => {
                        let idx: Vec<(usize, char)> = s.char_indices().collect();
                        match mid {
                            M::starts_with => Value::Bool(match idx.first() {
                                Some((_, c)) => char_matches(self, &p, *c)?,
                                None => false,
                            }),
                            M::ends_with => Value::Bool(match idx.last() {
                                Some((_, c)) => char_matches(self, &p, *c)?,
                                None => false,
                            }),
                            M::contains | M::find => {
                                let mut found = None;
                                for (i, c) in &idx {
                                    if char_matches(self, &p, *c)? {
                                        found = Some(*i);
                                        break;
                                    }
                                }
                                if mid == M::contains { Value::Bool(found.is_some()) } else { Value::opt(found.map(|i| Value::Int(i as i64))) }
                            }
                            M::rfind => {
                                let mut found = None;
                                for (i, c) in idx.iter().rev() {
                                    if char_matches(self, &p, *c)? {
                                        found = Some(*i);
                                        break;
                                    }
                                }
                                Value::opt(found.map(|i| Value::Int(i as i64)))
                            }
                            _ => {
                                let mut out = Vec::new();
                                for (_, c) in &idx {
                                    if char_matches(self, &p, *c)? {
                                        out.push(Value::string(c.to_string()));
                                    }
                                }
                                vals_iter(out)
                            }
                        }
                    }
                }
            }
            M::replace | M::replacen => {
                let to = str_of(&sarg(self, 1)?);
                let n = if mid == M::replacen { Some(self.as_int(&arg(&args, 2)?)? as usize) } else { None };
                match pat(self, 0)? {
                    Pat::S(x) => Value::string(match n {
                        Some(n) => s.replacen(x.as_str(), &to, n),
                        None => s.replace(x.as_str(), &to),
                    }),
                    p => {
                        let mut out = String::new();
                        let mut count = 0;
                        for c in s.chars() {
                            if n.is_none_or(|n| count < n) && char_matches(self, &p, c)? {
                                out.push_str(&to);
                                count += 1;
                            } else {
                                out.push(c);
                            }
                        }
                        Value::string(out)
                    }
                }
            }
            M::to_uppercase => Value::string(s.to_uppercase()),
            M::to_lowercase => Value::string(s.to_lowercase()),
            M::to_ascii_uppercase => Value::string(s.to_ascii_uppercase()),
            M::to_ascii_lowercase => Value::string(s.to_ascii_lowercase()),
            M::eq_ignore_ascii_case => Value::Bool(s.eq_ignore_ascii_case(&str_of(&sarg(self, 0)?))),
            M::is_ascii => Value::Bool(s.is_ascii()),
            M::is_char_boundary => Value::Bool(s.is_char_boundary(self.as_int(&arg(&args, 0)?)? as usize)),
            M::repeat => Value::string(s.repeat(self.as_int(&arg(&args, 0)?)? as usize)),
            M::get => match self.deref(arg(&args, 0)?) {
                Value::Range(r) => {
                    let lo = r.lo.as_ref().map(|v| self.as_int(v)).transpose()?.unwrap_or(0) as usize;
                    let hi = match &r.hi {
                        Some(v) => self.as_int(v)? as usize + r.inclusive as usize,
                        None => s.len(),
                    };
                    Value::opt(s.get(lo..hi).map(Value::str))
                }
                _ => return err("str::get expects a range"),
            },
            M::parse => self.parse_str(&s, hint)?,
            M::iter | M::into_iter => vals_iter(s.chars().map(Value::Char).collect()),
            M::cmp => {
                let o = sarg(self, 0)?;
                Interp::ordering_value(s.as_str().cmp(str_of(&o).as_str()))
            }
            M::concat | M::join => return self.no_method(&recv, sym, e),
            _ => {
                // char iterator methods on strings are a common mistake; fall back to an error
                return self.no_method(&recv, sym, e);
            }
        })
    }

    pub fn parse_str(&mut self, s: &str, hint: Option<&Type>) -> R<Value> {
        // find the target type name in the hint (e.g. Result<i32, _> or i32)
        fn find(t: &Type) -> Option<String> {
            let n = t.name.as_str();
            if INT_TYPES.contains(&n) || matches!(n, "f32" | "f64" | "bool" | "char" | "String") {
                return Some(n.to_string());
            }
            t.args.iter().find_map(find)
        }
        let ty = hint.and_then(find);
        Ok(match ty.as_deref() {
            Some(t) if INT_TYPES.contains(&t) => match parse_int_as(s, t) {
                Ok(v) => Value::ok(Value::Int(v)),
                Err(m) => Value::err(Value::str(&m)),
            },
            Some("f32") | Some("f64") => match s.parse::<f64>() {
                Ok(v) => Value::ok(Value::Float(v)),
                Err(_) => Value::err(Value::str("invalid float literal")),
            },
            Some("bool") => match s.parse::<bool>() {
                Ok(v) => Value::ok(Value::Bool(v)),
                Err(_) => Value::err(Value::str("provided string was not `true` or `false`")),
            },
            Some("char") => match s.parse::<char>() {
                Ok(v) => Value::ok(Value::Char(v)),
                Err(_) => Value::err(Value::str("invalid character")),
            },
            Some("String") => Value::ok(Value::string(s.to_string())),
            _ => {
                if let Ok(i) = s.parse::<i64>() {
                    Value::ok(Value::Int(i))
                } else if let Ok(f) = s.parse::<f64>() {
                    Value::ok(Value::Float(f))
                } else {
                    Value::err(Value::str(parse_error_msg(s.parse::<i64>().unwrap_err().kind().clone())))
                }
            }
        })
    }

    // ------------------------------------------------------------ vec/slice
    #[allow(clippy::too_many_arguments)]
    fn vec_method(&mut self, recv: Value, place: Option<Place>, mid: M, sym: Sym, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Value> {
        let (l, start, end) = slice_view(&recv).unwrap();
        let is_vec = matches!(recv, Value::Vec(_));
        let len = end - start;
        let idx = |me: &Self, n: usize| -> R<usize> { Ok(me.as_int(&arg(&args, n)?)? as usize) };
        // growable-vector methods
        if is_vec {
            match mid {
                M::push => {
                    let v = arg(&args, 0)?;
                    l.borrow_mut().push(v);
                    return Ok(Value::Unit);
                }
                M::pop => return Ok(Value::opt(l.borrow_mut().pop())),
                M::insert => {
                    let i = idx(self, 0)?;
                    if i > len {
                        return self.panic_e(e, format!("insertion index (is {}) should be <= len (is {})", i, len));
                    }
                    l.borrow_mut().insert(i, arg(&args, 1)?);
                    return Ok(Value::Unit);
                }
                M::remove => {
                    let i = idx(self, 0)?;
                    if i >= len {
                        return self.panic_e(e, format!("removal index (is {}) should be < len (is {})", i, len));
                    }
                    return Ok(l.borrow_mut().remove(i));
                }
                M::swap_remove => {
                    let i = idx(self, 0)?;
                    if i >= len {
                        return self.panic_e(e, format!("swap_remove index (is {}) should be < len (is {})", i, len));
                    }
                    return Ok(l.borrow_mut().swap_remove(i));
                }
                M::clear => {
                    let old = std::mem::take(&mut *l.borrow_mut());
                    drop(old);
                    return Ok(Value::Unit);
                }
                M::truncate => {
                    let n = idx(self, 0)?;
                    l.borrow_mut().truncate(n);
                    return Ok(Value::Unit);
                }
                M::extend | M::extend_from_slice | M::append => {
                    let src = arg(&args, 0)?;
                    if mid == M::append {
                        if let Value::Vec(o) = self.deref(src.clone()) {
                            let items = std::mem::take(&mut *o.borrow_mut());
                            l.borrow_mut().extend(items);
                            return Ok(Value::Unit);
                        }
                    }
                    let mut st = self.make_iter(src, e)?;
                    let items = self.drain_iter(&mut st)?;
                    let items: Vec<Value> = items.into_iter().map(|x| if x.is_ref() { let d = self.deref(x); self.clone_deep(&d) } else { x }).collect();
                    l.borrow_mut().extend(items);
                    return Ok(Value::Unit);
                }
                M::resize => {
                    let n = idx(self, 0)?;
                    let v = arg(&args, 1)?;
                    let cur = l.borrow().len();
                    if n <= cur {
                        l.borrow_mut().truncate(n);
                    } else {
                        let extra: Vec<Value> = (cur..n).map(|_| self.clone_deep(&v)).collect();
                        l.borrow_mut().extend(extra);
                    }
                    return Ok(Value::Unit);
                }
                M::retain => {
                    let f = arg(&args, 0)?;
                    let items = std::mem::take(&mut *l.borrow_mut());
                    let mut keep = Vec::with_capacity(items.len());
                    for x in items {
                        let r = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(x.clone()))])?;
                        if matches!(self.deref(r), Value::Bool(true)) {
                            keep.push(x);
                        }
                    }
                    *l.borrow_mut() = keep;
                    return Ok(Value::Unit);
                }
                M::dedup | M::dedup_by_key => {
                    let items = std::mem::take(&mut *l.borrow_mut());
                    let mut out: Vec<Value> = Vec::with_capacity(items.len());
                    let f = if mid == M::dedup_by_key { Some(arg(&args, 0)?) } else { None };
                    let mut last_key: Option<Value> = None;
                    for x in items {
                        let k = match &f {
                            Some(f) => self.call_value(f, vec![Value::RefBox(Interp::new_cell(x.clone()))])?,
                            None => x.clone(),
                        };
                        let dup = match &last_key {
                            Some(lk) => self.equals(lk, &k)?,
                            None => false,
                        };
                        if !dup {
                            out.push(x);
                        }
                        last_key = Some(k);
                    }
                    *l.borrow_mut() = out;
                    return Ok(Value::Unit);
                }
                M::drain => {
                    let (a, b) = match args.first().map(|v| self.deref(v.clone())) {
                        Some(Value::Range(r)) => self.range_bounds(&r, len, e)?,
                        _ => (0, len),
                    };
                    let drained: Vec<Value> = l.borrow_mut().drain(a..b).collect();
                    return Ok(vals_iter(drained));
                }
                M::split_off => {
                    let i = idx(self, 0)?;
                    let tail = l.borrow_mut().split_off(i);
                    return Ok(Value::vec(tail));
                }
                M::capacity => return Ok(Value::Int(l.borrow().capacity() as i64)),
                M::reserve | M::shrink_to_fit => return Ok(Value::Unit),
                M::into_iter if place.is_none() || !self.recv_was_ref() => {
                    return Ok(wrap_iter(IterState::List { list: l, i: 0, end, refs: false }));
                }
                _ => {}
            }
        }
        let items = || l.borrow()[start..end].to_vec();
        Ok(match mid {
            M::len => Value::Int(len as i64),
            M::is_empty => Value::Bool(len == 0),
            M::iter | M::iter_mut | M::into_iter => {
                let refs = !(mid == M::into_iter && matches!(recv, Value::Array(_)) && !self.recv_was_ref());
                wrap_iter(IterState::List { list: l, i: start, end, refs })
            }
            M::get | M::get_mut => match self.deref(arg(&args, 0)?) {
                Value::Int(i) => {
                    if i >= 0 && (i as usize) < len {
                        Value::some(Value::RefElem(l, (start + i as usize) as u32))
                    } else {
                        Value::none()
                    }
                }
                Value::Range(r) => {
                    let lo = r.lo.as_ref().map(|v| self.as_int(v)).transpose()?.unwrap_or(0) as usize;
                    let hi = match &r.hi {
                        Some(v) => self.as_int(v)? as usize + r.inclusive as usize,
                        None => len,
                    };
                    if lo <= hi && hi <= len { Value::some(slice_of(&l, start + lo, start + hi)) } else { Value::none() }
                }
                _ => return err("bad index for get"),
            },
            M::first | M::first_mut => {
                if len > 0 { Value::some(Value::RefElem(l, start as u32)) } else { Value::none() }
            }
            M::last | M::last_mut => {
                if len > 0 { Value::some(Value::RefElem(l, (end - 1) as u32)) } else { Value::none() }
            }
            M::split_first | M::split_last => {
                if len == 0 {
                    Value::none()
                } else if mid == M::split_first {
                    Value::some(Value::tuple(vec![Value::RefElem(l.clone(), start as u32), slice_of(&l, start + 1, end)]))
                } else {
                    Value::some(Value::tuple(vec![Value::RefElem(l.clone(), (end - 1) as u32), slice_of(&l, start, end - 1)]))
                }
            }
            M::contains => {
                let x = self.deref(arg(&args, 0)?);
                for v in items() {
                    let v = self.deref(v);
                    if self.equals(&v, &x)? {
                        return Ok(Value::Bool(true));
                    }
                }
                Value::Bool(false)
            }
            M::starts_with | M::ends_with => {
                let o = self.deref(arg(&args, 0)?);
                let other = match slice_view(&o) {
                    Some((ol, a, b)) => ol.borrow()[a..b].to_vec(),
                    None => return err("starts_with expects a slice"),
                };
                let mine = items();
                if other.len() > mine.len() {
                    return Ok(Value::Bool(false));
                }
                let off = if mid == M::starts_with { 0 } else { mine.len() - other.len() };
                for (i, x) in other.iter().enumerate() {
                    if !self.equals(&self.deref(mine[off + i].clone()), &self.deref(x.clone()))? {
                        return Ok(Value::Bool(false));
                    }
                }
                Value::Bool(true)
            }
            M::swap => {
                let (a, b) = (idx(self, 0)?, idx(self, 1)?);
                if a >= len || b >= len {
                    return self.panic_e(e, format!("index out of bounds: the len is {} but the index is {}", len, a.max(b)));
                }
                l.borrow_mut().swap(start + a, start + b);
                Value::Unit
            }
            M::reverse => {
                l.borrow_mut()[start..end].reverse();
                Value::Unit
            }
            M::sort | M::sort_unstable => {
                let mut v = items();
                self.sort_values(&mut v, None, false)?;
                l.borrow_mut()[start..end].clone_from_slice(&v);
                Value::Unit
            }
            M::sort_by | M::sort_unstable_by => {
                let f = arg(&args, 0)?;
                let mut v = items();
                self.sort_values(&mut v, Some(&f), false)?;
                l.borrow_mut()[start..end].clone_from_slice(&v);
                Value::Unit
            }
            M::sort_by_key | M::sort_unstable_by_key | M::sort_by_cached_key => {
                let f = arg(&args, 0)?;
                let mut v = items();
                self.sort_values(&mut v, Some(&f), true)?;
                l.borrow_mut()[start..end].clone_from_slice(&v);
                Value::Unit
            }
            M::fill => {
                let x = arg(&args, 0)?;
                for i in start..end {
                    let c = self.clone_deep(&x);
                    l.borrow_mut()[i] = c;
                }
                Value::Unit
            }
            M::rotate_left | M::rotate_right => {
                let n = idx(self, 0)?;
                if mid == M::rotate_left {
                    l.borrow_mut()[start..end].rotate_left(n);
                } else {
                    l.borrow_mut()[start..end].rotate_right(n);
                }
                Value::Unit
            }
            M::copy_from_slice | M::clone_from_slice => {
                let src = self.deref(arg(&args, 0)?);
                let Some((sl, a, b)) = slice_view(&src) else { return err("copy_from_slice expects a slice") };
                let vals = sl.borrow()[a..b].to_vec();
                if vals.len() != len {
                    return self.panic_e(e, "source slice length does not match destination slice length");
                }
                l.borrow_mut()[start..end].clone_from_slice(&vals);
                Value::Unit
            }
            M::to_vec | M::into_vec | M::to_owned => {
                let v: Vec<Value> = items().iter().map(|x| self.clone_deep(x)).collect();
                Value::vec(v)
            }
            M::as_slice | M::as_mut_slice | M::as_ref | M::as_mut => match place {
                Some(p) => Interp::to_ref(p),
                None => recv.clone(),
            },
            M::join | M::concat => {
                let sep = if mid == M::join { str_of(&self.deref(arg(&args, 0)?)) } else { String::new() };
                let parts = items();
                if parts.first().is_some_and(|x| slice_view(&self.deref(x.clone())).is_some()) {
                    // Vec<Vec<T>>::concat / join
                    let mut out = Vec::new();
                    for (i, p) in parts.iter().enumerate() {
                        if i > 0 && mid == M::join {
                            if let Some((sl, a, b)) = slice_view(&self.deref(arg(&args, 0)?)) {
                                out.extend(sl.borrow()[a..b].iter().cloned());
                            }
                        }
                        let (pl, a, b) = slice_view(&self.deref(p.clone())).unwrap();
                        out.extend(pl.borrow()[a..b].iter().cloned());
                    }
                    return Ok(Value::vec(out));
                }
                let mut out = String::new();
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(&sep);
                    }
                    let p = self.deref(p.clone());
                    match &p {
                        Value::Char(c) => out.push(*c),
                        _ => out.push_str(&str_of(&p)),
                    }
                }
                Value::string(out)
            }
            M::windows | M::chunks | M::chunks_exact => {
                let n = idx(self, 0)?;
                if n == 0 {
                    return self.panic_e(e, "window/chunk size must be non-zero");
                }
                let mut out = Vec::new();
                if mid == M::windows {
                    if len >= n {
                        for i in 0..=(len - n) {
                            out.push(slice_of(&l, start + i, start + i + n));
                        }
                    }
                } else {
                    let mut i = 0;
                    while i < len {
                        let j = (i + n).min(len);
                        if mid == M::chunks_exact && j - i < n {
                            break;
                        }
                        out.push(slice_of(&l, start + i, start + j));
                        i = j;
                    }
                }
                vals_iter(out)
            }
            M::split_at => {
                let i = idx(self, 0)?;
                if i > len {
                    return self.panic_e(e, "mid > len");
                }
                Value::tuple(vec![slice_of(&l, start, start + i), slice_of(&l, start + i, end)])
            }
            M::binary_search | M::binary_search_by | M::binary_search_by_key => {
                let v = items();
                let (mut lo, mut hi) = (0usize, v.len());
                let target = if mid == M::binary_search_by { None } else { Some(self.deref(arg(&args, 0)?)) };
                while lo < hi {
                    let m = (lo + hi) / 2;
                    let x = self.deref(v[m].clone());
                    let c = match mid {
                        M::binary_search => self.cmp_total(&x, target.as_ref().unwrap())?,
                        M::binary_search_by => {
                            let f = arg(&args, 0)?;
                            let r = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(x))])?;
                            self.value_to_ordering(&r).unwrap_or(Ordering::Equal)
                        }
                        _ => {
                            let f = arg(&args, 1)?;
                            let k = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(x))])?;
                            let k = self.deref(k);
                            self.cmp_total(&k, target.as_ref().unwrap())?
                        }
                    };
                    match c {
                        Ordering::Less => lo = m + 1,
                        Ordering::Greater => hi = m,
                        Ordering::Equal => return Ok(Value::ok(Value::Int(m as i64))),
                    }
                }
                Value::err(Value::Int(lo as i64))
            }
            M::repeat => {
                let n = idx(self, 0)?;
                let v = items();
                let mut out = Vec::with_capacity(v.len() * n);
                for _ in 0..n {
                    out.extend(v.iter().map(|x| self.clone_deep(x)));
                }
                Value::vec(out)
            }
            M::is_sorted => {
                let v = items();
                for w in v.windows(2) {
                    let (a, b) = (self.deref(w[0].clone()), self.deref(w[1].clone()));
                    if self.cmp_total(&a, &b)? == Ordering::Greater {
                        return Ok(Value::Bool(false));
                    }
                }
                Value::Bool(true)
            }
            _ => {
                // treat the slice as an iterator (e.g. `v.iter()` omitted for `max`, `sum` …)
                let it = wrap_iter(IterState::List { list: l, i: start, end, refs: true });
                match self.iter_method(it, mid, args, hint, e)? {
                    Some(v) => v,
                    None => return self.no_method(&recv, sym, e),
                }
            }
        })
    }

    fn recv_was_ref(&self) -> bool {
        false
    }

    /// Stable sort with optional comparator (`by_key` = closure returns key).
    fn sort_values(&mut self, v: &mut Vec<Value>, f: Option<&Value>, by_key: bool) -> R<()> {
        if f.is_none() {
            if v.iter().all(|x| matches!(x, Value::Int(_))) {
                v.sort_by_key(|x| if let Value::Int(i) = x { *i } else { 0 });
                return Ok(());
            }
        }
        let keys: Option<Vec<Value>> = match (f, by_key) {
            (Some(f), true) => {
                let mut ks = Vec::with_capacity(v.len());
                for x in v.iter() {
                    let k = self.call_value(f, vec![Value::RefBox(Interp::new_cell(x.clone()))])?;
                    ks.push(self.deref(k));
                }
                Some(ks)
            }
            _ => None,
        };
        let mut idx: Vec<usize> = (0..v.len()).collect();
        let mut error: Option<Ctrl> = None;
        let derefd: Vec<Value> = v.iter().map(|x| self.deref(x.clone())).collect();
        idx.sort_by(|&a, &b| {
            if error.is_some() {
                return Ordering::Equal;
            }
            let r = match (&keys, f) {
                (Some(ks), _) => self.cmp_total(&ks[a], &ks[b]),
                (None, Some(f)) => self
                    .call_value(f, vec![Value::RefBox(Interp::new_cell(derefd[a].clone())), Value::RefBox(Interp::new_cell(derefd[b].clone()))])
                    .map(|r| self.value_to_ordering(&r).unwrap_or(Ordering::Equal)),
                (None, None) => self.cmp_total(&derefd[a], &derefd[b]),
            };
            match r {
                Ok(o) => o,
                Err(e) => {
                    error = Some(e);
                    Ordering::Equal
                }
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
        let sorted: Vec<Value> = idx.into_iter().map(|i| v[i].clone()).collect();
        *v = sorted;
        Ok(())
    }

    // ------------------------------------------------------------------ maps
    fn map_method(&mut self, recv: Value, mid: M, sym: Sym, args: Vec<Value>, e: &Expr) -> R<Value> {
        let Value::Map(m) = &recv else { unreachable!() };
        let key = |me: &Self, n: usize| -> R<Key> { Ok(Key(me.deref(arg(&args, n)?))) };
        Ok(match mid {
            M::insert => {
                let k = key(self, 0)?;
                let v = arg(&args, 1)?;
                let old = m.borrow_mut().insert(k, Interp::new_cell(v));
                Value::opt(old.map(|c| c.borrow().clone()))
            }
            M::get | M::get_mut => {
                let k = key(self, 0)?;
                Value::opt(m.borrow().get(&k).map(|c| Value::RefBox(c.clone())))
            }
            M::contains_key => {
                let k = key(self, 0)?;
                Value::Bool(m.borrow().get(&k).is_some())
            }
            M::remove => {
                let k = key(self, 0)?;
                let r = m.borrow_mut().remove(&k);
                Value::opt(r.map(|c| c.borrow().clone()))
            }
            M::remove_entry => {
                let k = key(self, 0)?;
                let r = m.borrow_mut().remove(&k);
                Value::opt(r.map(|c| Value::tuple(vec![k.0.clone(), c.borrow().clone()])))
            }
            M::len => Value::Int(m.borrow().len() as i64),
            M::is_empty => Value::Bool(m.borrow().len() == 0),
            M::clear => {
                m.borrow_mut().clear();
                Value::Unit
            }
            M::keys | M::into_keys => vals_iter(m.borrow().entries().into_iter().map(|(k, _)| k).collect()),
            M::values | M::into_values => vals_iter(m.borrow().entries().into_iter().map(|(_, v)| v.borrow().clone()).collect()),
            M::values_mut => vals_iter(m.borrow().entries().into_iter().map(|(_, v)| Value::RefBox(v)).collect()),
            M::iter | M::iter_mut => {
                vals_iter(m.borrow().entries().into_iter().map(|(k, v)| Value::tuple(vec![k, Value::RefBox(v)])).collect())
            }
            M::into_iter | M::drain => {
                let entries = m.borrow().entries();
                if mid == M::drain {
                    m.borrow_mut().clear();
                }
                vals_iter(entries.into_iter().map(|(k, v)| Value::tuple(vec![k, v.borrow().clone()])).collect())
            }
            M::entry => {
                let k = self.deref(arg(&args, 0)?);
                Value::Entry(Rc::new((recv.clone(), k)))
            }
            M::extend => {
                let mut st = self.make_iter(arg(&args, 0)?, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    if let Value::Tuple(t) = self.deref(x) {
                        let t = t.borrow();
                        let k = self.deref(t[0].clone());
                        m.borrow_mut().insert(Key(k), Interp::new_cell(t[1].clone()));
                    }
                }
                Value::Unit
            }
            M::retain => {
                let f = arg(&args, 0)?;
                let entries = m.borrow().entries();
                for (k, v) in entries {
                    let r = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(k.clone())), Value::RefBox(v)])?;
                    if !matches!(self.deref(r), Value::Bool(true)) {
                        m.borrow_mut().remove(&Key(k));
                    }
                }
                Value::Unit
            }
            M::first_key_value | M::last_key_value | M::pop_first | M::pop_last => {
                let entries = m.borrow().entries();
                let pick = if matches!(mid, M::first_key_value | M::pop_first) { entries.first() } else { entries.last() };
                match pick {
                    Some((k, v)) => {
                        let r = Value::tuple(vec![k.clone(), v.borrow().clone()]);
                        if matches!(mid, M::pop_first | M::pop_last) {
                            m.borrow_mut().remove(&Key(k.clone()));
                        }
                        Value::some(r)
                    }
                    None => Value::none(),
                }
            }
            M::range => {
                let r = match self.deref(arg(&args, 0)?) {
                    Value::Range(r) => r,
                    _ => return err("range expects a range"),
                };
                let mut out = Vec::new();
                for (k, v) in m.borrow().entries() {
                    let ok_lo = match &r.lo {
                        Some(lo) => struct_cmp(&k, lo) != Some(Ordering::Less),
                        None => true,
                    };
                    let ok_hi = match &r.hi {
                        Some(hi) => match struct_cmp(&k, hi) {
                            Some(Ordering::Less) => true,
                            Some(Ordering::Equal) => r.inclusive,
                            _ => false,
                        },
                        None => true,
                    };
                    if ok_lo && ok_hi {
                        out.push(Value::tuple(vec![k, Value::RefBox(v)]));
                    }
                }
                vals_iter(out)
            }
            _ => return self.no_method(&recv, sym, e),
        })
    }

    fn entry_method(&mut self, recv: Value, mid: M, sym: Sym, args: Vec<Value>, e: &Expr) -> R<Value> {
        let Value::Entry(en) = &recv else { unreachable!() };
        let (map, k) = (&en.0, &en.1);
        let Value::Map(m) = map else { return err("bad entry") };
        let key = Key(k.clone());
        let existing = m.borrow().get(&key).cloned();
        Ok(match mid {
            M::or_insert | M::or_insert_with | M::or_default | M::or_insert_with_key => match existing {
                Some(c) => Value::RefBox(c),
                None => {
                    let v = match mid {
                        M::or_insert => arg(&args, 0)?,
                        M::or_insert_with => {
                            let f = arg(&args, 0)?;
                            self.call_value(&f, vec![])?
                        }
                        M::or_insert_with_key => {
                            let f = arg(&args, 0)?;
                            self.call_value(&f, vec![Value::RefBox(Interp::new_cell(k.clone()))])?
                        }
                        _ => Value::Lazy,
                    };
                    let c = Interp::new_cell(v);
                    m.borrow_mut().insert(key, c.clone());
                    Value::RefBox(c)
                }
            },
            M::and_modify => {
                if let Some(c) = existing {
                    let f = arg(&args, 0)?;
                    self.call_value(&f, vec![Value::RefBox(c)])?;
                }
                recv.clone()
            }
            M::key => k.clone(),
            _ => return self.no_method(&recv, sym, e),
        })
    }

    fn set_method(&mut self, recv: Value, mid: M, sym: Sym, args: Vec<Value>, e: &Expr) -> R<Value> {
        let Value::Set(s) = &recv else { unreachable!() };
        let key = |me: &Self, n: usize| -> R<Key> { Ok(Key(me.deref(arg(&args, n)?))) };
        Ok(match mid {
            M::insert => {
                let k = key(self, 0)?;
                Value::Bool(s.borrow_mut().insert(k))
            }
            M::contains => {
                let k = key(self, 0)?;
                Value::Bool(s.borrow().contains(&k))
            }
            M::remove => {
                let k = key(self, 0)?;
                Value::Bool(s.borrow_mut().remove(&k))
            }
            M::take => {
                let k = key(self, 0)?;
                let had = s.borrow_mut().remove(&k);
                if had { Value::some(k.0) } else { Value::none() }
            }
            M::get => {
                let k = key(self, 0)?;
                if s.borrow().contains(&k) { Value::some(k.0) } else { Value::none() }
            }
            M::len => Value::Int(s.borrow().len() as i64),
            M::is_empty => Value::Bool(s.borrow().len() == 0),
            M::clear => {
                s.borrow_mut().clear();
                Value::Unit
            }
            M::iter | M::into_iter => vals_iter(s.borrow().items()),
            M::drain => {
                let items = s.borrow().items();
                s.borrow_mut().clear();
                vals_iter(items)
            }
            M::extend => {
                let mut st = self.make_iter(arg(&args, 0)?, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    let x = self.deref(x);
                    s.borrow_mut().insert(Key(x));
                }
                Value::Unit
            }
            M::retain => {
                let f = arg(&args, 0)?;
                for k in s.borrow().items() {
                    let r = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(k.clone()))])?;
                    if !matches!(self.deref(r), Value::Bool(true)) {
                        s.borrow_mut().remove(&Key(k));
                    }
                }
                Value::Unit
            }
            M::union | M::intersection | M::difference | M::symmetric_difference => {
                let o = self.deref(arg(&args, 0)?);
                let op = match mid {
                    M::union => BinOp::BitOr,
                    M::intersection => BinOp::BitAnd,
                    M::difference => BinOp::Sub,
                    _ => BinOp::BitXor,
                };
                let r = set_op(op, &recv, &o)?;
                // preserve the iteration order of the receiver for intersection/difference
                let Value::Set(rs) = &r else { unreachable!() };
                let mut items = Vec::new();
                for k in s.borrow().items() {
                    if rs.borrow().contains(&Key(k.clone())) {
                        items.push(k);
                    }
                }
                if let Value::Set(os) = &o {
                    for k in os.borrow().items() {
                        if rs.borrow().contains(&Key(k.clone())) && !s.borrow().contains(&Key(k.clone())) {
                            items.push(k);
                        }
                    }
                }
                vals_iter(items)
            }
            M::is_subset | M::is_superset | M::is_disjoint => {
                let o = self.deref(arg(&args, 0)?);
                let Value::Set(os) = &o else { return err("expected a set") };
                let (a, b) = (s.borrow(), os.borrow());
                Value::Bool(match mid {
                    M::is_subset => a.items().into_iter().all(|k| b.contains(&Key(k))),
                    M::is_superset => b.items().into_iter().all(|k| a.contains(&Key(k))),
                    _ => a.items().into_iter().all(|k| !b.contains(&Key(k))),
                })
            }
            M::first | M::last | M::pop_first | M::pop_last => {
                let items = s.borrow().items();
                let pick = if matches!(mid, M::first | M::pop_first) { items.first().cloned() } else { items.last().cloned() };
                if let Some(k) = &pick {
                    if matches!(mid, M::pop_first | M::pop_last) {
                        s.borrow_mut().remove(&Key(k.clone()));
                    }
                }
                Value::opt(pick)
            }
            _ => {
                let it = vals_iter(s.borrow().items());
                match self.iter_method(it, mid, args, None, e)? {
                    Some(v) => v,
                    None => return self.no_method(&recv, sym, e),
                }
            }
        })
    }

    fn deque_method(&mut self, recv: Value, mid: M, sym: Sym, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Value> {
        let Value::Deque(d) = &recv else { unreachable!() };
        Ok(match mid {
            M::push_back => {
                d.borrow_mut().push_back(arg(&args, 0)?);
                Value::Unit
            }
            M::push_front => {
                d.borrow_mut().push_front(arg(&args, 0)?);
                Value::Unit
            }
            M::pop_back => Value::opt(d.borrow_mut().pop_back()),
            M::pop_front => Value::opt(d.borrow_mut().pop_front()),
            M::front => Value::opt(d.borrow().front().cloned()),
            M::back => Value::opt(d.borrow().back().cloned()),
            M::len => Value::Int(d.borrow().len() as i64),
            M::is_empty => Value::Bool(d.borrow().is_empty()),
            M::clear => {
                d.borrow_mut().clear();
                Value::Unit
            }
            M::get => {
                let i = self.as_int(&arg(&args, 0)?)?;
                Value::opt(if i < 0 { None } else { d.borrow().get(i as usize).cloned() })
            }
            M::contains => {
                let x = self.deref(arg(&args, 0)?);
                let items: Vec<Value> = d.borrow().iter().cloned().collect();
                for v in items {
                    if self.equals(&self.deref(v), &x)? {
                        return Ok(Value::Bool(true));
                    }
                }
                Value::Bool(false)
            }
            M::extend => {
                let mut st = self.make_iter(arg(&args, 0)?, e)?;
                let items = self.drain_iter(&mut st)?;
                d.borrow_mut().extend(items);
                Value::Unit
            }
            M::remove => {
                let i = self.as_int(&arg(&args, 0)?)? as usize;
                Value::opt(d.borrow_mut().remove(i))
            }
            M::insert => {
                let i = self.as_int(&arg(&args, 0)?)? as usize;
                d.borrow_mut().insert(i, arg(&args, 1)?);
                Value::Unit
            }
            M::drain => {
                let items: Vec<Value> = d.borrow_mut().drain(..).collect();
                vals_iter(items)
            }
            M::truncate => {
                let n = self.as_int(&arg(&args, 0)?)? as usize;
                d.borrow_mut().truncate(n);
                Value::Unit
            }
            M::swap => {
                let (a, b) = (self.as_int(&arg(&args, 0)?)? as usize, self.as_int(&arg(&args, 1)?)? as usize);
                d.borrow_mut().swap(a, b);
                Value::Unit
            }
            M::make_contiguous => Value::vec(d.borrow().iter().cloned().collect()),
            _ => {
                let it = vals_iter(d.borrow().iter().cloned().collect());
                match self.iter_method(it, mid, args, hint, e)? {
                    Some(v) => v,
                    None => return self.no_method(&recv, sym, e),
                }
            }
        })
    }

    // -------------------------------------------------------- binary heap
    pub fn heap_push(&mut self, h: &Value, x: Value) -> R<()> {
        let Value::Heap(l) = h else { return err("not a heap") };
        let mut v = std::mem::take(&mut *l.borrow_mut());
        v.push(x);
        let mut i = v.len() - 1;
        while i > 0 {
            let p = (i - 1) / 2;
            if self.cmp_total(&v[i], &v[p])? == Ordering::Greater {
                v.swap(i, p);
                i = p;
            } else {
                break;
            }
        }
        *l.borrow_mut() = v;
        Ok(())
    }

    fn heap_pop(&mut self, l: &List) -> R<Option<Value>> {
        let mut v = std::mem::take(&mut *l.borrow_mut());
        if v.is_empty() {
            *l.borrow_mut() = v;
            return Ok(None);
        }
        let n = v.len() - 1;
        v.swap(0, n);
        let top = v.pop();
        let len = v.len();
        let mut i = 0;
        loop {
            let (a, b) = (2 * i + 1, 2 * i + 2);
            let mut m = i;
            if a < len && self.cmp_total(&v[a], &v[m])? == Ordering::Greater {
                m = a;
            }
            if b < len && self.cmp_total(&v[b], &v[m])? == Ordering::Greater {
                m = b;
            }
            if m == i {
                break;
            }
            v.swap(i, m);
            i = m;
        }
        *l.borrow_mut() = v;
        Ok(top)
    }

    fn heap_method(&mut self, recv: Value, mid: M, sym: Sym, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Value> {
        let Value::Heap(l) = &recv else { unreachable!() };
        Ok(match mid {
            M::push => {
                self.heap_push(&recv, arg(&args, 0)?)?;
                Value::Unit
            }
            M::pop => Value::opt(self.heap_pop(l)?),
            M::peek => Value::opt(l.borrow().first().cloned()),
            M::len => Value::Int(l.borrow().len() as i64),
            M::is_empty => Value::Bool(l.borrow().is_empty()),
            M::clear => {
                l.borrow_mut().clear();
                Value::Unit
            }
            M::into_sorted_vec => {
                let mut v = l.borrow().clone();
                self.sort_values(&mut v, None, false)?;
                Value::vec(v)
            }
            M::into_vec => Value::vec(l.borrow().clone()),
            M::extend => {
                let mut st = self.make_iter(arg(&args, 0)?, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    self.heap_push(&recv, x)?;
                }
                Value::Unit
            }
            M::drain => {
                let items = std::mem::take(&mut *l.borrow_mut());
                vals_iter(items)
            }
            _ => {
                let it = vals_iter(l.borrow().clone());
                match self.iter_method(it, mid, args, hint, e)? {
                    Some(v) => v,
                    None => return self.no_method(&recv, sym, e),
                }
            }
        })
    }

    // ------------------------------------------------------ option / result
    #[allow(clippy::too_many_arguments)]
    fn option_method(&mut self, recv: Value, place: Option<Place>, mid: M, sym: Sym, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Value> {
        let Value::Enum(vid, payload) = &recv else { unreachable!() };
        let vid = *vid;
        let inner = || payload.as_ref().map(|l| l.borrow()[0].clone()).unwrap_or(Value::Unit);
        let inner_ref = || match payload {
            Some(l) => Value::RefElem(l.clone(), 0),
            None => Value::Unit,
        };
        let is_opt = vid == V_SOME || vid == V_NONE;
        let good = vid == V_SOME || vid == V_OK;
        let call = |me: &mut Self, n: usize, a: Vec<Value>| -> R<Value> {
            let f = arg(&args, n)?;
            me.call_value(&f, a)
        };
        let fmt_dbg = |me: &mut Self, v: &Value| -> R<String> {
            let mut s = String::new();
            me.debug_value(v, &crate::format::RSpec::plain(crate::format::FmtTy::Debug), &mut s, 0)?;
            Ok(s)
        };
        Ok(match mid {
            M::is_some | M::is_ok => Value::Bool(good),
            M::is_none | M::is_err => Value::Bool(!good),
            M::is_some_and | M::is_ok_and => {
                if good {
                    let r = call(self, 0, vec![inner()])?;
                    Value::Bool(matches!(self.deref(r), Value::Bool(true)))
                } else {
                    Value::Bool(false)
                }
            }
            M::is_none_or => {
                if good {
                    let r = call(self, 0, vec![inner()])?;
                    Value::Bool(matches!(self.deref(r), Value::Bool(true)))
                } else {
                    Value::Bool(true)
                }
            }
            M::is_err_and => {
                if !good {
                    let r = call(self, 0, vec![inner()])?;
                    Value::Bool(matches!(self.deref(r), Value::Bool(true)))
                } else {
                    Value::Bool(false)
                }
            }
            M::unwrap | M::expect | M::unwrap_unchecked => {
                if good {
                    inner()
                } else if mid == M::expect {
                    let msg = str_of(&self.deref(arg(&args, 0)?));
                    if is_opt {
                        return self.panic_e(e, msg);
                    }
                    let d = fmt_dbg(self, &inner())?;
                    return self.panic_e(e, format!("{}: {}", msg, d));
                } else if is_opt {
                    return self.panic_e(e, "called `Option::unwrap()` on a `None` value");
                } else {
                    let d = fmt_dbg(self, &inner())?;
                    return self.panic_e(e, format!("called `Result::unwrap()` on an `Err` value: {}", d));
                }
            }
            M::unwrap_err | M::expect_err => {
                if !good {
                    inner()
                } else {
                    let d = fmt_dbg(self, &inner())?;
                    let msg = if mid == M::expect_err { str_of(&self.deref(arg(&args, 0)?)) } else { "called `Result::unwrap_err()` on an `Ok` value".into() };
                    return self.panic_e(e, format!("{}: {}", msg, d));
                }
            }
            M::unwrap_or => {
                if good {
                    inner()
                } else {
                    arg(&args, 0)?
                }
            }
            M::unwrap_or_else => {
                if good {
                    inner()
                } else if is_opt {
                    call(self, 0, vec![])?
                } else {
                    call(self, 0, vec![inner()])?
                }
            }
            M::unwrap_or_default => {
                if good {
                    inner()
                } else {
                    match hint {
                        Some(t) => {
                            let inner_t = if t.name == "Result" && !t.args.is_empty() { &t.args[0] } else { t };
                            self.default_for_type(inner_t)?
                        }
                        None => Value::Lazy,
                    }
                }
            }
            M::map => {
                if good {
                    let r = call(self, 0, vec![inner()])?;
                    Value::Enum(vid, Some(new_list(vec![r])))
                } else {
                    recv.clone()
                }
            }
            M::map_err => {
                if !good {
                    let r = call(self, 0, vec![inner()])?;
                    Value::err(r)
                } else {
                    recv.clone()
                }
            }
            M::inspect => {
                if good {
                    call(self, 0, vec![inner_ref()])?;
                }
                recv.clone()
            }
            M::map_or => {
                if good {
                    call(self, 1, vec![inner()])?
                } else {
                    arg(&args, 0)?
                }
            }
            M::map_or_else => {
                if good {
                    call(self, 1, vec![inner()])?
                } else if is_opt {
                    call(self, 0, vec![])?
                } else {
                    call(self, 0, vec![inner()])?
                }
            }
            M::and_then => {
                if good {
                    call(self, 0, vec![inner()])?
                } else {
                    recv.clone()
                }
            }
            M::or_else => {
                if good {
                    recv.clone()
                } else if is_opt {
                    call(self, 0, vec![])?
                } else {
                    call(self, 0, vec![inner()])?
                }
            }
            M::or => {
                if good {
                    recv.clone()
                } else {
                    arg(&args, 0)?
                }
            }
            M::and => {
                if good {
                    arg(&args, 0)?
                } else {
                    recv.clone()
                }
            }
            M::xor => {
                let o = self.deref(arg(&args, 0)?);
                let og = matches!(o, Value::Enum(V_SOME, _));
                match (good, og) {
                    (true, false) => recv.clone(),
                    (false, true) => o,
                    _ => Value::none(),
                }
            }
            M::ok => {
                if good {
                    Value::some(inner())
                } else {
                    Value::none()
                }
            }
            M::err => {
                if good {
                    Value::none()
                } else {
                    Value::some(inner())
                }
            }
            M::ok_or => {
                if good {
                    Value::ok(inner())
                } else {
                    Value::err(arg(&args, 0)?)
                }
            }
            M::ok_or_else => {
                if good {
                    Value::ok(inner())
                } else {
                    Value::err(call(self, 0, vec![])?)
                }
            }
            M::filter => {
                if good {
                    let r = call(self, 0, vec![inner_ref()])?;
                    if matches!(self.deref(r), Value::Bool(true)) { recv.clone() } else { Value::none() }
                } else {
                    recv.clone()
                }
            }
            M::flatten => {
                if good {
                    inner()
                } else {
                    recv.clone()
                }
            }
            M::zip => {
                let o = self.deref(arg(&args, 0)?);
                match (&recv, &o) {
                    (Value::Enum(V_SOME, Some(a)), Value::Enum(V_SOME, Some(b))) => {
                        Value::some(Value::tuple(vec![a.borrow()[0].clone(), b.borrow()[0].clone()]))
                    }
                    _ => Value::none(),
                }
            }
            M::cloned | M::copied => {
                if good {
                    let d = self.deref(inner());
                    Value::Enum(vid, Some(new_list(vec![self.clone_deep(&d)])))
                } else {
                    recv.clone()
                }
            }
            M::as_ref | M::as_mut | M::as_deref | M::as_deref_mut | M::borrow | M::borrow_mut => {
                if payload.is_some() {
                    Value::Enum(vid, Some(new_list(vec![inner_ref()])))
                } else {
                    recv.clone()
                }
            }
            M::iter | M::iter_mut | M::into_iter => {
                if good {
                    vals_iter(vec![if mid == M::into_iter { inner() } else { inner_ref() }])
                } else {
                    wrap_iter(IterState::Empty)
                }
            }
            M::take | M::replace | M::insert | M::get_or_insert | M::get_or_insert_with => {
                let p = match place {
                    Some(p) => p,
                    None => return err(format!("{}:{}: `{}` requires a place", e.line, e.col, sym_name(sym))),
                };
                match mid {
                    M::take => {
                        self.store(&p, Value::none());
                        recv.clone()
                    }
                    M::replace => {
                        self.store(&p, Value::some(arg(&args, 0)?));
                        recv.clone()
                    }
                    M::insert => {
                        let l = new_list(vec![arg(&args, 0)?]);
                        self.store(&p, Value::Enum(V_SOME, Some(l.clone())));
                        Value::RefElem(l, 0)
                    }
                    _ => {
                        if good {
                            inner_ref()
                        } else {
                            let v = if mid == M::get_or_insert { arg(&args, 0)? } else { call(self, 0, vec![])? };
                            let l = new_list(vec![v]);
                            self.store(&p, Value::Enum(V_SOME, Some(l.clone())));
                            Value::RefElem(l, 0)
                        }
                    }
                }
            }
            M::unwrap_or_clone => inner(),
            _ => return self.no_method(&recv, sym, e),
        })
    }

    fn ordering_method(&mut self, recv: Value, mid: M, sym: Sym, args: Vec<Value>, e: &Expr) -> R<Value> {
        let o = self.value_to_ordering(&recv).unwrap();
        Ok(match mid {
            M::reverse => Interp::ordering_value(o.reverse()),
            M::then => {
                if o == Ordering::Equal {
                    self.deref(arg(&args, 0)?)
                } else {
                    recv.clone()
                }
            }
            M::then_with => {
                if o == Ordering::Equal {
                    let f = arg(&args, 0)?;
                    self.call_value(&f, vec![])?
                } else {
                    recv.clone()
                }
            }
            M::is_lt => Value::Bool(o == Ordering::Less),
            M::is_le => Value::Bool(o != Ordering::Greater),
            M::is_gt => Value::Bool(o == Ordering::Greater),
            M::is_ge => Value::Bool(o != Ordering::Less),
            M::is_eq => Value::Bool(o == Ordering::Equal),
            M::is_ne => Value::Bool(o != Ordering::Equal),
            _ => return self.no_method(&recv, sym, e),
        })
    }

    fn formatter_method(&mut self, f: Rc<FmtTarget>, mid: M, sym: Sym, args: Vec<Value>, e: &Expr) -> R<Value> {
        Ok(match mid {
            M::write_str | M::write_fmt => {
                let s = str_of(&self.deref(arg(&args, 0)?));
                f.buf.borrow_mut().push_str(&s);
                Value::ok(Value::Unit)
            }
            M::write_char => {
                if let Value::Char(c) = self.deref(arg(&args, 0)?) {
                    f.buf.borrow_mut().push(c);
                }
                Value::ok(Value::Unit)
            }
            M::pad => {
                let s = str_of(&self.deref(arg(&args, 0)?));
                let rs = crate::format::RSpec {
                    fill: f.spec.fill,
                    align: f.spec.align,
                    width: match f.spec.width {
                        Some(crate::format::Count::Fixed(n)) => Some(n),
                        _ => None,
                    },
                    precision: match f.spec.precision {
                        Some(crate::format::Count::Fixed(n)) => Some(n),
                        _ => None,
                    },
                    ..Default::default()
                };
                let padded = crate::format::fmt_str(&s, &rs);
                f.buf.borrow_mut().push_str(&padded);
                Value::ok(Value::Unit)
            }
            M::width | M::precision => {
                let c = if mid == M::width { f.spec.width } else { f.spec.precision };
                Value::opt(match c {
                    Some(crate::format::Count::Fixed(n)) => Some(Value::Int(n as i64)),
                    _ => None,
                })
            }
            M::alternate => Value::Bool(f.spec.alt),
            _ => return self.no_method(&Value::Formatter(f), sym, e),
        })
    }
}

fn intern_default() -> Sym {
    crate::ast::intern("default")
}

fn intern_m(m: M) -> Sym {
    crate::ast::intern(&format!("{:?}", m))
}

fn sym_name(s: Sym) -> String {
    crate::ast::sym_str(s)
}


