// Enums, pattern matching, Option/Result and the ? operator.
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Op(char),
    LParen,
    RParen,
}

#[derive(Debug)]
enum ParseError {
    BadChar(char),
    Unbalanced,
    Empty,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseError::BadChar(c) => write!(f, "bad character '{}'", c),
            ParseError::Unbalanced => write!(f, "unbalanced parentheses"),
            ParseError::Empty => write!(f, "empty expression"),
        }
    }
}

fn tokenize(s: &str) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' => {}
            '0'..='9' | '.' => {
                let start = i;
                while i + 1 < chars.len() && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '.') {
                    i += 1;
                }
                let text: String = chars[start..=i].iter().collect();
                out.push(Token::Num(text.parse().unwrap()));
            }
            '+' | '-' | '*' | '/' => out.push(Token::Op(c)),
            '(' => out.push(Token::LParen),
            ')' => out.push(Token::RParen),
            other => return Err(ParseError::BadChar(other)),
        }
        i += 1;
    }
    if out.is_empty() {
        return Err(ParseError::Empty);
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }
    fn expr(&mut self) -> Result<f64, ParseError> {
        let mut v = self.term()?;
        while let Some(Token::Op(op)) = self.peek().cloned() {
            if op != '+' && op != '-' {
                break;
            }
            self.pos += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, ParseError> {
        let mut v = self.atom()?;
        loop {
            match self.peek() {
                Some(Token::Op('*')) => {
                    self.pos += 1;
                    v *= self.atom()?;
                }
                Some(Token::Op('/')) => {
                    self.pos += 1;
                    v /= self.atom()?;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn atom(&mut self) -> Result<f64, ParseError> {
        let t = self.peek().cloned();
        self.pos += 1;
        match t {
            Some(Token::Num(n)) => Ok(n),
            Some(Token::Op('-')) => Ok(-self.atom()?),
            Some(Token::LParen) => {
                let v = self.expr()?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.pos += 1;
                        Ok(v)
                    }
                    _ => Err(ParseError::Unbalanced),
                }
            }
            _ => Err(ParseError::Unbalanced),
        }
    }
}

fn eval(s: &str) -> Result<f64, ParseError> {
    let toks = tokenize(s)?;
    let mut p = Parser { toks, pos: 0 };
    let v = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(ParseError::Unbalanced);
    }
    Ok(v)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

fn classify(n: i32) -> &'static str {
    match n {
        i32::MIN..=-1 => "negative",
        0 => "zero",
        1 | 2 | 3 => "small",
        x if x % 2 == 0 => "even",
        _ => "odd",
    }
}

fn main() {
    for e in ["1 + 2 * 3", "(1 + 2) * 3", "10 / 4 - -1", "2 * (3", "4 $ 4", ""] {
        match eval(e) {
            Ok(v) => println!("{:>12} = {}", e, v),
            Err(err) => println!("{:>12} -> error: {} ({:?})", e, err, err),
        }
    }
    let mut suits = vec![Suit::Spades, Suit::Clubs, Suit::Hearts, Suit::Diamonds];
    suits.sort();
    println!("{:?} {}", suits, Suit::Hearts > Suit::Diamonds);
    println!("{}", Suit::Spades as i32);
    for n in [-5, 0, 2, 8, 9] {
        print!("{} ", classify(n));
    }
    println!();
    let pairs = vec![(1, "one"), (2, "two"), (3, "three")];
    for &(n, name) in pairs.iter().rev() {
        if let (2, s) = (n, name) {
            println!("found two: {}", s);
        } else {
            println!("{} -> {}", n, name);
        }
    }
    let v = vec![1, 2, 3, 4, 5];
    match v.as_slice() {
        [first, .., last] => println!("first={} last={}", first, last),
        _ => println!("short"),
    }
    if let [a, b, rest @ ..] = &v[..] {
        println!("{} {} {:?}", a, b, rest);
    }
    let opt = Some(5);
    let Some(x) = opt else { panic!("none") };
    println!("x={} doubled={:?} {:?}", x, opt.map(|v| v * 2), opt.and_then(|v| if v > 10 { Some(v) } else { None }));
    let r: Result<i32, String> = "42".parse::<i32>().map_err(|e| e.to_string());
    println!("{:?} {:?}", r, "4x2".parse::<i32>().is_err());
    let n = 7;
    let msg = match n {
        k @ 1..=9 => format!("digit {}", k),
        _ => "big".to_string(),
    };
    println!("{} {}", msg, matches!(n, 5..=8));
}
