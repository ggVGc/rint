//! Tree-walking evaluator.

use crate::ast::*;
use crate::format::{self, FmtTy, RSpec};
use crate::iter::IterState;
use crate::program::*;
use crate::value::*;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::io::Write;
use std::rc::Rc;

/// Non-local control flow. Deliberately payload-free (1 byte) so that
/// `Result<Value, Ctrl>` stays 16 bytes and is returned in registers; the
/// payload lives in a thread-local (see `ctrl_*` helpers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctrl {
    Break,
    Continue,
    Return,
    /// A Rust panic (message includes the location header).
    Panic,
    /// `std::process::exit`
    Exit,
    /// Interpreter error (unsupported feature, type confusion, ...).
    Error,
}

#[derive(Default)]
struct CtrlData {
    label: Option<Sym>,
    val: Value,
    msg: String,
    code: i32,
}

thread_local! {
    static CTRL: RefCell<CtrlData> = RefCell::new(CtrlData::default());
}

pub fn ctrl_break(label: Option<Sym>, v: Value) -> Ctrl {
    CTRL.with(|c| {
        let mut c = c.borrow_mut();
        c.label = label;
        c.val = v;
    });
    Ctrl::Break
}
pub fn ctrl_continue(label: Option<Sym>) -> Ctrl {
    CTRL.with(|c| c.borrow_mut().label = label);
    Ctrl::Continue
}
pub fn ctrl_return(v: Value) -> Ctrl {
    CTRL.with(|c| c.borrow_mut().val = v);
    Ctrl::Return
}
pub fn ctrl_panic(msg: String) -> Ctrl {
    CTRL.with(|c| c.borrow_mut().msg = msg);
    Ctrl::Panic
}
pub fn ctrl_error(msg: String) -> Ctrl {
    CTRL.with(|c| c.borrow_mut().msg = msg);
    Ctrl::Error
}
pub fn ctrl_exit(code: i32) -> Ctrl {
    CTRL.with(|c| c.borrow_mut().code = code);
    Ctrl::Exit
}
pub fn ctrl_label() -> Option<Sym> {
    CTRL.with(|c| c.borrow().label)
}
/// Does a pending break/continue target the loop with `label`?
#[inline]
pub fn label_ok(label: Option<Sym>) -> bool {
    let l = ctrl_label();
    l.is_none() || l == label
}
pub fn take_ctrl_val() -> Value {
    CTRL.with(|c| std::mem::take(&mut c.borrow_mut().val))
}
pub fn take_ctrl_msg() -> String {
    CTRL.with(|c| std::mem::take(&mut c.borrow_mut().msg))
}
pub fn ctrl_code() -> i32 {
    CTRL.with(|c| c.borrow().code)
}

pub type R<T> = Result<T, Ctrl>;

#[derive(Clone)]
pub enum Place {
    Slot(usize),
    Elem(List, usize),
    Cell(VCell),
}

pub struct Interp {
    pub prog: Rc<Program>,
    pub stack: Vec<Value>,
    pub fp: usize,
    pub closure: Option<Rc<Closure>>,
    pub cur_fn: Option<Rc<FnDecl>>,
    pub consts: Vec<Option<Value>>,
    pub statics: Vec<Option<VCell>>,
    pub out: Box<dyn Write>,
    pub file: String,
    pub args: Vec<String>,
    pub stdin: Option<Box<dyn std::io::BufRead>>,
    pub depth: usize,
    pub start: std::time::Instant,
    syms: Syms,
}

/// Frequently used interned symbols.
pub struct Syms {
    pub next: Sym,
    pub display_fmt: Sym,
    pub debug_fmt: Sym,
    pub from: Sym,
    pub into_iter: Sym,
}

pub const MAX_DEPTH: usize = 20_000;

pub fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(ctrl_error(msg.into()))
}

impl Interp {
    pub fn new(prog: Program, out: Box<dyn Write>, file: &str, args: Vec<String>) -> Interp {
        let nconsts = prog.consts.len();
        Interp {
            prog: Rc::new(prog),
            stack: Vec::with_capacity(1 << 16),
            fp: 0,
            closure: None,
            cur_fn: None,
            consts: vec![None; nconsts],
            statics: vec![None; nconsts],
            out,
            file: file.to_string(),
            args,
            stdin: None,
            depth: 0,
            start: std::time::Instant::now(),
            syms: Syms {
                next: intern("next"),
                display_fmt: intern("Display::fmt"),
                debug_fmt: intern("Debug::fmt"),
                from: intern("from"),
                into_iter: intern("into_iter"),
            },
        }
    }

    pub fn syms(&self) -> &Syms {
        &self.syms
    }

    /// Run `main`. Returns the process exit code.
    pub fn run_main(&mut self) -> Result<i32, String> {
        let Some(main) = self.prog.main else { return Err("no `main` function found".into()) };
        let fd = self.prog.fns[main as usize].clone();
        let base = self.stack.len();
        let r = self.invoke(fd, base, None);
        let code = match r {
            Ok(v) => match self.deref(v) {
                Value::Enum(V_ERR, Some(l)) => {
                    let e = l.borrow()[0].clone();
                    let mut s = String::new();
                    let _ = self.format_value(&e, &RSpec::plain(FmtTy::Debug), &mut s);
                    let _ = self.out.flush();
                    eprintln!("Error: {}", s);
                    1
                }
                _ => 0,
            },
            Err(Ctrl::Panic) => {
                let msg = take_ctrl_msg();
                let _ = self.out.flush();
                eprintln!("{}", msg);
                eprintln!("note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace");
                101
            }
            Err(Ctrl::Exit) => ctrl_code(),
            Err(Ctrl::Error) => {
                let m = take_ctrl_msg();
                let _ = self.out.flush();
                return Err(m);
            }
            Err(Ctrl::Break) | Err(Ctrl::Continue) => {
                let _ = self.out.flush();
                return Err("`break`/`continue` outside of a loop".into());
            }
            Err(Ctrl::Return) => 0,
        };
        let _ = self.out.flush();
        Ok(code)
    }

    // ------------------------------------------------------------- errors
    pub fn panic_at<T>(&self, line: u32, col: u32, msg: impl AsRef<str>) -> R<T> {
        Err(ctrl_panic(format!("thread 'main' panicked at {}:{}:{}:\n{}", self.file, line, col, msg.as_ref())))
    }
    pub fn panic_e<T>(&self, e: &Expr, msg: impl AsRef<str>) -> R<T> {
        self.panic_at(e.line, e.col, msg)
    }
    fn overflow_err(&self, op: BinOp, e: &Expr) -> Ctrl {
        let what = match op {
            BinOp::Add => "add",
            BinOp::Sub => "subtract",
            BinOp::Mul => "multiply",
            BinOp::Div => "divide",
            BinOp::Rem => "calculate the remainder",
            BinOp::Shl => "shift left",
            BinOp::Shr => "shift right",
            _ => "compute",
        };
        ctrl_panic(format!("thread 'main' panicked at {}:{}:{}:\nattempt to {} with overflow", self.file, e.line, e.col, what))
    }

    // ------------------------------------------------------------- places
    #[inline]
    pub fn load(&self, p: &Place) -> Value {
        match p {
            Place::Slot(i) => self.stack[*i].clone(),
            Place::Elem(l, i) => l.borrow()[*i].clone(),
            Place::Cell(c) => c.borrow().clone(),
        }
    }
    #[inline]
    pub fn store(&mut self, p: &Place, v: Value) {
        match p {
            Place::Slot(i) => self.stack[*i] = v,
            Place::Elem(l, i) => {
                let old = std::mem::replace(&mut l.borrow_mut()[*i], v);
                drop(old);
            }
            Place::Cell(c) => {
                let old = c.replace(v);
                drop(old);
            }
        }
    }
    pub fn to_ref(p: Place) -> Value {
        match p {
            Place::Slot(i) => Value::RefSlot(i as u32),
            Place::Elem(l, i) => Value::RefElem(l, i as u32),
            Place::Cell(c) => Value::RefBox(c),
        }
    }
    pub fn ref_place(v: &Value) -> Option<Place> {
        match v {
            Value::RefSlot(i) => Some(Place::Slot(*i as usize)),
            Value::RefElem(l, i) => Some(Place::Elem(l.clone(), *i as usize)),
            Value::RefBox(c) => Some(Place::Cell(c.clone())),
            _ => None,
        }
    }
    /// Follow references (and `Rc`/`Box`) until a non-reference value.
    #[inline]
    pub fn deref(&self, v: Value) -> Value {
        match v {
            Value::RefSlot(_) | Value::RefElem(..) | Value::RefBox(_) | Value::RcBox(_) => self.deref_slow(v),
            v => v,
        }
    }

    #[inline(never)]
    fn deref_slow(&self, mut v: Value) -> Value {
        loop {
            v = match &v {
                Value::RefSlot(i) => self.stack[*i as usize].clone(),
                Value::RefElem(l, i) => l.borrow()[*i as usize].clone(),
                Value::RefBox(c) => c.borrow().clone(),
                Value::RcBox(c) => c.borrow().clone(),
                _ => return v,
            }
        }
    }
    /// Follow references only (not `Rc`), returning the final place too.
    pub fn deref_place(&self, mut v: Value, mut place: Option<Place>) -> (Value, Option<Place>) {
        while let Some(p) = Self::ref_place(&v) {
            v = self.load(&p);
            place = Some(p);
        }
        (v, place)
    }

    pub fn new_cell(v: Value) -> VCell {
        Rc::new(RefCell::new(v))
    }

    fn cap_place(&self, i: u32) -> R<Place> {
        let Some(c) = &self.closure else { return err("capture outside closure") };
        if c.fd.is_move {
            Ok(Place::Elem(c.caps.clone(), i as usize))
        } else {
            let v = c.caps.borrow()[i as usize].clone();
            match Self::ref_place(&v) {
                Some(p) => Ok(p),
                None => Ok(Place::Elem(c.caps.clone(), i as usize)),
            }
        }
    }

    pub fn place(&mut self, e: &Expr) -> R<Place> {
        match &e.kind {
            ExprKind::Local(s) => Ok(Place::Slot(self.fp + *s as usize)),
            ExprKind::Cap(i) => self.cap_place(*i),
            ExprKind::Static(id) => Ok(Place::Cell(self.static_cell(*id)?)),
            ExprKind::Field(b, sym, cache) => {
                let base = self.eval_noc(b)?;
                let base = self.deref(base);
                match &base {
                    Value::Struct(def, l) => {
                        let (cd, ci) = cache.get();
                        let idx = if cd == *def {
                            ci as usize
                        } else {
                            let sd = &self.prog.structs[*def as usize];
                            match sd.fields.iter().position(|f| f == sym) {
                                Some(i) => {
                                    cache.set((*def, i as u32));
                                    i
                                }
                                None => return err(format!("{}:{}: no field `{}` on `{}`", e.line, e.col, sym_str(*sym), sd.name)),
                            }
                        };
                        Ok(Place::Elem(l.clone(), idx))
                    }
                    Value::Enum(vid, Some(l)) => {
                        let vd = &self.prog.variants[*vid as usize];
                        match vd.fields.iter().position(|f| f == sym) {
                            Some(i) => Ok(Place::Elem(l.clone(), i)),
                            None => err(format!("{}:{}: no field `{}`", e.line, e.col, sym_str(*sym))),
                        }
                    }
                    Value::Lazy => err(format!("{}:{}: field access on a value of unknown type", e.line, e.col)),
                    other => err(format!("{}:{}: no field `{}` on {}", e.line, e.col, sym_str(*sym), other.type_name())),
                }
            }
            ExprKind::TupleField(b, i) => {
                let base = self.eval_noc(b)?;
                let base = self.deref(base);
                match &base {
                    Value::Tuple(l) | Value::Struct(_, l) | Value::Enum(_, Some(l)) => {
                        if (*i as usize) < l.borrow().len() {
                            Ok(Place::Elem(l.clone(), *i as usize))
                        } else {
                            err(format!("{}:{}: no field `{}`", e.line, e.col, i))
                        }
                    }
                    other => err(format!("{}:{}: no field `{}` on {}", e.line, e.col, i, other.type_name())),
                }
            }
            ExprKind::Index(b, idx) => {
                let base = self.eval_noc(b)?;
                let base = self.deref(base);
                let iv = self.eval(idx)?;
                let iv = self.deref(iv);
                self.index_place(base, iv, e)
            }
            ExprKind::Deref(x) => {
                let v = self.eval_noc(x)?;
                match v {
                    Value::RefSlot(i) => Ok(Place::Slot(i as usize)),
                    Value::RefElem(l, i) => Ok(Place::Elem(l, i as usize)),
                    Value::RefBox(c) | Value::RcBox(c) => Ok(Place::Cell(c)),
                    v => {
                        if x.is_place() {
                            self.place(x)
                        } else {
                            Ok(Place::Cell(Self::new_cell(v)))
                        }
                    }
                }
            }
            _ => {
                let v = self.eval(e)?;
                Ok(Place::Cell(Self::new_cell(v)))
            }
        }
    }

