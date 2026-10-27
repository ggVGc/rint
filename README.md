# rint

A dynamically typed interpreter for a subset of Rust.

## Embed in a Rust project

Keep the application entry point in `src/main.rs`. Add optional interpreter
dependencies and a separate binary:

```toml
[dependencies]
rint = { path = "../rint", optional = true }
my_dependency = "..."

[build-dependencies]
rint-build = { path = "../rint/rint-build", optional = true }

[features]
rint = ["dep:rint", "dep:rint-build"]

[[bin]]
name = "rint"
path = "src/bin/rint.rs"
required-features = ["rint"]
```

In `build.rs`, generate bindings only when the interpreter feature is enabled:

```rust
#[cfg(feature = "rint")]
fn main() {
    rint_build::generate("src/main.rs")
        .expect("discover bindings")
        .write_to_out_dir("rint_bindings.rs")
        .expect("write bindings");
}

#[cfg(not(feature = "rint"))]
fn main() {}
```

The normal application in `src/main.rs` imports and calls dependency functions:

```rust
use my_dependency::score;

fn main() {
    println!("{}", score("hello"));
}
```

Put the interpreter entry point and generated registrations in `src/bin/rint.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/rint_bindings.rs"));

fn main() -> Result<std::process::ExitCode, String> {
    let mut engine = rint::Engine::new();
    register_generated_bindings(&mut engine)?;
    let status = engine.run_cli(std::env::args())?;
    Ok(std::process::ExitCode::from(status as u8))
}
```

Set `default-run` in the package manifest to your normal application's binary
name so plain `cargo run` continues to select it even with multiple binaries.

rint parses the script into its AST and uses its resolver to discover qualified
external function references. It expands imports and aliases, respects local
bindings, and includes functions used as values. It emits registrations for
referenced functions only. The generated Rust is compiled in the host crate, so
dependencies use the host's Cargo names, versions and enabled features. They do
not need to be build dependencies. `rint-build` checks discovered paths against
the host's external dependencies from Cargo metadata, including dependency
aliases. It rejects native bindings to the application's own library or unknown
modules. Application file modules (`mod name;`) are loaded and interpreted,
including nested modules in `name.rs` or `name/mod.rs`. Module names and imports
are resolved within their module, with `crate`, `self`, and `super` paths.
`rint-build` discovers dependency calls across these files and tracks each source
file as a Cargo rebuild input.

For an adapter that needs compiled Rust APIs, explicitly opt a top-level file
module into compilation in `build.rs`:

```rust
rint_build::generate_with_native_modules("src/main.rs", &["terminal_ui"])
    .expect("discover bindings")
    .write_to_out_dir("rint_bindings.rs")
    .expect("write bindings");
```

The application declares `mod terminal_ui;` normally. Generated bindings include
that module in the runner and register its referenced functions. These functions
use the same supported native boundary as dependency functions. The module is
compiled when building the runner; edits to it require rebuilding. Other
application modules are read fresh and interpreted on each invocation.

Cargo reruns generation when the script changes. Scripts can change at runtime
within the set of compiled bindings; new functions or generic specializations
require rebuilding the host. Missing bindings produce resolution errors.

Run the self-contained example as a normal compiled application, or explicitly
select its interpreter entry point. The default build does not compile rint,
rint-build, or generate bindings:

```sh
cargo run --manifest-path examples/embedding/Cargo.toml
cargo run --manifest-path examples/embedding/Cargo.toml --features rint --bin rint -- examples/embedding/src/main.rs
```

For an interactive counter that can run natively or through rint, see the
[ratatui embedding example](examples/ratatui/README.md):

```sh
cargo run --manifest-path examples/ratatui/Cargo.toml
cargo run --manifest-path examples/ratatui/Cargo.toml --features rint --bin rint -- examples/ratatui/src/main.rs
```

Build the project-specific interpreter once, then edit and interpret the actual
application entry point without invoking Cargo:

```sh
cd examples/ratatui
CARGO_TARGET_DIR=target cargo build --features rint --bin rint
./target/debug/rint src/main.rs
# Edit src/main.rs, then repeat the same command:
./target/debug/rint src/main.rs
```

The runner reads the supplied file fresh on every invocation. Application logic
runs interpreted; its registered dependency functions run as compiled Rust.
The runner is specific to the project's compiled bindings, rather than a
universal binary containing every project's dependencies. New native references
or specializations require rebuilding. Source files still need to use rint's
supported Rust subset.

## Supported native boundary

Functions can take zero to six owned arguments. Built-in conversions cover unit,
booleans, characters, integers (checked against rint's i64 representation),
floats, strings and vectors of supported values. A single `&str` argument is also
supported. Native `Result<T, E>` returns become interpreted `Result` values;
returning `Err` does not itself raise an interpreter error. Applications can
implement `FromValue` and `IntoValue` for their own owned types.

Explicit generic specializations such as `dep::identity::<i64>` get separate
bindings. Generic type arguments currently use the interpreter's simplified type
AST; primitive types and supported containers are suitable. Use explicit
specializations when Rust cannot infer generic parameters from the generated
registration.

Dependency methods, native objects, dependency macros, external constants, glob
imports from dependencies, borrowed arguments other than the single `&str`
adapter, and async functions are not supported by automatic generation.
References rooted in declared external dependencies are emitted as function
bindings; Rust compilation reports nonexistent functions or unsupported
signatures. Other unresolved roots are rejected by binding generation.
Modules remain subject to the supported Rust subset: visibility and types are
not checked, and attributes such as `#[path]` and conditional compilation are
not evaluated.

## Host control

`Engine::compile` returns a resolved program containing its compiled bindings.
Use `Engine::compile_with_path(source, filename)` to load file modules relative
to a source file. `run_file`, `run_cli`, and `run_with_io` also load file modules.
For manual bindings, `register_native_module("adapter")` declares that a file
module is provided by the host; register its functions separately.
`run_with_io` accepts a filename, arguments, output writer and optional input
reader. Execution uses the caller's thread, so callbacks can capture application
state with `Rc<RefCell<_>>`. For deep recursion, create and use the engine on a
thread with a larger stack (see `rint::STACK_SIZE`).

`register_fn` also permits manual typed registrations. `register_raw` accepts a
callback over runtime values returning `Result<Value, String>` for custom
conversions and host errors. Native argument and callback errors report the
binding name and, for script calls, the source location. Native Rust panics
propagate normally. Script panics and a `main` returning `Err` retain rint's
existing stderr diagnostics and exit-code behavior.

The original `rint::compile`, `rint::run` and CLI remain available.
