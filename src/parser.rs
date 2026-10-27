//! Recursive-descent / Pratt parser for the supported Rust subset.

use crate::ast::*;
use crate::format::{parse_format_string, RawPiece};
use crate::lexer::{punct, Tok, Token};
use crate::value::Value;
use std::cell::Cell;
use std::rc::Rc;

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

type PResult<T> = Result<T, String>;

const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static", "struct", "super", "trait",
    "true", "type", "unsafe", "use", "where", "while", "dyn", "async", "await",
];

pub fn parse_program(toks: Vec<Token>) -> PResult<Vec<Item>> {
    let mut p = Parser { toks, pos: 0 };
    let mut items = Vec::new();
    while !p.at_eof() {
        if let Some(it) = p.parse_item()? {
            items.push(it);
        }
    }
    Ok(items)
}

impl Parser {
    // ---------------------------------------------------------------- helpers
    fn tok(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn tok_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }
    fn at_eof(&self) -> bool {
        matches!(self.tok(), Tok::Eof)
    }
    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }
    fn col(&self) -> u32 {
        self.toks[self.pos].col
    }
    fn advance(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn err<T>(&self, msg: &str) -> PResult<T> {
        Err(format!("{}:{}: {} (found {})", self.line(), self.col(), msg, self.describe()))
    }
    fn describe(&self) -> String {
        match self.tok() {
            Tok::Ident(s) => format!("`{}`", s),
            Tok::Punct(p) => format!("`{}`", p),
            Tok::Eof => "end of file".into(),
            t => format!("{:?}", t),
        }
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.tok(), Tok::Punct(q) if *q == p)
    }
    fn is_at(&self, n: usize, p: &str) -> bool {
        matches!(self.tok_at(n), Tok::Punct(q) if *q == p)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> PResult<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(&format!("expected `{}`", p))
        }
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.tok(), Tok::Ident(s) if s == k)
    }
    fn is_kw_at(&self, n: usize, k: &str) -> bool {
        matches!(self.tok_at(n), Tok::Ident(s) if s == k)
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, k: &str) -> PResult<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            self.err(&format!("expected `{}`", k))
        }
    }
    fn ident(&mut self) -> PResult<String> {
        match self.tok().clone() {
            Tok::Ident(s) => {
                self.advance();
                Ok(s)
            }
            _ => self.err("expected identifier"),
        }
    }
    fn is_ident(&self) -> bool {
        matches!(self.tok(), Tok::Ident(s) if !KEYWORDS.contains(&s.as_str()) || s == "self" || s == "crate" || s == "super")
    }
    /// Split a compound token (`>>`, `>=`, `>>=`, `&&`, `||`) consuming its
    /// first character only.
    fn split_first(&mut self, first: &'static str) -> bool {
        let rest = match self.tok() {
            Tok::Punct(p) if p.len() > 1 && p.starts_with(first) => &p[first.len()..],
            _ => return false,
        };
        let rest = punct(rest);
        self.toks[self.pos].tok = Tok::Punct(rest);
        self.toks[self.pos].col += 1;
        true
    }
    fn expect_gt(&mut self) -> PResult<()> {
        if self.eat(">") || self.split_first(">") {
            Ok(())
        } else {
            self.err("expected `>`")
        }
    }

    // ------------------------------------------------------------------ items
    fn skip_attr(&mut self) -> PResult<Vec<String>> {
        // at `#`
        self.advance();
        self.eat("!");
        self.expect("[")?;
        let mut derives = Vec::new();
        if self.is_kw("derive") {
            self.advance();
            self.expect("(")?;
            while !self.is(")") {
                let mut name = self.ident()?;
                while self.eat("::") {
                    name = self.ident()?;
                }
                derives.push(name);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
            self.expect("]")?;
            return Ok(derives);
        }
        let mut depth = 1;
        while depth > 0 {
            if self.at_eof() {
                return self.err("unterminated attribute");
            }
            match self.advance() {
                Tok::Punct("[") => depth += 1,
                Tok::Punct("]") => depth -= 1,
                _ => {}
            }
        }
        Ok(derives)
    }

    fn skip_vis(&mut self) {
        if self.eat_kw("pub") {
            if self.is("(") {
                let mut depth = 0;
                loop {
                    match self.advance() {
                        Tok::Punct("(") => depth += 1,
                        Tok::Punct(")") => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Tok::Eof => break,
                        _ => {}
                    }
                }
            }
        }
    }

    fn is_item_start(&self) -> bool {
        let mut n = 0;
        if self.is_kw("pub") {
            return true;
        }
        if self.is("#") {
            return true;
        }
        while self.is_kw_at(n, "unsafe") || self.is_kw_at(n, "async") || self.is_kw_at(n, "extern") && matches!(self.tok_at(n + 1), Tok::Str(_)) {
            n += if self.is_kw_at(n, "extern") { 2 } else { 1 };
        }
        match self.tok_at(n) {
            Tok::Ident(s) => match s.as_str() {
                "fn" | "struct" | "enum" | "trait" | "impl" | "mod" | "use" | "static" => true,
                "type" => true,
                "const" => !self.is_at(n + 1, "{"),
                "extern" => true,
                "union" => matches!(self.tok_at(n + 1), Tok::Ident(_)),
                "macro_rules" => self.is_at(n + 1, "!"),
                _ => false,
            },
            _ => false,
        }
    }

    pub fn parse_item(&mut self) -> PResult<Option<Item>> {
        let mut derives = Vec::new();
        while self.is("#") {
            derives.extend(self.skip_attr()?);
        }
        if self.at_eof() || self.is("}") {
            return Ok(None);
        }
        self.skip_vis();
        if self.is_kw("macro_rules") {
            return self.err("macro_rules! is not supported");
        }
        if self.is_kw("const") && !self.is_kw_at(1, "fn") && !self.is_kw_at(1, "unsafe") {
            self.advance();
            return Ok(Some(Item::Const(self.parse_const(false)?)));
        }
        if self.eat_kw("static") {
            return Ok(Some(Item::Const(self.parse_const(true)?)));
        }
        loop {
            if self.eat_kw("const") || self.eat_kw("unsafe") {
                continue;
            }
            if self.is_kw("async") {
                return self.err("async functions are not supported");
            }
            if self.is_kw("extern") {
                if matches!(self.tok_at(1), Tok::Str(_)) {
                    self.advance();
                    self.advance();
                    if self.is("{") {
                        return self.err("extern blocks are not supported");
                    }
                    continue;
                }
                if self.is_kw_at(1, "crate") {
                    while !self.eat(";") {
                        self.advance();
                    }
                    return Ok(None);
                }
            }
            break;
        }
        let kw = match self.tok() {
            Tok::Ident(s) => s.clone(),
            Tok::Punct(";") => {
                self.advance();
                return Ok(None);
            }
            _ => return self.err("expected item"),
        };
        match kw.as_str() {
            "fn" => Ok(Some(Item::Fn(self.parse_fn()?))),
            "struct" | "union" => {
                self.advance();
                Ok(Some(Item::Struct(self.parse_struct(derives)?)))
            }
            "enum" => {
                self.advance();
                Ok(Some(Item::Enum(self.parse_enum(derives)?)))
            }
            "impl" => Ok(Some(Item::Impl(self.parse_impl()?))),
            "trait" => Ok(Some(Item::Trait(self.parse_trait()?))),
            "use" => {
                self.advance();
                let mut out = Vec::new();
                self.parse_use_tree(Vec::new(), &mut out)?;
                self.expect(";")?;
                Ok(Some(Item::Use(out)))
            }
            "mod" => {
                self.advance();
                let name = self.ident()?;
                if self.eat(";") {
                    return Ok(Some(Item::FileMod(name)));
                }
                self.expect("{")?;
                let mut items = Vec::new();
                while self.is("#") && self.is_at(1, "!") {
                    self.skip_attr()?;
                }
                while !self.is("}") {
                    if self.at_eof() {
                        return self.err("unterminated module");
                    }
                    if let Some(it) = self.parse_item()? {
                        items.push(it);
                    }
                }
                self.expect("}")?;
                Ok(Some(Item::Mod(name, items)))
            }
            "type" => {
                self.advance();
                let name = self.ident()?;
                self.skip_generics()?;
                if self.eat(":") {
                    self.parse_bounds()?;
                }
                self.skip_where()?;
                let ty = if self.eat("=") { self.parse_type()? } else { Type::new("_", vec![]) };
                self.expect(";")?;
                Ok(Some(Item::TypeAlias(name, ty)))
            }
            _ => self.err("expected item"),
        }
    }

    fn parse_use_tree(&mut self, prefix: Vec<String>, out: &mut Vec<(String, Vec<String>)>) -> PResult<()> {
        let mut path = prefix;
        self.eat("::");
        loop {
            if self.eat("*") {
                path.push("*".into());
                out.push(("*".into(), path));
                return Ok(());
            }
            if self.is("{") {
                self.advance();
                while !self.is("}") {
                    self.parse_use_tree(path.clone(), out)?;
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("}")?;
                return Ok(());
            }
            let seg = self.ident()?;
            path.push(seg);
            if self.eat("::") {
                continue;
            }
            let mut alias = path.last().unwrap().clone();
            if alias == "self" {
                path.pop();
                alias = path.last().cloned().unwrap_or_default();
            }
            if self.eat_kw("as") {
                alias = self.ident()?;
            }
            out.push((alias, path));
            return Ok(());
        }
    }

    fn skip_generics(&mut self) -> PResult<()> {
        if !self.is("<") {
            return Ok(());
        }
        self.advance();
        let mut depth = 1;
        while depth > 0 {
            if self.at_eof() {
                return self.err("unterminated generics");
            }
            if self.eat("<") {
                depth += 1;
            } else if self.is(">") || self.is(">>") || self.is(">=") || self.is(">>=") {
                self.split_first(">") || {
                    self.advance();
                    true
                };
                depth -= 1;
            } else {
                self.advance();
            }
        }
        Ok(())
    }

    fn skip_where(&mut self) -> PResult<()> {
        if !self.eat_kw("where") {
            return Ok(());
        }
        let mut depth = 0i32;
        loop {
            if self.at_eof() {
                return self.err("unterminated where clause");
            }
            if depth == 0 && (self.is("{") || self.is(";")) {
                return Ok(());
            }
            match self.advance() {
                Tok::Punct("<") | Tok::Punct("(") | Tok::Punct("[") => depth += 1,
                Tok::Punct(">") | Tok::Punct(")") | Tok::Punct("]") => depth -= 1,
                Tok::Punct(">>") => depth -= 2,
                _ => {}
            }
        }
    }

    fn parse_const(&mut self, is_static: bool) -> PResult<ConstDecl> {
        self.eat_kw("mut");
        let name = if self.eat_kw("_") { "_".to_string() } else { self.ident()? };
        let ty = if self.eat(":") { Some(self.parse_type()?) } else { None };
        let init = if self.eat("=") { Some(self.parse_expr()?) } else { None };
        self.expect(";")?;
        Ok(ConstDecl { name, ty, init, is_static })
    }

    fn parse_fn(&mut self) -> PResult<FnDecl> {
        let line = self.line();
        self.expect_kw("fn")?;
        let name = self.ident()?;
        self.skip_generics()?;
        self.expect("(")?;
        let mut params = Vec::new();
        let mut self_kind = SelfKind::None;
        // self parameter
        let save = self.pos;
        if self.is("&") {
            self.advance();
            if let Tok::Lifetime(_) = self.tok() {
                self.advance();
            }
            let m = self.eat_kw("mut");
            if self.eat_kw("self") {
                self_kind = if m { SelfKind::RefMut } else { SelfKind::Ref };
            } else {
                self.pos = save;
            }
        } else if self.is_kw("self") || (self.is_kw("mut") && self.is_kw_at(1, "self")) {
            self.eat_kw("mut");
            self.advance();
            self_kind = SelfKind::Value;
            if self.eat(":") {
                let t = self.parse_type()?;
                if t.name == "&" {
                    self_kind = SelfKind::Ref;
                } else if t.name == "&mut" {
                    self_kind = SelfKind::RefMut;
                }
            }
        }
        if self_kind != SelfKind::None {
            params.push(Param {
                pat: Pat::Bind { name: "self".into(), slot: 0, by_ref: false, mutable: true, sub: None },
                ty: None,
            });
            if !self.eat(",") && !self.is(")") {
                return self.err("expected `,`");
            }
        }
        while !self.is(")") {
            while self.is("#") {
                self.skip_attr()?;
            }
            let pat = self.parse_pat_no_alt()?;
            self.expect(":")?;
            let ty = self.parse_type()?;
            params.push(Param { pat, ty: Some(ty) });
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        let ret = if self.eat("->") { Some(self.parse_type()?) } else { None };
        self.skip_where()?;
        let body = if self.eat(";") { None } else { Some(self.parse_block()?) };
        Ok(FnDecl {
            name,
            params,
            ret,
            body,
            self_kind,
            nslots: 0,
            is_closure: false,
            is_move: false,
            caps: Vec::new(),
            line,
        })
    }

    fn parse_fields(&mut self) -> PResult<Vec<(String, Type)>> {
        self.expect("{")?;
        let mut fields = Vec::new();
        while !self.is("}") {
            while self.is("#") {
                self.skip_attr()?;
            }
            self.skip_vis();
            let name = self.ident()?;
            self.expect(":")?;
            let ty = self.parse_type()?;
            fields.push((name, ty));
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(fields)
    }

    fn parse_tuple_fields(&mut self) -> PResult<Vec<(String, Type)>> {
        self.expect("(")?;
        let mut fields = Vec::new();
        while !self.is(")") {
            while self.is("#") {
                self.skip_attr()?;
            }
            self.skip_vis();
            let ty = self.parse_type()?;
            fields.push((fields.len().to_string(), ty));
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(fields)
    }

    fn parse_struct(&mut self, derives: Vec<String>) -> PResult<StructDecl> {
        let name = self.ident()?;
        self.skip_generics()?;
        self.skip_where()?;
        if self.eat(";") {
            return Ok(StructDecl { name, kind: StructKind::Unit, fields: vec![], derives });
        }
        if self.is("(") {
            let fields = self.parse_tuple_fields()?;
            self.skip_where()?;
            self.expect(";")?;
            return Ok(StructDecl { name, kind: StructKind::Tuple, fields, derives });
        }
        let fields = self.parse_fields()?;
        Ok(StructDecl { name, kind: StructKind::Named, fields, derives })
    }

    fn parse_enum(&mut self, derives: Vec<String>) -> PResult<EnumDecl> {
        let name = self.ident()?;
        self.skip_generics()?;
        self.skip_where()?;
        self.expect("{")?;
        let mut variants = Vec::new();
        while !self.is("}") {
            while self.is("#") {
                self.skip_attr()?;
            }
            let vname = self.ident()?;
            let (kind, fields) = if self.is("(") {
                (StructKind::Tuple, self.parse_tuple_fields()?)
            } else if self.is("{") {
                (StructKind::Named, self.parse_fields()?)
            } else {
                (StructKind::Unit, vec![])
            };
            let disc = if self.eat("=") { Some(self.parse_expr()?) } else { None };
            variants.push(VariantDecl { name: vname, kind, fields, disc });
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(EnumDecl { name, variants, derives })
    }

    fn parse_impl(&mut self) -> PResult<ImplDecl> {
        self.expect_kw("impl")?;
        if self.is("<") {
            self.skip_generics()?;
        }
        self.eat("!");
        let first = self.parse_type()?;
        let (trait_name, self_ty) = if self.eat_kw("for") {
            (Some(first.name.clone()), self.parse_type()?)
        } else {
            (None, first)
        };
        self.skip_where()?;
        self.expect("{")?;
        let mut fns = Vec::new();
        let mut consts = Vec::new();
        while !self.is("}") {
            while self.is("#") {
                self.skip_attr()?;
            }
            self.skip_vis();
            if self.is_kw("const") && !self.is_kw_at(1, "fn") && !self.is_kw_at(1, "unsafe") {
                self.advance();
                consts.push(self.parse_const(false)?);
                continue;
            }
            if self.eat_kw("type") {
                while !self.eat(";") {
                    self.advance();
                }
                continue;
            }
            while self.eat_kw("const") || self.eat_kw("unsafe") {}
            if self.is_kw("async") {
                return self.err("async functions are not supported");
            }
            fns.push(self.parse_fn()?);
        }
        self.expect("}")?;
        Ok(ImplDecl { self_ty, trait_name, fns, consts })
    }

    fn parse_trait(&mut self) -> PResult<TraitDecl> {
        self.expect_kw("trait")?;
        let name = self.ident()?;
        self.skip_generics()?;
        if self.eat(":") {
            self.parse_bounds()?;
        }
        self.skip_where()?;
        self.expect("{")?;
        let mut fns = Vec::new();
        let mut consts = Vec::new();
        while !self.is("}") {
            while self.is("#") {
                self.skip_attr()?;
            }
            if self.is_kw("const") && !self.is_kw_at(1, "fn") {
                self.advance();
                consts.push(self.parse_const(false)?);
                continue;
            }
            if self.eat_kw("type") {
                while !self.eat(";") {
                    self.advance();
                }
                continue;
            }
            while self.eat_kw("const") || self.eat_kw("unsafe") {}
            fns.push(self.parse_fn()?);
        }
        self.expect("}")?;
        Ok(TraitDecl { name, fns, consts })
    }

    // ------------------------------------------------------------------ types
    fn parse_bounds(&mut self) -> PResult<Vec<Type>> {
        let mut out = Vec::new();
        loop {
            if let Tok::Lifetime(_) = self.tok() {
                self.advance();
            } else if self.eat("?") {
                self.parse_type_path()?;
            } else if self.is("(") {
                self.advance();
                out.extend(self.parse_bounds()?);
                self.expect(")")?;
            } else {
                if self.eat_kw("for") {
                    self.skip_generics()?;
                }
                out.push(self.parse_type_path()?);
            }
            if !self.eat("+") {
                break;
            }
        }
        Ok(out)
    }

    pub fn parse_type(&mut self) -> PResult<Type> {
        if self.is("&") || self.is("&&") {
            let double = self.is("&&");
            self.advance();
            if let Tok::Lifetime(_) = self.tok() {
                self.advance();
            }
            let m = self.eat_kw("mut");
            let inner = self.parse_type()?;
            let t = Type::new(if m { "&mut" } else { "&" }, vec![inner]);
            return Ok(if double { Type::new("&", vec![t]) } else { t });
        }
        if self.eat("*") {
            if !self.eat_kw("const") {
                self.expect_kw("mut")?;
            }
            let inner = self.parse_type()?;
            return Ok(Type::new("*", vec![inner]));
        }
        if self.eat("(") {
            let mut elems = Vec::new();
            let mut trailing = false;
            while !self.is(")") {
                elems.push(self.parse_type()?);
                trailing = self.eat(",");
                if !trailing {
                    break;
                }
            }
            self.expect(")")?;
            if elems.len() == 1 && !trailing {
                return Ok(elems.pop().unwrap());
            }
            return Ok(Type::new("tuple", elems));
        }
        if self.eat("[") {
            let elem = self.parse_type()?;
            if self.eat(";") {
                self.parse_expr()?;
                self.expect("]")?;
                return Ok(Type::new("array", vec![elem]));
            }
            self.expect("]")?;
            return Ok(Type::new("slice", vec![elem]));
        }
        if self.eat("!") {
            return Ok(Type::new("!", vec![]));
        }
        if self.eat_kw("_") {
            return Ok(Type::new("_", vec![]));
        }
        if self.is_kw("impl") || self.is_kw("dyn") {
            let k = if self.is_kw("impl") { "impl" } else { "dyn" };
            self.advance();
            let b = self.parse_bounds()?;
            return Ok(Type::new(k, b));
        }
        if self.is_kw("fn") || self.is_kw("unsafe") || self.is_kw("extern") || self.is_kw("for") {
            if self.eat_kw("for") {
                self.skip_generics()?;
            }
            self.eat_kw("unsafe");
            if self.eat_kw("extern") {
                if let Tok::Str(_) = self.tok() {
                    self.advance();
                }
            }
            if self.is_kw("fn") {
                self.advance();
                self.expect("(")?;
                let mut args = Vec::new();
                while !self.is(")") {
                    if matches!(self.tok(), Tok::Ident(_)) && self.is_at(1, ":") && !self.is_at(1, "::") {
                        self.advance();
                        self.advance();
                    }
                    args.push(self.parse_type()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(")")?;
                if self.eat("->") {
                    args.push(self.parse_type()?);
                }
                return Ok(Type::new("fn", args));
            }
            return self.parse_type();
        }
        if self.is("<") {
            // qualified path <T as Trait>::Name
            self.advance();
            let t = self.parse_type()?;
            if self.eat_kw("as") {
                self.parse_type()?;
            }
            self.expect_gt()?;
            let mut name = t.name.clone();
            while self.eat("::") {
                name = self.ident()?;
            }
            return Ok(Type::new(&name, vec![]));
        }
        self.parse_type_path()
    }

    fn parse_type_path(&mut self) -> PResult<Type> {
        self.eat("::");
        let mut name = self.ident()?;
        let mut args = Vec::new();
        loop {
            if self.is("<") || (self.is("::") && self.is_at(1, "<")) {
                self.eat("::");
                self.advance();
                args.clear();
                while !self.is(">") && !self.is(">>") && !self.is(">=") && !self.is(">>=") {
                    if let Tok::Lifetime(_) = self.tok() {
                        self.advance();
                    } else if matches!(self.tok(), Tok::Ident(_)) && self.is_at(1, "=") {
                        self.advance();
                        self.advance();
                        args.push(self.parse_type()?);
                    } else if matches!(self.tok(), Tok::Ident(_)) && self.is_at(1, ":") && !self.is_at(1, "::") {
                        self.advance();
                        self.advance();
                        self.parse_bounds()?;
                    } else if matches!(self.tok(), Tok::Int(..)) || self.is("{") || self.is("-") {
                        self.parse_primary()?;
                    } else {
                        args.push(self.parse_type()?);
                    }
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect_gt()?;
            } else if self.is("(") && matches!(name.as_str(), "Fn" | "FnMut" | "FnOnce") {
                self.advance();
                while !self.is(")") {
                    args.push(self.parse_type()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(")")?;
                if self.eat("->") {
                    args.push(self.parse_type()?);
                }
            } else if self.is("::") && matches!(self.tok_at(1), Tok::Ident(_)) {
                self.advance();
                name = self.ident()?;
                args.clear();
            } else {
                break;
            }
        }
        Ok(Type { name, args })
    }

    // -------------------------------------------------------------- patterns
    pub fn parse_pat(&mut self) -> PResult<Pat> {
        self.eat("|");
        let first = self.parse_pat_no_alt()?;
        if !self.is("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat("|") {
            alts.push(self.parse_pat_no_alt()?);
        }
        Ok(Pat::Or(alts))
    }

    fn lit_value(&mut self, neg: bool) -> PResult<Option<Value>> {
        let v = match self.tok().clone() {
            Tok::Int(v, _) => {
                let v = v as i64;
                Value::Int(if neg { v.wrapping_neg() } else { v })
            }
            Tok::Float(s, _) => {
                let f: f64 = s.parse().map_err(|_| "bad float")?;
                Value::Float(if neg { -f } else { f })
            }
            Tok::Char(c) if !neg => Value::Char(c),
            Tok::Byte(b) if !neg => Value::Int(b as i64),
            Tok::Str(s) if !neg => Value::str(&s),
            Tok::Ident(s) if !neg && (s == "true" || s == "false") => Value::Bool(s == "true"),
            _ => return Ok(None),
        };
        self.advance();
        Ok(Some(v))
    }

    fn range_pat_end(&mut self) -> PResult<Option<Bound>> {
        let neg = self.eat("-");
        if let Some(v) = self.lit_value(neg)? {
            return Ok(Some(Bound::V(v)));
        }
        if self.is_ident() {
            let (line, col) = (self.line(), self.col());
            let mut segs = vec![self.ident()?];
            while self.eat("::") {
                segs.push(self.ident()?);
            }
            return Ok(Some(Bound::E(Expr::new(ExprKind::Path(Box::new(PathExpr { segs, generics: vec![] })), line, col))));
        }
        Ok(None)
    }



    fn parse_pat_no_alt(&mut self) -> PResult<Pat> {
        if self.is_kw("_") {
            self.advance();
            return Ok(Pat::Wild);
        }
        if self.is("..") {
            self.advance();
            return Ok(Pat::Rest);
        }
        if self.is("..=") {
            self.advance();
            let hi = self.range_pat_end()?;
            return Ok(range_pat(None, hi, true));
        }
        if self.is("&") || self.is("&&") {
            let double = self.is("&&");
            self.advance();
            self.eat_kw("mut");
            let inner = self.parse_pat_no_alt()?;
            let p = Pat::Ref(Box::new(inner));
            return Ok(if double { Pat::Ref(Box::new(p)) } else { p });
        }
        if self.eat("(") {
            let mut elems = Vec::new();
            let mut trailing = false;
            while !self.is(")") {
                elems.push(self.parse_pat()?);
                trailing = self.eat(",");
                if !trailing {
                    break;
                }
            }
            self.expect(")")?;
            if elems.len() == 1 && !trailing {
                return Ok(elems.pop().unwrap());
            }
            return Ok(Pat::Tuple(elems));
        }
        if self.eat("[") {
            let mut elems = Vec::new();
            while !self.is("]") {
                elems.push(self.parse_pat()?);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("]")?;
            return Ok(Pat::Slice(elems));
        }
        let neg = self.is("-");
        if neg {
            self.advance();
        }
        if let Some(v) = self.lit_value(neg)? {
            if self.eat("..=") || self.eat("...") {
                let hi = self.range_pat_end()?;
                return Ok(range_pat(Some(Bound::V(v)), hi, true));
            }
            if self.eat("..") {
                let hi = self.range_pat_end()?;
                return Ok(range_pat(Some(Bound::V(v)), hi, false));
            }
            return Ok(Pat::Lit(v));
        }
        if neg {
            return self.err("expected literal after `-` in pattern");
        }
        if self.is_kw("box") {
            self.advance();
            return self.parse_pat_no_alt();
        }
        // binding or path
        let by_ref = self.eat_kw("ref");
        let mutable = self.eat_kw("mut");
        if by_ref || mutable {
            let name = self.ident()?;
            let sub = if self.eat("@") { Some(Box::new(self.parse_pat_no_alt()?)) } else { None };
            return Ok(Pat::Bind { name, slot: 0, by_ref, mutable, sub });
        }
        let mut path = Vec::new();
        if self.eat("<") {
            let t = self.parse_type()?;
            self.expect_gt()?;
            path.push(t.name);
            self.expect("::")?;
        }
        self.eat("::");
        path.push(self.ident()?);
        while self.is("::") {
            self.advance();
            if self.is("<") {
                self.skip_generics()?;
                continue;
            }
            path.push(self.ident()?);
        }
        if self.is("<") && path.len() >= 1 {
            self.skip_generics()?;
        }
        if self.eat("(") {
            let mut elems = Vec::new();
            while !self.is(")") {
                elems.push(self.parse_pat()?);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
            return Ok(Pat::TupleStruct { path, target: Target::Unresolved, elems });
        }
        if self.is("{") {
            self.advance();
            let mut fields = Vec::new();
            let mut rest = false;
            while !self.is("}") {
                if self.eat("..") {
                    rest = true;
                    break;
                }
                let by_ref = self.eat_kw("ref");
                let mutable = self.eat_kw("mut");
                let fname = match self.tok().clone() {
                    Tok::Int(v, _) => {
                        self.advance();
                        v.to_string()
                    }
                    _ => self.ident()?,
                };
                let p = if !by_ref && !mutable && self.eat(":") {
                    self.parse_pat()?
                } else {
                    Pat::Bind { name: fname.clone(), slot: 0, by_ref, mutable, sub: None }
                };
                fields.push((fname, 0, p));
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}")?;
            return Ok(Pat::Struct { path, target: Target::Unresolved, fields, rest });
        }
        if path.len() == 1 && self.eat("@") {
            let sub = self.parse_pat_no_alt()?;
            return Ok(Pat::Bind { name: path.pop().unwrap(), slot: 0, by_ref: false, mutable: false, sub: Some(Box::new(sub)) });
        }
        if self.is("..=") || self.is("...") || (self.is("..") && !self.is_at(1, ")") && !self.is_at(1, "]") && !self.is_at(1, ",")) {
            let incl = !self.is("..");
            self.advance();
            let (line, col) = (self.line(), self.col());
            let lo = Bound::E(Expr::new(ExprKind::Path(Box::new(PathExpr { segs: path, generics: vec![] })), line, col));
            let hi = self.range_pat_end()?;
            return Ok(range_pat(Some(lo), hi, incl));
        }
        if path.len() == 1 {
            return Ok(Pat::Bind { name: path.pop().unwrap(), slot: 0, by_ref: false, mutable: false, sub: None });
        }
        Ok(Pat::Path { path, value: None })
    }

    // ----------------------------------------------------------- statements
    pub fn parse_block(&mut self) -> PResult<Block> {
        self.expect("{")?;
        let mut stmts = Vec::new();
        let mut items = Vec::new();
        let mut tail = None;
        while self.is("#") && self.is_at(1, "!") {
            self.skip_attr()?;
        }
        loop {
            if self.eat("}") {
                break;
            }
            if self.at_eof() {
                return self.err("unterminated block");
            }
            if self.eat(";") {
                continue;
            }
            // attributes on statements
            if self.is("#") {
                let save = self.pos;
                while self.is("#") {
                    self.skip_attr()?;
                }
                if !self.is_item_start() {
                    // attribute on a statement/expression: ignore it
                    let _ = save;
                    continue;
                }
                self.pos = save;
            }
            if self.is_item_start() {
                if let Some(it) = self.parse_item()? {
                    items.push(it);
                }
                continue;
            }
            if self.is_kw("let") {
                self.advance();
                let pat = self.parse_pat()?;
                let ty = if self.eat(":") { Some(self.parse_type()?) } else { None };
                let init = if self.eat("=") { Some(self.parse_expr()?) } else { None };
                let els = if self.eat_kw("else") { Some(self.parse_block()?) } else { None };
                self.expect(";")?;
                stmts.push(Stmt::Let(Box::new(LetStmt { pat, ty, init, els })));
                continue;
            }
            let block_like = self.is_block_like_start();
            let e = if block_like {
                let e = self.parse_block_like()?;
                if self.is(".") || self.is("?") {
                    let e = self.parse_postfix_from(e)?;
                    self.parse_bp_from(e, 0, true)?
                } else {
                    e
                }
            } else {
                self.parse_expr()?
            };
            if self.eat(";") {
                stmts.push(Stmt::Expr(e, true));
            } else if self.eat("}") {
                tail = Some(Box::new(e));
                break;
            } else if block_like {
                stmts.push(Stmt::Expr(e, false));
            } else {
                return self.err("expected `;` or `}`");
            }
        }
        Ok(Block { stmts, tail, label: None, items })
    }

    fn is_block_like_start(&self) -> bool {
        match self.tok() {
            Tok::Punct("{") => true,
            Tok::Ident(s) => match s.as_str() {
                "if" | "match" | "loop" | "while" | "for" => true,
                "unsafe" => self.is_at(1, "{"),
                "const" => self.is_at(1, "{"),
                _ => false,
            },
            Tok::Lifetime(_) => self.is_at(1, ":"),
            _ => false,
        }
    }

    fn parse_block_like(&mut self) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        let mut label = None;
        if let Tok::Lifetime(l) = self.tok().clone() {
            self.advance();
            self.expect(":")?;
            label = Some(intern(&l));
        }
        let kind = match self.tok().clone() {
            Tok::Punct("{") => {
                let mut b = self.parse_block()?;
                b.label = label;
                ExprKind::Block(Box::new(b))
            }
            Tok::Ident(s) => match s.as_str() {
                "unsafe" | "const" => {
                    self.advance();
                    let mut b = self.parse_block()?;
                    b.label = label;
                    ExprKind::Block(Box::new(b))
                }
                "if" => return self.parse_if(),
                "match" => {
                    self.advance();
                    let scrut = self.parse_expr_ns()?;
                    self.expect("{")?;
                    let mut arms = Vec::new();
                    while !self.is("}") {
                        while self.is("#") {
                            self.skip_attr()?;
                        }
                        let pat = self.parse_pat()?;
                        let guard = if self.eat_kw("if") { Some(self.parse_conds()?) } else { None };
                        self.expect("=>")?;
                        let body_block_like = self.is_block_like_start();
                        let body = self.parse_expr()?;
                        if !self.eat(",") && !body_block_like && !self.is("}") {
                            return self.err("expected `,` after match arm");
                        }
                        arms.push(Arm { pat, guard, body });
                    }
                    self.expect("}")?;
                    ExprKind::Match(Box::new((scrut, arms)))
                }
                "loop" => {
                    self.advance();
                    let b = self.parse_block()?;
                    ExprKind::Loop(Box::new((label, b)))
                }
                "while" => {
                    self.advance();
                    let conds = self.parse_conds()?;
                    let b = self.parse_block()?;
                    ExprKind::While(Box::new((label, conds, b)))
                }
                "for" => {
                    self.advance();
                    let pat = self.parse_pat()?;
                    self.expect_kw("in")?;
                    let it = self.parse_expr_ns()?;
                    let b = self.parse_block()?;
                    ExprKind::For(Box::new((label, pat, it, b)))
                }
                _ => return self.err("expected block expression"),
            },
            _ => return self.err("expected block expression"),
        };
        Ok(Expr::new(kind, line, col))
    }

    fn parse_if(&mut self) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        self.expect_kw("if")?;
        let conds = self.parse_conds()?;
        let then = self.parse_block()?;
        let els = if self.eat_kw("else") {
            if self.is_kw("if") {
                Some(self.parse_if()?)
            } else {
                let (l, c) = (self.line(), self.col());
                Some(Expr::new(ExprKind::Block(Box::new(self.parse_block()?)), l, c))
            }
        } else {
            None
        };
        Ok(Expr::new(ExprKind::If(Box::new((conds, then, els))), line, col))
    }

    /// Conditions of `if` / `while` / match guards: `a && let P = e && b`.
    fn parse_conds(&mut self) -> PResult<Vec<Cond>> {
        let e = self.parse_cond_expr()?;
        let mut out = Vec::new();
        flatten_conds(e, &mut out);
        Ok(out)
    }

    fn parse_cond_expr(&mut self) -> PResult<Expr> {
        self.parse_bp(0, false)
    }

    // ----------------------------------------------------------- expressions
    pub fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_bp(0, true)
    }
    fn parse_expr_ns(&mut self) -> PResult<Expr> {
        self.parse_bp(0, false)
    }

    fn binop(&self) -> Option<(BinOp, u8)> {
        let p = match self.tok() {
            Tok::Punct(p) => *p,
            _ => return None,
        };
        Some(match p {
            "||" => (BinOp::Or, 3),
            "&&" => (BinOp::And, 4),
            "==" => (BinOp::Eq, 5),
            "!=" => (BinOp::Ne, 5),
            "<" => (BinOp::Lt, 5),
            ">" => (BinOp::Gt, 5),
            "<=" => (BinOp::Le, 5),
            ">=" => (BinOp::Ge, 5),
            "|" => (BinOp::BitOr, 6),
            "^" => (BinOp::BitXor, 7),
            "&" => (BinOp::BitAnd, 8),
            "<<" => (BinOp::Shl, 9),
            ">>" => (BinOp::Shr, 9),
            "+" => (BinOp::Add, 10),
            "-" => (BinOp::Sub, 10),
            "*" => (BinOp::Mul, 11),
            "/" => (BinOp::Div, 11),
            "%" => (BinOp::Rem, 11),
            _ => return None,
        })
    }

    fn assign_op(&self) -> Option<Option<BinOp>> {
        let p = match self.tok() {
            Tok::Punct(p) => *p,
            _ => return None,
        };
        Some(Some(match p {
            "=" => return Some(None),
            "+=" => BinOp::Add,
            "-=" => BinOp::Sub,
            "*=" => BinOp::Mul,
            "/=" => BinOp::Div,
            "%=" => BinOp::Rem,
            "&=" => BinOp::BitAnd,
            "|=" => BinOp::BitOr,
            "^=" => BinOp::BitXor,
            "<<=" => BinOp::Shl,
            ">>=" => BinOp::Shr,
            _ => return None,
        }))
    }

    fn can_start_expr(&self) -> bool {
        match self.tok() {
            Tok::Eof => false,
            Tok::Punct(p) => matches!(*p, "(" | "[" | "-" | "!" | "*" | "&" | "&&" | "|" | "||" | "<" | "::" | ".." | "#"),
            Tok::Ident(s) => !matches!(s.as_str(), "as" | "else" | "in"),
            _ => true,
        }
    }

    fn parse_bp(&mut self, min: u8, allow_struct: bool) -> PResult<Expr> {
        let lhs = self.parse_unary(allow_struct)?;
        self.parse_bp_from(lhs, min, allow_struct)
    }

    fn parse_bp_from(&mut self, mut lhs: Expr, min: u8, allow_struct: bool) -> PResult<Expr> {
        loop {
            let (line, col) = (lhs.line, lhs.col);
            if let Some(op) = self.assign_op() {
                if min > 1 {
                    break;
                }
                self.advance();
                let rhs = self.parse_bp(1, allow_struct)?;
                lhs = match op {
                    None => Expr::new(ExprKind::Assign(Box::new(lhs), Box::new(rhs)), line, col),
                    Some(op) => Expr::new(ExprKind::OpAssign(op, Box::new(lhs), Box::new(rhs)), line, col),
                };
                continue;
            }
            if self.is("..") || self.is("..=") {
                if min > 2 {
                    break;
                }
                let incl = self.is("..=");
                self.advance();
                let hi = if self.can_start_expr() && !(self.is("{") && !allow_struct) {
                    Some(Box::new(self.parse_bp(3, allow_struct)?))
                } else {
                    None
                };
                lhs = Expr::new(ExprKind::Range(Some(Box::new(lhs)), hi, incl), line, col);
                continue;
            }
            if self.is_kw("as") {
                if min > 12 {
                    break;
                }
                self.advance();
                let ty = self.parse_type_no_bounds()?;
                lhs = Expr::new(ExprKind::Cast(Box::new(lhs), ty), line, col);
                continue;
            }
            let Some((op, prec)) = self.binop() else { break };
            if prec < min {
                break;
            }
            let (oline, ocol) = (self.line(), self.col());
            self.advance();
            // `let` inside conditions: `a && let P = e`
            let rhs = if (op == BinOp::And) && self.is_kw("let") {
                self.parse_let_cond(allow_struct)?
            } else {
                self.parse_bp(prec + 1, allow_struct)?
            };
            lhs = Expr::new(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), oline, ocol);
        }
        Ok(lhs)
    }

    fn parse_type_no_bounds(&mut self) -> PResult<Type> {
        self.parse_type()
    }

    fn parse_let_cond(&mut self, allow_struct: bool) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        self.expect_kw("let")?;
        let pat = self.parse_pat()?;
        self.expect("=")?;
        // the scrutinee binds tighter than `&&` / `||`
        let e = self.parse_bp(5, allow_struct)?;
        Ok(Expr::new(ExprKind::Let(Box::new((pat, e))), line, col))
    }

    fn parse_unary(&mut self, allow_struct: bool) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        if self.is_kw("let") {
            return self.parse_let_cond(allow_struct);
        }
        if self.eat("-") {
            let e = self.parse_unary(allow_struct)?;
            // fold negative literals
            if let ExprKind::Lit(Value::Int(i)) = e.kind {
                return Ok(Expr::new(ExprKind::Lit(Value::Int(i.wrapping_neg())), line, col));
            }
            if let ExprKind::Lit(Value::Float(f)) = e.kind {
                return Ok(Expr::new(ExprKind::Lit(Value::Float(-f)), line, col));
            }
            return Ok(Expr::new(ExprKind::Unary(UnOp::Neg, Box::new(e)), line, col));
        }
        if self.eat("!") {
            let e = self.parse_unary(allow_struct)?;
            return Ok(Expr::new(ExprKind::Unary(UnOp::Not, Box::new(e)), line, col));
        }
        if self.eat("*") {
            let e = self.parse_unary(allow_struct)?;
            return Ok(Expr::new(ExprKind::Deref(Box::new(e)), line, col));
        }
        if self.is("&") || self.is("&&") {
            let double = self.is("&&");
            self.advance();
            if self.is_kw("raw") && (self.is_kw_at(1, "const") || self.is_kw_at(1, "mut")) {
                self.advance();
                self.advance();
            }
            let m = self.eat_kw("mut");
            let e = self.parse_unary(allow_struct)?;
            let r = Expr::new(ExprKind::Ref(m, Box::new(e)), line, col);
            return Ok(if double { Expr::new(ExprKind::Ref(false, Box::new(r)), line, col) } else { r });
        }
        if self.is("..") || self.is("..=") {
            let incl = self.is("..=");
            self.advance();
            let hi = if self.can_start_expr() && !(self.is("{") && !allow_struct) {
                Some(Box::new(self.parse_bp(3, allow_struct)?))
            } else {
                None
            };
            return Ok(Expr::new(ExprKind::Range(None, hi, incl), line, col));
        }
        if self.is("|") || self.is("||") || (self.is_kw("move") && (self.is_at(1, "|") || self.is_at(1, "||"))) {
            return self.parse_closure();
        }
        let e = self.parse_primary_ctx(allow_struct)?;
        self.parse_postfix_from(e)
    }

    fn parse_closure(&mut self) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        let is_move = self.eat_kw("move");
        let mut params = Vec::new();
        if !self.eat("||") {
            self.expect("|")?;
            while !self.is("|") {
                let pat = self.parse_pat_no_alt()?;
                let ty = if self.eat(":") { Some(self.parse_type()?) } else { None };
                params.push(Param { pat, ty });
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("|")?;
        }
        let ret = if self.eat("->") { Some(self.parse_type()?) } else { None };
        let body_expr = self.parse_expr()?;
        let body = match body_expr.kind {
            ExprKind::Block(b) if b.label.is_none() => *b,
            _ => Block { stmts: vec![], tail: Some(Box::new(body_expr)), label: None, items: vec![] },
        };
        let fd = FnDecl {
            name: "<closure>".into(),
            params,
            ret,
            body: Some(body),
            self_kind: SelfKind::None,
            nslots: 0,
            is_closure: true,
            is_move,
            caps: Vec::new(),
            line,
        };
        Ok(Expr::new(ExprKind::Closure(Rc::new(fd)), line, col))
    }

    fn parse_postfix_from(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            let (line, col) = (self.line(), self.col());
            if self.eat("?") {
                e = Expr::new(ExprKind::Try(Box::new(e)), line, col);
            } else if self.is("(") {
                let args = self.parse_args(")")?;
                e = Expr::new(ExprKind::Call(Box::new(e), args), line, col);
            } else if self.eat("[") {
                let idx = self.parse_expr()?;
                self.expect("]")?;
                e = Expr::new(ExprKind::Index(Box::new(e), Box::new(idx)), line, col);
            } else if self.eat(".") {
                match self.tok().clone() {
                    Tok::Int(v, _) => {
                        self.advance();
                        e = Expr::new(ExprKind::TupleField(Box::new(e), v as u32), line, col);
                    }
                    Tok::Float(s, _) => {
                        // `x.0.1` lexes as `x` `.` `0.1`
                        self.advance();
                        for part in s.split('.') {
                            let idx: u32 = part.parse().map_err(|_| format!("{}:{}: bad tuple index", line, col))?;
                            e = Expr::new(ExprKind::TupleField(Box::new(e), idx), line, col);
                        }
                    }
                    Tok::Ident(name) => {
                        self.advance();
                        if name == "await" {
                            return self.err("async/await is not supported");
                        }
                        let mut turbofish = None;
                        if self.is("::") {
                            self.advance();
                            self.expect("<")?;
                            let mut args = Vec::new();
                            while !self.is(">") && !self.is(">>") {
                                args.push(self.parse_type()?);
                                if !self.eat(",") {
                                    break;
                                }
                            }
                            self.expect_gt()?;
                            turbofish = args.into_iter().next();
                        }
                        if self.is("(") {
                            let args = self.parse_args(")")?;
                            let sym = intern(&name);
                            let mid = crate::builtins::method_id(&name);
                            e = Expr::new(
                                ExprKind::MethodCall(Box::new(MethodCall {
                                    recv: e,
                                    name,
                                    sym,
                                    mid,
                                    args,
                                    turbofish,
                                    hint: None,
                                    cache: Cell::new((0, 0)),
                                })),
                                line,
                                col,
                            );
                        } else {
                            e = Expr::new(ExprKind::Field(Box::new(e), intern(&name), Cell::new((u32::MAX, 0))), line, col);
                        }
                    }
                    _ => return self.err("expected field or method name"),
                }
            } else {
                return Ok(e);
            }
        }
    }

    fn parse_args(&mut self, close: &str) -> PResult<Vec<Expr>> {
        self.advance(); // open delimiter
        let mut args = Vec::new();
        while !self.is(close) {
            args.push(self.parse_expr()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(close)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        self.parse_primary_ctx(true)
    }

    fn looks_like_struct_lit(&self) -> bool {
        // after path, at `{`
        match (self.tok_at(1), self.tok_at(2)) {
            (Tok::Punct("}"), _) => true,
            (Tok::Punct(".."), _) => true,
            (Tok::Ident(_), Tok::Punct(":")) => true,
            (Tok::Ident(_), Tok::Punct(",")) => true,
            (Tok::Ident(_), Tok::Punct("}")) => true,
            (Tok::Int(..), Tok::Punct(":")) => true,
            _ => false,
        }
    }

    fn parse_primary_ctx(&mut self, allow_struct: bool) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        let mk = |k| Expr::new(k, line, col);
        match self.tok().clone() {
            Tok::Int(v, suffix) => {
                self.advance();
                if matches!(suffix.as_deref(), Some("f32") | Some("f64")) {
                    return Ok(mk(ExprKind::Lit(Value::Float(v as f64))));
                }
                Ok(mk(ExprKind::Lit(Value::Int(v as i64))))
            }
            Tok::Float(s, _) => {
                self.advance();
                let f: f64 = s.parse().map_err(|_| format!("{}:{}: bad float literal", line, col))?;
                Ok(mk(ExprKind::Lit(Value::Float(f))))
            }
            Tok::Str(s) => {
                self.advance();
                Ok(mk(ExprKind::Lit(Value::str(&s))))
            }
            Tok::ByteStr(b) => {
                self.advance();
                let elems = b.iter().map(|&x| mk(ExprKind::Lit(Value::Int(x as i64)))).collect();
                Ok(mk(ExprKind::Ref(false, Box::new(mk(ExprKind::Array(elems))))))
            }
            Tok::Char(c) => {
                self.advance();
                Ok(mk(ExprKind::Lit(Value::Char(c))))
            }
            Tok::Byte(b) => {
                self.advance();
                Ok(mk(ExprKind::Lit(Value::Int(b as i64))))
            }
            Tok::Lifetime(_) => self.parse_block_like(),
            Tok::Punct("(") => {
                self.advance();
                let mut elems = Vec::new();
                let mut trailing = false;
                while !self.is(")") {
                    elems.push(self.parse_expr()?);
                    trailing = self.eat(",");
                    if !trailing {
                        break;
                    }
                }
                self.expect(")")?;
                if elems.len() == 1 && !trailing {
                    return Ok(elems.pop().unwrap());
                }
                if elems.is_empty() {
                    return Ok(mk(ExprKind::Lit(Value::Unit)));
                }
                Ok(mk(ExprKind::Tuple(elems)))
            }
            Tok::Punct("[") => {
                self.advance();
                if self.eat("]") {
                    return Ok(mk(ExprKind::Array(vec![])));
                }
                let first = self.parse_expr()?;
                if self.eat(";") {
                    let n = self.parse_expr()?;
                    self.expect("]")?;
                    return Ok(mk(ExprKind::Repeat(Box::new(first), Box::new(n))));
                }
                let mut elems = vec![first];
                while self.eat(",") {
                    if self.is("]") {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                }
                self.expect("]")?;
                Ok(mk(ExprKind::Array(elems)))
            }
            Tok::Punct("{") => self.parse_block_like(),
            Tok::Punct("<") | Tok::Punct("::") => self.parse_path_expr(allow_struct),
            Tok::Ident(s) => match s.as_str() {
                "true" | "false" => {
                    self.advance();
                    Ok(mk(ExprKind::Lit(Value::Bool(s == "true"))))
                }
                "if" | "match" | "loop" | "while" | "for" => self.parse_block_like(),
                "unsafe" | "const" if self.is_at(1, "{") => self.parse_block_like(),
                "return" => {
                    self.advance();
                    let v = if self.can_start_expr() && !self.is("}") { Some(Box::new(self.parse_expr()?)) } else { None };
                    Ok(mk(ExprKind::Return(v)))
                }
                "break" => {
                    self.advance();
                    let label = if let Tok::Lifetime(l) = self.tok().clone() {
                        self.advance();
                        Some(intern(&l))
                    } else {
                        None
                    };
                    let v = if self.can_start_expr() && !self.is("}") && !(self.is("{") && !allow_struct) {
                        Some(Box::new(self.parse_bp(0, allow_struct)?))
                    } else {
                        None
                    };
                    Ok(mk(ExprKind::Break(label, v)))
                }
                "continue" => {
                    self.advance();
                    let label = if let Tok::Lifetime(l) = self.tok().clone() {
                        self.advance();
                        Some(intern(&l))
                    } else {
                        None
                    };
                    Ok(mk(ExprKind::Continue(label)))
                }
                "async" => self.err("async blocks are not supported"),
                _ => self.parse_path_expr(allow_struct),
            },
            _ => self.err("expected expression"),
        }
    }

    fn parse_path_expr(&mut self, allow_struct: bool) -> PResult<Expr> {
        let (line, col) = (self.line(), self.col());
        let mut segs = Vec::new();
        let mut generics = Vec::new();
        if self.eat("<") {
            // <T as Trait>::item  or <T>::item
            let t = self.parse_type()?;
            if self.eat_kw("as") {
                let tr = self.parse_type()?;
                segs.push(format!("<{}>", t.name));
                segs.push(tr.name);
            } else {
                segs.push(t.name);
            }
            self.expect_gt()?;
            self.expect("::")?;
        } else {
            self.eat("::");
        }
        segs.push(self.ident()?);
        loop {
            if self.is("::") && self.is_at(1, "<") {
                self.advance();
                self.advance();
                while !self.is(">") && !self.is(">>") {
                    generics.push(self.parse_type()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect_gt()?;
            } else if self.is("::") && matches!(self.tok_at(1), Tok::Ident(_)) {
                self.advance();
                segs.push(self.ident()?);
            } else {
                break;
            }
        }
        // macro invocation
        if self.is("!") && segs.len() == 1 && (self.is_at(1, "(") || self.is_at(1, "[") || self.is_at(1, "{")) {
            self.advance();
            return self.parse_macro(&segs[0], line, col);
        }
        if self.is("{") && allow_struct && self.looks_like_struct_lit() {
            return self.parse_struct_lit(segs, line, col);
        }
        Ok(Expr::new(ExprKind::Path(Box::new(PathExpr { segs, generics })), line, col))
    }

    fn parse_struct_lit(&mut self, path: Vec<String>, line: u32, col: u32) -> PResult<Expr> {
        self.expect("{")?;
        let mut fields = Vec::new();
        let mut base = None;
        while !self.is("}") {
            if self.eat("..") {
                base = Some(self.parse_expr()?);
                break;
            }
            let (fl, fc) = (self.line(), self.col());
            let name = match self.tok().clone() {
                Tok::Int(v, _) => {
                    self.advance();
                    v.to_string()
                }
                _ => self.ident()?,
            };
            let value = if self.eat(":") {
                self.parse_expr()?
            } else {
                Expr::new(ExprKind::Path(Box::new(PathExpr { segs: vec![name.clone()], generics: vec![] })), fl, fc)
            };
            fields.push((name, 0, value));
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(Expr::new(ExprKind::StructLit(Box::new(StructLit { path, target: Target::Unresolved, fields, base })), line, col))
    }

    // ---------------------------------------------------------------- macros
    fn parse_macro(&mut self, name: &str, line: u32, col: u32) -> PResult<Expr> {
        let close = match self.tok() {
            Tok::Punct("(") => ")",
            Tok::Punct("[") => "]",
            _ => "}",
        };
        let mk = |k| Expr::new(k, line, col);
        match name {
            "vec" => {
                self.advance();
                if self.eat(close) {
                    return Ok(mk(ExprKind::VecLit(vec![])));
                }
                let first = self.parse_expr()?;
                if self.eat(";") {
                    let n = self.parse_expr()?;
                    self.expect(close)?;
                    return Ok(mk(ExprKind::VecRepeat(Box::new(first), Box::new(n))));
                }
                let mut elems = vec![first];
                while self.eat(",") {
                    if self.is(close) {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                }
                self.expect(close)?;
                Ok(mk(ExprKind::VecLit(elems)))
            }
            "matches" => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(",")?;
                let pat = self.parse_pat()?;
                let guard = if self.eat_kw("if") { Some(self.parse_expr()?) } else { None };
                self.eat(",");
                self.expect(close)?;
                Ok(mk(ExprKind::Matches(Box::new((e, pat, guard)))))
            }
            "println" | "print" | "eprintln" | "eprint" | "format" | "panic" | "write" | "writeln" | "assert"
            | "assert_eq" | "assert_ne" | "debug_assert" | "debug_assert_eq" | "debug_assert_ne" | "unreachable"
            | "todo" | "unimplemented" | "dbg" | "format_args" => {
                let kind = match name {
                    "println" => MacroKind::Println,
                    "print" => MacroKind::Print,
                    "eprintln" => MacroKind::Eprintln,
                    "eprint" => MacroKind::Eprint,
                    "format" | "format_args" => MacroKind::Format,
                    "panic" => MacroKind::Panic,
                    "write" => MacroKind::Write,
                    "writeln" => MacroKind::Writeln,
                    "assert" | "debug_assert" => MacroKind::Assert,
                    "assert_eq" | "debug_assert_eq" => MacroKind::AssertEq,
                    "assert_ne" | "debug_assert_ne" => MacroKind::AssertNe,
                    "unreachable" => MacroKind::Unreachable,
                    "todo" => MacroKind::Todo,
                    "unimplemented" => MacroKind::Unimplemented,
                    _ => MacroKind::Dbg,
                };
                let start = self.pos;
                self.advance();
                let n_operands = match kind {
                    MacroKind::Write | MacroKind::Writeln | MacroKind::Assert => 1,
                    MacroKind::AssertEq | MacroKind::AssertNe => 2,
                    MacroKind::Dbg => 99,
                    _ => 0,
                };
                let mut operands = Vec::new();
                let mut text = String::new();
                while operands.len() < n_operands && !self.is(close) {
                    let s = self.pos;
                    operands.push(self.parse_expr()?);
                    text = self.source_text(s, self.pos);
                    if !self.eat(",") {
                        break;
                    }
                }
                let mut pieces = None;
                let mut args = Vec::new();
                if !self.is(close) {
                    let fmt = match self.tok().clone() {
                        Tok::Str(s) => s,
                        _ => return self.err("expected format string literal"),
                    };
                    self.advance();
                    let mut named: Vec<(String, usize)> = Vec::new();
                    while self.eat(",") {
                        if self.is(close) {
                            break;
                        }
                        if let Tok::Ident(n) = self.tok().clone() {
                            if self.is_at(1, "=") {
                                self.advance();
                                self.advance();
                                named.push((n, args.len()));
                                args.push(self.parse_expr()?);
                                continue;
                            }
                        }
                        args.push(self.parse_expr()?);
                    }
                    let raw = parse_format_string(&fmt).map_err(|e| format!("{}:{}: {}", line, col, e))?;
                    let mut out = Vec::new();
                    let mut next_pos = 0usize;
                    let n_positional = args.len() - named.len();
                    let mut resolve = |name: &Option<String>, args: &mut Vec<Expr>, implicit: bool| -> PResult<usize> {
                        match name {
                            None => {
                                if !implicit {
                                    unreachable!()
                                }
                                let i = next_pos;
                                next_pos += 1;
                                if i >= n_positional {
                                    return Err(format!("{}:{}: missing format argument", line, col));
                                }
                                Ok(i)
                            }
                            Some(n) => {
                                if let Ok(i) = n.parse::<usize>() {
                                    return Ok(i);
                                }
                                if let Some((_, i)) = named.iter().find(|(m, _)| m == n) {
                                    return Ok(*i);
                                }
                                // implicit capture of a variable
                                named.push((n.clone(), args.len()));
                                args.push(Expr::new(
                                    ExprKind::Path(Box::new(PathExpr { segs: vec![n.clone()], generics: vec![] })),
                                    line,
                                    col,
                                ));
                                Ok(args.len() - 1)
                            }
                        }
                    };
                    for p in raw {
                        match p {
                            RawPiece::Lit(s) => out.push(FmtPiece::Lit(s)),
                            RawPiece::Arg { name, mut spec, width_name, prec_name, prec_star } => {
                                if prec_star {
                                    let i = resolve(&None, &mut args, true)?;
                                    spec.precision = Some(crate::format::Count::Arg(i));
                                }
                                let arg = resolve(&name, &mut args, true)?;
                                if let Some(w) = width_name {
                                    let i = resolve(&Some(w), &mut args, false)?;
                                    spec.width = Some(crate::format::Count::Arg(i));
                                }
                                if let Some(w) = prec_name {
                                    let i = resolve(&Some(w), &mut args, false)?;
                                    spec.precision = Some(crate::format::Count::Arg(i));
                                }
                                out.push(FmtPiece::Arg { arg, spec });
                            }
                        }
                    }
                    pieces = Some(out);
                }
                self.expect(close)?;
                if kind != MacroKind::Dbg {
                    text = self.source_text(start + 1, self.pos - 1);
                }
                Ok(mk(ExprKind::Macro(Box::new(MacroCall { kind, operands, pieces, args, text }))))
            }
            _ => self.err(&format!("unsupported macro `{}!`", name)),
        }
    }

    /// Reconstruct approximate source text of tokens [a, b) (for assert! messages).
    fn source_text(&self, a: usize, b: usize) -> String {
        let mut s = String::new();
        let mut prev: Option<&Tok> = None;
        for t in &self.toks[a..b] {
            let txt = match &t.tok {
                Tok::Ident(x) => x.clone(),
                Tok::Lifetime(x) => format!("'{}", x),
                Tok::Int(v, suf) => format!("{}{}", v, suf.clone().unwrap_or_default()),
                Tok::Float(v, suf) => format!("{}{}", v, suf.clone().unwrap_or_default()),
                Tok::Str(x) => format!("{:?}", x),
                Tok::ByteStr(_) => "b\"..\"".into(),
                Tok::Char(c) => format!("{:?}", c),
                Tok::Byte(b) => format!("b{:?}", *b as char),
                Tok::Punct(p) => p.to_string(),
                Tok::Eof => String::new(),
            };
            let tight = matches!(&t.tok, Tok::Punct(p) if matches!(*p, "." | "(" | ")" | "[" | "]" | "," | "::" | "?" | ";"))
                || matches!(prev, Some(Tok::Punct(p)) if matches!(*p, "." | "(" | "[" | "::" | "!" | "&" | "*"))
                || (matches!(&t.tok, Tok::Punct("!")) && matches!(prev, Some(Tok::Ident(_))));
            if !s.is_empty() && !tight {
                s.push(' ');
            }
            if matches!(&t.tok, Tok::Punct(",")) {
                s.push_str(",");
                prev = Some(&t.tok);
                continue;
            }
            s.push_str(&txt);
            prev = Some(&t.tok);
        }
        s.replace(" ,", ",")
    }
}

enum Bound {
    V(Value),
    E(Expr),
}

fn range_pat(lo: Option<Bound>, hi: Option<Bound>, incl: bool) -> Pat {
    let is_expr = |b: &Option<Bound>| matches!(b, Some(Bound::E(_)));
    if is_expr(&lo) || is_expr(&hi) {
        let to_e = |b: Option<Bound>| {
            b.map(|b| match b {
                Bound::E(e) => Box::new(e),
                Bound::V(v) => Box::new(Expr::new(ExprKind::Lit(v), 0, 0)),
            })
        };
        return Pat::RangeExpr(to_e(lo), to_e(hi), incl);
    }
    let to_v = |b: Option<Bound>| {
        b.map(|b| match b {
            Bound::V(v) => v,
            Bound::E(_) => unreachable!(),
        })
    };
    Pat::Range(to_v(lo), to_v(hi), incl)
}

/// Split `a && let P = e && b` into conditions.
fn flatten_conds(e: Expr, out: &mut Vec<Cond>) {
    match e.kind {
        ExprKind::Binary(BinOp::And, l, r) if contains_let(&l) || contains_let(&r) => {
            flatten_conds(*l, out);
            flatten_conds(*r, out);
        }
        ExprKind::Let(b) => {
            let (p, e) = *b;
            out.push(Cond::Let(p, e));
        }
        k => out.push(Cond::Expr(Expr { kind: k, line: e.line, col: e.col })),
    }
}

fn contains_let(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Let(_) => true,
        ExprKind::Binary(BinOp::And, l, r) => contains_let(l) || contains_let(r),
        _ => false,
    }
}
