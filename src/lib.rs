//! rint: a fast, dynamically-typed interpreter for a subset of Rust.
//!
//! Pipeline: [`lexer`] → [`parser`] → [`resolve`] → [`interp`].
//! Types are not checked and the borrow checker is not run; programs are
//! assumed to be valid Rust.

pub mod ast;
pub mod bindings;
pub mod engine;
pub mod native;
pub mod modules;
pub use engine::Engine;
pub mod builtins;
pub mod format;
pub mod interp;
pub mod iter;
pub mod lexer;
pub mod parser;
pub mod program;
pub mod resolve;
pub mod value;

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

/// Stack size for the interpreter thread (deeply recursive programs).
pub const STACK_SIZE: usize = 1 << 30;

/// Parse and resolve a source file.
pub fn compile(src: &str) -> Result<program::Program, String> {
    let toks = lexer::tokenize(src)?;
    let items = parser::parse_program(toks)?;
    resolve::resolve_program(items)
}

/// Compile and run `src`, writing program output to `out`.
/// Returns the process exit code, or an error message for compile errors and
/// unsupported features.
pub fn run(src: &str, file: &str, args: Vec<String>, out: Box<dyn Write>, stdin: Option<Box<dyn std::io::BufRead>>) -> Result<i32, String> {
    let source = modules::parse(src, std::path::Path::new(file), &[])
        .map_err(|e| format!("{file}:{e}"))?;
    let prog = resolve::resolve_program(source.items).map_err(|e| format!("{file}:{e}"))?;
    let mut it = interp::Interp::new(prog, out, file, args);
    it.stdin = stdin;
    it.run_main()
}

struct SharedBuf(Rc<RefCell<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Outcome of [`run_captured`].
#[derive(Debug, Clone, PartialEq)]
pub struct Captured {
    pub stdout: String,
    /// Exit code, or `Err(message)` for compile/interpreter errors.
    pub result: Result<i32, String>,
}

/// Run a program on a large-stack thread, capturing stdout (used by tests).
pub fn run_captured(src: &str, stdin: &str) -> Captured {
    let src = src.to_string();
    let stdin = stdin.to_string();
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let buf = Rc::new(RefCell::new(Vec::new()));
            let input: Box<dyn std::io::BufRead> = Box::new(std::io::Cursor::new(stdin.into_bytes()));
            let result = run(&src, "src/main.rs", vec!["main".into()], Box::new(SharedBuf(buf.clone())), Some(input));
            let stdout = String::from_utf8_lossy(&buf.borrow()).into_owned();
            Captured { stdout, result }
        })
        .expect("spawn interpreter thread")
        .join()
        .expect("interpreter thread panicked")
}
