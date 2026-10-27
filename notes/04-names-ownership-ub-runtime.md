# Rint implementer digest: names, resolution, ownership, drops, UB, panics, concurrency, unsafe, FFI, runtime

Sources: FLS (`/home/ggvgc/stuff/projects/rint/spec/fls/src/`) and the Rust Reference (`/home/ggvgc/stuff/projects/rint/spec/reference/src/`). "FLS" and "Ref" mark where a rule comes from. Places where the two disagree or the FLS simplifies things are marked **[divergence]**.

---

## 1. Entities

- **Entity**: anything that can be referred to by a path or a field access. **Name**: an identifier that refers to an entity. **Declaration**: introduces a name.
- **Explicitly declared**: associated items, bindings, constants, declarative macros, enums, enum variants, fields, functions, generic params, impls, labels, modules, statics, structs, traits, type aliases, unions.
- **Implicitly declared**: built-in attributes and language-prelude entities (primitive types).
- Item kinds (FLS `items.rst`): const, enum, extern block, `extern crate`, fn, impl, mod, static, struct, trait, type alias, union, `use`, `macro_rules!`, and terminated macro invocations (which expand to zero or more items). `ItemSafety ::= unsafe | safe` is used inside extern blocks.
- **Elaboration**: the process by which a declaration achieves its runtime effects. For an interpreter this mostly means static initializers and the like.

## 2. Namespaces (FLS 5 namespaces)

| Namespace | Contents |
|---|---|
| **type** | modules, structs, enums, unions, traits, type aliases, associated types, type params, `Self`, enum variants (as types/paths), primitives (`bool`, numerics, `char`, `str`) |
| **value** | fns, associated fns, consts, associated consts, const generic params, statics, bindings (locals), struct ctors (tuple/unit), enum variant ctors, `Self` ctor |
| **macro** | `macro_rules!` macros, function-like, attribute and derive proc macros, built-in attributes. Ref: there are two sub-namespaces (bang-macros vs. attr/derive), and candidates from the wrong one are ignored. `cfg` and `cfg_attr` are reserved in the attribute sub-namespace. |
| **lifetime** | lifetime params |
| **label** | loop and block labels |

- Fields (struct, enum, union) belong to **no** namespace. They are resolved by field resolution only.
- A tuple or unit struct `S` lives in **both** the type namespace and the value namespace (as its constructor). A braced struct lives only in the type namespace.

## 3. Scopes

- **Binding scopes**:
  - `let`: from after the statement to the end of the enclosing block.
  - fn and closure params: the body.
  - `for` / `while let`: the loop body.
  - `if let`: the consequent block (Ref: also the following `&&` let-chain conditions).
  - match arm bindings: the guard and the arm body.
  - `if let` guard bindings (1.95): later guard conditions and the arm body.
  - Outer bindings are **not** in scope inside `const { }` blocks, nor inside nested items. Locals never reach into items.
- **Item scope**:
  - Items in a module are visible throughout the module, independent of order.
  - Items in a block are visible throughout the whole block (whole-block, not after-declaration).
  - Items are **not** visible in nested modules, which need a path.
  - Duplicate names in the same namespace in the same module or block are an error. Glob imports are the exception.
- **Generic param scope**: the generic list (order-independent, e.g. `fn f<T: Tr<N>, const N: usize>`), fields/variants/body, and where-clauses. Not visible in nested items, except associated items. Items nested in a fn may shadow the fn's generic names. Otherwise shadowing a generic param is an error.
- **Self scope**:
  - In struct/enum/union/trait/impl: generics, where-clause, fields or body.
  - The `Self` constructor (value namespace) is in scope inside impl bodies.
  - `Self` is not in scope inside attributes.
- **Label scope**: the loop body. Labels do not reach into closures, async blocks, const contexts, items, or the iterator expression of the defining `for`. Inner labels shadow outer labels of the same name.
- **Textual macro scope** (`macro_rules!`):
  - Runs from the definition to the end of the enclosing block or module, including child modules declared after it.
  - `#[macro_use] mod m` extends m's macros past the end of `m` into the parent scope.
  - `#[macro_export]` puts the macro at the crate root for path-based access.
- **Pattern bindings may shadow anything except**: const generic params, statics, consts, and struct/enum constructors. For those names, an identifier pattern is treated as a path pattern; if it is not refutable-compatible, that is an error.
- **Associated items are not scoped**. They are reachable only via `Type::item`, `<T as Tr>::item`, or method call.
- Scoping constructs: block, closure, `macro_rules`, enum/struct/union/trait/type-alias/impl/fn declarations, fn-pointer type, `if let`, `let`, loops, match arm, module, trait bound, type-bound predicate (`for<'a>`).

## 4. Preludes

The prelude is consulted **after** all lexical scopes. Prelude entries are *not* members of the module, so `self::Box` does not resolve.