    fn index_place(&mut self, base: Value, iv: Value, e: &Expr) -> R<Place> {
        match (&base, &iv) {
            (Value::Vec(l) | Value::Array(l), Value::Int(i)) => {
                let len = l.borrow().len();
                if *i < 0 || *i as usize >= len {
                    return self.panic_e(e, format!("index out of bounds: the len is {} but the index is {}", len, i));
                }
                Ok(Place::Elem(l.clone(), *i as usize))
            }
            (Value::Vec(l) | Value::Array(l), Value::Range(r)) => {
                let len = l.borrow().len();
                let (a, b) = self.range_bounds(r, len, e)?;
                Ok(Place::Cell(Self::new_cell(crate::builtins::slice_of(l, a, b))))
            }
            (Value::Map(m), _) => {
                let k = Key(self.deref(iv.clone()));
                let c = m.borrow().get(&k).cloned();
                match c {
                    Some(c) => Ok(Place::Cell(c)),
                    None => self.panic_e(e, "key not found in map"),
                }
            }
            (Value::Deque(d), Value::Int(i)) => {
                let d = d.borrow();
                match d.get(*i as usize) {
                    Some(v) => Ok(Place::Cell(Self::new_cell(v.clone()))),
                    None => self.panic_e(e, format!("index out of bounds: the len is {} but the index is {}", d.len(), i)),
                }
            }
            (Value::Str(_) | Value::String(_), Value::Range(r)) => {
                let s = crate::builtins::str_of(&base);
                let (a, b) = self.range_bounds(r, s.len(), e)?;
                if !s.is_char_boundary(a) || !s.is_char_boundary(b) {
                    return self.panic_e(e, format!("byte index {} is not a char boundary", if s.is_char_boundary(a) { b } else { a }));
                }
                Ok(Place::Cell(Self::new_cell(Value::str(&s[a..b]))))
            }
            (Value::Lazy, _) => err(format!("{}:{}: indexing a value of unknown type", e.line, e.col)),
            (b, i) => match crate::builtins::slice_view(b) {
                Some((l, start, end)) => match i {
                    Value::Int(i) => {
                        let len = end - start;
                        if *i < 0 || *i as usize >= len {
                            return self.panic_e(e, format!("index out of bounds: the len is {} but the index is {}", len, i));
                        }
                        Ok(Place::Elem(l, start + *i as usize))
                    }
                    Value::Range(r) => {
                        let (a, b) = self.range_bounds(r, end - start, e)?;
                        Ok(Place::Cell(Self::new_cell(crate::builtins::slice_of(&l, start + a, start + b))))
                    }
                    _ => err(format!("{}:{}: cannot index with {}", e.line, e.col, i.type_name())),
                },
                None => err(format!("{}:{}: cannot index into a value of type {}", e.line, e.col, b.type_name())),
            },
        }
    }

    pub fn range_bounds(&self, r: &RangeVal, len: usize, e: &Expr) -> R<(usize, usize)> {
        let a = match &r.lo {
            Some(v) => self.as_int(v)? as usize,
            None => 0,
        };
        let b = match &r.hi {
            Some(v) => self.as_int(v)? as usize + r.inclusive as usize,
            None => len,
        };
        if a > b {
            return self.panic_e(e, format!("slice index starts at {} but ends at {}", a, b));
        }
        if b > len {
            return self.panic_e(e, format!("range end index {} out of range for slice of length {}", b, len));
        }
        Ok((a, b))
    }

    pub fn as_int(&self, v: &Value) -> R<i64> {
        match self.deref(v.clone()) {
            Value::Int(i) => Ok(i),
            Value::Float(f) => Ok(f as i64),
            Value::Char(c) => Ok(c as i64),
            Value::Bool(b) => Ok(b as i64),
            other => err(format!("expected an integer, found {}", other.type_name())),
        }
    }

