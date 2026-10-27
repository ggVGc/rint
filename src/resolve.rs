//! Name resolution: rewrites paths into local slots, captures and item ids,
//! registers items, impls and traits, and propagates type hints.

use crate::ast::*;
use crate::builtins::{self, StaticRes};
use crate::program::*;
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type RResult<T> = Result<T, String>;

#[derive(Clone, Copy, Debug)]
enum Global {
    Fn(u32),
    Struct(u32),
    Enum(u32),
    Variant(u32),
    Const(u32),
    Static(u32),
}

#[derive(Clone, Debug)]
enum Res {
    Local(u32),
    Cap(u32),
    Fn(u32),
    Native(u32),
    Const(u32),
    Static(u32),
    Struct(u32),
    Variant(u32),
    Builtin(u32),
    Value(Value),
    Method(String),
    DefaultOf(Type),
}

struct FnCtx {
    scopes: Vec<HashMap<String, u32>>,
    nslots: u32,
    is_closure: bool,
    caps: Vec<CapSrc>,
}

pub struct Resolver {
    pub prog: Program,
    native_paths: Vec<String>,
    discover_native: bool,
    ctxs: Vec<FnCtx>,
    globals: HashMap<String, Global>,
    aliases: HashMap<String, Vec<String>>,
    glob_enums: Vec<u32>,
    modules: HashSet<String>,
    traits: HashMap<String, Vec<(Sym, u32)>>,
    trait_names: HashSet<String>,
    pending_fns: Vec<(u32, FnDecl, Option<String>)>,
    pending_consts: Vec<(u32, Option<String>)>,
    const_inits: HashMap<u32, Expr>,
    impl_self: Option<String>,
    /// Declared parameter types per function id (for argument type hints).
    param_types: HashMap<u32, Vec<Option<Type>>>,
}

const STD_MODULES: &[&str] = &[
    "std", "core", "alloc", "crate", "self", "super", "collections", "cmp", "mem", "fmt", "io", "iter", "process",
    "rc", "cell", "ops", "string", "vec", "num", "convert", "hash", "sync", "time", "env", "thread", "boxed",
    "borrow", "prelude", "hash_map", "btree_map", "consts", "ascii", "array", "slice", "option", "result",
    "collections::hash_map", "atomic", "marker", "any", "default", "clone", "error",
];

const STD_TRAITS: &[&str] = &[
    "Clone", "ToString", "Iterator", "IntoIterator", "PartialEq", "Eq", "PartialOrd", "Ord", "Into", "AsRef",
    "AsMut", "Add", "Sub", "Mul", "Div", "Rem", "Neg", "Not", "Display", "Debug", "Hash", "DoubleEndedIterator",
    "ExactSizeIterator", "Borrow", "Deref", "DerefMut", "FromIterator", "Extend", "Write", "Read", "BufRead",
    "TryInto", "AddAssign", "SubAssign", "MulAssign", "Index", "IndexMut", "Fn", "FnMut", "FnOnce", "Sum",
    "Product", "ToOwned", "Copy",
];

const PRIM_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool",
    "char", "str", "String", "Vec", "Option", "Result", "slice", "HashMap", "HashSet", "BTreeMap", "BTreeSet",
    "VecDeque", "BinaryHeap", "Box", "Rc", "RefCell", "Cell", "Arc", "Mutex",
];

pub fn resolve_program(items: Vec<Item>) -> RResult<Program> {
    resolve_with_native(items, vec![], false).map(|(prog, _)| prog)
}

/// Resolve with compiled host bindings, or discover external function paths.
pub fn resolve_with_native(items: Vec<Item>, native_paths: Vec<String>, discover_native: bool) -> RResult<(Program, Vec<String>)> {
    let mut r = Resolver {
        native_paths,
        discover_native,
        prog: Program::default(),
        ctxs: Vec::new(),
        globals: HashMap::new(),
        aliases: HashMap::new(),
        glob_enums: Vec::new(),
        modules: HashSet::new(),
        traits: HashMap::new(),
        trait_names: HashSet::new(),
        pending_fns: Vec::new(),
        pending_consts: Vec::new(),
        const_inits: HashMap::new(),
        impl_self: None,
        param_types: HashMap::new(),
    };
    r.add_builtin_types();
    r.register(items)?;
    r.resolve_pending()?;
    match r.globals.get("main") {
        Some(Global::Fn(id)) => r.prog.main = Some(*id),
        _ => {}
    }
    Ok((r.prog, r.native_paths))
}

fn dummy_fn() -> Rc<FnDecl> {
    Rc::new(FnDecl {
        name: String::new(),
        params: vec![],
        ret: None,
        body: None,
        self_kind: SelfKind::None,
        nslots: 0,
        is_closure: false,
        is_move: false,
        caps: vec![],
        line: 0,
    })
}

