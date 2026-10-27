//! `format!`-style format strings: parsing and primitive formatting.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    None,
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Fixed(usize),
    Arg(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FmtTy {
    #[default]
    Display,
    Debug,
    LowerHex,
    UpperHex,
    Binary,
    Octal,
    LowerExp,
    UpperExp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub fill: char,
    pub align: Align,
    pub plus: bool,
    pub alt: bool,
    pub zero: bool,
    pub width: Option<Count>,
    pub precision: Option<Count>,
    pub ty: FmtTy,
}

impl Default for Spec {
    fn default() -> Spec {
        Spec { fill: ' ', align: Align::None, plus: false, alt: false, zero: false, width: None, precision: None, ty: FmtTy::Display }
    }
}

/// A spec whose width/precision have been resolved to concrete numbers.
#[derive(Clone, Debug, Default)]
pub struct RSpec {
    pub fill: char,
    pub align: Align,
    pub plus: bool,
    pub alt: bool,
    pub zero: bool,
    pub width: Option<usize>,
    pub precision: Option<usize>,
    pub ty: FmtTy,
}

impl RSpec {
    pub fn plain(ty: FmtTy) -> RSpec {
        RSpec { fill: ' ', ty, ..Default::default() }
    }
    pub fn is_plain(&self) -> bool {
        self.width.is_none() && self.precision.is_none() && !self.plus && !self.alt
    }
}

pub enum RawPiece {
    Lit(String),
    Arg { name: Option<String>, spec: Spec, width_name: Option<String>, prec_name: Option<String>, prec_star: bool },
}

pub fn parse_format_string(s: &str) -> Result<Vec<RawPiece>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if chars.get(i + 1) == Some(&'{') {
                lit.push('{');
                i += 2;
                continue;
            }
            if !lit.is_empty() {
                out.push(RawPiece::Lit(std::mem::take(&mut lit)));
            }
            let end = chars[i..].iter().position(|&c| c == '}').ok_or("unterminated `{` in format string")? + i;
            let inner: String = chars[i + 1..end].iter().collect();
            out.push(parse_arg(&inner)?);
            i = end + 1;
        } else if c == '}' {
            if chars.get(i + 1) == Some(&'}') {
                lit.push('}');
                i += 2;
                continue;
            }
            return Err("unmatched `}` in format string".into());
        } else {
            lit.push(c);
            i += 1;
        }
    }
    if !lit.is_empty() {
        out.push(RawPiece::Lit(lit));
    }
    Ok(out)
}

fn parse_arg(inner: &str) -> Result<RawPiece, String> {
    let (name, spec_s) = match inner.find(':') {
        Some(p) => (&inner[..p], Some(&inner[p + 1..])),
        None => (inner, None),
    };
    let name = name.trim();
    let name = if name.is_empty() { None } else { Some(name.to_string()) };
    let mut spec = Spec::default();
    let mut width_name = None;
    let mut prec_name = None;
    let mut prec_star = false;
    if let Some(s) = spec_s {
        let c: Vec<char> = s.chars().collect();
        let mut i = 0;
        let is_align = |ch: char| ch == '<' || ch == '>' || ch == '^';
        let to_align = |ch: char| match ch {
            '<' => Align::Left,
            '>' => Align::Right,
            _ => Align::Center,
        };
        if c.len() >= 2 && is_align(c[1]) {
            spec.fill = c[0];
            spec.align = to_align(c[1]);
            i = 2;
        } else if !c.is_empty() && is_align(c[0]) {
            spec.align = to_align(c[0]);
            i = 1;
        }
        if i < c.len() && (c[i] == '+' || c[i] == '-') {
            spec.plus = c[i] == '+';
            i += 1;
        }
        if i < c.len() && c[i] == '#' {
            spec.alt = true;
            i += 1;
        }
        if i < c.len() && c[i] == '0' && !(i + 1 < c.len() && c[i + 1] == '$') {
            spec.zero = true;
            i += 1;
        }
        // width
        let start = i;
        if i < c.len() && c[i].is_ascii_digit() {
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            if i < c.len() && c[i] == '$' {
                width_name = Some(word);
                i += 1;
            } else {
                spec.width = Some(Count::Fixed(word.parse().map_err(|_| "bad width")?));
            }
        } else {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            if i < c.len() && c[i] == '$' {
                width_name = Some(word);
                i += 1;
            } else {
                // the word was actually the type (e.g. `x`, `b`, `e`)
                i = start;
            }
        }
        if i < c.len() && c[i] == '.' {
            i += 1;
            if i < c.len() && c[i] == '*' {
                prec_star = true;
                i += 1;
            } else {
                let start = i;
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
                let word: String = c[start..i].iter().collect();
                if i < c.len() && c[i] == '$' {
                    prec_name = Some(word);
                    i += 1;
                } else {
                    // digits only
                    let digits: String = word.chars().take_while(|c| c.is_ascii_digit()).collect();
                    i = start + digits.len();
                    spec.precision = Some(Count::Fixed(digits.parse().map_err(|_| "bad precision")?));
                }
            }
        }
        let rest: String = c[i..].iter().collect();
        spec.ty = match rest.as_str() {
            "" => FmtTy::Display,
            "?" => FmtTy::Debug,
            "x" => FmtTy::LowerHex,
            "X" => FmtTy::UpperHex,
            "x?" | "X?" => FmtTy::Debug,
            "b" => FmtTy::Binary,
            "o" => FmtTy::Octal,
            "e" => FmtTy::LowerExp,
            "E" => FmtTy::UpperExp,
            _ => return Err(format!("unsupported format spec `{}`", s)),
        };
    }
    Ok(RawPiece::Arg { name, spec, width_name, prec_name, prec_star })
}

