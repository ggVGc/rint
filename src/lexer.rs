//! Tokenizer for Rust source text (see notes/01 for the lexical grammar).

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Lifetime(String),
    Int(u128, Option<String>),
    Float(String, Option<String>),
    Str(String),
    ByteStr(Vec<u8>),
    Char(char),
    Byte(u8),
    Punct(&'static str),
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

const PUNCTS: &[&str] = &[
    ">>=", "<<=", "...", "..=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=",
    "*=", "/=", "%=", "^=", "&=", "|=", "<<", ">>", "..", "+", "-", "*", "/", "%", "^", "!", "&",
    "|", "=", "<", ">", "@", ".", ",", ";", ":", "#", "$", "?", "{", "}", "[", "]", "(", ")", "~",
];

pub fn punct(s: &str) -> &'static str {
    PUNCTS.iter().find(|p| **p == s).copied().unwrap_or("?")
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut lx = Lexer { src: src.as_bytes(), pos: 0, line: 1, col: 1 };
    // Skip shebang (but not inner attributes `#![...]`).
    if src.starts_with("#!") && !src[2..].trim_start().starts_with('[') {
        while lx.pos < lx.src.len() && lx.src[lx.pos] != b'\n' {
            lx.pos += 1;
        }
    }
    let mut out = Vec::new();
    loop {
        lx.skip_ws()?;
        let (line, col) = (lx.line, lx.col);
        if lx.pos >= lx.src.len() {
            out.push(Token { tok: Tok::Eof, line, col });
            return Ok(out);
        }
        let tok = lx.next_tok()?;
        out.push(Token { tok, line, col });
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}
fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl<'a> Lexer<'a> {
    fn peek(&self, off: usize) -> u8 {
        *self.src.get(self.pos + off).unwrap_or(&0)
    }
    fn bump(&mut self) -> u8 {
        let c = self.src[self.pos];
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if (c & 0xC0) != 0x80 {
            self.col += 1;
        }
        c
    }
    fn err<T>(&self, msg: &str) -> Result<T, String> {
        Err(format!("{}:{}: {}", self.line, self.col, msg))
    }

    fn skip_ws(&mut self) -> Result<(), String> {
        loop {
            let c = self.peek(0);
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.bump();
            } else if c == b'/' && self.peek(1) == b'/' {
                while self.pos < self.src.len() && self.peek(0) != b'\n' {
                    self.bump();
                }
            } else if c == b'/' && self.peek(1) == b'*' {
                self.bump();
                self.bump();
                let mut depth = 1;
                while depth > 0 {
                    if self.pos >= self.src.len() {
                        return self.err("unterminated block comment");
                    }
                    if self.peek(0) == b'/' && self.peek(1) == b'*' {
                        self.bump();
                        self.bump();
                        depth += 1;
                    } else if self.peek(0) == b'*' && self.peek(1) == b'/' {
                        self.bump();
                        self.bump();
                        depth -= 1;
                    } else {
                        self.bump();
                    }
                }
            } else {
                return Ok(());
            }
        }
    }

    fn ident(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.src.len() && is_ident_cont(self.peek(0)) {
            self.bump();
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn next_tok(&mut self) -> Result<Tok, String> {
        let c = self.peek(0);
        // raw strings / byte strings / raw identifiers
        if c == b'r' && (self.peek(1) == b'"' || (self.peek(1) == b'#' && (self.peek(2) == b'"' || self.peek(2) == b'#'))) {
            self.bump();
            let s = self.raw_string()?;
            return Ok(Tok::Str(s));
        }
        if c == b'r' && self.peek(1) == b'#' && is_ident_start(self.peek(2)) {
            self.bump();
            self.bump();
            return Ok(Tok::Ident(self.ident()));
        }
        if c == b'b' && self.peek(1) == b'"' {
            self.bump();
            let s = self.string()?;
            return Ok(Tok::ByteStr(s.into_bytes()));
        }
        if c == b'b' && self.peek(1) == b'r' && (self.peek(2) == b'"' || self.peek(2) == b'#') {
            self.bump();
            self.bump();
            let s = self.raw_string()?;
            return Ok(Tok::ByteStr(s.into_bytes()));
        }
        if c == b'b' && self.peek(1) == b'\'' {
            self.bump();
            self.bump();
            let ch = if self.peek(0) == b'\\' { self.escape()? } else { self.utf8_char() };
            if self.peek(0) != b'\'' {
                return self.err("bad byte literal");
            }
            self.bump();
            return Ok(Tok::Byte(ch as u32 as u8));
        }
        if c == b'c' && self.peek(1) == b'"' {
            return self.err("C string literals are not supported");
        }
        if is_ident_start(c) {
            return Ok(Tok::Ident(self.ident()));
        }
        if c.is_ascii_digit() {
            return self.number();
        }
        if c == b'"' {
            return Ok(Tok::Str(self.string()?));
        }
        if c == b'\'' {
            // char literal or lifetime
            if self.peek(1) == b'\\' {
                self.bump();
                let ch = self.escape()?;
                if self.peek(0) != b'\'' {
                    return self.err("bad char literal");
                }
                self.bump();
                return Ok(Tok::Char(ch));
            }
            // find the end of one UTF-8 char after the quote
            let save = (self.pos, self.line, self.col);
            self.bump();
            let ch = self.utf8_char();
            if self.peek(0) == b'\'' {
                self.bump();
                return Ok(Tok::Char(ch));
            }
            (self.pos, self.line, self.col) = save;
            self.bump();
            if is_ident_start(self.peek(0)) {
                return Ok(Tok::Lifetime(self.ident()));
            }
            return self.err("bad lifetime or char literal");
        }
        for p in PUNCTS {
            let pb = p.as_bytes();
            if self.src[self.pos..].starts_with(pb) {
                for _ in 0..pb.len() {
                    self.bump();
                }
                return Ok(Tok::Punct(p));
            }
        }
        self.err(&format!("unexpected character '{}'", c as char))
    }

    fn utf8_char(&mut self) -> char {
        let rest = &self.src[self.pos..];
        let len = match rest[0] {
            b if b < 0x80 => 1,
            b if b >= 0xF0 => 4,
            b if b >= 0xE0 => 3,
            _ => 2,
        };
        let s = std::str::from_utf8(&rest[..len.min(rest.len())]).unwrap_or("\u{FFFD}");
        let ch = s.chars().next().unwrap_or('\u{FFFD}');
        for _ in 0..len {
            self.bump();
        }
        ch
    }

    fn escape(&mut self) -> Result<char, String> {
        self.bump(); // backslash
        let c = self.bump();
        Ok(match c {
            b'n' => '\n',
            b't' => '\t',
            b'r' => '\r',
            b'0' => '\0',
            b'\\' => '\\',
            b'\'' => '\'',
            b'"' => '"',
            b'x' => {
                let h = [self.bump(), self.bump()];
                let s = std::str::from_utf8(&h).map_err(|e| e.to_string())?;
                u8::from_str_radix(s, 16).map_err(|e| e.to_string())? as char
            }
            b'u' => {
                if self.bump() != b'{' {
                    return self.err("bad unicode escape");
                }
                let mut v = 0u32;
                while self.peek(0) != b'}' {
                    let d = self.bump();
                    if d == b'_' {
                        continue;
                    }
                    v = v * 16 + (d as char).to_digit(16).ok_or("bad unicode escape")?;
                }
                self.bump();
                char::from_u32(v).ok_or("bad unicode escape")?
            }
            _ => return self.err("unknown escape"),
        })
    }

    fn string(&mut self) -> Result<String, String> {
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            if self.pos >= self.src.len() {
                return self.err("unterminated string");
            }
            match self.peek(0) {
                b'"' => {
                    self.bump();
                    return Ok(s);
                }
                b'\\' if self.peek(1) == b'\n' || (self.peek(1) == b'\r' && self.peek(2) == b'\n') => {
                    self.bump();
                    while matches!(self.peek(0), b' ' | b'\t' | b'\n' | b'\r') {
                        self.bump();
                    }
                }
                b'\\' => s.push(self.escape()?),
                b'\r' if self.peek(1) == b'\n' => {
                    self.bump();
                }
                _ => s.push(self.utf8_char()),
            }
        }
    }

    fn raw_string(&mut self) -> Result<String, String> {
        let mut hashes = 0;
        while self.peek(0) == b'#' {
            self.bump();
            hashes += 1;
        }
        if self.bump() != b'"' {
            return self.err("bad raw string");
        }
        let start = self.pos;
        loop {
            if self.pos >= self.src.len() {
                return self.err("unterminated raw string");
            }
            if self.peek(0) == b'"' && (0..hashes).all(|i| self.peek(1 + i) == b'#') {
                let s = String::from_utf8_lossy(&self.src[start..self.pos]).replace("\r\n", "\n");
                self.bump();
                for _ in 0..hashes {
                    self.bump();
                }
                return Ok(s);
            }
            self.bump();
        }
    }

    fn number(&mut self) -> Result<Tok, String> {
        let radix = if self.peek(0) == b'0' {
            match self.peek(1) {
                b'x' => 16,
                b'o' => 8,
                b'b' => 2,
                _ => 10,
            }
        } else {
            10
        };
        if radix != 10 {
            self.bump();
            self.bump();
            let mut v: u128 = 0;
            while self.peek(0).is_ascii_hexdigit() || self.peek(0) == b'_' {
                // a hex suffix like `0xffu8` is ambiguous; only treat [0-9a-f] as digits for hex
                let c = self.peek(0);
                if c == b'_' {
                    self.bump();
                    continue;
                }
                let d = (c as char).to_digit(radix);
                match d {
                    Some(d) => {
                        v = v.wrapping_mul(radix as u128).wrapping_add(d as u128);
                        self.bump();
                    }
                    None => break,
                }
            }
            let suffix = if is_ident_start(self.peek(0)) { Some(self.ident()) } else { None };
            return Ok(Tok::Int(v, suffix));
        }
        let mut text = String::new();
        let mut is_float = false;
        while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
            let c = self.bump();
            if c != b'_' {
                text.push(c as char);
            }
        }
        if self.peek(0) == b'.' && self.peek(1) != b'.' && !is_ident_start(self.peek(1)) {
            is_float = true;
            self.bump();
            text.push('.');
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
                let c = self.bump();
                if c != b'_' {
                    text.push(c as char);
                }
            }
        }
        if (self.peek(0) == b'e' || self.peek(0) == b'E')
            && (self.peek(1).is_ascii_digit() || ((self.peek(1) == b'+' || self.peek(1) == b'-') && self.peek(2).is_ascii_digit()))
        {
            is_float = true;
            text.push(self.bump() as char);
            if self.peek(0) == b'+' || self.peek(0) == b'-' {
                text.push(self.bump() as char);
            }
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
                let c = self.bump();
                if c != b'_' {
                    text.push(c as char);
                }
            }
        }
        let suffix = if is_ident_start(self.peek(0)) { Some(self.ident()) } else { None };
        if let Some(s) = &suffix {
            if s == "f32" || s == "f64" {
                is_float = true;
            }
        }
        if is_float {
            Ok(Tok::Float(text, suffix))
        } else {
            let v: u128 = text.parse().map_err(|_| format!("{}:{}: bad integer literal", self.line, self.col))?;
            Ok(Tok::Int(v, suffix))
        }
    }
}