impl Resolver {
    fn add_builtin_types(&mut self) {
        let mk_enum = |r: &mut Resolver, name: &str, vars: &[(&str, StructKind, i64)]| {
            let eid = r.prog.enums.len() as u32;
            let mut ids = Vec::new();
            for (vn, k, d) in vars {
                let vid = r.prog.variants.len() as u32;
                r.prog.variants.push(VariantDef {
                    name: vn.to_string(),
                    enum_id: eid,
                    kind: *k,
                    fields: if *k == StructKind::Tuple { vec![intern("0")] } else { vec![] },
                    field_types: vec![],
                    disc: *d,
                });
                ids.push(vid);
            }
            r.prog.enums.push(EnumDef { name: name.into(), variants: ids, copy: false, derives: vec![] });
            r.globals.insert(name.into(), Global::Enum(eid));
        };
        mk_enum(self, "Option", &[("None", StructKind::Unit, 0), ("Some", StructKind::Tuple, 1)]);
        mk_enum(self, "Result", &[("Ok", StructKind::Tuple, 0), ("Err", StructKind::Tuple, 1)]);
        mk_enum(
            self,
            "Ordering",
            &[("Less", StructKind::Unit, -1), ("Equal", StructKind::Unit, 0), ("Greater", StructKind::Unit, 1)],
        );
        self.prog.enums[E_ORDERING as usize].copy = true;
        for (n, v) in [("None", V_NONE), ("Some", V_SOME), ("Ok", V_OK), ("Err", V_ERR)] {
            self.globals.insert(n.into(), Global::Variant(v));
        }
        for name in ["Reverse", "Wrapping"] {
            let id = self.prog.structs.len() as u32;
            self.prog.structs.push(StructDef {
                name: name.into(),
                kind: StructKind::Tuple,
                fields: vec![intern("0")],
                field_types: vec![],
                copy: false,
                derives: vec![],
            });
            self.globals.insert(name.into(), Global::Struct(id));
        }
    }

