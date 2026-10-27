# Rust spec notes for rint

Digests of the Rust language spec, written from the point of view of someone implementing the language. Each paragraph points back to the source rules: FLS paragraph IDs look like `fls_xxxx`, and Reference rule IDs look like `r[...]`.

## Sources (`../spec/`)

These are shallow clones.

| Dir | What | Version |
|---|---|---|
| `spec/fls` | **FLS**: the Ferrocene Language Specification, donated to the Rust Project in 2025 and maintained at rust-lang/fls. It is written in RST under `src/`, has a strict structure, and every paragraph has an ID. | rustc 1.98.0, edition 2021 (the changelog already covers 1.99) |
| `spec/reference` | **The Rust Reference**: rust-lang/reference, Markdown under `src/`. Its README says this is the normative reference. It covers editions up to 2024. | upstream as of 2026-10-20 |

Each FLS subchapter is split into the same segments: **Syntax**, **Legality Rules** (checked after macro expansion), **Dynamic Semantics**, **Undefined Behavior** and **Implementation Requirements**. The only observable behaviour is external interaction (FFI calls, extern statics, the result of `main`, exported functions). This gives an interpreter "as-if" freedom.

## Digests

1. [01-lexical-macros-structure-attributes.md](01-lexical-macros-structure-attributes.md): input pipeline, tokens and literals, keywords, `macro_rules` matching, transcription and hygiene, proc macros, crates and modules, `cfg`, attributes.
2. [02-types-traits-generics.md](02-types-traits-generics.md): every type kind, layout and `repr`, DSTs, the inference algorithm, coercions and LUB, variance, traits, dyn compatibility, auto traits, coherence and orphan rules, generics, elision, consts and statics, const eval.
3. [03-expressions-statements-patterns.md](03-expressions-statements-patterns.md): place vs value expressions, precedence, evaluation order, overflow and `as` casts, method resolution, closures and captures, loops, match, patterns and binding modes, temporaries, statements.
4. [04-names-ownership-ub-runtime.md](04-names-ownership-ub-runtime.md): namespaces, scopes, preludes, path and `use` resolution, privacy, moves and drop flags, exact drop order, panics and unwinding, the full UB list, Send/Sync, unsafe operations, FFI and ABIs, the entry point, recent language changes.

## Decisions rint needs to make early

- **Edition.** The FLS describes 2021. The Reference and rustc also cover 2024, which changes several things:
  - `if let` and tail-expression temporary scopes
  - let chains
  - match-ergonomics restrictions
  - never-type fallback to `!`
  - `unsafe extern` and `unsafe(..)` attributes
  - the `gen` keyword
  - prelude additions

  Wherever behaviour depends on the edition, it should be switchable per crate.
- **Overflow mode.** Debug mode panics; release mode wraps. Shift amounts are masked in release. Integer division by zero and `MIN / -1` always panic.
- **Memory model.** Values can be abstract, or memory can be bytes with initialization state and provenance (Miri-style). Bytes are needed for `transmute`, raw pointers, `repr(C)` and FFI, and for detecting UB.
- **Static checking.** Decide how much checking happens before running: type inference, trait selection and method resolution are *required* to execute at all. The borrow checker is not.
- **std.** Interpret the real `core`/`alloc`/`std` sources (this needs intrinsics and lang items), or provide a hand-written std.

## Places where the FLS is wrong or out of date (follow rustc and the Reference)

- `main` may return any `T: Termination`.
- `repr(C)` enums with fields are laid out as a struct of a tag plus a union.
- Comparison operators use `PartialOrd`.
- The remainder operator traits are `Rem`/`RemAssign`, not `Mod`.
- For a closure to be `Sync`, its captures must be `Sync`.
- The follow sets for `pat` and `vis` fragments differ from the FLS.
- `$x:ident` accepts keywords.
- Byte escapes accept `\x00`–`\xFF`.
- Lifetime tokens and reserved prefixes are missing from the FLS lexer chapter.
- Each digest has a list of these divergences at the end.
