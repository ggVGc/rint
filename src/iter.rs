//! Lazy iterators and iterator adapters/consumers.

use crate::ast::{Expr, Type};
use crate::builtins::M;
use crate::interp::{err, Ctrl, Interp, R};
use crate::program::*;
use crate::value::*;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::rc::Rc;

pub enum IterState {
    Empty,
    /// Elements of a list; `refs` yields references to the elements.
    List { list: List, i: usize, end: usize, refs: bool },
    Range { cur: i64, end: i64 },
    RangeFrom(i64),
    CharRange { cur: u32, end: u32 },
    Vals(VecDeque<Value>),
    Shared(Rc<RefCell<IterState>>),
    Map(Box<IterState>, Value),
    Filter(Box<IterState>, Value),
    FilterMap(Box<IterState>, Value),
    Enumerate(Box<IterState>, i64),
    Zip(Box<IterState>, Box<IterState>),
    Chain(Box<IterState>, Box<IterState>),
    Take(Box<IterState>, usize),
    Skip(Box<IterState>, usize),
    StepBy(Box<IterState>, usize, bool),
    TakeWhile(Box<IterState>, Value, bool),
    SkipWhile(Box<IterState>, Value, bool),
    MapWhile(Box<IterState>, Value, bool),
    FlatMap(Box<IterState>, Option<Value>, Option<Box<IterState>>),
    Peekable(Box<IterState>, Option<Option<Value>>),
    Inspect(Box<IterState>, Value),
    Scan(Box<IterState>, VCell, Value),
    Cloned(Box<IterState>),
    Rev(Box<IterState>),
    Cycle(Vec<Value>, usize, Box<IterState>, bool),
    User(Value),
    FromFn(Value),
    Successors(Option<Value>, Value),
    Repeat(Value),
    Fuse(Box<IterState>, bool),
}

impl IterState {
    pub fn try_clone(&self) -> Option<IterState> {
        Some(match self {
            IterState::Empty => IterState::Empty,
            IterState::List { list, i, end, refs } => IterState::List { list: list.clone(), i: *i, end: *end, refs: *refs },
            IterState::Range { cur, end } => IterState::Range { cur: *cur, end: *end },
            IterState::RangeFrom(c) => IterState::RangeFrom(*c),
            IterState::CharRange { cur, end } => IterState::CharRange { cur: *cur, end: *end },
            IterState::Vals(v) => IterState::Vals(v.clone()),
            IterState::Map(i, f) => IterState::Map(Box::new(i.try_clone()?), f.clone()),
            IterState::Filter(i, f) => IterState::Filter(Box::new(i.try_clone()?), f.clone()),
            IterState::Enumerate(i, n) => IterState::Enumerate(Box::new(i.try_clone()?), *n),
            IterState::Take(i, n) => IterState::Take(Box::new(i.try_clone()?), *n),
            IterState::Skip(i, n) => IterState::Skip(Box::new(i.try_clone()?), *n),
            IterState::Rev(i) => IterState::Rev(Box::new(i.try_clone()?)),
            IterState::Cloned(i) => IterState::Cloned(Box::new(i.try_clone()?)),
            IterState::Chain(a, b) => IterState::Chain(Box::new(a.try_clone()?), Box::new(b.try_clone()?)),
            IterState::Zip(a, b) => IterState::Zip(Box::new(a.try_clone()?), Box::new(b.try_clone()?)),
            IterState::StepBy(i, s, f) => IterState::StepBy(Box::new(i.try_clone()?), *s, *f),
            IterState::Peekable(i, p) => IterState::Peekable(Box::new(i.try_clone()?), p.clone()),
            IterState::Repeat(v) => IterState::Repeat(v.clone()),
            IterState::Shared(s) => s.borrow().try_clone()?,
            _ => return None,
        })
    }

    /// Exact remaining length if cheaply known.
    pub fn len_hint(&self) -> Option<usize> {
        match self {
            IterState::Empty => Some(0),
            IterState::List { i, end, .. } => Some(end - i),
            IterState::Range { cur, end } => Some((end - cur).max(0) as usize),
            IterState::CharRange { cur, end } => Some(end.saturating_sub(*cur) as usize),
            IterState::Vals(v) => Some(v.len()),
            IterState::Map(i, _) | IterState::Cloned(i) | IterState::Rev(i) | IterState::Inspect(i, _) => i.len_hint(),
            IterState::Enumerate(i, _) => i.len_hint(),
            IterState::Take(i, n) => i.len_hint().map(|l| l.min(*n)),
            IterState::Skip(i, n) => i.len_hint().map(|l| l.saturating_sub(*n)),
            IterState::Zip(a, b) => Some(a.len_hint()?.min(b.len_hint()?)),
            IterState::Chain(a, b) => Some(a.len_hint()? + b.len_hint()?),
            IterState::Shared(s) => s.borrow().len_hint(),
            _ => None,
        }
    }
}