/// Pad `s` according to width/fill/align. `numeric` selects right alignment
/// as the default.
pub fn pad(s: &str, spec: &RSpec, numeric: bool) -> String {
    let Some(width) = spec.width else { return s.to_string() };
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let fill = width - len;
    if spec.zero && numeric {
        // zero padding goes after the sign / prefix
        let (sign, rest) = if s.starts_with('-') || s.starts_with('+') { s.split_at(1) } else { ("", s) };
        let (prefix, digits) = if rest.starts_with("0x") || rest.starts_with("0b") || rest.starts_with("0o") {
            rest.split_at(2)
        } else {
            ("", rest)
        };
        return format!("{}{}{}{}", sign, prefix, "0".repeat(fill), digits);
    }
    let align = match spec.align {
        Align::None => {
            if numeric {
                Align::Right
            } else {
                Align::Left
            }
        }
        a => a,
    };
    let f = spec.fill.to_string();
    match align {
        Align::Left => format!("{}{}", s, f.repeat(fill)),
        Align::Right => format!("{}{}", f.repeat(fill), s),
        _ => {
            let l = fill / 2;
            format!("{}{}{}", f.repeat(l), s, f.repeat(fill - l))
        }
    }
}

pub fn fmt_int(i: i64, spec: &RSpec) -> String {
    let s = match spec.ty {
        FmtTy::LowerHex => {
            let b = format!("{:x}", i);
            if spec.alt { format!("0x{}", b) } else { b }
        }
        FmtTy::UpperHex => {
            let b = format!("{:X}", i);
            if spec.alt { format!("0x{}", b) } else { b }
        }
        FmtTy::Binary => {
            let b = format!("{:b}", i);
            if spec.alt { format!("0b{}", b) } else { b }
        }
        FmtTy::Octal => {
            let b = format!("{:o}", i);
            if spec.alt { format!("0o{}", b) } else { b }
        }
        FmtTy::LowerExp => format!("{:e}", i),
        FmtTy::UpperExp => format!("{:E}", i),
        _ => {
            if spec.plus && i >= 0 {
                format!("+{}", i)
            } else {
                i.to_string()
            }
        }
    };
    pad(&s, spec, true)
}

pub fn fmt_float(f: f64, spec: &RSpec) -> String {
    let mut s = match (spec.ty, spec.precision) {
        (FmtTy::LowerExp, Some(p)) => format!("{:.*e}", p, f),
        (FmtTy::LowerExp, None) => format!("{:e}", f),
        (FmtTy::UpperExp, Some(p)) => format!("{:.*E}", p, f),
        (FmtTy::UpperExp, None) => format!("{:E}", f),
        (FmtTy::Debug, Some(p)) => format!("{:.*?}", p, f),
        (FmtTy::Debug, None) => format!("{:?}", f),
        (_, Some(p)) => format!("{:.*}", p, f),
        (_, None) => format!("{}", f),
    };
    if spec.plus && !s.starts_with('-') {
        s.insert(0, '+');
    }
    pad(&s, spec, true)
}

pub fn fmt_str(s: &str, spec: &RSpec) -> String {
    if spec.ty == FmtTy::Debug {
        return pad(&format!("{:?}", s), spec, false);
    }
    match spec.precision {
        Some(p) => {
            let t: String = s.chars().take(p).collect();
            pad(&t, spec, false)
        }
        None => pad(s, spec, false),
    }
}