| Prelude | Contents | Where |
|---|---|---|
| Language | `bool char str i8..i128 isize u8..u128 usize f32 f64`, built-in attributes, built-in derives (Ref) | every module, always, even under `no_implicit_prelude` |
| Standard library | `std::prelude::rust_20xx` (edition-dependent), or `core::prelude::rust_20xx` under `#![no_std]`. FLS says "core prelude = `core::prelude::rust_2021`" **[divergence]**. 2024 adds `Future` and `IntoFuture`. | every module |
| Extern | crates from `--extern` and root `extern crate` (an alias replaces the name). `core` is always present (unless `no_core`); `std` unless `no_std`. | every module, as a bare first segment |
| `macro_use` | macros from `#[macro_use] extern crate` | crate root |
| Tool | `rustfmt`, `clippy`, `diagnostic`, … (type namespace, used for tool attributes) | every module |

- **Shadowing priority**, highest first (both sources agree): **Extern > Tool > macro_use > Std > Language**. Any local item or import shadows any prelude name.
- `#![no_implicit_prelude]` on a crate or module (inherited by descendants) removes the std, extern, macro_use, and tool preludes. The language prelude remains.
- **Primitive-vs-module special case (FLS)**: if the first segment resolves to a *module* but the identifier names a language-prelude type, and either the path has more than one segment or the context is the type namespace, the primitive type wins. Example: a `mod u8` coexisting with `u8::MAX`.

## 5. Paths

- Segments are identifiers, `crate`, `$crate`, `self`, `super`, or `Self`.
- `crate`, `$crate`, and `Self` may appear **only first**.
- `self` may appear first, or last (in `use`).
- `super` may repeat only at the start, optionally after a leading `self` (`self::super::super::x`).
- `$crate` is allowed only in a macro transcriber. It resolves to the crate that **defines** the macro, which needs hygiene metadata on tokens.
- **Global path** `::x`: in 2018+ editions the first segment must be an **extern-prelude crate**. In 2015 `::x` means crate-root-relative.
- **Qualified paths**:
  - `<T as Trait>::item`: the resolution context is the impls of `Trait` for `T`.
  - `<T>::item`: `T`'s inherent impls plus its impls of in-scope traits.
  - `<T as Tr>::Assoc` in type position is an associated type projection.
- **Fn sugar** `Fn(A) -> R` is allowed only on the last segment and must resolve to `Fn`, `FnMut`, or `FnOnce`.
- **Generic args**: in expressions they need the turbofish `Vec::<u8>::new`. In types, `::` before `<` is optional.

### Resolving the first segment (FLS algorithm)