fn boxed(st: IterState) -> Box<IterState> {
    Box::new(st)
}

impl Interp {
    /// `IntoIterator::into_iter` for a value.
    pub fn make_iter(&mut self, v: Value, e: &Expr) -> R<IterState> {
        let by_ref = v.is_ref();
        let v = if by_ref { self.deref(v) } else { v };
        Ok(match v {
            Value::Iter(rc) => IterState::Shared(rc),
            Value::Vec(l) | Value::Array(l) => {
                let end = l.borrow().len();
                IterState::List { list: l, i: 0, end, refs: by_ref }
            }
            Value::Slice(s) => IterState::List { list: s.list.clone(), i: s.start, end: s.end, refs: true },
            Value::Range(r) => self.range_iter(&r, e)?,
            Value::Map(m) => {
                let entries = m.borrow().entries();
                IterState::Vals(
                    entries
                        .into_iter()
                        .map(|(k, c)| {
                            let val = if by_ref { Value::RefBox(c) } else { c.borrow().clone() };
                            Value::tuple(vec![k, val])
                        })
                        .collect(),
                )
            }
            Value::Set(s) => IterState::Vals(s.borrow().items().into()),
            Value::Deque(d) => IterState::Vals(d.borrow().iter().cloned().collect()),
            Value::Heap(l) => IterState::Vals(l.borrow().iter().cloned().collect()),
            Value::Enum(V_SOME, Some(l)) => {
                let x = l.borrow()[0].clone();
                IterState::Vals(VecDeque::from(vec![x]))
            }
            Value::Enum(V_NONE, _) => IterState::Empty,
            v @ (Value::Struct(..) | Value::Enum(..)) => {
                let tk = self.type_key(&v);
                if let Some(fid) = self.prog.find_method(tk, self.syms().into_iter) {
                    let r = self.call_user_method(fid, v, None, vec![])?;
                    return self.make_iter(r, e);
                }
                if self.prog.find_method(tk, self.syms().next).is_some() {
                    IterState::User(Value::RefBox(Interp::new_cell(v)))
                } else {
                    return err(format!("{}:{}: `{}` is not an iterator", e.line, e.col, self.value_type_name(&v)));
                }
            }
            other => return err(format!("{}:{}: {} is not iterable", e.line, e.col, other.type_name())),
        })
    }

    pub fn range_iter(&self, r: &RangeVal, e: &Expr) -> R<IterState> {
        let lo = r.lo.as_ref().map(|v| self.deref(v.clone()));
        let hi = r.hi.as_ref().map(|v| self.deref(v.clone()));
        Ok(match (lo, hi) {
            (Some(Value::Int(a)), Some(Value::Int(b))) => IterState::Range { cur: a, end: if r.inclusive { b + 1 } else { b } },
            (Some(Value::Int(a)), None) => IterState::RangeFrom(a),
            (Some(Value::Char(a)), Some(Value::Char(b))) => {
                IterState::CharRange { cur: a as u32, end: if r.inclusive { b as u32 + 1 } else { b as u32 } }
            }
            _ => return err(format!("{}:{}: cannot iterate over this range", e.line, e.col)),
        })
    }

