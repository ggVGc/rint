# Ratatui counter

`src/main.rs` contains the event loop and `src/counter.rs` contains the counter.
Both run as ordinary compiled Rust or as interpreted source. The compiled
terminal adapter lives in `src/terminal_ui.rs`, using ratatui and crossterm
directly. No separate adapter library is needed, and the application source
contains no rint setup.

Run normally in an interactive terminal:

```sh
cd examples/ratatui
cargo run
```

Build the separate interpreter entry point once:

```sh
CARGO_TARGET_DIR=target cargo build --features rint --bin rint
```

Then run the actual application source:

```sh
./target/debug/rint src/main.rs
```

Change `count: 0` to `count: 100` in `src/counter.rs`, or change the
increment amount. Run the same executable again:

```sh
./target/debug/rint src/main.rs
```

It reads the edited file at runtime. No Cargo invocation or recompilation occurs.
The filename is a command-line argument, not a hard-coded script path. Additional
arguments are forwarded to the interpreted program.

Press `+` / Up to increment, `-` / Down to decrement, and `q` / Esc / Ctrl-C to quit.

With the `rint` feature enabled, `build.rs` uses
`generate_with_native_modules("src/main.rs", &["terminal_ui"])` to explicitly
compile the terminal adapter into the runner. Generated bindings register its
referenced functions. Native terminal/frame objects stay inside this module.
`counter` is an ordinary interpreted file module, read fresh at runtime along
with `main.rs`. Adding new native references or changing the compiled adapter
requires rebuilding the runner; editing interpreted logic using existing
bindings does not. The runner restores the terminal if execution fails.

The default native build does not compile rint or generate bindings. Test native
rendering and interpreted application logic without a terminal:

```sh
cargo test
cargo test --features rint
```
