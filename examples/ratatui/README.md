# Ratatui counter

`src/main.rs` contains the application's counter and event loop. It runs either
as ordinary compiled Rust or as interpreted source. The external `terminal-ui`
dependency exposes the compiled terminal adapter; the main file contains no
rint setup. Application code is interpreted; only calls into external
dependencies cross into compiled Rust.

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

Change `let mut count = 0;` to `let mut count = 100;` in `src/main.rs`, or
change the increment amount. Run the same executable again:

```sh
./target/debug/rint src/main.rs
```

It reads the edited file at runtime. No Cargo invocation or recompilation occurs.
The filename is a command-line argument, not a hard-coded script path. Additional
arguments are forwarded to the interpreted program.

Press `+` / Up to increment, `-` / Down to decrement, and `q` / Esc / Ctrl-C to quit.

With the `rint` feature enabled, `build.rs` discovers native calls in
`src/main.rs` and generates bindings to external Cargo dependencies only.
The application package's own library and modules are not native bindings.
File-module imports are not supported yet and will eventually be interpreted.
Native terminal/frame objects stay inside the `terminal-ui` dependency. Adding new native references
requires rebuilding the runner; editing logic using existing bindings does not.
The runner also restores the terminal if interpreted execution fails.

The default native build does not compile rint or generate bindings. Test native
rendering and interpreted application logic without a terminal:

```sh
cargo test
cargo test --features rint
cargo test --manifest-path terminal-ui/Cargo.toml
```