    pub fn iter_next(&mut self, st: &mut IterState) -> R<Option<Value>> {
        Ok(match st {
            IterState::Empty => None,
            IterState::List { list, i, end, refs } => {
                if *i >= *end {
                    return Ok(None);
                }
                let idx = *i;
                *i += 1;
                if *refs {
                    Some(Value::RefElem(list.clone(), idx as u32))
                } else {
                    let v = list.borrow().get(idx).cloned();
                    v.map(|v| self.copy_val(v))
                }
            }
            IterState::Range { cur, end } => {
                if *cur >= *end {
                    None
                } else {
                    *cur += 1;
                    Some(Value::Int(*cur - 1))
                }
            }
            IterState::RangeFrom(c) => {
                *c += 1;
                Some(Value::Int(*c - 1))
            }
            IterState::CharRange { cur, end } => {
                while *cur < *end {
                    let c = *cur;
                    *cur += 1;
                    if let Some(ch) = char::from_u32(c) {
                        return Ok(Some(Value::Char(ch)));
                    }
                }
                None
            }
            IterState::Vals(v) => v.pop_front(),
            IterState::Shared(rc) => {
                let rc = rc.clone();
                let mut b = rc.borrow_mut();
                self.iter_next(&mut b)?
            }
            IterState::Map(inner, f) => match self.iter_next(inner)? {
                Some(x) => Some(self.call_value(f, vec![x])?),
                None => None,
            },
            IterState::Filter(inner, f) => loop {
                match self.iter_next(inner)? {
                    Some(x) => {
                        let arg = match &x {
                            v if v.is_ref() => v.clone(),
                            v => Value::RefBox(Interp::new_cell(v.clone())),
                        };
                        let keep = self.call_value(f, vec![arg])?;
                        if matches!(self.deref(keep), Value::Bool(true)) {
                            break Some(x);
                        }
                    }
                    None => break None,
                }
            },
            IterState::FilterMap(inner, f) => loop {
                match self.iter_next(inner)? {
                    Some(x) => match self.call_value(f, vec![x])? {
                        Value::Enum(V_SOME, Some(l)) => break Some(l.borrow()[0].clone()),
                        _ => {}
                    },
                    None => break None,
                }
            },
            IterState::Enumerate(inner, n) => match self.iter_next(inner)? {
                Some(x) => {
                    *n += 1;
                    Some(Value::tuple(vec![Value::Int(*n - 1), x]))
                }
                None => None,
            },
            IterState::Zip(a, b) => match self.iter_next(a)? {
                Some(x) => self.iter_next(b)?.map(|y| Value::tuple(vec![x, y])),
                None => None,
            },
            IterState::Chain(a, b) => match self.iter_next(a)? {
                Some(x) => Some(x),
                None => self.iter_next(b)?,
            },
            IterState::Take(inner, n) => {
                if *n == 0 {
                    None
                } else {
                    *n -= 1;
                    self.iter_next(inner)?
                }
            }
            IterState::Skip(inner, n) => {
                while *n > 0 {
                    *n -= 1;
                    if self.iter_next(inner)?.is_none() {
                        return Ok(None);
                    }
                }
                self.iter_next(inner)?
            }
            IterState::StepBy(inner, step, first) => {
                if *first {
                    *first = false;
                    self.iter_next(inner)?
                } else {
                    for _ in 0..*step - 1 {
                        if self.iter_next(inner)?.is_none() {
                            return Ok(None);
                        }
                    }
                    self.iter_next(inner)?
                }
            }
            IterState::TakeWhile(inner, f, done) => {
                if *done {
                    return Ok(None);
                }
                match self.iter_next(inner)? {
                    Some(x) => {
                        let arg = Value::RefBox(Interp::new_cell(x.clone()));
                        let ok = self.call_value(f, vec![arg])?;
                        if matches!(self.deref(ok), Value::Bool(true)) {
                            Some(x)
                        } else {
                            *done = true;
                            None
                        }
                    }
                    None => None,
                }
            }
            IterState::SkipWhile(inner, f, done) => loop {
                match self.iter_next(inner)? {
                    Some(x) => {
                        if *done {
                            break Some(x);
                        }
                        let arg = Value::RefBox(Interp::new_cell(x.clone()));
                        let skip = self.call_value(f, vec![arg])?;
                        if !matches!(self.deref(skip), Value::Bool(true)) {
                            *done = true;
                            break Some(x);
                        }
                    }
                    None => break None,
                }
            },
            IterState::MapWhile(inner, f, done) => {
                if *done {
                    return Ok(None);
                }
                match self.iter_next(inner)? {
                    Some(x) => match self.call_value(f, vec![x])? {
                        Value::Enum(V_SOME, Some(l)) => Some(l.borrow()[0].clone()),
                        _ => {
                            *done = true;
                            None
                        }
                    },
                    None => None,
                }
            }
            IterState::FlatMap(inner, f, cur) => loop {
                if let Some(c) = cur {
                    if let Some(x) = self.iter_next(c)? {
                        break Some(x);
                    }
                    *cur = None;
                }
                match self.iter_next(inner)? {
                    Some(x) => {
                        let y = match f {
                            Some(f) => self.call_value(f, vec![x])?,
                            None => x,
                        };
                        let dummy = Expr::new(crate::ast::ExprKind::Lit(Value::Unit), 0, 0);
                        *cur = Some(boxed(self.make_iter(y, &dummy)?));
                    }
                    None => break None,
                }
            },
            IterState::Peekable(inner, peeked) => match peeked.take() {
                Some(p) => p,
                None => self.iter_next(inner)?,
            },
            IterState::Inspect(inner, f) => match self.iter_next(inner)? {
                Some(x) => {
                    let arg = Value::RefBox(Interp::new_cell(x.clone()));
                    self.call_value(f, vec![arg])?;
                    Some(x)
                }
                None => None,
            },
            IterState::Scan(inner, state, f) => match self.iter_next(inner)? {
                Some(x) => {
                    let r = self.call_value(f, vec![Value::RefBox(state.clone()), x])?;
                    match self.deref(r) {
                        Value::Enum(V_SOME, Some(l)) => Some(l.borrow()[0].clone()),
                        _ => None,
                    }
                }
                None => None,
            },
            IterState::Cloned(inner) => match self.iter_next(inner)? {
                Some(x) => {
                    let d = self.deref(x);
                    Some(self.clone_deep(&d))
                }
                None => None,
            },
            IterState::Rev(inner) => match self.iter_next_back(inner)? {
                Some(x) => x,
                None => {
                    let mut all = self.drain_iter(inner)?;
                    all.reverse();
                    **inner = IterState::Vals(all.into());
                    self.iter_next(inner)?
                }
            },
            IterState::Cycle(buf, i, inner, exhausted) => {
                if !*exhausted {
                    if let Some(x) = self.iter_next(inner)? {
                        buf.push(x.clone());
                        return Ok(Some(x));
                    }
                    *exhausted = true;
                }
                if buf.is_empty() {
                    None
                } else {
                    let x = buf[*i % buf.len()].clone();
                    *i += 1;
                    Some(x)
                }
            }
            IterState::User(obj) => {
                let obj = obj.clone();
                let next = self.syms().next;
                let r = self.call_method_value(obj, next, vec![], None)?;
                match self.deref(r) {
                    Value::Enum(V_SOME, Some(l)) => Some(l.borrow()[0].clone()),
                    _ => None,
                }
            }
            IterState::FromFn(f) => {
                let f = f.clone();
                let r = self.call_value(&f, vec![])?;
                match self.deref(r) {
                    Value::Enum(V_SOME, Some(l)) => Some(l.borrow()[0].clone()),
                    _ => None,
                }
            }
            IterState::Successors(next, f) => match next.take() {
                Some(x) => {
                    let arg = Value::RefBox(Interp::new_cell(x.clone()));
                    let r = self.call_value(f, vec![arg])?;
                    *next = match self.deref(r) {
                        Value::Enum(V_SOME, Some(l)) => Some(l.borrow()[0].clone()),
                        _ => None,
                    };
                    Some(x)
                }
                None => None,
            },
            IterState::Repeat(v) => Some(self.clone_deep(v)),
            IterState::Fuse(inner, done) => {
                if *done {
                    None
                } else {
                    let r = self.iter_next(inner)?;
                    if r.is_none() {
                        *done = true;
                    }
                    r
                }
            }
        })
    }