    // ------------------------------------------------------------ registration
    fn collect_types(&mut self, items: &mut Vec<Item>) -> RResult<()> {
        for it in items.iter_mut() {
            match it {
                Item::Struct(s) => {
                    let id = self.prog.structs.len() as u32;
                    self.prog.structs.push(StructDef {
                        name: s.name.clone(),
                        kind: s.kind,
                        fields: s.fields.iter().map(|(n, _)| intern(n)).collect(),
                        field_types: s.fields.iter().map(|(_, t)| t.clone()).collect(),
                        copy: s.derives.iter().any(|d| d == "Copy"),
                        derives: s.derives.clone(),
                    });
                    self.globals.insert(s.name.clone(), Global::Struct(id));
                }
                Item::Enum(e) => {
                    let eid = self.prog.enums.len() as u32;
                    let mut ids = Vec::new();
                    let mut next_disc = 0i64;
                    for v in e.variants.iter_mut() {
                        let vid = self.prog.variants.len() as u32;
                        let disc = match &v.disc {
                            Some(Expr { kind: ExprKind::Lit(Value::Int(i)), .. }) => *i,
                            Some(_) => return Err(format!("enum `{}`: only literal discriminants are supported", e.name)),
                            None => next_disc,
                        };
                        next_disc = disc + 1;
                        self.prog.variants.push(VariantDef {
                            name: v.name.clone(),
                            enum_id: eid,
                            kind: v.kind,
                            fields: v.fields.iter().map(|(n, _)| intern(n)).collect(),
                            field_types: v.fields.iter().map(|(_, t)| t.clone()).collect(),
                            disc,
                        });
                        ids.push(vid);
                    }
                    self.prog.enums.push(EnumDef {
                        name: e.name.clone(),
                        variants: ids,
                        copy: e.derives.iter().any(|d| d == "Copy"),
                        derives: e.derives.clone(),
                    });
                    self.globals.insert(e.name.clone(), Global::Enum(eid));
                }
                Item::Mod(name, sub) => {
                    self.modules.insert(name.clone());
                    self.collect_types(sub)?;
                }
                Item::Trait(t) => {
                    self.trait_names.insert(t.name.clone());
                }
                Item::TypeAlias(name, ty) => {
                    if name != &ty.name {
                        self.aliases.insert(name.clone(), vec![ty.name.clone()]);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn type_key(&self, ty: &Type) -> u32 {
        let b = ty.base();
        match self.globals.get(&b.name) {
            Some(Global::Struct(id)) => *id,
            Some(Global::Enum(id)) => TK_ENUM + id,
            _ => {
                if let Some(tk) = builtin_type_key(&b.name) {
                    tk
                } else if b.name == "dyn" || b.name == "impl" {
                    TK_OTHER
                } else {
                    TK_ANY
                }
            }
        }
    }

    fn add_fn(&mut self, f: FnDecl, self_ty: Option<String>) -> u32 {
        let id = self.prog.fns.len() as u32;
        self.prog.fns.push(dummy_fn());
        self.param_types.insert(id, f.params.iter().map(|p| p.ty.clone()).collect());
        self.pending_fns.push((id, f, self_ty));
        id
    }

    fn add_const(&mut self, c: ConstDecl, global_name: String, self_ty: Option<String>) -> u32 {
        let id = self.prog.consts.len() as u32;
        self.prog.consts.push(ConstDef { name: c.name.clone(), init: None, is_static: c.is_static, nslots: 0 });
        if let Some(mut init) = c.init {
            if let Some(t) = &c.ty {
                apply_hint(&mut init, &Rc::new(t.clone()));
            }
            self.const_inits.insert(id, init);
        }
        self.pending_consts.push((id, self_ty));
        self.globals.insert(global_name, if c.is_static { Global::Static(id) } else { Global::Const(id) });
        id
    }

    fn register(&mut self, mut items: Vec<Item>) -> RResult<()> {
        self.collect_types(&mut items)?;
        self.register_rest(items)
    }

    fn register_rest(&mut self, items: Vec<Item>) -> RResult<()> {
        // traits first so impls can pick up default methods
        let mut rest = Vec::new();
        for it in items {
            match it {
                Item::Trait(t) => {
                    let mut defaults = Vec::new();
                    for f in t.fns {
                        if f.body.is_some() {
                            let sym = intern(&f.name);
                            let id = self.add_fn(f, None);
                            defaults.push((sym, id));
                        }
                    }
                    for c in t.consts {
                        if c.init.is_some() {
                            let n = format!("{}::{}", t.name, c.name);
                            self.add_const(c, n, None);
                        }
                    }
                    self.traits.insert(t.name.clone(), defaults);
                }
                Item::Mod(_, sub) => self.register_rest_mod(sub, &mut rest)?,
                other => rest.push(other),
            }
        }
        let mut impls = Vec::new();
        for it in rest {
            match it {
                Item::Fn(f) => {
                    let name = f.name.clone();
                    let id = self.add_fn(f, None);
                    self.globals.insert(name, Global::Fn(id));
                }
                Item::Const(c) => {
                    let n = c.name.clone();
                    self.add_const(c, n, None);
                }
                Item::Use(list) => {
                    for (alias, path) in list {
                        if alias == "*" {
                            // `use Enum::*`
                            let p: Vec<&String> = path.iter().filter(|s| !STD_MODULES.contains(&s.as_str())).collect();
                            if p.len() >= 2 {
                                if let Some(Global::Enum(e)) = self.globals.get(p[p.len() - 2].as_str()) {
                                    self.glob_enums.push(*e);
                                }
                            }
                            continue;
                        }
                        if path.len() == 1 && path[0] == alias {
                            continue;
                        }
                        self.aliases.insert(alias, path);
                    }
                }
                Item::Impl(i) => impls.push(i),
                _ => {}
            }
        }
        for imp in impls {
            self.register_impl(imp)?;
        }
        Ok(())
    }

    fn register_rest_mod(&mut self, items: Vec<Item>, rest: &mut Vec<Item>) -> RResult<()> {
        for it in items {
            match it {
                Item::Trait(t) => self.register_rest(vec![Item::Trait(t)])?,
                Item::Mod(_, sub) => self.register_rest_mod(sub, rest)?,
                other => rest.push(other),
            }
        }
        Ok(())
    }

    fn register_impl(&mut self, imp: ImplDecl) -> RResult<()> {
        let tk = self.type_key(&imp.self_ty);
        let self_name = imp.self_ty.base().name.clone();
        let self_name = if tk == TK_ANY { None } else { Some(self_name) };
        let mut defined = HashSet::new();
        for f in imp.fns {
            let sym = intern(&f.name);
            let name = f.name.clone();
            let id = self.add_fn(f, self_name.clone());
            defined.insert(sym);
            if let Some(tr) = &imp.trait_name {
                // trait-qualified key (e.g. Display::fmt vs Debug::fmt)
                self.prog.methods.insert((tk, intern(&format!("{}::{}", tr, name))), id);
                if tr == "PartialEq" && name == "eq" {
                    self.prog.user_eq.insert(tk, id);
                }
                if tr == "Ord" && name == "cmp" {
                    self.prog.user_cmp.insert(tk, id);
                }
                if tr == "PartialOrd" && name == "partial_cmp" {
                    self.prog.user_partial_cmp.insert(tk, id);
                }
                if matches!(tr.as_str(), "Display" | "Debug") {
                    continue;
                }
            }
            self.prog.methods.insert((tk, sym), id);
        }
        for c in imp.consts {
            if let Some(n) = &self_name {
                let gname = format!("{}::{}", n, c.name);
                self.add_const(c, gname, self_name.clone());
            }
        }
        if let Some(tr) = &imp.trait_name {
            self.prog.impls.entry(tk).or_default().push(tr.clone());
            if tr == "Copy" {
                if tk < TK_ENUM {
                    self.prog.structs[tk as usize].copy = true;
                } else if tk < TK_INT {
                    self.prog.enums[(tk - TK_ENUM) as usize].copy = true;
                }
            }
            if let Some(defs) = self.traits.get(tr).cloned() {
                for (sym, id) in defs {
                    if !defined.contains(&sym) {
                        self.prog.methods.entry((tk, sym)).or_insert(id);
                    }
                }
            }
        }
        Ok(())
    }

    fn resolve_pending(&mut self) -> RResult<()> {
        while !self.pending_fns.is_empty() || !self.pending_consts.is_empty() {
            let fns = std::mem::take(&mut self.pending_fns);
            for (id, mut f, self_ty) in fns {
                let saved = std::mem::replace(&mut self.impl_self, self_ty);
                self.resolve_fn(&mut f)?;
                self.impl_self = saved;
                self.prog.fns[id as usize] = Rc::new(f);
            }
            let consts = std::mem::take(&mut self.pending_consts);
            for (id, self_ty) in consts {
                if let Some(mut init) = self.const_inits.remove(&id) {
                    let saved = std::mem::replace(&mut self.impl_self, self_ty);
                    let saved_ctxs = std::mem::take(&mut self.ctxs);
                    self.ctxs.push(FnCtx { scopes: vec![HashMap::new()], nslots: 0, is_closure: false, caps: vec![] });
                    self.resolve_expr(&mut init)?;
                    let ctx = self.ctxs.pop().unwrap();
                    self.ctxs = saved_ctxs;
                    self.impl_self = saved;
                    let c = &mut self.prog.consts[id as usize];
                    c.nslots = ctx.nslots;
                    c.init = Some(init);
                }
            }
        }
        Ok(())
    }

    // --------------------------------------------------------------- scopes
    fn ctx(&mut self) -> &mut FnCtx {
        self.ctxs.last_mut().unwrap()
    }
    fn push_scope(&mut self) {
        self.ctx().scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.ctx().scopes.pop();
    }
    fn declare(&mut self, name: &str, reuse: bool) -> u32 {
        let ctx = self.ctx();
        if reuse {
            if let Some(&s) = ctx.scopes.last().unwrap().get(name) {
                return s;
            }
        }
        let s = ctx.nslots;
        ctx.nslots += 1;
        ctx.scopes.last_mut().unwrap().insert(name.to_string(), s);
        s
    }

    fn lookup_local(&mut self, k: usize, name: &str) -> Option<Res> {
        for scope in self.ctxs[k].scopes.iter().rev() {
            if let Some(&s) = scope.get(name) {
                return Some(Res::Local(s));
            }
        }
        if self.ctxs[k].is_closure && k > 0 {
            let src = match self.lookup_local(k - 1, name)? {
                Res::Local(s) => CapSrc::Local(s),
                Res::Cap(c) => CapSrc::Cap(c),
                _ => return None,
            };
            let caps = &mut self.ctxs[k].caps;
            let idx = match caps.iter().position(|c| *c == src) {
                Some(i) => i,
                None => {
                    caps.push(src);
                    caps.len() - 1
                }
            };
            return Some(Res::Cap(idx as u32));
        }
        None
    }

    // ------------------------------------------------------------ functions
    fn resolve_fn(&mut self, f: &mut FnDecl) -> RResult<()> {
        let saved_ctxs = if f.is_closure { Vec::new() } else { std::mem::take(&mut self.ctxs) };
        self.ctxs.push(FnCtx {
            scopes: vec![HashMap::new()],
            nslots: f.params.len() as u32,
            is_closure: f.is_closure,
            caps: vec![],
        });
        // simple params bind directly to their argument slot
        for (i, p) in f.params.iter_mut().enumerate() {
            if let Pat::Bind { name, slot, by_ref: false, sub: None, .. } = &mut p.pat {
                if !self.is_unit_like(name) {
                    *slot = i as u32;
                    let n = name.clone();
                    self.ctx().scopes.last_mut().unwrap().insert(n, i as u32);
                }
            }
        }
        for p in f.params.iter_mut() {
            let simple = matches!(&p.pat, Pat::Bind { by_ref: false, sub: None, name, .. } if !self.is_unit_like(name));
            if !simple {
                self.resolve_pat(&mut p.pat, false)?;
            }
        }
        if let Some(body) = &mut f.body {
            self.resolve_block(body)?;
            if let (Some(ret), Some(tail)) = (&f.ret, &mut body.tail) {
                apply_hint(tail, &Rc::new(ret.clone()));
            }
        }
        let ctx = self.ctxs.pop().unwrap();
        f.nslots = ctx.nslots;
        f.caps = ctx.caps;
        if !f.is_closure {
            self.ctxs = saved_ctxs;
        }
        Ok(())
    }

    fn is_unit_like(&self, name: &str) -> bool {
        if self.ctxs.iter().any(|c| c.scopes.iter().any(|s| s.contains_key(name))) {
            return false;
        }
        match self.globals.get(name) {
            Some(Global::Variant(v)) => self.prog.variants[*v as usize].kind == StructKind::Unit,
            Some(Global::Struct(s)) => self.prog.structs[*s as usize].kind == StructKind::Unit,
            Some(Global::Const(_)) | Some(Global::Static(_)) => true,
            _ => {
                for e in &self.glob_enums {
                    for v in &self.prog.enums[*e as usize].variants {
                        let vd = &self.prog.variants[*v as usize];
                        if vd.name == name && vd.kind == StructKind::Unit {
                            return true;
                        }
                    }
                }
                if let Some(p) = self.aliases.get(name) {
                    if p.len() >= 2 {
                        if let Some(Global::Enum(e)) = self.globals.get(p[p.len() - 2].as_str()) {
                            return self.prog.enums[*e as usize]
                                .variants
                                .iter()
                                .any(|v| self.prog.variants[*v as usize].name == *name && self.prog.variants[*v as usize].kind == StructKind::Unit);
                        }
                    }
                }
                false
            }
        }
    }

    // ---------------------------------------------------------------- paths
    fn resolve_value_path(&mut self, p: &PathExpr) -> RResult<Res> {
        if !p.generics.is_empty() {
            let mut segs = p.segs.clone();
            if let Some(prefix) = self.aliases.get(&segs[0]) {
                let mut expanded = prefix.clone();
                expanded.extend(segs[1..].iter().cloned());
                segs = expanded;
            }
            if segs.len() > 1 && self.is_external_root(&segs[0]) {
                let types = p.generics.iter().map(crate::bindings::rust_type).collect::<Result<Vec<_>, _>>()?;
                let last = segs.last_mut().unwrap();
                *last = format!("{}::<{}>", last, types.join(", "));
                return self.resolve_path(&segs);
            }
        }
        self.resolve_path(&p.segs)
    }

    fn is_external_root(&self, root: &str) -> bool {
        !STD_MODULES.contains(&root)
            && !PRIM_TYPES.contains(&root)
            && !self.modules.contains(root)
            && !self.globals.contains_key(root)
            && !STD_TRAITS.contains(&root)
            && !self.trait_names.contains(root)
    }

    fn resolve_path(&mut self, segs: &[String]) -> RResult<Res> {
        if segs.len() == 1 {
            let name = &segs[0];
            if let Some(r) = self.lookup_local(self.ctxs.len() - 1, name) {
                return Ok(r);
            }
            return self.resolve_global(name);
        }
        // expand a leading alias
        let mut segs: Vec<String> = segs.to_vec();
        if let Some(p) = self.aliases.get(&segs[0]) {
            let mut v = p.clone();
            v.extend(segs[1..].iter().cloned());
            segs = v;
        }
        if let Some(id) = self.native_paths.iter().position(|p| p == &segs.join("::")) {
            return Ok(Res::Native(id as u32));
        }
        if self.is_external_root(&segs[0]) {
            if self.discover_native {
                let id = self.native_paths.len() as u32;
                self.native_paths.push(segs.join("::"));
                return Ok(Res::Native(id));
            }
            return Err(format!("unresolved path `{}` (no native binding registered)", segs.join("::")));
        }
        let is_module = |r: &Resolver, s: &str| STD_MODULES.contains(&s) || r.modules.contains(s);
        while segs.len() > 2 && is_module(self, &segs[0]) {
            segs.remove(0);
        }
        if segs.len() == 2 && is_module(self, &segs[0]) {
            if let Some(r) = self.builtin_static(&segs[0], &segs[1]) {
                return Ok(r);
            }
            segs.remove(0);
        }
        if let Some(i) = segs.iter().position(|s| s == "consts") {
            segs.drain(..=i);
        }
        if segs.len() == 1 {
            if let Some(v) = builtins::lookup_const(&segs[0]) {
                return Ok(Res::Value(v));
            }
            return self.resolve_global(&segs[0]);
        }
        let n = segs.len();
        let mut ty = segs[n - 2].clone();
        let item = segs[n - 1].clone();
        if ty == "Self" {
            ty = self.impl_self.clone().ok_or("`Self` used outside of an impl")?;
        }
        if let Some(p) = self.aliases.get(&ty) {
            ty = p.last().unwrap().clone();
        }
        match self.globals.get(&ty).copied() {
            Some(Global::Enum(e)) => {
                for &v in &self.prog.enums[e as usize].variants {
                    if self.prog.variants[v as usize].name == item {
                        return Ok(Res::Variant(v));
                    }
                }
                if let Some(id) = self.prog.methods.get(&(TK_ENUM + e, intern(&item))) {
                    return Ok(Res::Fn(*id));
                }
                if let Some(g) = self.globals.get(&format!("{}::{}", ty, item)) {
                    return Ok(global_res(*g));
                }
                if let Some(r) = self.builtin_static(&ty, &item) {
                    return Ok(r);
                }
                if item == "default" {
                    return Ok(Res::DefaultOf(Type::new(&ty, vec![])));
                }
                return Err(format!("no variant or associated item `{}` in enum `{}`", item, ty));
            }
            Some(Global::Struct(s)) => {
                if let Some(id) = self.prog.methods.get(&(s, intern(&item))) {
                    return Ok(Res::Fn(*id));
                }
                if let Some(g) = self.globals.get(&format!("{}::{}", ty, item)) {
                    return Ok(global_res(*g));
                }
                if item == "default" {
                    return Ok(Res::DefaultOf(Type::new(&ty, vec![])));
                }
                if let Some(r) = self.builtin_static(&ty, &item) {
                    return Ok(r);
                }
                return Err(format!("no associated item `{}` in `{}`", item, ty));
            }
            _ => {}
        }
        if let Some(g) = self.globals.get(&format!("{}::{}", ty, item)) {
            return Ok(global_res(*g));
        }
        if let Some(r) = self.builtin_static(&ty, &item) {
            return Ok(r);
        }
        if self.trait_names.contains(&ty) || STD_TRAITS.contains(&ty.as_str()) || PRIM_TYPES.contains(&ty.as_str()) {
            if item == "default" {
                return Ok(Res::DefaultOf(Type::new(&ty, vec![])));
            }
            return Ok(Res::Method(item));
        }
        if ty.starts_with('<') {
            return Ok(Res::Method(item));
        }
        Err(format!("unresolved path `{}`", segs.join("::")))
    }

    fn builtin_static(&self, ty: &str, item: &str) -> Option<Res> {
        match builtins::lookup_static(ty, item)? {
            StaticRes::Fn(id) => Some(Res::Builtin(id)),
            StaticRes::Value(v) => Some(Res::Value(v)),
        }
    }

    fn resolve_global(&mut self, name: &str) -> RResult<Res> {
        if name == "Self" {
            let s = self.impl_self.clone().ok_or("`Self` used outside of an impl")?;
            return self.resolve_global(&s);
        }
        if let Some(g) = self.globals.get(name) {
            return Ok(global_res(*g));
        }
        for e in &self.glob_enums {
            for &v in &self.prog.enums[*e as usize].variants {
                if self.prog.variants[v as usize].name == name {
                    return Ok(Res::Variant(v));
                }
            }
        }
        if let Some(p) = self.aliases.get(name).cloned() {
            if p.len() > 1 || p[0] != name {
                return self.resolve_path(&p);
            }
        }
        if let Some(id) = builtins::lookup_free(name) {
            return Ok(Res::Builtin(id));
        }
        Err(format!("cannot find value `{}` in this scope", name))
    }

    fn res_to_expr(&self, r: Res, line: u32) -> RResult<ExprKind> {
        Ok(match r {
            Res::Local(s) => ExprKind::Local(s),
            Res::Cap(c) => ExprKind::Cap(c),
            Res::Fn(id) => ExprKind::Lit(Value::Fn(id)),
            Res::Native(id) => ExprKind::Lit(Value::NativeFn(id)),
            Res::Const(id) => ExprKind::Const(id),
            Res::Static(id) => ExprKind::Static(id),
            Res::Struct(id) => {
                let sd = &self.prog.structs[id as usize];
                match sd.kind {
                    StructKind::Unit => ExprKind::MakeStruct(id, vec![]),
                    StructKind::Tuple => ExprKind::Lit(Value::Ctor(id, false)),
                    StructKind::Named => return Err(format!("line {}: expected value, found struct `{}`", line, sd.name)),
                }
            }
            Res::Variant(v) => {
                let vd = &self.prog.variants[v as usize];
                match vd.kind {
                    StructKind::Unit => ExprKind::Lit(Value::Enum(v, None)),
                    StructKind::Tuple => ExprKind::Lit(Value::Ctor(v, true)),
                    StructKind::Named => return Err(format!("line {}: expected value, found struct variant `{}`", line, vd.name)),
                }
            }
            Res::Builtin(id) => ExprKind::Lit(Value::BuiltinFn(id)),
            Res::Value(v) => ExprKind::Lit(v),
            Res::Method(m) => ExprKind::Lit(Value::BuiltinFn(builtins::METHOD_FN_BASE + intern(&m))),
            Res::DefaultOf(t) => ExprKind::CallBuiltin(builtins::bf::DEFAULT, vec![], Some(Rc::new(t))),
        })
    }

    // --------------------------------------------------------------- blocks
    fn resolve_block(&mut self, b: &mut Block) -> RResult<()> {
        self.push_scope();
        if !b.items.is_empty() {
            let items = std::mem::take(&mut b.items);
            // nested items: register now, resolve their bodies with a clean context
            let saved_pf = std::mem::take(&mut self.pending_fns);
            let saved_pc = std::mem::take(&mut self.pending_consts);
            self.register(items)?;
            let saved_ctxs = std::mem::take(&mut self.ctxs);
            let saved_self = self.impl_self.take();
            self.resolve_pending()?;
            self.ctxs = saved_ctxs;
            self.impl_self = saved_self;
            self.pending_fns = saved_pf;
            self.pending_consts = saved_pc;
        }
        for s in b.stmts.iter_mut() {
            match s {
                Stmt::Let(l) => {
                    if let Some(init) = &mut l.init {
                        self.resolve_expr(init)?;
                        if let Some(t) = &l.ty {
                            apply_hint(init, &Rc::new(t.clone()));
                        }
                    }
                    if let Some(els) = &mut l.els {
                        self.resolve_block(els)?;
                    }
                    self.resolve_pat(&mut l.pat, false)?;
                }
                Stmt::Expr(e, _) => self.resolve_expr(e)?,
            }
        }
        if let Some(t) = &mut b.tail {
            self.resolve_expr(t)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn resolve_conds(&mut self, conds: &mut [Cond]) -> RResult<()> {
        for c in conds.iter_mut() {
            match c {
                Cond::Expr(e) => self.resolve_expr(e)?,
                Cond::Let(p, e) => {
                    self.resolve_expr(e)?;
                    self.resolve_pat(p, false)?;
                }
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------- patterns
    fn resolve_pat(&mut self, p: &mut Pat, reuse: bool) -> RResult<()> {
        match p {
            Pat::Bind { name, slot, by_ref, mutable, sub } => {
                if sub.is_none() && !*by_ref && !*mutable && self.is_unit_like(name) {
                    let r = self.resolve_global(name)?;
                    let k = self.res_to_expr(r, 0)?;
                    *p = Pat::Path { path: vec![], value: Some(Box::new(Expr::new(k, 0, 0))) };
                    return Ok(());
                }
                if let Some(s) = sub {
                    self.resolve_pat(s, reuse)?;
                }
                *slot = self.declare(name, reuse);
            }
            Pat::TupleStruct { path, target, elems } => {
                *target = match self.resolve_path(path)? {
                    Res::Struct(id) => Target::Struct(id),
                    Res::Variant(v) => Target::Variant(v),
                    _ => return Err(format!("`{}` is not a tuple struct or variant", path.join("::"))),
                };
                for e in elems {
                    self.resolve_pat(e, reuse)?;
                }
            }
            Pat::Struct { path, target, fields, .. } => {
                let (t, names) = match self.resolve_path(path)? {
                    Res::Struct(id) => (Target::Struct(id), self.prog.structs[id as usize].fields.clone()),
                    Res::Variant(v) => (Target::Variant(v), self.prog.variants[v as usize].fields.clone()),
                    _ => return Err(format!("`{}` is not a struct or variant", path.join("::"))),
                };
                *target = t;
                for (fname, idx, fp) in fields.iter_mut() {
                    let s = intern(fname);
                    *idx = names.iter().position(|n| *n == s).ok_or_else(|| format!("no field `{}` in `{}`", fname, path.join("::")))? as u32;
                    self.resolve_pat(fp, reuse)?;
                }
            }
            Pat::Path { path, value } => {
                if value.is_none() {
                    let r = self.resolve_path(path)?;
                    let k = self.res_to_expr(r, 0)?;
                    *value = Some(Box::new(Expr::new(k, 0, 0)));
                }
            }
            Pat::Or(alts) => {
                for (i, a) in alts.iter_mut().enumerate() {
                    self.resolve_pat(a, reuse || i > 0)?;
                }
            }
            Pat::Tuple(ps) | Pat::Slice(ps) => {
                for x in ps {
                    self.resolve_pat(x, reuse)?;
                }
            }
            Pat::Ref(x) => self.resolve_pat(x, reuse)?,
            Pat::RangeExpr(lo, hi, _) => {
                if let Some(lo) = lo {
                    self.resolve_expr(lo)?;
                }
                if let Some(hi) = hi {
                    self.resolve_expr(hi)?;
                }
            }
            Pat::Wild | Pat::Rest | Pat::Lit(_) | Pat::Range(..) => {}
        }
        Ok(())
    }

    // ----------------------------------------------------------- expressions
    fn resolve_exprs(&mut self, es: &mut [Expr]) -> RResult<()> {
        for e in es {
            self.resolve_expr(e)?;
        }
        Ok(())
    }

    fn resolve_expr(&mut self, e: &mut Expr) -> RResult<()> {
        let line = e.line;
        let loc = |m: String| format!("{}:{}: {}", line, e.col, m);
        match &mut e.kind {
            ExprKind::Path(p) => {
                if p.segs.len() == 1 && p.segs[0] == "_" {
                    e.kind = ExprKind::Lit(Value::Lazy);
                    return Ok(());
                }
                let r = self.resolve_value_path(p).map_err(loc)?;
                let hint = p.generics.first().cloned();
                let mut k = self.res_to_expr(r, line).map_err(loc)?;
                if let (ExprKind::CallBuiltin(_, _, h), Some(t)) = (&mut k, hint) {
                    *h = Some(Rc::new(t));
                }
                e.kind = k;
            }
            ExprKind::Call(callee, args) => {
                self.resolve_exprs(args)?;
                if let ExprKind::Path(p) = &callee.kind {
                    let r = self.resolve_value_path(p).map_err(loc)?;
                    let hint = p.generics.first().cloned().map(Rc::new);
                    let mut args = std::mem::take(args);
                    let hints: Vec<Option<Type>> = match &r {
                        Res::Fn(id) => self.param_types.get(id).cloned().unwrap_or_default(),
                        Res::Struct(id) => self.prog.structs[*id as usize].field_types.iter().cloned().map(Some).collect(),
                        Res::Variant(v) => self.prog.variants[*v as usize].field_types.iter().cloned().map(Some).collect(),
                        _ => vec![],
                    };
                    for (a, t) in args.iter_mut().zip(hints) {
                        if let Some(t) = t {
                            apply_hint(a, &Rc::new(t));
                        }
                    }
                    e.kind = match r {
                        Res::Fn(id) => ExprKind::CallFn(id, args),
                        Res::Struct(id) => ExprKind::MakeStruct(id, args),
                        Res::Variant(v) => ExprKind::MakeVariant(v, args),
                        Res::Builtin(id) => ExprKind::CallBuiltin(id, args, hint),
                        Res::DefaultOf(t) => ExprKind::CallBuiltin(builtins::bf::DEFAULT, args, Some(Rc::new(t))),
                        Res::Method(m) => {
                            let mut it = args.into_iter();
                            let recv = it.next().ok_or_else(|| loc(format!("cannot call `{}` without a receiver", m)))?;
                            let sym = intern(&m);
                            ExprKind::MethodCall(Box::new(MethodCall {
                                recv,
                                mid: builtins::method_id(&m),
                                name: m,
                                sym,
                                args: it.collect(),
                                turbofish: hint.map(|h| (*h).clone()),
                                hint: None,
                                cache: std::cell::Cell::new((0, 0)),
                            }))
                        }
                        other => {
                            let k = self.res_to_expr(other, line).map_err(loc)?;
                            ExprKind::Call(Box::new(Expr::new(k, line, e.col)), args)
                        }
                    };
                } else {
                    self.resolve_expr(callee)?;
                }
            }
            ExprKind::MethodCall(m) => {
                self.resolve_expr(&mut m.recv)?;
                self.resolve_exprs(&mut m.args)?;
                if let Some(t) = &m.turbofish {
                    m.hint = Some(Rc::new(t.clone()));
                }
            }
            ExprKind::Block(b) => self.resolve_block(b)?,
            ExprKind::If(b) => {
                let (conds, then, els) = &mut **b;
                self.push_scope();
                self.resolve_conds(conds)?;
                self.resolve_block(then)?;
                self.pop_scope();
                if let Some(e) = els {
                    self.resolve_expr(e)?;
                }
            }
            ExprKind::Match(b) => {
                let (scrut, arms) = &mut **b;
                self.resolve_expr(scrut)?;
                for arm in arms.iter_mut() {
                    self.push_scope();
                    self.resolve_pat(&mut arm.pat, false)?;
                    if let Some(g) = &mut arm.guard {
                        self.resolve_conds(g)?;
                    }
                    self.resolve_expr(&mut arm.body)?;
                    self.pop_scope();
                }
            }
            ExprKind::While(b) => {
                let (_, conds, body) = &mut **b;
                self.push_scope();
                self.resolve_conds(conds)?;
                self.resolve_block(body)?;
                self.pop_scope();
            }
            ExprKind::Loop(b) => self.resolve_block(&mut b.1)?,
            ExprKind::For(b) => {
                let (_, pat, it, body) = &mut **b;
                self.resolve_expr(it)?;
                self.push_scope();
                self.resolve_pat(pat, false)?;
                self.resolve_block(body)?;
                self.pop_scope();
            }
            ExprKind::Break(_, v) | ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.resolve_expr(v)?;
                }
            }
            ExprKind::Field(x, ..) | ExprKind::TupleField(x, _) | ExprKind::Unary(_, x) | ExprKind::Cast(x, _)
            | ExprKind::Ref(_, x) | ExprKind::Deref(x) | ExprKind::Try(x) => self.resolve_expr(x)?,
            ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Assign(a, b) | ExprKind::OpAssign(_, a, b)
            | ExprKind::Repeat(a, b) | ExprKind::VecRepeat(a, b) => {
                self.resolve_expr(a)?;
                self.resolve_expr(b)?;
                superinstruction(e);
            }
            ExprKind::StructLit(s) => {
                let (target, names, types) = match self.resolve_path(&s.path).map_err(loc)? {
                    Res::Struct(id) => {
                        let sd = &self.prog.structs[id as usize];
                        (Target::Struct(id), sd.fields.clone(), sd.field_types.clone())
                    }
                    Res::Variant(v) => {
                        let vd = &self.prog.variants[v as usize];
                        (Target::Variant(v), vd.fields.clone(), vd.field_types.clone())
                    }
                    _ => return Err(loc(format!("`{}` is not a struct", s.path.join("::")))),
                };
                s.target = target;
                for (fname, idx, fe) in s.fields.iter_mut() {
                    let sym = intern(fname);
                    let i = names.iter().position(|n| *n == sym).ok_or_else(|| loc(format!("no field `{}`", fname)))?;
                    *idx = i as u32;
                    self.resolve_expr(fe)?;
                    if let Some(t) = types.get(i) {
                        apply_hint(fe, &Rc::new(t.clone()));
                    }
                }
                if let Some(b) = &mut s.base {
                    self.resolve_expr(b)?;
                }
            }
            ExprKind::Tuple(es) | ExprKind::Array(es) | ExprKind::VecLit(es) | ExprKind::CallFn(_, es)
            | ExprKind::MakeVariant(_, es) | ExprKind::MakeStruct(_, es) | ExprKind::CallBuiltin(_, es, _) => {
                self.resolve_exprs(es)?
            }
            ExprKind::Range(a, b, _) => {
                if let Some(a) = a {
                    self.resolve_expr(a)?;
                }
                if let Some(b) = b {
                    self.resolve_expr(b)?;
                }
            }
            ExprKind::Closure(fd) => {
                let fd = Rc::get_mut(fd).ok_or("closure already shared")?;
                self.resolve_fn(fd)?;
            }
            ExprKind::Macro(m) => {
                self.resolve_exprs(&mut m.operands)?;
                self.resolve_exprs(&mut m.args)?;
            }
            ExprKind::Matches(b) => {
                let (x, pat, guard) = &mut **b;
                self.resolve_expr(x)?;
                self.push_scope();
                self.resolve_pat(pat, false)?;
                if let Some(g) = guard {
                    self.resolve_expr(g)?;
                }
                self.pop_scope();
            }
            ExprKind::Let(_) => return Err(loc("`let` expressions are only supported in `if`/`while` conditions".into())),
            ExprKind::DestructAssign(_) | ExprKind::BinLL(..) | ExprKind::BinLI(..) => {}
            ExprKind::AssignLocal(_, x) | ExprKind::OpAssignLocal(_, _, x) => self.resolve_expr(x)?,
            ExprKind::Lit(_) | ExprKind::Local(_) | ExprKind::Cap(_) | ExprKind::Static(_) | ExprKind::Const(_)
            | ExprKind::Continue(_) => {}
        }
        Ok(())
    }
}

/// Rewrite common expression shapes into specialized nodes.
fn superinstruction(e: &mut Expr) {
    let new = match &mut e.kind {
        ExprKind::Binary(op, a, b) if !matches!(op, BinOp::And | BinOp::Or) => match (&a.kind, &b.kind) {
            (ExprKind::Local(x), ExprKind::Local(y)) => Some(ExprKind::BinLL(*op, *x, *y)),
            (ExprKind::Local(x), ExprKind::Lit(Value::Int(c))) => Some(ExprKind::BinLI(*op, *x, *c)),
            _ => None,
        },
        ExprKind::Assign(a, b) => match a.kind {
            ExprKind::Local(s) => Some(ExprKind::AssignLocal(s, std::mem::replace(b, Box::new(Expr::new(ExprKind::Lit(Value::Unit), 0, 0))))),
            _ => None,
        },
        ExprKind::OpAssign(op, a, b) => match a.kind {
            ExprKind::Local(s) => {
                Some(ExprKind::OpAssignLocal(*op, s, std::mem::replace(b, Box::new(Expr::new(ExprKind::Lit(Value::Unit), 0, 0)))))
            }
            _ => None,
        },
        _ => None,
    };
    if let Some(k) = new {
        e.kind = k;
    }
}

fn global_res(g: Global) -> Res {
    match g {
        Global::Fn(id) => Res::Fn(id),
        Global::Struct(id) => Res::Struct(id),
        Global::Enum(_) => Res::Value(Value::Unit),
        Global::Variant(v) => Res::Variant(v),
        Global::Const(c) => Res::Const(c),
        Global::Static(s) => Res::Static(s),
    }
}

const HINTED_METHODS: &[&str] =
    &["collect", "parse", "sum", "product", "into", "try_into", "default", "unwrap_or_default", "from_iter", "pow"];

/// Propagate an expected type into expressions whose behaviour depends on it.
pub fn apply_hint(e: &mut Expr, t: &Rc<Type>) {
    match &mut e.kind {
        ExprKind::MethodCall(m) => {
            if HINTED_METHODS.contains(&m.name.as_str()) && m.hint.is_none() {
                m.hint = Some(t.clone());
            }
            if matches!(m.name.as_str(), "unwrap" | "expect" | "unwrap_or" | "unwrap_or_else" | "unwrap_or_default") {
                let wrapped = Rc::new(Type::new("Result", vec![(**t).clone()]));
                apply_hint(&mut m.recv, &wrapped);
            }
        }
        ExprKind::Try(inner) => {
            let wrapped = Rc::new(Type::new("Result", vec![(**t).clone()]));
            apply_hint(inner, &wrapped);
        }
        ExprKind::CallBuiltin(_, _, h) => {
            if h.is_none() {
                *h = Some(t.clone());
            }
        }
        ExprKind::Block(b) => {
            if let Some(tail) = &mut b.tail {
                apply_hint(tail, t);
            }
        }
        ExprKind::If(b) => {
            if let Some(tail) = &mut b.1.tail {
                apply_hint(tail, t);
            }
            if let Some(e) = &mut b.2 {
                apply_hint(e, t);
            }
        }
        ExprKind::Match(b) => {
            for arm in b.1.iter_mut() {
                apply_hint(&mut arm.body, t);
            }
        }
        ExprKind::Lit(Value::Int(i)) if matches!(t.name.as_str(), "f64" | "f32") => {
            e.kind = ExprKind::Lit(Value::Float(*i as f64));
        }
        _ => {}
    }
}