1. `crate` resolves to the current crate root. `$crate` resolves to the macro-defining crate. `self` resolves to the current module. `super` resolves to the parent module. `Self` resolves to the ADT, impl self type, or trait.
2. For an identifier:
   - In a global path: look it up in the extern prelude.
   - Otherwise it is a **candidate direct entity**. Search textual macro scopes first, then the scope hierarchy innermost to outermost (blocks, then the enclosing module's items and imports, stopping at module boundaries), then the preludes.
   - It is an error if the identifier resolves to an extern-prelude crate that is also shadowed. In practice rustc reports this as an ambiguity when a crate name collides with a local item used as a path root.
3. For a qualified type: the qualified type, with a resolution context as described above.

### Remaining segments (left to right)

- `super` resolves to the parent of the previous context.
- **Simple paths** (`use`, visibility, attributes, macros): look the name up in the previous context, which must be a module for non-final segments. Namespace context:
  - macro namespace for attributes and macro invocations;
  - type + value + macro namespaces for `use`;
  - type namespace for `pub(in …)`.
- **Path expressions** (value namespace for the final segment):
  - Previous context is an enum: the variant.
  - Previous context is a module: the item.
  - Previous context is a type or trait: first the inherent impls (consts and fns), then impls of **in-scope** traits.
  - Non-final segments are looked up in the type namespace.
- **Type paths**: module item, trait associated item, or type associated item (inherent impls first, then in-scope trait impls).
- `use` resolves at **expansion time** (Ref). Type-relative paths cannot appear in `use`: `use m::Alias::Variant` and `use m::E::ASSOC_CONST` are errors, while `use m::E::V` is OK. FLS: a simple import shall not reach enum variants through a type alias.

## 6. `use` imports

- Forms:
  - `use a::b;`
  - `use a::b as c;`
  - `use a::b as _;` imports anonymously. This is useful for traits, which become in scope for method resolution without a name.
  - `use a::*;`
  - `use a::{b, c::d, self};` (nesting)
- An import is valid within the block or module containing it. **Import path prefix** is the concatenation of enclosing nesting prefixes plus its own prefix.
- **Simple import**:
  - Imports *all* entities named `b` in every namespace that are visible from the import site. For example, a type and a value of the same name are imported together.
  - `{self}` inside a nest imports the type-namespace entity of the prefix (a module, enum, or trait).
- **Glob import** of a module imports every name that is *visible from the import site*. A glob of an enum imports its variants. A bare `use *;` is rejected.
- Required renames:
  - Importing the current module via `self`, the parent via `super`, or `use crate`/`use $crate` alone requires `as name`. This has been allowed since 1.95 ("importing path-segment keywords with renaming"), and 1.97 widened trailing `self`.
  - `use ::{self}` is invalid.
- **Shadowing and globs** (Ref, which is more precise than the FLS):
  - Explicit items and imports shadow glob-imported names in the same scope.
  - Two globs bringing *different* entities under the same name and namespace is an error **only when the name is used** (error E0659 at the use site).
  - Two globs bringing the *same* entity (after following re-exports) is fine. The visibility is the max of the two.
  - Inner-scope imports shadow outer-scope names, *except* that names in imports and macro paths may not resolve through a glob when an outer-scope candidate exists (ambiguity error). Macro-expanded definitions that would shadow an outer definition used from outside the expansion are ambiguous ("more expanded" shadowing is disallowed).
  - Resolution must be order-independent. Rustc resolves imports by fixpoint iteration, and speculative results are validated after expansion.
- Macro lookup order (Ref): derive helpers, textual `macro_rules` scopes, path-based scope (imports and items), the macro_use prelude, the std prelude, then built-in attributes.

## 7. Shadowing (FLS)

- The only names that may be shadowed are:
  - prelude names (by anything);
  - `macro_rules` names within textual scope (later definitions win);
  - variables (`let x = …; let x = …;`).
- A shadowed name is unreachable.
- Everything else is a duplicate-definition error, apart from the glob rules above. Inner block items or imports shadowing outer-module names is ordinary lexical scoping.

## 8. Visibility and privacy

- Modifiers:
  - `pub`
  - `pub(crate)`
  - `pub(super)`, which equals `pub(in super)`
  - `pub(self)`, which equals private
  - `pub(in path)`: the path must start with `crate`, `self`, or `super` (2018+), must name an **ancestor** module (or the current one), and each segment must be a real module, not a `use` alias.
- The default is private, meaning visible in the defining module and its descendants. This applies to items, fields, and extern items.
- Inherited visibility:
  - Trait associated items take the trait's visibility.
  - Enum variants and their fields take the enum's visibility.
  - Impl items have their own `pub`. Trait impl items carry no modifier.
- **Access rule**: an item is accessible from module M iff:
  - it is (restricted-)public with M inside its restriction, *and* every ancestor module on the path is accessible from M; or
  - it is private and M is its module or a descendant.
- `pub use` re-exports short-circuit the ancestor chain. A re-export cannot exceed the visibility of the item it re-exports.
- Privacy applies to `use`, paths in expressions and types, field access, method resolution (invisible candidates are skipped), and struct literals and patterns (all fields must be visible).
- `#[non_exhaustive]` (outside the defining crate) forbids struct literals and requires `..` or a wildcard. Since 1.83 non-exhaustive structs may also be empty.

## 9. Field, method, and call resolution

**Deref chain**: starting from `T`, the next type is the pointee if `T` is a reference; otherwise `<T as Deref>::Target`. Raw pointers are **not** auto-dereffed.

**Field access `e.f` / `e.0`**:
- Walk the deref chain of `typeof(e)`.
- The first type that has a *visible* field with that name or index wins.
- An implicit borrow or deref of the operand happens as needed. Field access is a place expression.

**Method call `recv.m(args)`** (FLS ordering, matching rustc's probe order):
```
for U in deref_chain(typeof(recv)) [+ if last U is [T;N], also [T]]:   // unsized coercion at end
    for R in [U, &U, &mut U]:                    // autoref
        inherent methods: for impl with self-type unifying with U, find visible m whose
                          self-param type unifies with R → pick, stop
        trait methods:    for impl of in-scope trait for U, same check → pick, stop
```
- Rustc's exact `by_value` → `autoref` → `autoref_mut` step happens per deref step, as in the FLS.
- Ambiguity among same-level trait candidates is an error. Trait candidates are considered only if the trait is in scope (including via `use Tr as _` and prelude traits).
- Method candidates must be visible. Since 1.81, method resolution constrains opaque hidden types rather than rejecting them.
- **Note (1.99, FCW)**: trait methods are now resolved on an adjusted never type.

**Call `f(args)`**: walk the deref chain of the callee type until it reaches a callee type: fn item, fn pointer, closure, or a type implementing `Fn*`.

**Path-call to associated functions**: `Type::f` uses path-expression resolution, with inherent before trait.

## 10. Ownership, initialization, moves

- Every value has exactly one owner, which is a variable (local, param, or temporary). All memory starts uninitialized.
- Locals are allocated (uninitialized) at frame entry and must be **definitely initialized** on all paths before use. A move makes the variable uninitialized again; reassigning re-initializes it.
- **Copy types** (`impl Copy`) are passed by copy: a bitwise copy, and the source stays valid. **Move types** (Sized and not Copy) are passed by move.
- A **place** can be moved out of only if it is:
  - a variable that is not currently borrowed;
  - a temporary;
  - a field of a movable place whose **containing type does not implement `Drop`** (partial move). Partial moves through `Box` deref are also allowed (`let x = *b;`, a `Box` special case).
  - You cannot move out of `&`, `&mut`, indices, or raw-pointer derefs (use `ptr::read`).
- A value expression is always moved (and is a temporary).
- **Partial moves**: after `let a = s.f;`, `s` is partially initialized. Only the remaining initialized fields are dropped at scope end. `s` as a whole cannot be used until re-initialized, though individual still-initialized fields can be.
- **Assignment** `place = v`: if the place is initialized, its old value is **dropped first**. Assigning to an uninitialized place (after a move or a deferred `let x;`) runs no destructor.
- **Interpreter implication**: track per-variable (and per-field, for partial moves) drop flags at runtime. Rustc uses dynamic drop flags whenever initialization state is path-dependent.

## 11. Borrowing (as specified)

- `&e` / `&mut e`: the lifetime is inferred (an NLL-style region). A borrow is **active** from creation to its last use, stopping early at reassignment or end of scope.
- Rules:
  - A referent may not be moved while any reference to it is active.
  - It may not be modified while any reference is active, except through that `&mut` itself, or interior mutability via `&` (`UnsafeCell`).
  - While a `&mut` is active, no other reference may point to an overlapping value.
  - The referent's lifetime must be at least the reference's lifetime.
  - The referent must be initialized.
- **Implicit borrows** (autoref) occur for:
  - the call operand;
  - comparison operands (`a == b` is `PartialEq::eq(&a, &b)`);
  - the compound-assignment LHS (`AddAssign::add_assign(&mut a, b)`);
  - field-access operands;
  - deref operands (`*x` on `Deref` is `*Deref::deref(&x)`);
  - the indexed operand (`*Index::index(&a, i)`);
  - method receivers.
- Borrowing one union field borrows **all** of the union's fields with the same lifetime.
- Since 1.92, `&raw const` / `&raw mut` of a union field is allowed in safe code. Since 1.84, raw borrows of `*ptr` are safe. Since 1.82, `&raw` of any static (including `static mut`) is safe.
- UB (FLS): accessing a value through aliasing `&mut` from unsafe code. The Ref aliasing model is broader (see §15).
- **Interpreter note**: borrow checking is static. An interpreter may skip it for valid programs, but the runtime aliasing UB (Stacked/Tree Borrows à la Miri) is only detectable dynamically.

## 12. Destructors

- **Drop type**: implements `Drop`, or contains a field of a drop type (has "drop glue").
- Destroying a value of type `T` proceeds as follows:
  1. If `T: Drop`, call `Drop::drop(&mut v)`.
  2. Then recursively drop the fields:
     - struct and tuple: declaration order (first to last);
     - enum: the active variant's fields in declaration order;
     - array and owned slice: first element to last;
     - closure: by-move captures in **unspecified** order;
     - `dyn Trait`: the vtable's drop of the concrete type;
     - anything else: nothing.
- Uninitialized (moved-out) variables and fields are not dropped.
- `mem::forget` and `ManuallyDrop` suppress drops, and this is safe. Soundness may never rely on destructors running.
- `ptr::drop_in_place` runs drop glue manually.
- Since 1.82, dropping `[T; 0]` does not require `T` to be live.

## 13. Drop scopes and order

**Drop scopes** exist for: the whole function, the function body block, each statement, each expression, each block, and each match arm. To reason about `if`/`while`/`for`, desugar them to `match`/`loop`/`break`.

Nesting rules:
- The function scope is outermost. It contains the body block.
- An expression statement's operand has the statement as its parent.
- A `let` initializer has the `let` statement as its parent.
- A statement has the enclosing block as its parent.
- A guard, guard condition, and guard scrutinee have the arm as their parent.
- An arm body has the arm as its parent.
- An arm has the `match` as its parent.
- Everything else has the immediately enclosing expression as its parent.

**Association of values**:
- `let` bindings belong to the enclosing block.
- `for` / `while let` / `if let` bindings belong to the loop or consequent block.
- Match arm bindings and `if let`-guard bindings belong to the arm.
- **Function parameters belong to the function scope**, so they are dropped *after* all body locals.

**Temporary scope** (when not promoted and not extended) is the innermost of:
- the whole function;
- a statement;
- the body of `if` / `while` / `loop`;
- an `else` block;
- the condition of `if` or `while` (non-pattern), so temporaries are dropped right after the condition is evaluated;
- a non-pattern guard condition operand;
- the **pattern-matching guard plus the body** of a match arm (`if let` guard temporaries live until the arm is left);
- each operand of `&&` / `||` (the right operand gets its own scope);
- **edition 2024**: the `if let` scrutinee scope covers the condition plus the consequent only. Scrutinee temporaries are dropped **before** `else`. In 2021 they lived through `else` until the end of the statement.
- **edition 2024**: a block's tail expression is its own temporary scope. Tail temporaries are dropped right after the tail is evaluated, *before* the block's locals. In 2021 they outlived the block's locals, until the end of the enclosing statement.
- A **`match` scrutinee is NOT a temporary scope**: its temporaries live to the end of the enclosing statement (classic `match mutex.lock()…` behavior). The same holds for `if let` before 2024.
- **Operand temporaries**: while other operands are evaluated, earlier operand results are held in temporaries of the enclosing expression. If evaluation diverges (`break`, `return`, `?`, panic), the already-evaluated operands are dropped in reverse order.

**Drop order when leaving a scope**:
1. Temporaries are dropped in reverse creation order. Bindings are dropped in reverse declaration order.
2. Bindings in a pattern are dropped in reverse declaration order within the pattern. **For or-patterns the declaration order is taken from the first alternative**, whichever alternative matched (1.91: bindings are lowered in written order).
3. `if let` guards (1.95):
   - Evaluated guard let-patterns are processed in reverse source order. For each one, its bindings are dropped (reverse) and then its scrutinee temporaries (reverse creation).
   - Then the arm's pattern bindings are dropped.
   - If the guard is false, the guard bindings and temporaries are dropped *before* the next pattern or arm is tried. With self-overlapping or-patterns, a guard can run, and be dropped, more than once.
4. When several scopes are exited at once (`return`, `break`, `?`, panic unwind), values are dropped innermost scope first.
5. **Function parameters** are dropped right to left. For each parameter, its pattern bindings (reverse) are dropped first, then the parameter value itself. Example: `fn f((x,_):(A,B),(_,y):(C,D))` drops `y`, then param 2's remainder `C`, then `x`, then param 1's remainder `B`.

### Temporary lifetime extension (in `let` and in `static`/`const` initializers)

- **Extending patterns** (FLS + Ref): `ref` / `ref mut` identifier patterns, and struct/tuple/tuple-struct/slice/or patterns containing an extending subpattern. `&ref x` is not extending. If the `let` pattern is extending, the initializer's temporary scope becomes the enclosing block.
- **Extending expressions**:
  - the `let` initializer;
  - the operand of an extending `&` / `&mut` / `&raw`, array, cast, braced-struct, or tuple expression;
  - the arguments of an extending **tuple-struct or tuple-variant constructor call** (1.89; e.g. `Some(&temp())`);
  - the tail of an extending block, including `unsafe {}` and `const {}` (not `async {}`);
  - the tails of an extending `if` / `else`;
  - extending match-arm expressions;
  - super operands of `pin!` and `format_args!`.
  - Extension also propagates through the operand of `&`, `*`, field access, tuple index, and indexing (the indexed operand) when those have extended scope.
- **Not extending**: function and method call arguments (`f(&temp())`), method receivers, match scrutinees, closure bodies, async block tails, `break` values.
- In `const` / `static` initializers, extended temporaries become `'static`.

### Constant promotion

- `&<const-evaluable expression>` with no interior mutability and no `Drop` is promoted to a `'static` allocation, so there is no temporary and no drop. Example: `&None`, `&5`.
- Promoted bytes are immutable; mutating them is UB.
- Since 1.95, const blocks are no longer evaluated to decide whether fallible expressions can be implicitly promoted.

## 14. Panic, abort, unwinding

- Error tiers: `Option` (absence), `Result` (recoverable), panic (bug). Rust has no exceptions.
- **Panic** (FLS): halts the current thread's normal control flow and calls the `#[panic_handler]` function. The handler has signature `fn(&PanicInfo) -> !`, and exactly one must exist in the crate graph (std provides one).
- **Implicit panics** come from:
  - array or slice index out of bounds;
  - integer overflow when debug assertions are on (otherwise wrapping two's complement is allowed);
  - division or remainder by zero, and `MIN / -1`;
  - shift amount ≥ bit width (with checks on);
  - `unwrap` and similar library functions.
- **Abort**: halts all threads immediately and terminates. No destructors run.
- **Strategies** (`-C panic`):
  - `unwind`: walk frames, dropping every live, initialized value as if its scope ended (same drop order as §13), until caught by `catch_unwind` or a thread boundary.
  - `abort`: terminate.
  - std also aborts on a panic *while* panicking, for example a panic inside `Drop` during unwinding.
- The **Rust ABI always permits unwinding**. `-unwind` ABIs (`C-unwind`, `system-unwind`, …) permit it across FFI. Behavior table:

| runtime | ABI | Rust panic | foreign native unwind |
|---|---|---|---|
| unwind | unwinding | unwind | unwind |
| unwind | non-unwinding (`"C"`) | **abort** (1.81). It is unspecified whether destructors up to the boundary run; 1.84 runs drop glue. | **UB** |
| abort | any unwinding | abort, no unwind | abort |
| abort | non-unwinding | abort | **UB** |

- A foreign exception caught by `catch_unwind` or `join`, or escaping `main`, either aborts or yields an opaque `Err`; which one is unspecified.
- Process exit without unwinding (`process::exit`, `abort`) runs no destructors. Leaks, deadlocks, and exiting without destructors are **not** UB.

## 15. Undefined behavior: consolidated list

The FLS spreads its UB rubrics across chapters. The Ref list is broader and non-exhaustive. An interpreter in "Miri mode" should detect as many of these as feasible.

1. **Data races**: two threads, at least one write, no synchronization. Since 1.83 a race between an atomic read and a non-atomic read is *not* UB.
2. **Dangling or misaligned access**: loading or storing a place that is dangling (not all bytes in one live allocation) or *based on a misaligned pointer*. "Based on a misaligned pointer" means the last `*` projection used a pointer unaligned for **the pointee type of that pointer**, not the field type. `&raw` of such a place is OK. A ZST access is never dangling, even through null (1.80).
3. **Out-of-bounds place projection**: field, tuple-index, or index projections that violate in-bounds `offset` rules.
4. **Aliasing violations**:
   - `&T` pointee mutated while live (except inside `UnsafeCell`).
   - `&mut T` pointee accessed through a non-derived pointer while live.
   - `Box<T>` behaves like `&'static mut T`.
   - A reference passed as a function argument is live for the whole call.
   - These rules apply to references nested in fields, but not behind pointers.
   - FLS: accessing through aliasing `&mut` from unsafe code.
5. **Mutating immutable bytes**: promoted constants, extended temporaries in `const`/`static`, immutable bindings and statics without `UnsafeCell`, anything behind `&` (transitively). Any write of more than 0 bytes counts, even if it writes the same value. FLS: mutating an immutable static that lacks interior mutability.
6. **Invalid values**, produced at any read, write, pass, or return:
   - `bool` other than 0 or 1;
   - `char` that is a surrogate or > `0x10FFFF`;
   - any value of type `!`;
   - uninitialized integer, float, or raw pointer (also `str`, which is treated like `[u8]`);
   - enum with an invalid discriminant or invalid fields;
   - struct, tuple, or array containing an invalid field;
   - null, unaligned, or dangling reference or `Box`, or one pointing to an invalid value;
   - null fn pointer;
   - wide-pointer metadata mismatch: a bad vtable, or a total size > `isize::MAX`;
   - `NonNull` / `NonZero` out of range;
   - FLS: creating any value from uninitialized memory unless it is a union;
   - `MaybeUninit` is always valid; padding may be uninitialized.
   - **Safety invariants** (library-level, not immediate UB): `str` must be UTF-8; naked fn bodies must respect the ABI and diverge.
7. **Provenance**: accessing memory through a pointer whose provenance does not permit the access. In const eval, integer-typed values must carry no provenance and pointer bytes must stay in order, so ptr→int transmute in const is UB.
8. **Allocation limits**: creating an allocation at address null, one that wraps past `usize::MAX`, or one larger than `isize::MAX`.
9. **Union reads**: reading a union field, or deconstructing it in a pattern, when its bytes are invalid for the field type.
10. **Raw deref**: dereferencing a dangling or unaligned raw pointer.
11. **Wrong ABI**: calling a function through the wrong ABI, or unwinding through a frame that does not allow unwinding (a foreign exception crossing a non-`-unwind` boundary).
12. **Target features**: executing code that needs CPU features the CPU lacks.
13. **`track_caller` mismatch** between an extern declaration and its definition (FLS).
14. **Inline assembly**:
    - a non-output register changed;
    - x86 `DF` left set;
    - a `pure` block with side effects;
    - control reaching the end of a `noreturn` block;
    - the stack pointer not restored;
    - flags not restored under `preserves_flags`;
    - and similar.
15. **Intrinsics**: UB via compiler intrinsics (`unreachable_unchecked`, `assume`, unchecked arithmetic, …).
16. **Runtime assumptions**: a Rust frame deallocated without running its destructors (`longjmp` over Rust frames).
17. **Futures**: polling a future after it returned `Ready` is a contract violation in the FLS ("shall not"). It is a library contract violation, not language UB.

## 16. Concurrency

- **`Send`**: safe to move to another thread. **`Sync`**: `&T` is safe to share, i.e. `T: Sync ⇔ &T: Send`.
- Both are auto traits. An ADT gets them if all its fields have them. They can be opted out via negative or `unsafe impl` (unsafe trait).
- Raw pointers are neither Send nor Sync. `UnsafeCell` is not Sync. `Rc` is neither. Implementing them manually is an `unsafe impl`.
- **Atomics** (`core::sync::atomic`):
  - `AtomicBool`, `AtomicI8/16/32/64/Isize`, `AtomicU8/16/32/64/Usize`, `AtomicPtr<T>` (`AtomicI128`/`U128` are unstable).
  - `Ordering` values follow the C++20 memory model: `Relaxed`, `Acquire`, `Release`, `AcqRel`, `SeqCst`. Fences are available too.
  - The Ref memory model says bytes are either initialized (a `u8` plus optional provenance) or uninitialized. The model is incomplete.
- **Async**:
  - A `Future` is computed via `.await` or `Future::poll(Pin<&mut Self>, &mut Context) -> Poll`.
  - `async fn` and async blocks desugar to state machines. Async closures are stable since 1.85.
  - Never poll again after `Ready`.
- **Interpreter guidance**:
  - Threads can be emulated with a deterministic scheduler.
  - Data-race detection needs vector clocks (as Miri does).
  - `thread::spawn` panics are caught at the thread boundary and surface as `Err` from `join`.

## 17. Unsafe operations

FLS list:
1. Dereferencing a raw pointer.
2. Reading or writing an extern static (unless it is declared `safe` inside an `unsafe extern` block).
3. Reading or writing a `static mut`.
4. Reading a union field (assignment is safe).
5. Calling an `unsafe fn`, including extern fns not marked `safe`.
6. Calling `asm!`.

Additional items from the Ref **[divergence]**:
7. Calling a safe `#[target_feature]` fn from a context that lacks those features (1.86).
8. `unsafe impl` of an unsafe trait (`Send`, `Sync`, `GlobalAlloc`, user `unsafe trait`).
9. Declaring an extern block: edition 2024 requires `unsafe extern`.
10. Unsafe attributes such as `#[unsafe(no_mangle)]`, `#[unsafe(export_name)]`, `#[unsafe(link_section)]` (required in 2024; opt-in since 1.82).

Unsafe context:
- An unsafe context is an `unsafe {}` block, or the body of an `unsafe fn`.
- 2024: the `unsafe_op_in_unsafe_fn` lint warns by default, so an explicit `unsafe {}` is expected inside an `unsafe fn` body.
- `unsafe` never makes UB defined. **Soundness**: safe callers can never trigger UB.

## 18. FFI, extern blocks, ABIs

- ABI strings:

  | ABI | Meaning |
  |---|---|
  | `"Rust"` | the default for fns |
  | `extern` with no string | `"C"` |
  | `"C"` / `"C-unwind"` | C, with or without unwinding |
  | `"system"` / `"system-unwind"` | `stdcall` on Win32 x86 for non-variadic fns, otherwise `"C"` |

  Tool-specific: `aapcs`, `cdecl`, `efiapi`, `fastcall`, `stdcall`, `sysv64`, `vectorcall`, `win64`, with `-unwind` variants, plus `"custom"`, which can only be called via asm.
- **Extern block**: `[unsafe] extern "abi" { fn …; static …; }`.
  - Items take the block's ABI (default `"C"`).
  - Items may be marked `safe` or `unsafe`, but only inside an `unsafe extern` block.
  - Extern fns have no body, no type or const generics, no qualifiers, and patterns limited to identifiers or `_`.
  - Only the last parameter may be variadic `...`.
  - Extern statics have no initializer. An immutable extern static must be initialized before Rust code runs.
  - Calling or accessing extern items needs unsafe unless they are `safe`.
- **Defining exported fns**: `extern "C" fn f() {}` with `#[unsafe(no_mangle)]` or `#[unsafe(export_name="sym")]` (`export_name` wins over `no_mangle`). `no_mangle` is not allowed on generic items. The C-variadic ABI story:
  - variadic *declarations* for many ABIs: 1.91 and 1.93;
  - variadic *definitions* (`unsafe extern "C" fn f(x: i32, mut args: ...)`): stable since 1.99;
  - variadic naked fns: 1.99.
- Other FFI attributes:
  - `#[link(name=…, kind=…)]`;
  - `#[link_section]`;
  - `#[used]`, which keeps a static in the object file;
  - `#[repr(C)]` / `repr(transparent)` / `repr(u*)` for layout. `repr(u128)` / `repr(i128)` have been stable since 1.89.
- `#[naked]` fns (1.88): the body is `naked_asm!` only. They cannot be combined with `inline`, `track_caller`, or tests.
- **Interpreter implication**: FFI needs a libffi-style shim, or stubs for common libc symbols. Mismatched-ABI calls are UB and can be diagnosed.

## 19. Inline assembly (skim)

- `asm!` (inside fn bodies, unsafe), `global_asm!` (module level), `naked_asm!` (naked fns).
- Arguments, in this order: template strings, an optional label block (asm goto, 1.87), register operands (`in`, `out`, `inout`, `lateout`, `inlateout`, `const`, `sym`), `clobber_abi("C")`, then `options(...)`. Options are `att_syntax`, `nomem`, `noreturn`, `nostack`, `preserves_flags`, `pure`, `raw`, `readonly`.
- Only `cfg` / `cfg_attr` attributes are allowed on arguments.
- Register operands are evaluated in declaration order. Inputs are loaded and outputs written back in undefined order.
- `global_asm!` and `naked_asm!` accept only `const` and `sym` operands, and only the `att_syntax` and `raw` options.
- **For an interpreter: almost certainly unsupported**. Diagnose it as "unsupported operation", as Miri does.

## 20. Program entry and runtime

- **Crate types**: the default is `bin`. A bin crate (without `#![no_main]`) must have `main` in scope at the crate root; it may be imported (`use foo::bar as main;`).
- **`main` signature**:
  - FLS: no `async`/`unsafe`, Rust ABI, no generics, no params, no where-clause, has a body, and "lacks a return type" **[divergence]**.
  - Ref: no params, no bounds or where-clauses, return type implements `std::process::Termination`, which covers `()`, `!`, `Infallible`, `ExitCode`, and `Result<T: Termination, E: Debug>`.
  - Returning `Err(e)` prints `Error: {e:?}` and exits with failure.
- **Before and after `main`** (std runtime, not specified by the FLS):
  - std sets up stack guards, args, and the panic hook.
  - Statics are compile-time values, so there is no dynamic init order.
  - After `main` returns: no TLS destructors are guaranteed for the main thread, and statics are **never dropped**. This last point matters for an interpreter: do not run `Drop` on statics.
  - A panic escaping `main` prints a message and the process exits with code 101 (std behavior). A foreign unwind escaping `main` terminates the process, possibly by abort.
- **Runtime attributes**:
  - `#[global_allocator]` on a `static` implementing `GlobalAlloc`, at most once per crate graph;
  - `#[panic_handler]` (required for `no_std` bin, dylib, cdylib, or staticlib);
  - `#![windows_subsystem]`;
  - `#![no_std]` (`core` prelude, no `std`);
  - `#![no_main]`;
  - `#![no_core]` (unstable).
- **Const evaluation** (`const_eval.md`):
  - **Const contexts** are: array lengths and repeat counts, `const`/`static` initializers, enum discriminants, const generic arguments, and `const {}` blocks.
  - In those contexts, overflow, out-of-bounds indexing, and panics become **compile errors**.
  - Const contexts evaluate with **target** semantics, e.g. `usize` width.
  - Allowed: `let`, assignment, `loop`/`while`, `if`/`match`, const fn calls, `&mut` (1.83), float arithmetic (1.82), and dereferences.
  - Disallowed: ptr→int casts, writes to statics, reads of extern statics, reads of mutable statics outside static initializers, and running `Drop` (a value must not need dropping).
  - Final values may reference mutable or extern memory (1.90), but such constants cannot be used as patterns.
  - **Interpreter implication**: the same evaluator can run const eval, but it needs a "const mode" that enforces these restrictions and the strict provenance rules.

## 21. Notable recent changes (FLS changelog, ≈1.80–1.99)

- **1.99** (in the changelog even though the stated version is 1.98):
  - C-variadic **function definitions** are stable, as are C-variadic naked fns.
  - `#[my_attr] mod foo;` (attribute macros on out-of-line modules) is stable.
  - Anonymous lifetimes in associated-const types are inferred as `'static`.
  - The contents of `UnsafeCell` may be accessed without `get` (library guarantee).
  - x86 `asm!` can pass 128-bit integers in vector registers.
  - Trait methods resolve on an adjusted never type (FCW).
- **1.98**: lints only, plus `CoerceUnsized` for `&mut` may shorten lifetimes.
- **1.97**: trailing `self` is allowed in more import positions.
- **1.96**: compiler bug fixes. Never types now always coerce in tuple expressions.
- **1.95**:
  - **`if let` guards on match arms** (`MatchArmGuardChain`), with new drop-scope rules (§13).
  - **Importing path keywords with renaming** (`use super as p;`, `use crate as c;`).
  - A provenance model was added to the FLS (original and derived pointers, well-formed pointers).
  - Padding behavior in const-eval typed copies is more consistent.
  - Const blocks no longer participate in deciding promotion.
  - The operational semantics of pattern matching no longer depend on crate or module.
- **1.93**:
  - Byte-wise pointer copies are supported in const eval.
  - `const` items may contain `&mut` to statics.
  - LUB coercion handles fn items and differing fn safety.
  - `asm_cfg` is stable.
- **1.92**: safe `&raw` of union fields; `#[track_caller]` combined with `#[no_mangle]`; `MaybeUninit` validity documented.
- **1.91**: pattern bindings are lowered in written order, and drop order follows the primary (first) alternative's bindings.
- **1.90**: constants may reference mutable or extern memory (but cannot be patterns); volatile access to non-Rust memory, including address 0.
- **1.89**: temporary lifetime extension through tuple-struct and tuple-variant constructors; `repr(u128)` / `repr(i128)`; inferred const args `_`.
- **1.88**: **let chains** (2024 edition); naked functions; `cfg(true)` / `cfg(false)`.
- **1.87**: `asm_goto`; `use<..>` precise capturing in RPITIT.
- **1.86**: trait-object **upcasting**; safe `#[target_feature]` fns.
- **1.85**: **edition 2024**, which brings:
  - tail-expression and `if let` temporary rescoping;
  - `unsafe extern`;
  - unsafe attributes;
  - the `gen` keyword reserved;
  - prelude additions;
  - never-type fallback to `!`;
  - RPIT capturing all in-scope lifetimes;
  - `static mut` reference lint at deny.

  Async closures are also stable in 1.85.
- **1.84**:
  - Raw borrow of `*ptr` is safe.
  - Drop glue runs when unwinding hits an `extern "C"` function boundary (before the abort).
  - Coercions may drop the principal trait of a trait object.
- **1.83**:
  - `&mut`, `*mut`, and `&Cell` are allowed in const.
  - References to statics are allowed in const initializers.
  - Raw lifetimes and labels (`'r#a`).
  - Races between atomic and non-atomic reads are defined.
  - `expr_2021` fragment specifier.
- **1.82**:
  - `&raw const` / `&raw mut`.
  - `unsafe extern` blocks with `safe` items.
  - `unsafe(...)` attributes.
  - Precise capturing `use<..>`.
  - Omitting arms for empty types.
  - Floats in `const fn`.
  - `const` asm operands.
- **1.81**: abort on a panic escaping `extern "C"`.
- **1.80**: exclusive range patterns; zero-byte offsets and ZST accesses on any pointer are allowed; max allocation size documented.
- **1.79**: inline `const {}` blocks.

---

**Implementation takeaways for rint**:
1. Use a two-phase resolver: expansion-time (macros plus `use`, fixpoint with glob ambiguity checks), then type-relative resolution (methods and associated items, following §9's probe order).
2. Give each frame explicit drop-scope stacks with per-binding and per-field initialization flags, implementing §13 exactly. This is edition-sensitive for tail expressions and `if let`.
3. Implement panics as Rust-level unwinding that pops scopes and runs drops. Abort on a panic during a drop-while-unwinding, and abort at non-unwinding ABI boundaries.
4. Statics are never dropped.
5. Use a byte-level memory model (initialized/uninitialized bytes plus provenance) if UB detection is a goal.