    /// `next_back`; returns `None` when the iterator is not double-ended.
    pub fn iter_next_back(&mut self, st: &mut IterState) -> R<Option<Option<Value>>> {
        Ok(Some(match st {
            IterState::Empty => None,
            IterState::List { list, i, end, refs } => {
                if *i >= *end {
                    None
                } else {
                    *end -= 1;
                    if *refs {
                        Some(Value::RefElem(list.clone(), *end as u32))
                    } else {
                        let v = list.borrow()[*end].clone();
                        Some(self.copy_val(v))
                    }
                }
            }
            IterState::Range { cur, end } => {
                if *cur >= *end {
                    None
                } else {
                    *end -= 1;
                    Some(Value::Int(*end))
                }
            }
            IterState::CharRange { cur, end } => {
                if *cur >= *end {
                    None
                } else {
                    *end -= 1;
                    char::from_u32(*end).map(Value::Char)
                }
            }
            IterState::Vals(v) => v.pop_back(),
            IterState::Shared(rc) => {
                let rc = rc.clone();
                let mut b = rc.borrow_mut();
                return self.iter_next_back(&mut b);
            }
            IterState::Map(inner, f) => match self.iter_next_back(inner)? {
                Some(Some(x)) => Some(self.call_value(f, vec![x])?),
                Some(None) => None,
                None => return Ok(None),
            },
            IterState::Cloned(inner) => match self.iter_next_back(inner)? {
                Some(Some(x)) => {
                    let d = self.deref(x);
                    Some(self.clone_deep(&d))
                }
                Some(None) => None,
                None => return Ok(None),
            },
            IterState::Rev(inner) => match self.iter_next(inner)? {
                Some(x) => Some(x),
                None => None,
            },
            IterState::Filter(inner, f) => loop {
                match self.iter_next_back(inner)? {
                    Some(Some(x)) => {
                        let arg = Value::RefBox(Interp::new_cell(x.clone()));
                        let keep = self.call_value(f, vec![arg])?;
                        if matches!(self.deref(keep), Value::Bool(true)) {
                            break Some(x);
                        }
                    }
                    Some(None) => break None,
                    None => return Ok(None),
                }
            },
            IterState::Enumerate(inner, n) => {
                let Some(len) = inner.len_hint() else { return Ok(None) };
                match self.iter_next_back(inner)? {
                    Some(Some(x)) => Some(Value::tuple(vec![Value::Int(*n + len as i64 - 1), x])),
                    Some(None) => None,
                    None => return Ok(None),
                }
            }
            IterState::Chain(a, b) => match self.iter_next_back(b)? {
                Some(Some(x)) => Some(x),
                Some(None) => match self.iter_next_back(a)? {
                    Some(x) => x,
                    None => return Ok(None),
                },
                None => return Ok(None),
            },
            _ => return Ok(None),
        }))
    }

