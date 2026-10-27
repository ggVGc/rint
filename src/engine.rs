//! Library interface for embedding rint with compiled dependency bindings.
use crate::{
    native::{NativeBinding, NativeFunction},
    program::Program,
};
use std::{
    io::{BufRead, Write},
    path::Path,
    rc::Rc,
};

#[derive(Default)]
pub struct Engine {
    bindings: Vec<NativeBinding>,
    native_modules: Vec<String>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a compiled function under its fully qualified script path.
    pub fn register_fn<Args, F: NativeFunction<Args>>(
        &mut self,
        path: &str,
        function: F,
    ) -> Result<(), String> {
        self.register_raw(path, move |args| function.call(args))
    }

    /// Register a callback with explicit value conversions or host errors.
    pub fn register_raw(
        &mut self,
        path: &str,
        callback: impl Fn(&[crate::value::Value]) -> Result<crate::value::Value, String> + 'static,
    ) -> Result<(), String> {
        if self.bindings.iter().any(|binding| binding.path == path) {
            return Err(format!("duplicate native binding `{path}`"));
        }
        self.bindings.push(NativeBinding {
            path: path.into(),
            callback: Rc::new(callback),
        });
        Ok(())
    }

    /// Declare a file module implemented by the compiled host. Its functions
    /// must also be registered with `register_fn` or `register_raw`.
    pub fn register_native_module(&mut self, path: &str) {
        if !self.native_modules.iter().any(|module| module == path) {
            self.native_modules.push(path.into());
        }
    }

    pub fn compile(&self, src: &str) -> Result<Program, String> {
        self.compile_with_path(src, "<script>")
    }

    /// Compile source, loading interpreted modules relative to `file`.
    pub fn compile_with_path(&self, src: &str, file: impl AsRef<Path>) -> Result<Program, String> {
        let items = crate::modules::parse(src, file.as_ref(), &self.native_modules)?.items;
        let paths = self.bindings.iter().map(|b| b.path.clone()).collect();
        let (mut program, _) = crate::resolve::resolve_with_native(items, paths, false)?;
        program.native_fns = self.bindings.clone();
        Ok(program)
    }

    /// Run on the caller's thread, with application-controlled I/O and arguments.
    pub fn run_with_io(
        &self,
        src: &str,
        file: &str,
        args: Vec<String>,
        out: Box<dyn Write>,
        stdin: Option<Box<dyn BufRead>>,
    ) -> Result<i32, String> {
        let program = self
            .compile_with_path(src, file)
            .map_err(|e| format!("{file}:{e}"))?;
        let mut interp = crate::interp::Interp::new(program, out, file, args);
        interp.stdin = stdin;
        interp.run_main()
    }

    pub fn run_source(&self, src: &str) -> Result<i32, String> {
        self.run_with_io(
            src,
            "<script>",
            vec!["<script>".into()],
            Box::new(std::io::stdout()),
            None,
        )
    }

    /// Run a source file selected by command-line arguments.
    ///
    /// Pass the complete argument list, including the runner's executable name.
    /// The interpreted program receives the source path as argv[0], followed by
    /// any remaining arguments. No compilation of native Rust occurs here.
    pub fn run_cli(&self, args: impl IntoIterator<Item = String>) -> Result<i32, String> {
        let mut args = args.into_iter();
        let runner = args.next().unwrap_or_else(|| "rint".into());
        let file = args
            .next()
            .ok_or_else(|| format!("usage: {runner} <file.rs> [args...]"))?;
        if file == "--help" || file == "-h" {
            println!("usage: {runner} <file.rs> [args...]");
            return Ok(0);
        }
        let source = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
        let mut script_args = vec![file.clone()];
        script_args.extend(args);
        self.run_with_io(
            &source,
            &file,
            script_args,
            Box::new(std::io::stdout()),
            None,
        )
    }

    pub fn run_file(&self, path: impl AsRef<Path>) -> Result<i32, String> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = path.to_string_lossy();
        self.run_with_io(
            &src,
            &file,
            vec![file.to_string()],
            Box::new(std::io::stdout()),
            None,
        )
    }
}