    // ------------------------------------------------------- copy / clone
    pub fn is_copy(&self, v: &Value) -> bool {
        match v {
            Value::Tuple(l) | Value::Array(l) => l.borrow().iter().all(|x| self.is_copy(x)),
            Value::Struct(d, _) => self.prog.structs[*d as usize].copy,
            Value::Enum(vid, p) => {
                let eid = self.prog.variants[*vid as usize].enum_id;
                if eid <= E_RESULT {
                    p.as_ref().is_none_or(|l| l.borrow().iter().all(|x| self.is_copy(x)))
                } else {
                    self.prog.enums[eid as usize].copy
                }
            }
            Value::String(_)
            | Value::Vec(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Deque(_)
            | Value::Heap(_)
            | Value::Iter(_)
            | Value::RcBox(_)
            | Value::CellBox(_)
            | Value::Closure(_) => false,
            _ => true,
        }
    }

    /// Value-context read: copy `Copy` containers so they are not aliased.
    #[inline]
    pub fn copy_val(&self, v: Value) -> Value {
        match &v {
            Value::Tuple(_) | Value::Array(_) | Value::Struct(..) | Value::Enum(_, Some(_)) => {
                if self.is_copy(&v) {
                    self.deep_copy(&v)
                } else {
                    v
                }
            }
            _ => v,
        }
    }

    fn deep_copy(&self, v: &Value) -> Value {
        let cp = |l: &List| new_list(l.borrow().iter().map(|x| self.deep_copy(x)).collect());
        match v {
            Value::Tuple(l) => Value::Tuple(cp(l)),
            Value::Array(l) => Value::Array(cp(l)),
            Value::Struct(d, l) => Value::Struct(*d, cp(l)),
            Value::Enum(vid, Some(l)) => Value::Enum(*vid, Some(cp(l))),
            _ => v.clone(),
        }
    }

    /// `.clone()`: deep copy of owned data; `Rc` is shared.
    pub fn clone_deep(&self, v: &Value) -> Value {
        let cp = |l: &List| new_list(l.borrow().iter().map(|x| self.clone_deep(x)).collect());
        match v {
            Value::String(s) => Value::string(s.borrow().clone()),
            Value::Tuple(l) => Value::Tuple(cp(l)),
            Value::Array(l) => Value::Array(cp(l)),
            Value::Vec(l) => Value::Vec(cp(l)),
            Value::Heap(l) => Value::Heap(cp(l)),
            Value::Struct(d, l) => Value::Struct(*d, cp(l)),
            Value::Enum(vid, Some(l)) => Value::Enum(*vid, Some(cp(l))),
            Value::Map(m) => {
                let m = m.borrow();
                let mut out = if m.is_tree() { MapData::Tree(Default::default()) } else { MapData::Hash(Default::default()) };
                for (k, val) in m.entries() {
                    out.insert(Key(k), Self::new_cell(self.clone_deep(&val.borrow())));
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            Value::Set(s) => {
                let s = s.borrow();
                let mut out = if s.is_tree() { SetData::Tree(Default::default()) } else { SetData::Hash(Default::default()) };
                for k in s.items() {
                    out.insert(Key(k));
                }
                Value::Set(Rc::new(RefCell::new(out)))
            }
            Value::Deque(d) => Value::Deque(Rc::new(RefCell::new(d.borrow().iter().map(|x| self.clone_deep(x)).collect()))),
            Value::CellBox(c) => Value::CellBox(Self::new_cell(self.clone_deep(&c.borrow()))),
            Value::Iter(it) => match it.borrow().try_clone() {
                Some(s) => Value::Iter(Rc::new(RefCell::new(s))),
                None => v.clone(),
            },
            _ => v.clone(),
        }
    }

    // ---------------------------------------------------------------- consts
    fn get_const(&mut self, id: u32) -> R<Value> {
        if let Some(v) = &self.consts[id as usize] {
            return Ok(self.copy_val(v.clone()));
        }
        let v = self.eval_const_init(id)?;
        self.consts[id as usize] = Some(v.clone());
        Ok(self.copy_val(v))
    }

    fn eval_const_init(&mut self, id: u32) -> R<Value> {
        let prog = self.prog.clone();
        let c = &prog.consts[id as usize];
        let Some(init) = &c.init else { return err(format!("constant `{}` has no value", c.name)) };
        let base = self.stack.len();
        self.stack.resize(base + c.nslots as usize, Value::Unit);
        let saved = std::mem::replace(&mut self.fp, base);
        let saved_cl = self.closure.take();
        let r = self.eval(init);
        self.fp = saved;
        self.closure = saved_cl;
        self.stack.truncate(base);
        r
    }

    fn static_cell(&mut self, id: u32) -> R<VCell> {
        if let Some(c) = &self.statics[id as usize] {
            return Ok(c.clone());
        }
        let v = self.eval_const_init(id)?;
        let c = Self::new_cell(v);
        self.statics[id as usize] = Some(c.clone());
        Ok(c)
    }

    // ---------------------------------------------------------------- calls
    /// Invoke `fd` with its arguments already pushed at `stack[base..]`.
    pub fn invoke(&mut self, fd: Rc<FnDecl>, base: usize, closure: Option<Rc<Closure>>) -> R<Value> {
        let nargs = self.stack.len() - base;
        if nargs != fd.params.len() {
            self.stack.truncate(base);
            return err(format!("function `{}` takes {} arguments but {} were supplied", fd.name, fd.params.len(), nargs));
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            self.stack.truncate(base);
            return Err(ctrl_panic(format!(
                "\nthread 'main' has overflowed its stack\nfatal runtime error: stack overflow (interpreter depth limit {})",
                MAX_DEPTH
            )));
        }
        self.stack.resize(base + fd.nslots as usize, Value::Unit);
        let saved_fp = std::mem::replace(&mut self.fp, base);
        let saved_cl = std::mem::replace(&mut self.closure, closure);
        let saved_fn = if fd.is_closure { None } else { Some(std::mem::replace(&mut self.cur_fn, Some(fd.clone()))) };
        let r = self.run_body(&fd);
        self.fp = saved_fp;
        self.closure = saved_cl;
        if let Some(f) = saved_fn {
            self.cur_fn = f;
        }
        self.depth -= 1;
        self.stack.truncate(base);
        match r {
            Ok(v) => Ok(v),
            Err(Ctrl::Return) => Ok(take_ctrl_val()),
            Err(Ctrl::Break) | Err(Ctrl::Continue) => err("`break` or `continue` escaped a function"),
            Err(e) => Err(e),
        }
    }

    fn run_body(&mut self, fd: &FnDecl) -> R<Value> {
        for (i, p) in fd.params.iter().enumerate() {
            match &p.pat {
                Pat::Bind { slot, by_ref: false, sub: None, .. } if *slot == i as u32 => {}
                pat => {
                    let v = std::mem::take(&mut self.stack[self.fp + i]);
                    if !self.match_pat(pat, v, None, false)? {
                        return err("refutable pattern in function argument");
                    }
                }
            }
        }
        match &fd.body {
            Some(b) => self.eval_block(b),
            None => err(format!("function `{}` has no body", fd.name)),
        }
    }

    fn push_args(&mut self, args: &[Expr]) -> R<usize> {
        let base = self.stack.len();
        for a in args {
            match self.eval(a) {
                Ok(v) => self.stack.push(v),
                Err(e) => {
                    self.stack.truncate(base);
                    return Err(e);
                }
            }
        }
        Ok(base)
    }

    pub fn eval_args(&mut self, args: &[Expr]) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
    }

    /// Call any callable value with arguments.
    pub fn call_value(&mut self, f: &Value, args: Vec<Value>) -> R<Value> {
        let base = self.stack.len();
        self.stack.extend(args);
        self.call_value_at(f, base)
    }

    fn call_value_at(&mut self, f: &Value, base: usize) -> R<Value> {
        match f {
            Value::Fn(id) => {
                let fd = self.prog.fns[*id as usize].clone();
                self.invoke(fd, base, None)
            }
            Value::Closure(c) => self.invoke(c.fd.clone(), base, Some(c.clone())),
            Value::Ctor(id, is_variant) => {
                let args: Vec<Value> = self.stack.drain(base..).collect();
                Ok(if *is_variant { Value::Enum(*id, Some(new_list(args))) } else { Value::Struct(*id, new_list(args)) })
            }
            Value::BuiltinFn(id) => {
                let args: Vec<Value> = self.stack.drain(base..).collect();
                if *id >= crate::builtins::METHOD_FN_BASE {
                    let sym = *id - crate::builtins::METHOD_FN_BASE;
                    let mut it = args.into_iter();
                    let recv = it.next().unwrap_or(Value::Unit);
                    return self.call_method_value(recv, sym, it.collect(), None);
                }
                let dummy = Expr::new(ExprKind::Lit(Value::Unit), 0, 0);
                self.call_builtin(*id, args, None, &dummy)
            }
            Value::RefSlot(_) | Value::RefElem(..) | Value::RefBox(_) | Value::RcBox(_) => {
                let f = self.deref(f.clone());
                self.call_value_at(&f, base)
            }
            other => {
                self.stack.truncate(base);
                err(format!("value of type {} is not callable", other.type_name()))
            }
        }
    }

    /// Call a method by symbol on an already-evaluated receiver.
    pub fn call_method_value(&mut self, recv: Value, sym: Sym, args: Vec<Value>, hint: Option<&Type>) -> R<Value> {
        let (recv, place) = self.deref_place(recv, None);
        let tk = self.type_key(&recv);
        if let Some(fid) = self.prog.find_method(tk, sym) {
            return self.call_user_method(fid, recv, place, args);
        }
        let dummy = Expr::new(ExprKind::Lit(Value::Unit), 0, 0);
        let mid = crate::builtins::method_id(&sym_str(sym));
        self.builtin_method(recv, place, mid, sym, args, hint, &dummy)
    }

    pub fn call_user_method(&mut self, fid: u32, recv: Value, place: Option<Place>, args: Vec<Value>) -> R<Value> {
        let fd = self.prog.fns[fid as usize].clone();
        let selfv = match fd.self_kind {
            SelfKind::Value => self.copy_val(recv),
            SelfKind::Ref | SelfKind::RefMut => match place {
                Some(p) => Self::to_ref(p),
                None => Value::RefBox(Self::new_cell(recv)),
            },
            SelfKind::None => {
                // associated function called with method syntax via a path
                let base = self.stack.len();
                self.stack.push(recv);
                self.stack.extend(args);
                return self.invoke(fd, base, None);
            }
        };
        let base = self.stack.len();
        self.stack.push(selfv);
        self.stack.extend(args);
        self.invoke(fd, base, None)
    }

    pub fn type_key(&self, v: &Value) -> u32 {
        match v {
            Value::Struct(d, _) => *d,
            Value::Enum(vid, _) => TK_ENUM + self.prog.variants[*vid as usize].enum_id,
            Value::Int(_) => TK_INT,
            Value::Float(_) => TK_FLOAT,
            Value::Bool(_) => TK_BOOL,
            Value::Char(_) => TK_CHAR,
            Value::Str(_) | Value::String(_) => TK_STR,
            Value::Vec(_) | Value::Array(_) => TK_VEC,
            Value::Tuple(_) => TK_TUPLE,
            Value::Map(_) => TK_MAP,
            Value::Set(_) => TK_SET,
            Value::Unit => TK_UNIT,
            Value::Deque(_) => TK_DEQUE,
            _ => TK_OTHER,
        }
    }

    #[inline(never)]
    fn eval_method_call(&mut self, m: &MethodCall, e: &Expr) -> R<Value> {
        let (recv, place) = if m.recv.is_place() {
            let p = self.place(&m.recv)?;
            (self.load(&p), Some(p))
        } else {
            (self.eval(&m.recv)?, None)
        };
        let (mut recv, mut place) = self.deref_place(recv, place);
        let mut tk = self.type_key(&recv);
        // Rc<T> auto-deref for user methods
        if let Value::RcBox(c) = &recv {
            if m.name != "clone" {
                let inner = c.borrow().clone();
                let itk = self.type_key(&inner);
                if self.prog.find_method(itk, m.sym).is_some() {
                    place = Some(Place::Cell(c.clone()));
                    recv = inner;
                    tk = itk;
                }
            }
        }
        let (ctk, cfid) = m.cache.get();
        let fid = if ctk == tk + 1 {
            if cfid == u32::MAX { None } else { Some(cfid) }
        } else {
            let f = self.prog.find_method(tk, m.sym);
            m.cache.set((tk + 1, f.unwrap_or(u32::MAX)));
            f
        };
        if let Some(fid) = fid {
            let fd = self.prog.fns[fid as usize].clone();
            let selfv = match fd.self_kind {
                SelfKind::Value => self.copy_val(recv),
                SelfKind::Ref | SelfKind::RefMut => match place {
                    Some(p) => Self::to_ref(p),
                    None => Value::RefBox(Self::new_cell(recv)),
                },
                SelfKind::None => return err(format!("{}:{}: `{}` is not a method", e.line, e.col, m.name)),
            };
            let base = self.stack.len();
            self.stack.push(selfv);
            for a in &m.args {
                match self.eval(a) {
                    Ok(v) => self.stack.push(v),
                    Err(err) => {
                        self.stack.truncate(base);
                        return Err(err);
                    }
                }
            }
            return self.invoke(fd, base, None);
        }
        let args = self.eval_args(&m.args)?;
        self.builtin_method(recv, place, m.mid, m.sym, args, m.hint.as_deref(), e)
    }

    // ------------------------------------------------------------- blocks
    pub fn eval_block(&mut self, b: &Block) -> R<Value> {
        if let Some(label) = b.label {
            return match self.eval_block_inner(b) {
                Err(Ctrl::Break) if ctrl_label() == Some(label) => Ok(take_ctrl_val()),
                r => r,
            };
        }
        self.eval_block_inner(b)
    }

    fn eval_block_inner(&mut self, b: &Block) -> R<Value> {
        for s in &b.stmts {
            match s {
                Stmt::Expr(e, _) => {
                    self.eval(e)?;
                }
                Stmt::Let(l) => self.exec_let(l)?,
            }
        }
        match &b.tail {
            Some(t) => self.eval(t),
            None => Ok(Value::Unit),
        }
    }

    #[inline(never)]
    fn exec_let(&mut self, l: &LetStmt) -> R<()> {
        match &l.init {
            None => {
                if let Pat::Bind { slot, .. } = &l.pat {
                    self.stack[self.fp + *slot as usize] = Value::Unit;
                }
                Ok(())
            }
            Some(init) => {
                if let Pat::Bind { slot, by_ref: false, sub: None, .. } = &l.pat {
                    let v = self.eval(init)?;
                    self.stack[self.fp + *slot as usize] = v;
                    return Ok(());
                }
                let (v, place) = self.eval_scrutinee(init)?;
                if self.match_pat(&l.pat, v, place, false)? {
                    return Ok(());
                }
                match &l.els {
                    Some(b) => {
                        self.eval_block(b)?;
                        err("`let ... else` block did not diverge")
                    }
                    None => err("refutable pattern in `let` did not match"),
                }
            }
        }
    }

    pub fn eval_scrutinee(&mut self, e: &Expr) -> R<(Value, Option<Place>)> {
        if e.is_place() {
            let p = self.place(e)?;
            Ok((self.load(&p), Some(p)))
        } else {
            Ok((self.eval(e)?, None))
        }
    }

    fn eval_conds(&mut self, conds: &[Cond]) -> R<bool> {
        for c in conds {
            match c {
                Cond::Expr(e) => match self.eval(e)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => return Ok(false),
                    v => match self.deref(v) {
                        Value::Bool(true) => {}
                        Value::Bool(false) => return Ok(false),
                        other => return err(format!("{}:{}: expected bool, found {}", e.line, e.col, other.type_name())),
                    },
                },
                Cond::Let(p, e) => {
                    let (v, place) = self.eval_scrutinee(e)?;
                    if !self.match_pat(p, v, place, false)? {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    #[inline]
    fn truthy(&self, v: Value, e: &Expr) -> R<bool> {
        match v {
            Value::Bool(b) => Ok(b),
            v => match self.deref(v) {
                Value::Bool(b) => Ok(b),
                other => err(format!("{}:{}: expected bool, found {}", e.line, e.col, other.type_name())),
            },
        }
    }

    // ----------------------------------------------------------- patterns
    fn auto_deref(&self, v: Value, place: Option<Place>, ref_mode: bool) -> (Value, Option<Place>, bool) {
        if v.is_ref() {
            let (v, p) = self.deref_place(v, place);
            (v, p, true)
        } else if let Value::RcBox(c) = &v {
            let inner = c.borrow().clone();
            (inner, Some(Place::Cell(c.clone())), ref_mode)
        } else {
            (v, place, ref_mode)
        }
    }

    #[inline(never)]
    pub fn match_pat(&mut self, pat: &Pat, v: Value, place: Option<Place>, ref_mode: bool) -> R<bool> {
        match pat {
            Pat::Wild | Pat::Rest => Ok(true),
            Pat::Bind { slot, by_ref, sub, .. } => {
                if let Some(s) = sub {
                    if !matches!(**s, Pat::Rest) && !self.match_pat(s, v.clone(), place.clone(), ref_mode)? {
                        return Ok(false);
                    }
                }
                let bound = if *by_ref || ref_mode {
                    match place {
                        Some(p) => Self::to_ref(p),
                        None => {
                            if *by_ref {
                                Value::RefBox(Self::new_cell(v))
                            } else {
                                v
                            }
                        }
                    }
                } else {
                    self.copy_val(v)
                };
                self.stack[self.fp + *slot as usize] = bound;
                Ok(true)
            }
            Pat::Ref(inner) => {
                let (v, place) = if v.is_ref() {
                    let p = Self::ref_place(&v).unwrap();
                    (self.load(&p), Some(p))
                } else {
                    (v, place)
                };
                self.match_pat(inner, v, place, false)
            }
            Pat::Lit(l) => {
                let v = self.deref(v);
                Ok(match (&v, l) {
                    (Value::Int(a), Value::Int(b)) => a == b,
                    _ => struct_eq(&v, l),
                })
            }
            Pat::Range(lo, hi, incl) => {
                let v = self.deref(v);
                if let Some(lo) = lo {
                    if struct_cmp(&v, lo) == Some(Ordering::Less) {
                        return Ok(false);
                    }
                }
                if let Some(hi) = hi {
                    match struct_cmp(&v, hi) {
                        Some(Ordering::Greater) => return Ok(false),
                        Some(Ordering::Equal) if !incl => return Ok(false),
                        None => return Ok(false),
                        _ => {}
                    }
                }
                Ok(true)
            }
            Pat::RangeExpr(lo, hi, incl) => {
                let v = self.deref(v);
                if let Some(lo) = lo {
                    let lo = self.eval(lo)?;
                    if self.cmp_total(&v, &lo)? == Ordering::Less {
                        return Ok(false);
                    }
                }
                if let Some(hi) = hi {
                    let hi = self.eval(hi)?;
                    match self.cmp_total(&v, &hi)? {
                        Ordering::Greater => return Ok(false),
                        Ordering::Equal if !incl => return Ok(false),
                        _ => {}
                    }
                }
                Ok(true)
            }
            Pat::Tuple(ps) => {
                let (v, _place, rm) = self.auto_deref(v, place, ref_mode);
                match &v {
                    Value::Tuple(l) => self.match_seq(ps, l, 0, l.borrow().len(), rm),
                    Value::Unit if ps.is_empty() => Ok(true),
                    other => err(format!("tuple pattern cannot match {}", other.type_name())),
                }
            }
            Pat::TupleStruct { target, elems, .. } => {
                let (v, _place, rm) = self.auto_deref(v, place, ref_mode);
                match (&v, target) {
                    (Value::Enum(vid, payload), Target::Variant(t)) => {
                        if vid != t {
                            return Ok(false);
                        }
                        match payload {
                            Some(l) => self.match_seq(elems, l, 0, l.borrow().len(), rm),
                            None => Ok(elems.is_empty()),
                        }
                    }
                    (Value::Struct(d, l), Target::Struct(t)) => {
                        if d != t {
                            return Ok(false);
                        }
                        self.match_seq(elems, l, 0, l.borrow().len(), rm)
                    }
                    (Value::Enum(..), Target::Struct(_)) | (Value::Struct(..), Target::Variant(_)) => Ok(false),
                    (other, _) => err(format!("tuple-struct pattern cannot match {}", other.type_name())),
                }
            }
            Pat::Struct { target, fields, .. } => {
                let (v, _place, rm) = self.auto_deref(v, place, ref_mode);
                let l = match (&v, target) {
                    (Value::Enum(vid, payload), Target::Variant(t)) => {
                        if vid != t {
                            return Ok(false);
                        }
                        match payload {
                            Some(l) => l.clone(),
                            None => return Ok(fields.is_empty()),
                        }
                    }
                    (Value::Struct(d, l), Target::Struct(t)) => {
                        if d != t {
                            return Ok(false);
                        }
                        l.clone()
                    }
                    (Value::Enum(..), Target::Struct(_)) | (Value::Struct(..), Target::Variant(_)) => return Ok(false),
                    (other, _) => return err(format!("struct pattern cannot match {}", other.type_name())),
                };
                for (_, idx, p) in fields {
                    let x = l.borrow()[*idx as usize].clone();
                    if !self.match_pat(p, x, Some(Place::Elem(l.clone(), *idx as usize)), rm)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Pat::Path { value, .. } => {
                let v = self.deref(v);
                let c = self.eval(value.as_ref().unwrap())?;
                match (&v, &c) {
                    (Value::Enum(a, _), Value::Enum(b, None)) => Ok(a == b),
                    _ => self.equals(&v, &c),
                }
            }
            Pat::Or(alts) => {
                for a in alts {
                    if self.match_pat(a, v.clone(), place.clone(), ref_mode)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Pat::Slice(ps) => {
                let (v, _place, rm) = self.auto_deref(v, place, ref_mode);
                match crate::builtins::slice_view(&v) {
                    Some((l, start, end)) => self.match_seq(ps, &l, start, end, rm),
                    None => err(format!("slice pattern cannot match {}", v.type_name())),
                }
            }
        }
    }

    fn match_seq(&mut self, ps: &[Pat], l: &List, start: usize, end: usize, rm: bool) -> R<bool> {
        let len = end - start;
        let rest = ps.iter().position(|p| matches!(p, Pat::Rest) || matches!(p, Pat::Bind { sub: Some(s), .. } if matches!(**s, Pat::Rest)));
        match rest {
            None => {
                if ps.len() != len {
                    return Ok(false);
                }
                for (i, p) in ps.iter().enumerate() {
                    let x = l.borrow()[start + i].clone();
                    if !self.match_pat(p, x, Some(Place::Elem(l.clone(), start + i)), rm)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Some(r) => {
                let after = ps.len() - r - 1;
                if len < r + after {
                    return Ok(false);
                }
                for i in 0..r {
                    let x = l.borrow()[start + i].clone();
                    if !self.match_pat(&ps[i], x, Some(Place::Elem(l.clone(), start + i)), rm)? {
                        return Ok(false);
                    }
                }
                for j in 0..after {
                    let idx = end - after + j;
                    let x = l.borrow()[idx].clone();
                    if !self.match_pat(&ps[r + 1 + j], x, Some(Place::Elem(l.clone(), idx)), rm)? {
                        return Ok(false);
                    }
                }
                if let Pat::Bind { slot, .. } = &ps[r] {
                    let sl = crate::builtins::slice_of(l, start + r, end - after);
                    self.stack[self.fp + *slot as usize] = sl;
                }
                Ok(true)
            }
        }
    }

    // --------------------------------------------------------- expressions
    /// Read an expression without copying (place reads share the value).
    #[inline]
    pub fn eval_noc(&mut self, e: &Expr) -> R<Value> {
        match &e.kind {
            ExprKind::Local(s) => Ok(self.stack[self.fp + *s as usize].clone()),
            ExprKind::Field(b, sym, cache) => {
                let base = self.eval_noc(b)?;
                let base = self.deref(base);
                if let Value::Struct(def, l) = &base {
                    let (cd, ci) = cache.get();
                    if cd == *def {
                        return Ok(l.borrow()[ci as usize].clone());
                    }
                }
                let p = self.place(e)?;
                Ok(self.load(&p))
            }
            ExprKind::TupleField(b, i) => {
                let base = self.eval_noc(b)?;
                if let Value::Tuple(l) | Value::Struct(_, l) = self.deref(base) {
                    if let Some(v) = l.borrow().get(*i as usize) {
                        return Ok(v.clone());
                    }
                }
                let p = self.place(e)?;
                Ok(self.load(&p))
            }
            ExprKind::Index(b, idx) => {
                let base = self.eval_noc(b)?;
                let base = self.deref(base);
                let iv = self.eval(idx)?;
                if let (Value::Vec(l) | Value::Array(l), Value::Int(i)) = (&base, &iv) {
                    if let Some(v) = l.borrow().get(*i as usize) {
                        if *i >= 0 {
                            return Ok(v.clone());
                        }
                    }
                }
                let iv = self.deref(iv);
                let p = self.index_place(base, iv, e)?;
                Ok(self.load(&p))
            }
            ExprKind::Field(..) | ExprKind::TupleField(..) | ExprKind::Index(..) | ExprKind::Deref(_) | ExprKind::Cap(_)
            | ExprKind::Static(_) => {
                let p = self.place(e)?;
                Ok(self.load(&p))
            }
            _ => self.eval(e),
        }
    }

    /// Evaluate in a value context. Small hot dispatcher; everything else
    /// lives in `eval_slow` so this function keeps a tiny stack frame.
    #[inline]
    pub fn eval(&mut self, e: &Expr) -> R<Value> {
        match &e.kind {
            ExprKind::Lit(Value::Int(i)) => Ok(Value::Int(*i)),
            ExprKind::Local(s) => {
                let v = &self.stack[self.fp + *s as usize];
                match v {
                    Value::Int(i) => Ok(Value::Int(*i)),
                    Value::Bool(b) => Ok(Value::Bool(*b)),
                    Value::Float(f) => Ok(Value::Float(*f)),
                    _ => self.eval_slow(e),
                }
            }
            ExprKind::Binary(op, a, b) => self.eval_binary(*op, a, b, e),
            ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::TupleField(..) => {
                let v = self.eval_noc(e)?;
                match v {
                    Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::Char(_) => Ok(v),
                    v => Ok(self.copy_val(v)),
                }
            }
            ExprKind::BinLI(op, s, c) => {
                if let Value::Int(x) = self.stack[self.fp + *s as usize] {
                    return self.int_op(*op, x, *c, e);
                }
                let l = self.stack[self.fp + *s as usize].clone();
                self.bin_values(*op, l, Value::Int(*c), e)
            }
            ExprKind::BinLL(op, s1, s2) => {
                match (&self.stack[self.fp + *s1 as usize], &self.stack[self.fp + *s2 as usize]) {
                    (Value::Int(x), Value::Int(y)) => return self.int_op(*op, *x, *y, e),
                    (Value::Float(x), Value::Float(y)) => {
                        let (x, y) = (*x, *y);
                        match op {
                            BinOp::Add => return Ok(Value::Float(x + y)),
                            BinOp::Sub => return Ok(Value::Float(x - y)),
                            BinOp::Mul => return Ok(Value::Float(x * y)),
                            BinOp::Div => return Ok(Value::Float(x / y)),
                            BinOp::Lt => return Ok(Value::Bool(x < y)),
                            BinOp::Gt => return Ok(Value::Bool(x > y)),
                            _ => {}
                        }
                    }
                    _ => {}
                }
                let l = self.stack[self.fp + *s1 as usize].clone();
                let r = self.stack[self.fp + *s2 as usize].clone();
                self.bin_values(*op, l, r, e)
            }
            ExprKind::AssignLocal(s, rhs) => {
                let v = self.eval(rhs)?;
                self.stack[self.fp + *s as usize] = v;
                Ok(Value::Unit)
            }
            ExprKind::OpAssignLocal(op, s, rhs) => {
                let rv = self.eval(rhs)?;
                let slot = self.fp + *s as usize;
                if let (Value::Int(a), Value::Int(b)) = (&self.stack[slot], &rv) {
                    let r = self.int_op(*op, *a, *b, e)?;
                    self.stack[slot] = r;
                    return Ok(Value::Unit);
                }
                self.op_assign(*op, Place::Slot(slot), rv, false, e)?;
                Ok(Value::Unit)
            }
            _ => self.eval_slow(e),
        }
    }

    /// Binary operator on already-evaluated operands.
    #[inline(never)]
    fn bin_values(&mut self, op: BinOp, l: Value, r: Value, e: &Expr) -> R<Value> {
        match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => self.compare_op(op, l, r),
            _ => self.binary_slow(op, l, r, e),
        }
    }

    /// `place <op>= rv`
    #[inline(never)]
    fn op_assign(&mut self, op: BinOp, p: Place, rv: Value, lhs_is_deref: bool, e: &Expr) -> R<()> {
        let cur = self.load(&p);
        let (cur, p) = match Self::ref_place(&cur) {
            Some(rp) if !lhs_is_deref => (self.load(&rp), rp),
            _ => (cur, p),
        };
        match (&cur, &rv) {
            (Value::Int(a), Value::Int(b)) => {
                let r = self.int_op(op, *a, *b, e)?;
                self.store(&p, r);
                return Ok(());
            }
            (Value::Float(a), Value::Float(b)) if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) => {
                let r = match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    _ => a / b,
                };
                self.store(&p, Value::Float(r));
                return Ok(());
            }
            _ => {}
        }
        self.op_assign_slow(op, p, cur, rv, e)
    }

    #[inline(never)]
    fn eval_slow(&mut self, e: &Expr) -> R<Value> {
        match &e.kind {
            ExprKind::Lit(v) => Ok(v.clone()),
            ExprKind::Local(s) => {
                let v = &self.stack[self.fp + *s as usize];
                match v {
                    Value::Int(i) => Ok(Value::Int(*i)),
                    Value::Bool(b) => Ok(Value::Bool(*b)),
                    Value::Float(f) => Ok(Value::Float(*f)),
                    _ => {
                        let v = v.clone();
                        Ok(self.copy_val(v))
                    }
                }
            }
            ExprKind::Field(..) | ExprKind::TupleField(..) | ExprKind::Index(..) | ExprKind::Deref(_) | ExprKind::Cap(_)
            | ExprKind::Static(_) => {
                let v = self.eval_noc(e)?;
                Ok(self.copy_val(v))
            }
            ExprKind::Const(id) => self.get_const(*id),
            ExprKind::Binary(op, a, b) => self.eval_binary(*op, a, b, e),
            ExprKind::Unary(op, x) => {
                let v = self.eval(x)?;
                self.unary(*op, v, e)
            }
            ExprKind::Block(b) => self.eval_block(b),
            ExprKind::If(b) => {
                let (conds, then, els) = &**b;
                if conds.len() == 1 {
                    if let Cond::Expr(c) = &conds[0] {
                        let cv = self.eval(c)?;
                        return if self.truthy(cv, c)? {
                            self.eval_block(then)
                        } else if let Some(e) = els {
                            self.eval(e)
                        } else {
                            Ok(Value::Unit)
                        };
                    }
                }
                if self.eval_conds(conds)? {
                    self.eval_block(then)
                } else if let Some(e) = els {
                    self.eval(e)
                } else {
                    Ok(Value::Unit)
                }
            }
            ExprKind::Match(b) => {
                let (scrut, arms) = &**b;
                let (v, place) = self.eval_scrutinee(scrut)?;
                for arm in arms {
                    if self.match_pat(&arm.pat, v.clone(), place.clone(), false)? {
                        if let Some(g) = &arm.guard {
                            if !self.eval_conds(g)? {
                                continue;
                            }
                        }
                        return self.eval(&arm.body);
                    }
                }
                let v = self.deref(v);
                let mut s = String::new();
                let _ = self.format_value(&v, &RSpec::plain(FmtTy::Debug), &mut s);
                err(format!("{}:{}: no match arm matched value {}", e.line, e.col, s))
            }
            ExprKind::While(b) => {
                let (label, conds, body) = &**b;
                let single = match conds.as_slice() {
                    [Cond::Expr(c)] => Some(c),
                    _ => None,
                };
                loop {
                    let go = match single {
                        Some(c) => {
                            let v = self.eval(c)?;
                            self.truthy(v, c)?
                        }
                        None => self.eval_conds(conds)?,
                    };
                    if !go {
                        break;
                    }
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Ctrl::Break) if label_ok(*label) => {
                            take_ctrl_val();
                            break;
                        }
                        Err(Ctrl::Continue) if label_ok(*label) => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }
            ExprKind::Loop(b) => {
                let (label, body) = &**b;
                loop {
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Ctrl::Break) if label_ok(*label) => return Ok(take_ctrl_val()),
                        Err(Ctrl::Continue) if label_ok(*label) => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            ExprKind::For(b) => self.eval_for(b),
            ExprKind::Break(label, v) => {
                let v = match v {
                    Some(x) => self.eval(x)?,
                    None => Value::Unit,
                };
                Err(ctrl_break(*label, v))
            }
            ExprKind::Continue(label) => Err(ctrl_continue(*label)),
            ExprKind::Return(v) => {
                let v = match v {
                    Some(x) => self.eval(x)?,
                    None => Value::Unit,
                };
                Err(ctrl_return(v))
            }
            ExprKind::CallFn(id, args) => {
                let base = self.push_args(args)?;
                let fd = self.prog.fns[*id as usize].clone();
                self.invoke(fd, base, None)
            }
            ExprKind::Call(callee, args) => {
                let f = self.eval_noc(callee)?;
                let base = self.push_args(args)?;
                self.call_value_at(&f, base)
            }
            ExprKind::CallBuiltin(id, args, hint) => {
                let args = self.eval_args(args)?;
                self.call_builtin(*id, args, hint.as_deref(), e)
            }
            ExprKind::MakeVariant(vid, args) => {
                let args = self.eval_args(args)?;
                Ok(Value::Enum(*vid, Some(new_list(args))))
            }
            ExprKind::MakeStruct(id, args) => {
                let args = self.eval_args(args)?;
                Ok(Value::Struct(*id, new_list(args)))
            }
            ExprKind::MethodCall(m) => self.eval_method_call(m, e),
            ExprKind::Assign(lhs, rhs) => {
                let v = self.eval(rhs)?;
                self.assign_to(lhs, v)?;
                Ok(Value::Unit)
            }
            ExprKind::OpAssign(op, lhs, rhs) => {
                let rv = self.eval(rhs)?;
                let p = self.place(lhs)?;
                self.op_assign(*op, p, rv, matches!(lhs.kind, ExprKind::Deref(_)), e)?;
                Ok(Value::Unit)
            }
            ExprKind::Cast(x, ty) => {
                let v = self.eval(x)?;
                self.cast(v, ty, e)
            }
            ExprKind::Ref(_, x) => {
                if x.is_place() {
                    let p = self.place(x)?;
                    Ok(Self::to_ref(p))
                } else {
                    let v = self.eval(x)?;
                    match v {
                        Value::Str(_) => Ok(v),
                        v => Ok(Value::RefBox(Self::new_cell(v))),
                    }
                }
            }
            ExprKind::StructLit(s) => self.eval_struct_lit(s, e),
            ExprKind::Tuple(es) => {
                let vs = self.eval_args(es)?;
                Ok(Value::Tuple(new_list(vs)))
            }
            ExprKind::Array(es) => {
                let vs = self.eval_args(es)?;
                Ok(Value::Array(new_list(vs)))
            }
            ExprKind::VecLit(es) => {
                let vs = self.eval_args(es)?;
                Ok(Value::Vec(new_list(vs)))
            }
            ExprKind::Repeat(x, n) | ExprKind::VecRepeat(x, n) => {
                let v = self.eval(x)?;
                let n = self.eval(n)?;
                let n = self.as_int(&n)? as usize;
                let mut items = Vec::with_capacity(n);
                if n > 0 {
                    for _ in 1..n {
                        items.push(self.clone_deep(&v));
                    }
                    items.push(v);
                }
                Ok(if matches!(e.kind, ExprKind::Repeat(..)) { Value::Array(new_list(items)) } else { Value::Vec(new_list(items)) })
            }
            ExprKind::Range(lo, hi, incl) => {
                let lo = match lo {
                    Some(x) => Some(self.eval(x)?),
                    None => None,
                };
                let hi = match hi {
                    Some(x) => Some(self.eval(x)?),
                    None => None,
                };
                let lo = lo.map(|v| self.deref(v));
                let hi = hi.map(|v| self.deref(v));
                Ok(Value::Range(Rc::new(RangeVal { lo, hi, inclusive: *incl })))
            }
            ExprKind::Closure(fd) => {
                let mut caps = Vec::with_capacity(fd.caps.len());
                for src in &fd.caps {
                    let p = match src {
                        CapSrc::Local(s) => Place::Slot(self.fp + *s as usize),
                        CapSrc::Cap(j) => self.cap_place(*j)?,
                    };
                    caps.push(if fd.is_move {
                        let v = self.load(&p);
                        self.copy_val(v)
                    } else {
                        Self::to_ref(p)
                    });
                }
                Ok(Value::Closure(Rc::new(Closure { fd: fd.clone(), caps: new_list(caps) })))
            }
            ExprKind::Try(x) => {
                let v = self.eval(x)?;
                let v = self.deref(v);
                match &v {
                    Value::Enum(V_OK, Some(l)) | Value::Enum(V_SOME, Some(l)) => {
                        let x = l.borrow()[0].clone();
                        Ok(x)
                    }
                    Value::Enum(V_NONE, _) => Err(ctrl_return(v)),
                    Value::Enum(V_ERR, Some(l)) => {
                        let ev = l.borrow()[0].clone();
                        let ev = self.convert_err(ev)?;
                        Err(ctrl_return(Value::err(ev)))
                    }
                    other => err(format!("{}:{}: `?` applied to {}", e.line, e.col, other.type_name())),
                }
            }
            ExprKind::Macro(m) => self.eval_macro(m, e),
            ExprKind::Matches(b) => {
                let (x, pat, guard) = &**b;
                let (v, place) = self.eval_scrutinee(x)?;
                if !self.match_pat(pat, v, place, false)? {
                    return Ok(Value::Bool(false));
                }
                match guard {
                    Some(g) => {
                        let gv = self.eval(g)?;
                        Ok(Value::Bool(self.truthy(gv, g)?))
                    }
                    None => Ok(Value::Bool(true)),
                }
            }
            ExprKind::Path(p) => err(format!("{}:{}: unresolved path `{}`", e.line, e.col, p.segs.join("::"))),
            ExprKind::Let(_) => err(format!("{}:{}: unexpected `let`", e.line, e.col)),
            ExprKind::DestructAssign(_) => err("unsupported destructuring assignment"),
            ExprKind::BinLL(..) | ExprKind::BinLI(..) | ExprKind::AssignLocal(..) | ExprKind::OpAssignLocal(..) | ExprKind::Binary(..) => self.eval(e),
        }
    }

    /// `?` error conversion through a user `From` impl on the function's error type.
    fn convert_err(&mut self, ev: Value) -> R<Value> {
        let Some(fd) = &self.cur_fn else { return Ok(ev) };
        let Some(ret) = &fd.ret else { return Ok(ev) };
        if ret.name != "Result" || ret.args.len() < 2 {
            return Ok(ev);
        }
        let ename = ret.args[1].base().name.clone();
        let Some(tk) = self.user_type_key(&ename) else { return Ok(ev) };
        if self.type_key(&self.deref(ev.clone())) == tk {
            return Ok(ev);
        }
        match self.prog.find_method(tk, self.syms.from) {
            Some(fid) => {
                let fd = self.prog.fns[fid as usize].clone();
                let base = self.stack.len();
                self.stack.push(ev);
                self.invoke(fd, base, None)
            }
            None => Ok(ev),
        }
    }

    pub fn user_type_key(&self, name: &str) -> Option<u32> {
        if let Some(i) = self.prog.structs.iter().position(|s| s.name == name) {
            return Some(i as u32);
        }
        self.prog.enums.iter().position(|s| s.name == name).map(|i| TK_ENUM + i as u32)
    }

    #[inline(never)]
    fn eval_struct_lit(&mut self, s: &StructLit, e: &Expr) -> R<Value> {
        let n = match s.target {
            Target::Struct(id) => self.prog.structs[id as usize].fields.len(),
            Target::Variant(v) => self.prog.variants[v as usize].fields.len(),
            Target::Unresolved => return err("unresolved struct literal"),
        };
        let mut vals = vec![Value::Unit; n];
        let mut set = vec![false; n];
        for (_, idx, fe) in &s.fields {
            vals[*idx as usize] = self.eval(fe)?;
            set[*idx as usize] = true;
        }
        if let Some(b) = &s.base {
            let bv = self.eval_noc(b)?;
            let bv = self.deref(bv);
            match &bv {
                Value::Struct(_, l) | Value::Enum(_, Some(l)) => {
                    let l = l.borrow();
                    for i in 0..n {
                        if !set[i] {
                            vals[i] = self.copy_val(l[i].clone());
                        }
                    }
                }
                _ => return err(format!("{}:{}: functional update base is not a struct", e.line, e.col)),
            }
        }
        Ok(match s.target {
            Target::Struct(id) => Value::Struct(id, new_list(vals)),
            Target::Variant(v) => Value::Enum(v, Some(new_list(vals))),
            Target::Unresolved => unreachable!(),
        })
    }

    fn assign_to(&mut self, lhs: &Expr, v: Value) -> R<()> {
        match &lhs.kind {
            ExprKind::Tuple(es) | ExprKind::Array(es) => {
                let v = self.deref(v);
                let l = match &v {
                    Value::Tuple(l) | Value::Array(l) | Value::Vec(l) => l.clone(),
                    other => return err(format!("cannot destructure {}", other.type_name())),
                };
                let items = l.borrow().clone();
                let rest = es.iter().position(|x| matches!(x.kind, ExprKind::Range(None, None, false)));
                match rest {
                    None => {
                        for (x, val) in es.iter().zip(items) {
                            let val = self.copy_val(val);
                            self.assign_to(x, val)?;
                        }
                    }
                    Some(r) => {
                        let after = es.len() - r - 1;
                        for i in 0..r {
                            let val = self.copy_val(items[i].clone());
                            self.assign_to(&es[i], val)?;
                        }
                        for j in 0..after {
                            let val = self.copy_val(items[items.len() - after + j].clone());
                            self.assign_to(&es[r + 1 + j], val)?;
                        }
                    }
                }
                Ok(())
            }
            ExprKind::MakeStruct(_, es) => {
                let v = self.deref(v);
                if let Value::Struct(_, l) = &v {
                    let items = l.borrow().clone();
                    for (x, val) in es.iter().zip(items) {
                        self.assign_to(x, val)?;
                    }
                    Ok(())
                } else {
                    err("cannot destructure value")
                }
            }
            ExprKind::StructLit(s) => {
                let v = self.deref(v);
                if let Value::Struct(_, l) | Value::Enum(_, Some(l)) = &v {
                    let items = l.borrow().clone();
                    for (_, idx, x) in &s.fields {
                        self.assign_to(x, items[*idx as usize].clone())?;
                    }
                    Ok(())
                } else {
                    err("cannot destructure value")
                }
            }
            ExprKind::Lit(Value::Lazy) => Ok(()),
            _ => {
                let p = self.place(lhs)?;
                self.store(&p, v);
                Ok(())
            }
        }
    }

    #[inline(never)]
    fn eval_for(&mut self, b: &(Option<Sym>, Pat, Expr, Block)) -> R<Value> {
        let (label, pat, it, body) = b;
        // fast path: integer ranges
        if let ExprKind::Range(Some(lo), Some(hi), incl) = &it.kind {
            let lo = self.eval(lo)?;
            let hi = self.eval(hi)?;
            if let (Value::Int(a), Value::Int(b)) = (self.deref(lo.clone()), self.deref(hi.clone())) {
                let end = if *incl { b.saturating_add(1) } else { b };
                let simple_slot = match pat {
                    Pat::Bind { slot, by_ref: false, sub: None, .. } => Some(*slot as usize),
                    _ => None,
                };
                let mut i = a;
                while i < end {
                    match simple_slot {
                        Some(s) => self.stack[self.fp + s] = Value::Int(i),
                        None => {
                            self.match_pat(pat, Value::Int(i), None, false)?;
                        }
                    }
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Ctrl::Break) if label_ok(*label) => {
                            take_ctrl_val();
                            break;
                        }
                        Err(Ctrl::Continue) if label_ok(*label) => {}
                        Err(e) => return Err(e),
                    }
                    i += 1;
                }
                return Ok(Value::Unit);
            }
            let r = Value::Range(Rc::new(RangeVal { lo: Some(lo), hi: Some(hi), inclusive: *incl }));
            let st = self.make_iter(r, it)?;
            return self.run_for(st, label, pat, body);
        }
        let v = self.eval(it)?;
        let st = self.make_iter(v, it)?;
        self.run_for(st, label, pat, body)
    }

    fn run_for(&mut self, mut st: IterState, label: &Option<Sym>, pat: &Pat, body: &Block) -> R<Value> {
        let simple_slot = match pat {
            Pat::Bind { slot, by_ref: false, sub: None, .. } => Some(*slot as usize),
            _ => None,
        };
        while let Some(x) = self.iter_next(&mut st)? {
            match simple_slot {
                Some(s) => self.stack[self.fp + s] = x,
                None => {
                    if !self.match_pat(pat, x, None, false)? {
                        return err("refutable pattern in `for` loop");
                    }
                }
            }
            match self.eval_block(body) {
                Ok(_) => {}
                Err(Ctrl::Break) if label_ok(*label) => {
                            take_ctrl_val();
                            break;
                        }
                Err(Ctrl::Continue) if label_ok(*label) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(Value::Unit)
    }

    // ---------------------------------------------------------- operators
    #[inline(never)]
    fn eval_binary(&mut self, op: BinOp, a: &Expr, b: &Expr, e: &Expr) -> R<Value> {
        match op {
            BinOp::And => {
                let l = self.eval(a)?;
                if !self.truthy(l, a)? {
                    return Ok(Value::Bool(false));
                }
                let r = self.eval(b)?;
                return Ok(Value::Bool(self.truthy(r, b)?));
            }
            BinOp::Or => {
                let l = self.eval(a)?;
                if self.truthy(l, a)? {
                    return Ok(Value::Bool(true));
                }
                let r = self.eval(b)?;
                return Ok(Value::Bool(self.truthy(r, b)?));
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let l = self.eval_noc(a)?;
                let r = self.eval_noc(b)?;
                if let (Value::Int(x), Value::Int(y)) = (&l, &r) {
                    return Ok(Value::Bool(match op {
                        BinOp::Eq => x == y,
                        BinOp::Ne => x != y,
                        BinOp::Lt => x < y,
                        BinOp::Le => x <= y,
                        BinOp::Gt => x > y,
                        _ => x >= y,
                    }));
                }
                if let (Value::Float(x), Value::Float(y)) = (&l, &r) {
                    return Ok(Value::Bool(match op {
                        BinOp::Eq => x == y,
                        BinOp::Ne => x != y,
                        BinOp::Lt => x < y,
                        BinOp::Le => x <= y,
                        BinOp::Gt => x > y,
                        _ => x >= y,
                    }));
                }
                return self.compare_op(op, l, r);
            }
            _ => {}
        }
        let l = self.eval(a)?;
        let r = self.eval(b)?;
        match (&l, &r) {
            (Value::Int(x), Value::Int(y)) => self.int_op(op, *x, *y, e),
            (Value::Float(x), Value::Float(y)) => match op {
                BinOp::Add => Ok(Value::Float(x + y)),
                BinOp::Sub => Ok(Value::Float(x - y)),
                BinOp::Mul => Ok(Value::Float(x * y)),
                BinOp::Div => Ok(Value::Float(x / y)),
                _ => self.binary_slow(op, l, r, e),
            },
            _ => self.binary_slow(op, l, r, e),
        }
    }

    #[inline(never)]
    pub fn compare_op(&mut self, op: BinOp, l: Value, r: Value) -> R<Value> {
        let l = self.deref(l);
        let r = self.deref(r);
        Ok(Value::Bool(match op {
            BinOp::Eq => self.equals(&l, &r)?,
            BinOp::Ne => !self.equals(&l, &r)?,
            _ => {
                let c = self.compare(&l, &r)?;
                match (op, c) {
                    (_, None) => false,
                    (BinOp::Lt, Some(c)) => c == Ordering::Less,
                    (BinOp::Le, Some(c)) => c != Ordering::Greater,
                    (BinOp::Gt, Some(c)) => c == Ordering::Greater,
                    (_, Some(c)) => c != Ordering::Less,
                }
            }
        }))
    }

    #[inline]
    pub fn int_op(&self, op: BinOp, x: i64, y: i64, e: &Expr) -> R<Value> {
        let r = match op {
            BinOp::Add => x.checked_add(y),
            BinOp::Sub => x.checked_sub(y),
            BinOp::Mul => x.checked_mul(y),
            BinOp::Div => {
                if y == 0 {
                    return self.panic_e(e, "attempt to divide by zero");
                }
                x.checked_div(y)
            }
            BinOp::Rem => {
                if y == 0 {
                    return self.panic_e(e, "attempt to calculate the remainder with a divisor of zero");
                }
                x.checked_rem(y)
            }
            BinOp::BitAnd => Some(x & y),
            BinOp::BitOr => Some(x | y),
            BinOp::BitXor => Some(x ^ y),
            BinOp::Shl => {
                if !(0..64).contains(&y) {
                    return Err(self.overflow_err(op, e));
                }
                Some(x.wrapping_shl(y as u32))
            }
            BinOp::Shr => {
                if !(0..64).contains(&y) {
                    return Err(self.overflow_err(op, e));
                }
                Some(x >> y)
            }
            BinOp::Eq => return Ok(Value::Bool(x == y)),
            BinOp::Ne => return Ok(Value::Bool(x != y)),
            BinOp::Lt => return Ok(Value::Bool(x < y)),
            BinOp::Le => return Ok(Value::Bool(x <= y)),
            BinOp::Gt => return Ok(Value::Bool(x > y)),
            BinOp::Ge => return Ok(Value::Bool(x >= y)),
            BinOp::And | BinOp::Or => return err("logical operator on integers"),
        };
        match r {
            Some(v) => Ok(Value::Int(v)),
            None => Err(self.overflow_err(op, e)),
        }
    }

    fn float_op(&self, op: BinOp, x: f64, y: f64) -> R<Value> {
        Ok(match op {
            BinOp::Add => Value::Float(x + y),
            BinOp::Sub => Value::Float(x - y),
            BinOp::Mul => Value::Float(x * y),
            BinOp::Div => Value::Float(x / y),
            BinOp::Rem => Value::Float(x % y),
            _ => return err("unsupported float operator"),
        })
    }

    fn op_trait_method(op: BinOp) -> &'static str {
        match op {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Rem => "rem",
            BinOp::BitAnd => "bitand",
            BinOp::BitOr => "bitor",
            BinOp::BitXor => "bitxor",
            BinOp::Shl => "shl",
            BinOp::Shr => "shr",
            _ => "",
        }
    }

    #[inline(never)]
    pub fn binary_slow(&mut self, op: BinOp, l: Value, r: Value, e: &Expr) -> R<Value> {
        let l = self.deref(l);
        let r = self.deref(r);
        match (&l, &r) {
            (Value::Int(x), Value::Int(y)) => self.int_op(op, *x, *y, e),
            (Value::Float(x), Value::Float(y)) => self.float_op(op, *x, *y),
            (Value::Int(x), Value::Float(y)) => self.float_op(op, *x as f64, *y),
            (Value::Float(x), Value::Int(y)) => self.float_op(op, *x, *y as f64),
            (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(match op {
                BinOp::BitAnd => *x & *y,
                BinOp::BitOr => *x | *y,
                BinOp::BitXor => *x ^ *y,
                _ => return err("unsupported operator on bool"),
            })),
            (Value::Str(_) | Value::String(_), Value::Str(_) | Value::String(_) | Value::Char(_)) if op == BinOp::Add => {
                let mut s = crate::builtins::str_of(&l).to_string();
                match &r {
                    Value::Char(c) => s.push(*c),
                    _ => s.push_str(&crate::builtins::str_of(&r)),
                }
                Ok(Value::string(s))
            }
            (Value::Set(_), Value::Set(_)) => crate::builtins::set_op(op, &l, &r),
            (Value::Struct(..) | Value::Enum(..), _) => {
                let tk = self.type_key(&l);
                let name = Self::op_trait_method(op);
                match self.prog.find_method(tk, intern(name)) {
                    Some(fid) => self.call_user_method(fid, l, None, vec![r]),
                    None => err(format!("{}:{}: no `{}` implementation for `{}`", e.line, e.col, name, self.value_type_name(&l))),
                }
            }
            (Value::Lazy, _) => {
                let z = crate::builtins::zero_like(&r);
                self.binary_slow(op, z, r, e)
            }
            _ => err(format!("{}:{}: unsupported operands for {:?}: {} and {}", e.line, e.col, op, l.type_name(), r.type_name())),
        }
    }

    #[inline(never)]
    fn op_assign_slow(&mut self, op: BinOp, p: Place, cur: Value, rv: Value, e: &Expr) -> R<()> {
        let cur = if matches!(cur, Value::Lazy) { crate::builtins::zero_like(&self.deref(rv.clone())) } else { cur };
        match (&cur, op) {
            (Value::String(s), BinOp::Add) => {
                let r = self.deref(rv);
                match &r {
                    Value::Char(c) => s.borrow_mut().push(*c),
                    _ => {
                        let rs = crate::builtins::str_of(&r).to_string();
                        s.borrow_mut().push_str(&rs);
                    }
                }
                self.store(&p, cur);
                return Ok(());
            }
            (Value::Struct(..) | Value::Enum(..), _) => {
                let tk = self.type_key(&cur);
                let name = format!("{}_assign", Self::op_trait_method(op));
                if let Some(fid) = self.prog.find_method(tk, intern(&name)) {
                    self.call_user_method(fid, cur, Some(p), vec![rv])?;
                    return Ok(());
                }
            }
            _ => {}
        }
        let r = self.binary_slow(op, cur, rv, e)?;
        self.store(&p, r);
        Ok(())
    }

    fn unary(&mut self, op: UnOp, v: Value, e: &Expr) -> R<Value> {
        let v = self.deref(v);
        match (op, &v) {
            (UnOp::Neg, Value::Int(i)) => match i.checked_neg() {
                Some(x) => Ok(Value::Int(x)),
                None => self.panic_e(e, "attempt to negate with overflow"),
            },
            (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (UnOp::Not, Value::Int(i)) => Ok(Value::Int(!i)),
            (_, Value::Struct(..) | Value::Enum(..)) => {
                let tk = self.type_key(&v);
                let name = if op == UnOp::Neg { "neg" } else { "not" };
                match self.prog.find_method(tk, intern(name)) {
                    Some(fid) => self.call_user_method(fid, v, None, vec![]),
                    None => err(format!("{}:{}: no `{}` implementation", e.line, e.col, name)),
                }
            }
            _ => err(format!("{}:{}: unsupported unary operator on {}", e.line, e.col, v.type_name())),
        }
    }

    fn cast(&mut self, v: Value, ty: &Type, e: &Expr) -> R<Value> {
        let v = self.deref(v);
        let name = ty.name.as_str();
        let as_i = |v: &Value| -> Option<i64> {
            match v {
                Value::Int(i) => Some(*i),
                Value::Bool(b) => Some(*b as i64),
                Value::Char(c) => Some(*c as i64),
                _ => None,
            }
        };
        match name {
            "f64" | "f32" => {
                let f = match &v {
                    Value::Float(f) => *f,
                    other => match as_i(other) {
                        Some(i) => i as f64,
                        None => return err(format!("{}:{}: cannot cast {} to {}", e.line, e.col, other.type_name(), name)),
                    },
                };
                Ok(Value::Float(if name == "f32" { f as f32 as f64 } else { f }))
            }
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => {
                let i = match &v {
                    Value::Float(f) => {
                        return Ok(Value::Int(match name {
                            "i8" => *f as i8 as i64,
                            "i16" => *f as i16 as i64,
                            "i32" => *f as i32 as i64,
                            "u8" => *f as u8 as i64,
                            "u16" => *f as u16 as i64,
                            "u32" => *f as u32 as i64,
                            "u64" | "usize" | "u128" => {
                                if *f <= 0.0 {
                                    0
                                } else {
                                    *f as i64
                                }
                            }
                            _ => *f as i64,
                        }))
                    }
                    Value::Enum(vid, None) => self.prog.variants[*vid as usize].disc,
                    other => match as_i(other) {
                        Some(i) => i,
                        None => return err(format!("{}:{}: cannot cast {} to {}", e.line, e.col, other.type_name(), name)),
                    },
                };
                Ok(Value::Int(wrap_int(i, name)))
            }
            "char" => match &v {
                Value::Int(i) => Ok(Value::Char(char::from_u32(*i as u32 & 0xFF).unwrap_or('\0'))),
                Value::Char(_) => Ok(v),
                other => err(format!("{}:{}: cannot cast {} to char", e.line, e.col, other.type_name())),
            },
            "bool" => Ok(v),
            _ => Ok(v),
        }
    }

    // --------------------------------------------------- equality / ordering
    pub fn equals(&mut self, a: &Value, b: &Value) -> R<bool> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(x == y),
            (Value::Struct(..) | Value::Enum(..), _) => {
                let tk = self.type_key(a);
                if let Some(fid) = self.prog.user_eq.get(&tk).copied() {
                    let r = self.call_user_method(fid, a.clone(), None, vec![Value::RefBox(Self::new_cell(b.clone()))])?;
                    return Ok(matches!(self.deref(r), Value::Bool(true)));
                }
                match (a, b) {
                    (Value::Struct(d1, x), Value::Struct(d2, y)) => {
                        if d1 != d2 {
                            return Ok(false);
                        }
                        self.list_equals(x, y)
                    }
                    (Value::Enum(v1, x), Value::Enum(v2, y)) => {
                        if v1 != v2 {
                            return Ok(false);
                        }
                        match (x, y) {
                            (Some(x), Some(y)) => self.list_equals(x, y),
                            (None, None) => Ok(true),
                            _ => Ok(false),
                        }
                    }
                    _ => Ok(false),
                }
            }
            (Value::Tuple(x), Value::Tuple(y)) | (Value::Vec(x) | Value::Array(x), Value::Vec(y) | Value::Array(y)) => {
                self.list_equals(x, y)
            }
            (a, b) if a.is_ref() || b.is_ref() => {
                let a = self.deref(a.clone());
                let b = self.deref(b.clone());
                self.equals(&a, &b)
            }
            _ => Ok(struct_eq(a, b)),
        }
    }

    fn list_equals(&mut self, x: &List, y: &List) -> R<bool> {
        if Rc::ptr_eq(x, y) {
            return Ok(true);
        }
        let (xs, ys) = (x.borrow().clone(), y.borrow().clone());
        if xs.len() != ys.len() {
            return Ok(false);
        }
        for (a, b) in xs.iter().zip(ys.iter()) {
            if !self.equals(a, b)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn ordering_value(o: Ordering) -> Value {
        Value::Enum(
            match o {
                Ordering::Less => V_LESS,
                Ordering::Equal => V_EQUAL,
                Ordering::Greater => V_GREATER,
            },
            None,
        )
    }

    pub fn value_to_ordering(&self, v: &Value) -> Option<Ordering> {
        match self.deref(v.clone()) {
            Value::Enum(V_LESS, _) => Some(Ordering::Less),
            Value::Enum(V_EQUAL, _) => Some(Ordering::Equal),
            Value::Enum(V_GREATER, _) => Some(Ordering::Greater),
            Value::Enum(V_SOME, Some(l)) => self.value_to_ordering(&l.borrow()[0]),
            _ => None,
        }
    }

    pub fn compare(&mut self, a: &Value, b: &Value) -> R<Option<Ordering>> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Some(x.cmp(y))),
            (Value::Float(x), Value::Float(y)) => Ok(x.partial_cmp(y)),
            (Value::Struct(S_REVERSE, x), Value::Struct(S_REVERSE, y)) => {
                let (x, y) = (x.borrow()[0].clone(), y.borrow()[0].clone());
                Ok(self.compare(&y, &x)?)
            }
            (Value::Struct(..) | Value::Enum(..), _) => {
                let tk = self.type_key(a);
                let fid = self.prog.user_cmp.get(&tk).or_else(|| self.prog.user_partial_cmp.get(&tk)).copied();
                if let Some(fid) = fid {
                    let r = self.call_user_method(fid, a.clone(), None, vec![Value::RefBox(Self::new_cell(b.clone()))])?;
                    return Ok(self.value_to_ordering(&r));
                }
                match (a, b) {
                    (Value::Struct(_, x), Value::Struct(_, y)) => self.list_compare(x, y),
                    (Value::Enum(v1, x), Value::Enum(v2, y)) => {
                        if v1 != v2 {
                            return Ok(Some(v1.cmp(v2)));
                        }
                        match (x, y) {
                            (Some(x), Some(y)) => self.list_compare(x, y),
                            _ => Ok(Some(Ordering::Equal)),
                        }
                    }
                    _ => Ok(None),
                }
            }
            (Value::Tuple(x), Value::Tuple(y)) | (Value::Vec(x) | Value::Array(x), Value::Vec(y) | Value::Array(y)) => {
                self.list_compare(x, y)
            }
            (a, b) if a.is_ref() || b.is_ref() || matches!(a, Value::RcBox(_)) => {
                let a = self.deref(a.clone());
                let b = self.deref(b.clone());
                self.compare(&a, &b)
            }
            _ => Ok(struct_cmp(a, b)),
        }
    }

    fn list_compare(&mut self, x: &List, y: &List) -> R<Option<Ordering>> {
        let (xs, ys) = (x.borrow().clone(), y.borrow().clone());
        for (a, b) in xs.iter().zip(ys.iter()) {
            match self.compare(a, b)? {
                Some(Ordering::Equal) => {}
                other => return Ok(other),
            }
        }
        Ok(Some(xs.len().cmp(&ys.len())))
    }

    /// Total order used by sorting (panics are avoided: incomparable = Equal).
    pub fn cmp_total(&mut self, a: &Value, b: &Value) -> R<Ordering> {
        Ok(self.compare(a, b)?.unwrap_or(Ordering::Equal))
    }

    // ----------------------------------------------------------- formatting
    pub fn value_type_name(&self, v: &Value) -> String {
        match v {
            Value::Struct(d, _) => self.prog.structs[*d as usize].name.clone(),
            Value::Enum(vid, _) => self.prog.enums[self.prog.variants[*vid as usize].enum_id as usize].name.clone(),
            other => other.type_name().to_string(),
        }
    }

    pub fn format_value(&mut self, v: &Value, spec: &RSpec, out: &mut String) -> R<()> {
        match spec.ty {
            FmtTy::Debug => self.debug_value(v, spec, out, 0),
            _ => self.display_value(v, spec, out),
        }
    }

    pub fn display_value(&mut self, v: &Value, spec: &RSpec, out: &mut String) -> R<()> {
        match v {
            Value::Int(i) => out.push_str(&format::fmt_int(*i, spec)),
            Value::Float(f) => out.push_str(&format::fmt_float(*f, spec)),
            Value::Str(s) => out.push_str(&format::fmt_str(s, spec)),
            Value::String(s) => {
                let s = s.borrow();
                out.push_str(&format::fmt_str(&s, spec))
            }
            Value::Bool(b) => out.push_str(&format::pad(if *b { "true" } else { "false" }, spec, false)),
            Value::Char(c) => out.push_str(&format::pad(&c.to_string(), spec, false)),
            Value::RefSlot(_) | Value::RefElem(..) | Value::RefBox(_) | Value::RcBox(_) => {
                let d = self.deref(v.clone());
                self.display_value(&d, spec, out)?
            }
            Value::Struct(..) | Value::Enum(..) => {
                let tk = self.type_key(v);
                match self.prog.find_method(tk, self.syms.display_fmt) {
                    Some(fid) => self.call_fmt(fid, v, spec, out)?,
                    None => {
                        if let Value::Enum(V_LESS..=V_GREATER, _) = v {
                            return self.debug_value(v, spec, out, 0);
                        }
                        return err(format!("`{}` doesn't implement `std::fmt::Display`", self.value_type_name(v)));
                    }
                }
            }
            Value::Lazy => out.push_str(&format::pad("0", spec, true)),
            Value::Instant(f) => out.push_str(&format!("{:?}", std::time::Duration::from_secs_f64(*f))),
            _ => self.debug_value(v, spec, out, 0)?,
        }
        Ok(())
    }

    fn call_fmt(&mut self, fid: u32, v: &Value, spec: &RSpec, out: &mut String) -> R<()> {
        let mut sp = format::Spec::default();
        sp.fill = spec.fill;
        sp.align = spec.align;
        sp.width = spec.width.map(format::Count::Fixed);
        sp.precision = spec.precision.map(format::Count::Fixed);
        sp.alt = spec.alt;
        sp.plus = spec.plus;
        sp.zero = spec.zero;
        let target = Rc::new(FmtTarget { buf: RefCell::new(String::new()), spec: sp });
        let r = self.call_user_method(fid, v.clone(), None, vec![Value::Formatter(target.clone())])?;
        if let Value::Enum(V_ERR, _) = self.deref(r) {
            return self.panic_at(0, 0, "a formatting trait implementation returned an error");
        }
        out.push_str(&target.buf.borrow());
        Ok(())
    }

    pub fn debug_value(&mut self, v: &Value, spec: &RSpec, out: &mut String, indent: usize) -> R<()> {
        let pretty = spec.alt;
        match v {
            Value::Int(i) => out.push_str(&format::fmt_int(*i, &RSpec { ty: FmtTy::Display, ..spec.clone() })),
            Value::Float(f) => out.push_str(&format::fmt_float(*f, spec)),
            Value::Str(s) => out.push_str(&format::fmt_str(s, spec)),
            Value::String(s) => {
                let s = s.borrow();
                out.push_str(&format::fmt_str(&s, spec))
            }
            Value::Bool(b) => out.push_str(&format::pad(if *b { "true" } else { "false" }, spec, false)),
            Value::Char(c) => out.push_str(&format::pad(&format!("{:?}", c), spec, false)),
            Value::Unit => out.push_str(&format::pad("()", spec, false)),
            Value::RefSlot(_) | Value::RefElem(..) | Value::RefBox(_) | Value::RcBox(_) => {
                let d = self.deref(v.clone());
                self.debug_value(&d, spec, out, indent)?
            }
            Value::CellBox(c) => {
                let inner = c.borrow().clone();
                self.debug_struct_like("RefCell", &[("value".into(), inner)], true, spec, out, indent)?
            }
            Value::Tuple(l) => {
                let items = l.borrow().clone();
                self.debug_seq("(", ")", &items, spec, out, indent, items.len() == 1)?
            }
            Value::Vec(l) | Value::Array(l) | Value::Heap(l) => {
                let items = l.borrow().clone();
                self.debug_seq("[", "]", &items, spec, out, indent, false)?
            }
            Value::Deque(d) => {
                let items: Vec<Value> = d.borrow().iter().cloned().collect();
                self.debug_seq("[", "]", &items, spec, out, indent, false)?
            }
            Value::Set(s) => {
                let items = s.borrow().items();
                self.debug_seq("{", "}", &items, spec, out, indent, false)?
            }
            Value::Map(m) => {
                let entries = m.borrow().entries();
                if entries.is_empty() {
                    out.push_str("{}");
                    return Ok(());
                }
                out.push('{');
                for (i, (k, val)) in entries.iter().enumerate() {
                    if pretty {
                        out.push('\n');
                        push_indent(out, indent + 1);
                    } else if i > 0 {
                        out.push_str(", ");
                    }
                    self.debug_value(k, spec, out, indent + 1)?;
                    out.push_str(": ");
                    let val = val.borrow().clone();
                    self.debug_value(&val, spec, out, indent + 1)?;
                    if pretty {
                        out.push(',');
                    }
                }
                if pretty {
                    out.push('\n');
                    push_indent(out, indent);
                }
                out.push('}');
            }
            Value::Struct(d, l) => {
                let tk = *d;
                if let Some(fid) = self.prog.find_method(tk, self.syms.debug_fmt) {
                    return self.call_fmt(fid, v, spec, out);
                }
                let sd = &self.prog.structs[*d as usize];
                let name = sd.name.clone();
                let kind = sd.kind;
                let fields: Vec<String> = sd.fields.iter().map(|s| sym_str(*s)).collect();
                let items = l.borrow().clone();
                match kind {
                    StructKind::Unit => out.push_str(&name),
                    StructKind::Tuple => {
                        out.push_str(&name);
                        self.debug_seq("(", ")", &items, spec, out, indent, false)?;
                    }
                    StructKind::Named => {
                        let pairs: Vec<(String, Value)> = fields.into_iter().zip(items).collect();
                        self.debug_struct_like(&name, &pairs, true, spec, out, indent)?;
                    }
                }
            }
            Value::Enum(vid, payload) => {
                let tk = self.type_key(v);
                if let Some(fid) = self.prog.find_method(tk, self.syms.debug_fmt) {
                    return self.call_fmt(fid, v, spec, out);
                }
                let vd = &self.prog.variants[*vid as usize];
                let name = vd.name.clone();
                let kind = vd.kind;
                let fields: Vec<String> = vd.fields.iter().map(|s| sym_str(*s)).collect();
                let items = payload.as_ref().map(|l| l.borrow().clone()).unwrap_or_default();
                match kind {
                    StructKind::Unit => out.push_str(&name),
                    StructKind::Tuple => {
                        out.push_str(&name);
                        self.debug_seq("(", ")", &items, spec, out, indent, false)?;
                    }
                    StructKind::Named => {
                        let pairs: Vec<(String, Value)> = fields.into_iter().zip(items).collect();
                        self.debug_struct_like(&name, &pairs, true, spec, out, indent)?;
                    }
                }
            }
            Value::Range(r) => {
                if let Some(lo) = &r.lo {
                    self.debug_value(lo, spec, out, indent)?;
                }
                out.push_str(if r.inclusive { "..=" } else { ".." });
                if let Some(hi) = &r.hi {
                    self.debug_value(hi, spec, out, indent)?;
                }
            }
            Value::Lazy => out.push('0'),
            Value::Instant(f) => out.push_str(&format!("{:?}", std::time::Duration::from_secs_f64(*f))),
            other => match crate::builtins::slice_view(other) {
                Some((l, a, b)) => {
                    let items = l.borrow()[a..b].to_vec();
                    self.debug_seq("[", "]", &items, spec, out, indent, false)?
                }
                None => out.push_str(&format!("<{}>", other.type_name())),
            },
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn debug_seq(
        &mut self,
        open: &str,
        close: &str,
        items: &[Value],
        spec: &RSpec,
        out: &mut String,
        indent: usize,
        trailing_comma: bool,
    ) -> R<()> {
        out.push_str(open);
        if items.is_empty() {
            out.push_str(close);
            return Ok(());
        }
        let pretty = spec.alt;
        for (i, x) in items.iter().enumerate() {
            if pretty {
                out.push('\n');
                push_indent(out, indent + 1);
            } else if i > 0 {
                out.push_str(", ");
            }
            self.debug_value(x, spec, out, indent + 1)?;
            if pretty {
                out.push(',');
            }
        }
        if pretty {
            out.push('\n');
            push_indent(out, indent);
        } else if trailing_comma {
            out.push(',');
        }
        out.push_str(close);
        Ok(())
    }

    fn debug_struct_like(
        &mut self,
        name: &str,
        fields: &[(String, Value)],
        _named: bool,
        spec: &RSpec,
        out: &mut String,
        indent: usize,
    ) -> R<()> {
        out.push_str(name);
        if fields.is_empty() {
            return Ok(());
        }
        let pretty = spec.alt;
        out.push_str(" {");
        for (i, (f, x)) in fields.iter().enumerate() {
            if pretty {
                out.push('\n');
                push_indent(out, indent + 1);
            } else {
                out.push_str(if i > 0 { ", " } else { " " });
            }
            out.push_str(f);
            out.push_str(": ");
            self.debug_value(x, spec, out, indent + 1)?;
            if pretty {
                out.push(',');
            }
        }
        if pretty {
            out.push('\n');
            push_indent(out, indent);
            out.push('}');
        } else {
            out.push_str(" }");
        }
        Ok(())
    }

    pub fn format_pieces(&mut self, pieces: &[FmtPiece], args: &[Expr]) -> R<String> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval_noc(a)?);
        }
        let mut s = String::new();
        for p in pieces {
            match p {
                FmtPiece::Lit(l) => s.push_str(l),
                FmtPiece::Arg { arg, spec } => {
                    let count = |c: &Option<format::Count>, me: &Self| -> R<Option<usize>> {
                        Ok(match c {
                            None => None,
                            Some(format::Count::Fixed(n)) => Some(*n),
                            Some(format::Count::Arg(i)) => Some(me.as_int(&vals[*i])? as usize),
                        })
                    };
                    let rs = RSpec {
                        fill: spec.fill,
                        align: spec.align,
                        plus: spec.plus,
                        alt: spec.alt,
                        zero: spec.zero,
                        width: count(&spec.width, self)?,
                        precision: count(&spec.precision, self)?,
                        ty: spec.ty,
                    };
                    let v = vals[*arg].clone();
                    self.format_value(&v, &rs, &mut s)?;
                }
            }
        }
        Ok(s)
    }

    #[inline(never)]
    fn eval_macro(&mut self, m: &MacroCall, e: &Expr) -> R<Value> {
        let text = |me: &mut Self| -> R<String> {
            match &m.pieces {
                Some(p) => me.format_pieces(p, &m.args),
                None => Ok(String::new()),
            }
        };
        match m.kind {
            MacroKind::Println | MacroKind::Print => {
                let mut s = text(self)?;
                if m.kind == MacroKind::Println {
                    s.push('\n');
                }
                if self.out.write_all(s.as_bytes()).is_err() {
                    return Err(ctrl_exit(0));
                }
                Ok(Value::Unit)
            }
            MacroKind::Eprintln | MacroKind::Eprint => {
                let mut s = text(self)?;
                if m.kind == MacroKind::Eprintln {
                    s.push('\n');
                }
                let _ = self.out.flush();
                eprint!("{}", s);
                Ok(Value::Unit)
            }
            MacroKind::Format => Ok(Value::string(text(self)?)),
            MacroKind::Panic => {
                let msg = if m.pieces.is_some() { text(self)? } else { "explicit panic".to_string() };
                self.panic_e(e, msg)
            }
            MacroKind::Unreachable => {
                let msg = if m.pieces.is_some() {
                    format!("internal error: entered unreachable code: {}", text(self)?)
                } else {
                    "internal error: entered unreachable code".to_string()
                };
                self.panic_e(e, msg)
            }
            MacroKind::Todo => {
                let msg = if m.pieces.is_some() { format!("not yet implemented: {}", text(self)?) } else { "not yet implemented".into() };
                self.panic_e(e, msg)
            }
            MacroKind::Unimplemented => {
                let msg = if m.pieces.is_some() { format!("not implemented: {}", text(self)?) } else { "not implemented".into() };
                self.panic_e(e, msg)
            }
            MacroKind::Write | MacroKind::Writeln => {
                let target = self.eval_noc(&m.operands[0])?;
                let mut s = text(self)?;
                if m.kind == MacroKind::Writeln {
                    s.push('\n');
                }
                let target = self.deref(target);
                match &target {
                    Value::Formatter(f) => f.buf.borrow_mut().push_str(&s),
                    Value::String(st) => st.borrow_mut().push_str(&s),
                    Value::Stdin | Value::Unit => {
                        let _ = self.out.write_all(s.as_bytes());
                    }
                    Value::Vec(l) => l.borrow_mut().extend(s.bytes().map(|b| Value::Int(b as i64))),
                    other => return err(format!("{}:{}: cannot write! to {}", e.line, e.col, other.type_name())),
                }
                Ok(Value::ok(Value::Unit))
            }
            MacroKind::Assert => {
                let c = self.eval(&m.operands[0])?;
                if self.truthy(c, &m.operands[0])? {
                    return Ok(Value::Unit);
                }
                let msg = if m.pieces.is_some() { text(self)? } else { format!("assertion failed: {}", m.text) };
                self.panic_e(e, msg)
            }
            MacroKind::AssertEq | MacroKind::AssertNe => {
                let a = self.eval_noc(&m.operands[0])?;
                let b = self.eval_noc(&m.operands[1])?;
                let (a, b) = (self.deref(a), self.deref(b));
                let eq = self.equals(&a, &b)?;
                if eq == (m.kind == MacroKind::AssertEq) {
                    return Ok(Value::Unit);
                }
                let mut l = String::new();
                let mut r = String::new();
                self.debug_value(&a, &RSpec::plain(FmtTy::Debug), &mut l, 0)?;
                self.debug_value(&b, &RSpec::plain(FmtTy::Debug), &mut r, 0)?;
                let op = if m.kind == MacroKind::AssertEq { "==" } else { "!=" };
                let mut msg = format!("assertion `left {} right` failed", op);
                if m.pieces.is_some() {
                    msg.push_str(": ");
                    msg.push_str(&text(self)?);
                }
                msg.push_str(&format!("\n  left: {}\n right: {}", l, r));
                self.panic_e(e, msg)
            }
            MacroKind::Dbg => {
                let mut last = Value::Unit;
                for op in &m.operands {
                    let v = self.eval(op)?;
                    let mut s = String::new();
                    self.debug_value(&v, &RSpec { alt: true, ..RSpec::plain(FmtTy::Debug) }, &mut s, 0)?;
                    let _ = self.out.flush();
                    eprintln!("[{}:{}:{}] {} = {}", self.file, e.line, e.col, m.text, s);
                    last = v;
                }
                Ok(last)
            }
        }
    }
}

fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("    ");
    }
}

pub fn wrap_int(i: i64, name: &str) -> i64 {
    match name {
        "i8" => i as i8 as i64,
        "i16" => i as i16 as i64,
        "i32" => i as i32 as i64,
        "u8" => i as u8 as i64,
        "u16" => i as u16 as i64,
        "u32" => i as u32 as i64,
        _ => i,
    }
}