    pub fn drain_iter(&mut self, st: &mut IterState) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(st.len_hint().unwrap_or(0));
        while let Some(x) = self.iter_next(st)? {
            out.push(x);
        }
        Ok(out)
    }

    fn take_iter(&mut self, recv: Value, e: &Expr) -> R<IterState> {
        self.make_iter(recv, e)
    }

    fn closure_arg(args: &[Value], i: usize, e: &Expr) -> R<Value> {
        match args.get(i) {
            Some(v) => Ok(v.clone()),
            None => err(format!("{}:{}: missing argument", e.line, e.col)),
        }
    }

    /// Iterator adapter or consumer methods. Returns `None` if `mid` is not
    /// an iterator method.
    pub fn iter_method(&mut self, recv: Value, mid: M, args: Vec<Value>, hint: Option<&Type>, e: &Expr) -> R<Option<Value>> {
        let wrap = |st: IterState| Value::Iter(Rc::new(RefCell::new(st)));
        let adapter = |me: &mut Self, recv: Value| -> R<Box<IterState>> { Ok(boxed(me.take_iter(recv, e)?)) };
        Ok(Some(match mid {
            M::next => {
                let mut st = self.take_iter(recv, e)?;
                let r = self.iter_next(&mut st)?;
                Value::opt(r)
            }
            M::next_back => {
                let mut st = self.take_iter(recv, e)?;
                let r = match self.iter_next_back(&mut st)? {
                    Some(r) => r,
                    None => {
                        let mut all = self.drain_iter(&mut st)?;
                        all.pop()
                    }
                };
                Value::opt(r)
            }
            M::peek => {
                let rc = match &recv {
                    Value::Iter(rc) => rc.clone(),
                    _ => return err(format!("{}:{}: peek on non-peekable", e.line, e.col)),
                };
                let mut st = rc.borrow_mut();
                let mut st = &mut *st;
                while let IterState::Shared(inner) = st {
                    let inner = inner.clone();
                    drop(inner);
                    break;
                }
                if let IterState::Peekable(inner, peeked) = &mut st {
                    if peeked.is_none() {
                        let x = self.iter_next(inner)?;
                        *peeked = Some(x);
                    }
                    Value::opt(peeked.as_ref().unwrap().clone())
                } else {
                    return err(format!("{}:{}: peek on non-peekable iterator", e.line, e.col));
                }
            }
            M::next_if => {
                let rc = match &recv {
                    Value::Iter(rc) => rc.clone(),
                    _ => return err("next_if on non-peekable"),
                };
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = rc.borrow_mut();
                if let IterState::Peekable(inner, peeked) = &mut *st {
                    if peeked.is_none() {
                        let x = self.iter_next(inner)?;
                        *peeked = Some(x);
                    }
                    match peeked.as_ref().unwrap().clone() {
                        Some(x) => {
                            let ok = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(x.clone()))])?;
                            if matches!(self.deref(ok), Value::Bool(true)) {
                                *peeked = None;
                                Value::some(x)
                            } else {
                                Value::none()
                            }
                        }
                        None => Value::none(),
                    }
                } else {
                    return err("next_if on non-peekable iterator");
                }
            }
            M::map => wrap(IterState::Map(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?)),
            M::filter => wrap(IterState::Filter(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?)),
            M::filter_map => wrap(IterState::FilterMap(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?)),
            M::enumerate => wrap(IterState::Enumerate(adapter(self, recv)?, 0)),
            M::zip => {
                let a = adapter(self, recv)?;
                let b = boxed(self.make_iter(Self::closure_arg(&args, 0, e)?, e)?);
                wrap(IterState::Zip(a, b))
            }
            M::chain => {
                let a = adapter(self, recv)?;
                let b = boxed(self.make_iter(Self::closure_arg(&args, 0, e)?, e)?);
                wrap(IterState::Chain(a, b))
            }
            M::take => {
                let n = self.as_int(&Self::closure_arg(&args, 0, e)?)? as usize;
                wrap(IterState::Take(adapter(self, recv)?, n))
            }
            M::skip => {
                let n = self.as_int(&Self::closure_arg(&args, 0, e)?)? as usize;
                wrap(IterState::Skip(adapter(self, recv)?, n))
            }
            M::step_by => {
                let n = self.as_int(&Self::closure_arg(&args, 0, e)?)? as usize;
                if n == 0 {
                    return self.panic_e(e, "assertion failed: step != 0");
                }
                wrap(IterState::StepBy(adapter(self, recv)?, n, true))
            }
            M::take_while => wrap(IterState::TakeWhile(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?, false)),
            M::skip_while => wrap(IterState::SkipWhile(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?, false)),
            M::map_while => wrap(IterState::MapWhile(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?, false)),
            M::flat_map => wrap(IterState::FlatMap(adapter(self, recv)?, Some(Self::closure_arg(&args, 0, e)?), None)),
            M::flatten => wrap(IterState::FlatMap(adapter(self, recv)?, None, None)),
            M::peekable => wrap(IterState::Peekable(adapter(self, recv)?, None)),
            M::inspect => wrap(IterState::Inspect(adapter(self, recv)?, Self::closure_arg(&args, 0, e)?)),
            M::scan => {
                let init = Self::closure_arg(&args, 0, e)?;
                let f = Self::closure_arg(&args, 1, e)?;
                wrap(IterState::Scan(adapter(self, recv)?, Interp::new_cell(init), f))
            }
            M::cloned | M::copied => wrap(IterState::Cloned(adapter(self, recv)?)),
            M::rev => wrap(IterState::Rev(adapter(self, recv)?)),
            M::cycle => wrap(IterState::Cycle(vec![], 0, adapter(self, recv)?, false)),
            M::fuse => wrap(IterState::Fuse(adapter(self, recv)?, false)),
            M::by_ref | M::into_iter | M::iter => match recv {
                Value::Iter(_) => recv,
                other => wrap(self.take_iter(other, e)?),
            },
            M::collect => {
                let mut st = self.take_iter(recv, e)?;
                self.collect(&mut st, hint, e)?
            }
            M::sum | M::product => {
                let mut st = self.take_iter(recv, e)?;
                let is_float = hint.is_some_and(|t| matches!(t.base().name.as_str(), "f64" | "f32"));
                let mut acc = if mid == M::sum {
                    if is_float { Value::Float(0.0) } else { Value::Int(0) }
                } else if is_float {
                    Value::Float(1.0)
                } else {
                    Value::Int(1)
                };
                let op = if mid == M::sum { crate::ast::BinOp::Add } else { crate::ast::BinOp::Mul };
                let mut first = true;
                while let Some(x) = self.iter_next(&mut st)? {
                    let x = self.deref(x);
                    if first {
                        if let Value::Float(_) = x {
                            if let Value::Int(i) = acc {
                                acc = Value::Float(i as f64);
                            }
                        }
                        first = false;
                    }
                    acc = match (&acc, &x) {
                        (Value::Int(a), Value::Int(b)) => self.int_op(op, *a, *b, e)?,
                        _ => self.binary_slow(op, acc, x, e)?,
                    };
                }
                acc
            }
            M::count => {
                let mut st = self.take_iter(recv, e)?;
                if let Some(n) = st.len_hint() {
                    if !matches!(st, IterState::Map(..) | IterState::Inspect(..)) {
                        return Ok(Some(Value::Int(n as i64)));
                    }
                }
                let mut n = 0;
                while self.iter_next(&mut st)?.is_some() {
                    n += 1;
                }
                Value::Int(n)
            }
            M::last => {
                let mut st = self.take_iter(recv, e)?;
                let mut last = None;
                while let Some(x) = self.iter_next(&mut st)? {
                    last = Some(x);
                }
                Value::opt(last)
            }
            M::nth => {
                let n = self.as_int(&Self::closure_arg(&args, 0, e)?)?;
                let mut st = self.take_iter(recv, e)?;
                let mut r = None;
                for _ in 0..=n {
                    r = self.iter_next(&mut st)?;
                    if r.is_none() {
                        break;
                    }
                }
                Value::opt(r)
            }
            M::for_each => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    self.call_value(&f, vec![x])?;
                }
                Value::Unit
            }
            M::fold => {
                let mut acc = Self::closure_arg(&args, 0, e)?;
                let f = Self::closure_arg(&args, 1, e)?;
                let mut st = self.take_iter(recv, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    acc = self.call_value(&f, vec![acc, x])?;
                }
                acc
            }
            M::reduce => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                match self.iter_next(&mut st)? {
                    None => Value::none(),
                    Some(mut acc) => {
                        while let Some(x) = self.iter_next(&mut st)? {
                            acc = self.call_value(&f, vec![acc, x])?;
                        }
                        Value::some(acc)
                    }
                }
            }
            M::any | M::all => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                let want = mid == M::any;
                while let Some(x) = self.iter_next(&mut st)? {
                    let r = self.call_value(&f, vec![x])?;
                    if matches!(self.deref(r), Value::Bool(b) if b == want) {
                        return Ok(Some(Value::Bool(want)));
                    }
                }
                Value::Bool(!want)
            }
            M::find => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    let arg = Value::RefBox(Interp::new_cell(x.clone()));
                    let r = self.call_value(&f, vec![arg])?;
                    if matches!(self.deref(r), Value::Bool(true)) {
                        return Ok(Some(Value::some(x)));
                    }
                }
                Value::none()
            }
            M::find_map => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                while let Some(x) = self.iter_next(&mut st)? {
                    let r = self.call_value(&f, vec![x])?;
                    if let Value::Enum(V_SOME, _) = self.deref(r.clone()) {
                        return Ok(Some(r));
                    }
                }
                Value::none()
            }
            M::position | M::rposition => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                let mut items = Vec::new();
                if mid == M::rposition {
                    items = self.drain_iter(&mut st)?;
                    for (i, x) in items.into_iter().enumerate().rev() {
                        let r = self.call_value(&f, vec![x])?;
                        if matches!(self.deref(r), Value::Bool(true)) {
                            return Ok(Some(Value::some(Value::Int(i as i64))));
                        }
                    }
                    return Ok(Some(Value::none()));
                }
                let _ = &mut items;
                let mut i = 0;
                while let Some(x) = self.iter_next(&mut st)? {
                    let r = self.call_value(&f, vec![x])?;
                    if matches!(self.deref(r), Value::Bool(true)) {
                        return Ok(Some(Value::some(Value::Int(i))));
                    }
                    i += 1;
                }
                Value::none()
            }
            M::max | M::min => {
                let mut st = self.take_iter(recv, e)?;
                let mut best: Option<Value> = None;
                while let Some(x) = self.iter_next(&mut st)? {
                    best = Some(match best {
                        None => x,
                        Some(b) => {
                            let (bd, xd) = (self.deref(b.clone()), self.deref(x.clone()));
                            let c = self.cmp_total(&xd, &bd)?;
                            if (mid == M::max && c != Ordering::Less) || (mid == M::min && c == Ordering::Less) { x } else { b }
                        }
                    });
                }
                Value::opt(best)
            }
            M::max_by_key | M::min_by_key => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                let mut best: Option<(Value, Value)> = None;
                while let Some(x) = self.iter_next(&mut st)? {
                    let k = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(x.clone()))])?;
                    let k = self.deref(k);
                    best = Some(match best {
                        None => (k, x),
                        Some((bk, bx)) => {
                            let c = self.cmp_total(&k, &bk)?;
                            if (mid == M::max_by_key && c != Ordering::Less) || (mid == M::min_by_key && c == Ordering::Less) {
                                (k, x)
                            } else {
                                (bk, bx)
                            }
                        }
                    });
                }
                Value::opt(best.map(|b| b.1))
            }
            M::max_by | M::min_by => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                let mut best: Option<Value> = None;
                while let Some(x) = self.iter_next(&mut st)? {
                    best = Some(match best {
                        None => x,
                        Some(b) => {
                            let r = self.call_value(
                                &f,
                                vec![Value::RefBox(Interp::new_cell(x.clone())), Value::RefBox(Interp::new_cell(b.clone()))],
                            )?;
                            let c = self.value_to_ordering(&r).unwrap_or(Ordering::Equal);
                            if (mid == M::max_by && c != Ordering::Less) || (mid == M::min_by && c == Ordering::Less) { x } else { b }
                        }
                    });
                }
                Value::opt(best)
            }
            M::partition => {
                let f = Self::closure_arg(&args, 0, e)?;
                let mut st = self.take_iter(recv, e)?;
                let (mut a, mut b) = (Vec::new(), Vec::new());
                while let Some(x) = self.iter_next(&mut st)? {
                    let r = self.call_value(&f, vec![Value::RefBox(Interp::new_cell(x.clone()))])?;
                    if matches!(self.deref(r), Value::Bool(true)) {
                        a.push(x);
                    } else {
                        b.push(x);
                    }
                }
                Value::tuple(vec![Value::vec(a), Value::vec(b)])
            }
            M::unzip => {
                let mut st = self.take_iter(recv, e)?;
                let (mut a, mut b) = (Vec::new(), Vec::new());
                while let Some(x) = self.iter_next(&mut st)? {
                    if let Value::Tuple(l) = self.deref(x) {
                        let l = l.borrow();
                        a.push(l[0].clone());
                        b.push(l[1].clone());
                    }
                }
                Value::tuple(vec![Value::vec(a), Value::vec(b)])
            }
            M::is_sorted => {
                let items = {
                    let mut st = self.take_iter(recv, e)?;
                    self.drain_iter(&mut st)?
                };
                for w in items.windows(2) {
                    let (a, b) = (self.deref(w[0].clone()), self.deref(w[1].clone()));
                    if self.cmp_total(&a, &b)? == Ordering::Greater {
                        return Ok(Some(Value::Bool(false)));
                    }
                }
                Value::Bool(true)
            }
            M::eq | M::ne if matches!(recv, Value::Iter(_)) => {
                let mut a = self.take_iter(recv, e)?;
                let mut b = self.make_iter(Self::closure_arg(&args, 0, e)?, e)?;
                let a = self.drain_iter(&mut a)?;
                let b = self.drain_iter(&mut b)?;
                let r = self.equals(&Value::vec(a), &Value::vec(b))?;
                Value::Bool(if mid == M::eq { r } else { !r })
            }
            M::size_hint => {
                let st = self.take_iter(recv, e)?;
                let n = st.len_hint();
                Value::tuple(vec![Value::Int(n.unwrap_or(0) as i64), Value::opt(n.map(|n| Value::Int(n as i64)))])
            }
            M::len => {
                let st = self.take_iter(recv, e)?;
                match st.len_hint() {
                    Some(n) => Value::Int(n as i64),
                    None => return Ok(None),
                }
            }
            _ => return Ok(None),
        }))
    }

    pub fn collect(&mut self, st: &mut IterState, hint: Option<&Type>, e: &Expr) -> R<Value> {
        let name = hint.map(|t| t.base().name.as_str()).unwrap_or("Vec");
        Ok(match name {
            "String" => {
                let mut s = String::new();
                while let Some(x) = self.iter_next(st)? {
                    match self.deref(x) {
                        Value::Char(c) => s.push(c),
                        v => s.push_str(&crate::builtins::str_of(&v)),
                    }
                }
                Value::string(s)
            }
            "HashMap" | "BTreeMap" => {
                let mut m = if name == "BTreeMap" { MapData::Tree(Default::default()) } else { MapData::Hash(Default::default()) };
                while let Some(x) = self.iter_next(st)? {
                    if let Value::Tuple(l) = self.deref(x) {
                        let l = l.borrow();
                        let k = self.deref(l[0].clone());
                        m.insert(Key(k), Interp::new_cell(l[1].clone()));
                    }
                }
                Value::Map(Rc::new(RefCell::new(m)))
            }
            "HashSet" | "BTreeSet" => {
                let mut s = if name == "BTreeSet" { SetData::Tree(Default::default()) } else { SetData::Hash(Default::default()) };
                while let Some(x) = self.iter_next(st)? {
                    let x = self.deref(x);
                    s.insert(Key(x));
                }
                Value::Set(Rc::new(RefCell::new(s)))
            }
            "VecDeque" => Value::Deque(Rc::new(RefCell::new(self.drain_iter(st)?.into()))),
            "BinaryHeap" => {
                let items = self.drain_iter(st)?;
                let h = Value::Heap(new_list(vec![]));
                for x in items {
                    self.heap_push(&h, x)?;
                }
                h
            }
            "Result" | "Option" => {
                let inner = hint.and_then(|t| t.args.first());
                let mut items = Vec::new();
                while let Some(x) = self.iter_next(st)? {
                    match self.deref(x) {
                        Value::Enum(V_OK | V_SOME, Some(l)) => items.push(l.borrow()[0].clone()),
                        v @ Value::Enum(V_ERR | V_NONE, _) => return Ok(v),
                        v => items.push(v),
                    }
                }
                let mut vs = IterState::Vals(items.into());
                let c = self.collect(&mut vs, inner, e)?;
                if name == "Result" { Value::ok(c) } else { Value::some(c) }
            }
            _ => Value::Vec(new_list(self.drain_iter(st)?)),
        })
    }
}
