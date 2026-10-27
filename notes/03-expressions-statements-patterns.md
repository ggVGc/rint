# rint evaluation digest: Rust expressions, statements, patterns and functions

Sources: FLS 1.98 (`fls/src/expressions.rst`, `statements.rst`, `patterns.rst`, `functions.rst`) and the Rust Reference (`expressions*.md`, `statements.md`, `patterns.md`, `divergence.md`, `destructors.md`, `types/closure.md`). Where they disagree or one is silent, this says so. Grammar is in simplified EBNF.

---

## 0. Core model for the interpreter

- **Evaluation** applies to expressions and produces a value or a place. **Execution** applies to statements.
- **Place expressions** denote a memory location:
  - paths resolving to a local variable or a `static`
  - `*e`
  - `e.f` and `e.0`
  - `e[i]`
  - `(place)`
  - temporaries

  Everything else is a **value expression**.
- **Place expression contexts** evaluate their operand as a location rather than reading it:
  - LHS of `=` and of compound assignment
  - operand of `&`, `&mut`, `&raw const/mut` and `*`
  - container of field access, tuple-index operand, indexed operand of `a[i]`
  - initializer of `let`
  - scrutinee of `match`, `if let` and `while let` (plus guard `let` scrutinees)
  - base of functional record update `S { ..base }`
  - operand of any **implicit borrow**: method receiver, field container, call operand, indexed operand, `*` operand, **both operands of comparison operators**, LHS of compound assignment, and `format_args!` arguments

  All other contexts are value contexts.
- **Place in a value context.** The value is read. If the type is `Copy` it is copied. Otherwise it is moved and the source becomes uninitialized (only for unborrowed variables, temporaries, fields of movable places whose type is not `Drop`, and `*box`).
- **Value in a place context.** The value is materialized into a **temporary**, and that location is used (FLS `fls_8uhfwqurbyqf`).
- **Implication: comparisons do not move.** `a == b` evaluates to `PartialEq::eq(&a, &b)`, so neither `a` nor `b` is moved.
- **Mutable place expressions** are:
  - a `mut` variable that is not currently borrowed, or a `static mut` (unsafe)
  - a temporary
  - a field of a mutable place
  - `*p` where `p: *mut T`
  - `*r` where `r: &mut T` (works even if the variable `r` itself is not `mut`)
  - `*x` where `x: DerefMut`, which requires `x` itself to be evaluated in a mutable place context
  - `a[i]` where `a: IndexMut`, which evaluates `a` (but not `i`) in a mutable place context

  Assignment, `&mut`, implicit `&mut` and `ref mut` bindings all require a mutable place.
- **Control-flow boundaries** are function bodies, closure bodies, async blocks (async boundary), and const blocks. `return` and `?` target the innermost boundary. `break` and `continue` cannot cross a boundary, so no `break` out of a closure or async block. Model `break`, `continue`, `return`, `?`-early-return and panics as non-local control signals that unwind drop scopes.

## 1. Operator precedence (strongest to weakest)

| Level | Expressions | Associativity |
|---|---|---|
| 1 | Paths, literals, blocks, `if`, `match`, `loop`, parenthesized, tuple, array, struct, `_`, `continue` | n/a |
| 2 | Method call `a.m()` | n/a |
| 3 | Field `a.f`, `a.0`, `.await` | left |
| 4 | Call `f()`, index `a[i]` | n/a |
| 5 | `?` | n/a |
| 6 | Unary `-`, `!`, `*`, `&`, `&mut`, `&raw` | prefix |
| 7 | `as` | left |
| 8 | `*`, `/`, `%` | left |
| 9 | `+`, `-` | left |
| 10 | `<<`, `>>` | left |
| 11 | `&` | left |
| 12 | `^` | left |
| 13 | `\|` | left |
| 14 | `==`, `!=`, `<`, `>`, `<=`, `>=` | **non-associative** (parentheses required; `a < b < c` is an error) |
| 15 | `&&` | left |
| 16 | `\|\|` | left |
| 17 | `..`, `..=` | **non-associative** (parentheses required) |
| 18 | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `\|=`, `^=`, `<<=`, `>>=` | **right** |
| 19 | `return`, `break`, closures | n/a (lowest; they extend as far right as possible) |

Parser notes:
- `-x.pow(2)` parses as `-(x.pow(2))`.
- `&a as *const T` parses as `(&a) as …`.
- `a as u8 < b` is ambiguous with generics (rustc errors), so require parentheses.
- Struct literals are not allowed as a direct `if`, `while`, `match` or `for` subject (`SubjectExpression` excludes `StructExpression`).
- Let-chain operands and guard conditions also exclude lazy boolean, range and assignment expressions.

## 2. Evaluation order (general rule)

Operands are evaluated **left to right, before the operator's effect**. This applies to unary operators, binary operators, `as`, `?`, calls, method calls, index, field access, tuple, array, struct, range, `break`, `return` and `.await`.

**Exceptions:**
- **Assignment `place = value`.** Evaluate the RHS value first, then the LHS place. Drop the old value at the place unless the place is uninitialized. Then move or copy the new value in.
- **Compound assignment `a op= b`.**
  - If both operand types are primitive integer or float: evaluate the RHS first, then the LHS place, then apply the built-in operation.
  - Otherwise: evaluate the LHS place first, then the RHS, then call `OpAssign::op_assign(&mut a, b)`.

  This is explicitly specified in FLS. Overflow rules are the same as for the binary operator.
- **`&&` and `||`** short-circuit: the RHS is evaluated only if the LHS is `true` (for `&&`) or `false` (for `||`).
- **`if`, `match`, loops** evaluate their operands conditionally (see their sections).
- **Destructuring assignment**: the value operand first, then the assignee subexpressions left to right.

**Operands holding temporaries.** While later operands are evaluated, earlier operand results sit in temporaries scoped to the enclosing expression. If a later operand breaks, returns or panics, those earlier temporaries are dropped (in reverse order of creation).

## 3. Expressions: grammar and dynamic semantics

### Literals and paths
- Literal evaluation has no effect; it yields the literal's value.
- Unsuffixed integer literals fall back to `i32`; unsuffixed float literals fall back to `f64`.
- Out-of-range integer literals truncate (for example `128_i8 == -128`; a deny-by-default lint).
- A path resolves to one of: a local (place), a `static` (place), a const (value, freshly instantiated at each use), a const parameter, a fn item (a zero-sized fn-item value), a tuple struct or tuple variant constructor (a fn), or a unit struct or unit variant (a value).

### Block `{ stmt* expr? }`
- Statements execute in order, then the tail expression is evaluated.
- Value:
  - the tail expression's value, or
  - `()` if there is no tail expression and the block does not diverge, or
  - type `!` if there is no tail and every path diverges (`{ loop{}; }: !`, but `{ loop{}; () }: ()`).
- Items declared in a block are visible throughout the block.
- Locals are dropped in reverse declaration order at block exit.
- Edition 2024: temporaries of the **tail expression** are dropped right after it is evaluated, before the block's locals.

Block variants:
- `unsafe { }` behaves like a block.
- `const { }` is evaluated at compile time. If reached at runtime its value is guaranteed to have been evaluated; it is a boundary.
- `'label: { … break 'label v; … }` is a named block. Its value is either the break value or the tail. `continue` cannot target it. A `break` inside a named block that is not inside a loop **must** carry a label.
- `async move? { }` evaluates to an anonymous `Future` that **captures** like a closure (see §7). The body runs only when polled. `return` and `?` inside it complete the future. `break` and `continue` cannot escape it.

### Borrow `& mut? e`, raw borrow `&raw (const|mut) e`
- Evaluate `e` as a place. If it is a value, use a temporary (raw borrow of a temporary is an error).
- Yield the address: `&T`, `&mut T`, `*const T` or `*mut T`.
- `&mut` requires a mutable place.
- `&raw` creates no borrow, and `&raw const *ptr` needs no `unsafe`.

### Dereference `*e`
- Evaluate `e`. For `&T`, `&mut T`, `*const T` or `*mut T`, the result is the pointee place (raw pointers require `unsafe`; dangling or unaligned is UB).
- Otherwise it is `*Deref::deref(&e)`, or `*DerefMut::deref_mut(&mut e)` in a mutable place context.
- Box: `*b` can be moved out of.

### Error propagation `e?` (boundary-relative)
```rust
match Try::branch(e) {
    ControlFlow::Continue(v) => v,
    ControlFlow::Break(r)    => return FromResidual::from_residual(r),
}
```
- `return` here goes to the innermost control-flow boundary: function, closure or async block.
- Practical lowering for the standard types:
  - `Result`: `Ok(v) => v`, `Err(e) => return Err(From::from(e))`
  - `Option`: `Some(v) => v`, `None => return None`
  - `ControlFlow` and `Poll<Result>` also implement `Try`.

### Negation `!e`, `-e`
- `!`: integers give the bitwise NOT; `bool` gives the logical NOT; otherwise call `Not::not`.
- `-`: signed integers negate with **overflow** possible on `MIN`; floats flip the sign (never overflows); otherwise call `Neg::neg`. Unary `-` on unsigned types is a type error.
- Literal exception: `-<literal>` with the most-negative value (for example `-128i8`, `-(128i8)`) never panics.

### Arithmetic `+ - * / %` and bit operators `& | ^ << >>`
- Evaluate the LHS, then the RHS.
- If both operands are the same primitive integer or float type, use the built-in operation. Otherwise call `Add::add(l, r)` and so on.
- `&`, `|`, `^` on `bool` are non-short-circuit logical operators.
- Shifts: the RHS may be **any integer type** (impls exist for all pairs). The result has the LHS type. `>>` is arithmetic for signed types and logical for unsigned types.
- Integer `/` truncates toward zero. `%` takes the sign of the dividend (`-7 % 3 == -1`).
- Float `/` and `%` follow IEEE (`%` is fmod with the sign of the dividend). Division by zero gives ±inf or NaN with no panic.

### Overflow semantics (§9 of the FLS rules; the Reference's "Overflow" section)

| Condition | Debug (overflow-checks on) | Release (overflow-checks off) |
|---|---|---|
| `+`, `-`, `*` out of range | panic "attempt to add with overflow" (etc.) | two's-complement wrap |
| unary `-` on `MIN` (not a literal) | panic | wrap (stays `MIN`) |
| `/` or `%` by 0 (integer) | **always panic** | **always panic** |
| `MIN / -1`, `MIN % -1` (signed) | **always panic** | **always panic** |
| `<<` or `>>` with rhs ≥ bit-width, or rhs negative | panic | rustc masks: `rhs & (bits-1)`, i.e. `wrapping_shl` |
| constant evaluation overflow | compile error | compile error |

- The FLS allows either panic or truncation for overflow. Pick one mode flag in rint; debug panics are the safest default.
- Explicit `wrapping_*`, `checked_*`, `saturating_*` and `overflowing_*` methods are library functions.

### Comparison `== != < > <= >=`
- Evaluate the LHS, then the RHS, as **places** (implicit shared borrow).
- Then call `PartialEq::eq` or `ne`, or `PartialOrd::lt`, `le`, `gt` or `ge`, with `(&l, &r)`. The result is `bool`.
- Primitives: floats use IEEE (NaN is unequal to everything and all orderings are false). `char` compares by code point. `bool` has `false < true`.
- Tuples, arrays, slices and `str` compare lexicographically through their impls.

### Lazy boolean `&&`, `||`
- Both operands are `bool`. Short-circuit as described in §2.
- Each operand is its own temporary scope.

### Type cast `e as T`
- `e` is evaluated, then converted. A cast is legal if it is a coercion or one of the rows below. **Integer casts never panic.**

| From | To | Semantics |
|---|---|---|
| int | int, same size | bit reinterpret (no-op): `-1i8 as u8 == 255` |
| int | smaller int | truncate to the low bits |
| int | larger int | zero-extend if the source is unsigned, sign-extend if it is signed |
| float | int | round toward 0; NaN gives 0; saturate to MIN/MAX (±inf saturates) |
| int | float | nearest value, ties-to-even; overflow gives ±inf (only `u128` to `f32`) |
| f32 | f64 | exact |
| f64 | f32 | nearest value, ties-to-even; overflow gives ±inf; NaN stays NaN |
| fieldless enum (no `Drop`) | int | discriminant, then numeric cast |
| `bool` | int | `false` gives 0, `true` gives 1 |
| `char` | int | code point, then numeric cast (truncation: `'Ö' as u8 == 214`, `'€' as u8 == 0xAC`) |
| `u8` | `char` | code point (only from `u8`; use `char::from_u32` for others) |
| `*T` | `*U` | thin to thin: unchanged; wide to thin: drop metadata; wide to wide: keep metadata (when compatible) |
| `*T` (sized) | int | address (truncated if the int is smaller) |
| int | `*V` (sized) | pointer from address |
| `&[mut] [T;N]` or `*[mut] [T;N]` | `*const/mut T` | array-to-pointer (cannot add `mut`) |
| fn item | fn pointer, `*V`, or int | reify / address |
| fn pointer | `*V` or int | address |
| non-capturing closure | fn pointer | coerce |

- Coercions through `as` include `&mut T` to `&T`, `&T` to `*const T`, `&mut T` to `*mut T`, unsizing (`&[T;N]` to `&[T]`, `&T` to `&dyn Trait`), and fn item to fn pointer.

### Assignment `assignee = value`
- **Basic** (the assignee is a place): see §2. The result is `()`.
- **Destructuring**: the assignee is a tuple, array/slice, struct, tuple-struct or unit-struct expression made of assignee expressions, `_`, and `..` (`..` only inside arrays, tuples and tuple structs).
- It desugars to the following, and the pattern must be irrefutable:
  ```rust
  { let (_a, _b) = value; a = _a; b = _b; }
  ```
  - The assignee expression is lowered to a pattern: places become fresh by-value identifiers, `_` becomes `_`, and `..` becomes rest.
  - **No default binding modes** apply.
  - The same place may appear more than once.
  - The RHS temporaries die at the end of the hidden `let`.
- Swap: `(a, b) = (b, a)` works because the RHS tuple is fully built first.

### Parenthesized `(e)`
- Transparent. It is a place if `e` is a place.

### Array `[a, b, c]` and `[v; N]`
- Elements are evaluated left to right.
- `[v; N]`: `N` is a `usize` const. `v` is evaluated **once**, then copied `N` times. `v` must be `Copy` or a path to a `const` (a const is re-instantiated per element).
- If `N == 0`, `v` is still evaluated once and then dropped.

### Index `a[i]`
- Evaluate `a` as a place, then `i`.
- In a mutable place context: `*IndexMut::index_mut(&mut a, i)`. Otherwise `*Index::index(&a, i)`.
- Built-in arrays and slices with a `usize` index: bounds-checked, **panic** "index out of bounds: the len is L but the index is I".
- Ranges index slices and `str` (with a char-boundary check for `str`).
- Autoderef applies to `a`.

### Tuple `(a, b,)`, unit `()`, field `e.f`, tuple field `e.0`
- Tuple initializers are evaluated left to right. A 1-tuple needs the trailing comma.
- Field access evaluates the container as a place, with **auto-deref** through `&`, `&mut`, `Box` and `Deref` (field resolution). The result is a place; it is mutable iff the container is mutable.
- Reading a union field requires `unsafe` and reinterprets the bits.

### Struct `Path { f: e, g, 0: e, ..base }`
- Operands are evaluated in source order (left to right, base last).
- Shorthand `g` means `g: g`.
- With `..base`: `base` is a place expression context. Fields **not** explicitly listed are moved or copied from `base` (a partial move of `base`). The base must have the same type.
- A union literal has exactly one field.
- Tuple structs and variants can be built as `S { 0: x }` or `S(x)`.

### Call `f(args)`
- Evaluate the callee operand, then the arguments left to right, then invoke.
- Callee kinds:
  - fn item or fn pointer: call directly
  - tuple-struct or tuple-variant constructor: build the value
  - closure or other `Fn*` implementor: `Fn::call`, `FnMut::call_mut` or `FnOnce::call_once` with `(callee_adjusted, (args,))`
- The callee is auto-dereferenced or borrowed as needed: a `&dyn Fn` or `Box<dyn FnOnce>` can be called.
- Argument passing moves or copies into the parameter patterns.
- Calling `unsafe` or `extern` fns needs `unsafe`.
- `<T as Trait>::f(x)` and `Type::method(x)` are plain calls; there is no autoref on arguments.

### Method call `recv.m::<G>(args)`
- Evaluate the receiver (as a place, since there may be an implicit borrow), then the arguments left to right.
- It is then exactly `ResolvedFn(adjusted_recv, args…)`.
- **Resolution (static, but rint must do it).** Build the candidate receiver types:
  1. Start from the receiver type `T0` and repeatedly deref (built-in `*` and `Deref::Target`) to get `T0, T1, … Tn`.
  2. At the end, try an unsizing coercion (array `[T;N]` to `[T]`) and add it if it applies.
  3. Expand each `Ti` into the sequence `Ti, &Ti, &mut Ti`.

  For each candidate in order, look for a visible method whose `self` type equals that candidate, in this order:
  1. **Inherent** methods of the type.
  2. **Trait** methods. For a type parameter, its bounds come first. The trait must be in scope.

  The first step that yields a match wins. More than one match at the same step is an ambiguity error. Mutability, lifetimes and `unsafe` are checked **after** selection.
- Consequence: a `&self` trait method on `T` beats a `&mut self` inherent method on `T`, because `&T` precedes `&mut T`.
- `Box<[i32;2]>` gives `Box<…>`, `&Box`, `&mut Box`, `[i32;2]`, `&[i32;2]`, `&mut [i32;2]`, `[i32]`, `&[i32]`, `&mut [i32]`.
- The receiver adjustment is applied as that many derefs plus an optional `&` or `&mut` autoref (`&mut` is an implicit reborrow; the place must be mutable).
- `dyn Trait` receivers dispatch dynamically through the vtable.
- Edition ≥ 2021: `array.into_iter()` resolves to `IntoIterator for [T;N]` (by value). Before 2021 it resolved to the slice iterator.

### Closure `async? move? |pat: T?, …| body`, or `|…| -> R { block }`
- Evaluating it creates a closure value that holds the captures. **The body does not run.**
- Parameters are irrefutable patterns with no top-level `|`.
- The body is a control-flow boundary.
- `async` closures return a future when called.

Capture rules and trait inference are in §7.

### Loops (`Label? loop | while | while let | for`, with body `{}`)
- The body type must be `()`.
- **`loop { b }`**: run `b` repeatedly. The value is the operand of the `break` that exits it. Without any `break` the type is `!`.
- **`while cond { b }`**: evaluate `cond` (`bool`); if `true`, run the body and repeat. The value is `()`. `break` carries no value.
- **`while let P = e { b }`**: `'l: loop { match e { P => b, _ => break } }`. Let-chains are allowed in Reference edition 2024: `while let P = e && cond && let Q = f`.
- **`for P in e { b }`** desugars to:
  ```rust
  match IntoIterator::into_iter(e) { mut iter => 'l: loop {
      match Iterator::next(&mut iter) { Some(v) => { let P = v; b } None => break } } }
  ```
  The value is `()`. `P` is irrefutable. The temporaries of `e` live for the whole loop (because `match` is used).
- **`break 'l? e?`**:
  - Evaluate `e` (the default is `()`).
  - Terminate all loops and labelled blocks up to and including the target, running drops.
  - The target is the innermost loop, or the labelled loop or block. `break` with a value is only allowed for `loop` and labelled blocks.
  - The type of `break` itself is `!`.
- **`continue 'l?`**: terminate the inner iterations and restart the target loop. For `while` that means re-testing the condition; for `for` it means calling `next`. Its type is `!`.
- Labels are hygienic and can shadow. `'_` is not a valid label.

### Range expressions

| Syntax | Value |
|---|---|
| `a..b` | `Range { start: a, end: b }` |
| `a..` | `RangeFrom { start: a }` |
| `..b` | `RangeTo { end: b }` |
| `..` | `RangeFull` |
| `a..=b` | `RangeInclusive::new(a, b)` (has a private `exhausted` flag) |
| `..=b` | `RangeToInclusive { end: b }` |

- Operands are evaluated left to right; the bound types must unify.
- They are just struct values. Iteration comes from the `Iterator` impls (`Range<int>`, `RangeFrom` overflow panics in debug, and so on).
- In a destructuring-assignment array, `..` is the rest pattern.

### `if` / `if let` / let chains
- Grammar:
  ```
  if Conditions Block (else (Block | if…))?
  Conditions = Expr | LetChain
  LetChain   = (Expr | let Pat = Scrutinee) (&& …)*
  ```
- Plain `if`: evaluate the `bool` condition, then the then-block or the else branch. Without `else` the value is `()` and the then-block must have type `()`.
- `if let P = e {A} else {B}` is `match e { P => A, _ => B }`.
- **Let chains** (edition 2024; the FLS grammar here only has a single `let`):
  - Condition operands are evaluated left to right and stop at the first `false` or failed match.
  - Bindings from earlier `let`s are in scope for later operands and for the then-block.
  - `||` is not allowed at the top level of a chain containing `let`; use parentheses.
  - Nested-`if` equivalence holds.
- Temporaries: the plain condition is its own temporary scope (dropped before the branch runs).
  - Edition 2024: `if let` scrutinee temporaries are dropped **before the `else` block**.
  - Earlier editions keep them until the end of the whole `if let` statement.

### `match`
- Grammar:
  ```
  match Scrutinee { (OuterAttr* Pattern (if Guard)? => Expr ,?)* }
  ```
  An arm body that is an expression-without-block needs a `,` unless it is the last arm.
- Steps:
  1. Evaluate the scrutinee **as a place** (a value becomes a temporary). It is not moved by itself, only by by-value bindings.
  2. Try arms in source order. For each pattern alternative of an or-pattern, in source order: match it. If it matches and there is no guard, select the arm. If there is a guard, evaluate it; on `true` select; on `false` leave the arm's drop scope and try the **next alternative** (the same guard may be re-run per alternative) or the next arm.
  3. Evaluate the selected arm body. Its value is the match's value.
- Guards:
  - During the guard, bindings are accessed via **shared reference** to the matched part.
  - By-value moves or copies into the bindings happen **only after the guard succeeds**.
  - Bindings are immutable inside the guard.
  - The guard may be a chain `if a && let P = e && …` with the same left-to-right semantics as let chains; guard-`let` bindings are visible in the arm body.
- Exhaustiveness is checked statically (arms without guards must cover the type). A `match` on an uninhabited type with zero arms has type `!`.
- Scrutinee temporaries live until the end of the enclosing **statement** (`match` is not a temporary scope).

### `return e?`
- Evaluate `e` (the default is `()`), move it to the boundary's output, unwind all scopes in the frame (dropping locals inside-out), and return to the caller.
- Type `!`.

### `.await` (only directly inside an async boundary: async fn, async block, or async closure)
```rust
match IntoFuture::into_future(e) { mut fut => loop {
    match unsafe { Pin::new_unchecked(&mut fut) }.poll(cx) {
        Poll::Ready(v) => break v,
        Poll::Pending  => { yield Poll::Pending /* suspend; resume here when re-polled */ }
    } } }
```
- `cx` is the `Context` passed to the enclosing future's `poll`.
- For rint, implement async bodies as resumable state machines or coroutines.

### Underscore `_`
- Only valid as (part of) an assignee in destructuring assignment. It discards the value.

### Constant expressions and contexts
- Const contexts:
  - initializers of `const`, `static` and associated consts
  - array lengths and repeat counts
  - enum discriminants
  - const generic arguments and defaults
  - `const {}` blocks
- They are evaluated at compile time. A panic or overflow there is a **compile error**.
- Calls are allowed only to `const fn`. Trait operator calls are not allowed (only built-in scalar operators).
- `const fn` called at runtime behaves like a normal fn.
- Constant promotion: `&<const-evaluable expr without Drop or interior mutability>` (for example `&5`, `&None`) becomes `&'static`.

## 4. Temporaries and drop scopes

- **Drop scopes**: the whole function (parameters), each statement, each expression, each block, and each match arm. Leaving a scope drops its variables in reverse declaration order and its temporaries in reverse creation order. Multiple scopes are exited inside-out.
- **Temporary scope** of a value in a place context: the smallest enclosing one of:
  - the function
  - a statement
  - the body of an `if`, `while` or `loop`
  - an `else` block
  - an `if` or `while` condition, or a non-`let` guard
  - a match-arm (guard plus body)
  - each operand of `&&` and `||`
  - (2024) the `if let` condition plus consequent
  - the `while let` condition plus body
  - (2024) the block tail expression

  Usually this means temporaries die at the `;` of the enclosing statement. `match` and `for` scrutinees are not temporary scopes.
- **Parameters** live in the function scope and are dropped last, after body locals. Each parameter value is dropped after the bindings of its pattern.
- **Pattern bindings** drop in reverse declaration order. An or-pattern uses the order of its first alternative.
- **Lifetime extension** applies to `let` with an initializer (and `static`/`const`, where it extends to `'static`). The temporary lives until the end of the enclosing **block** if either of the following holds.
  1. **The pattern is extending.** That means `ref` or `ref mut` identifier patterns, or struct, tuple, tuple-struct, slice or or-patterns with an extending direct subpattern. `&ref x` is not extending.
  2. **The temporary is the operand of an extending borrow.** The extending expressions are:
     - the initializer itself
     - the operand of an extending `&`, `&mut` or `&raw`
     - operands of extending array, tuple, braced-struct, cast, tuple-struct or tuple-variant constructor expressions
     - the tail of an extending block (also `unsafe` and `const` blocks, but **not** `async`)
     - the tails of the branches of an extending `if`/`else`
     - the arms of an extending `match`
     - super operands of `pin!` and `format_args!`

  Propagation: if a borrow, deref, field, tuple-index or index expression is extended, so is its operand (for index, the indexed operand).

  Extended: `let x = &temp();`, `let x = Some(&temp());`, `let x = if c { &a() } else { &b() };`.
  Not extended: `f(&temp())`, `(&temp()).m()`, `match &temp() { x => x }`, `loop { break &temp() }`, and closure or async bodies.

## 5. Statements

- Grammar:
  ```
  Statement = ; | Item | LetStatement | ExpressionStatement | MacroInvocation;
  ```
- **Empty `;`**: no effect.
- **Item statement**: scoped to the block and visible throughout it (order-independent). It does **not** capture outer locals, generics or parameters. Impl items inside remain globally effective.
- **`let PatNoTopAlt (: T)? (= e (else { … })?)? ;`**
  1. Evaluate `e` (a place context).
  2. Match it against the pattern and bind.
  3. Without an initializer, the bindings are **uninitialized** (deferred initialization: `let x; if c { x = 1 } else { x = 2 }`). Assigning to an uninitialized variable does not drop anything.
  - Without `else`, the pattern must be irrefutable.
  - **let-else**: the pattern may be refutable. On mismatch, run the `else` block, which **must diverge** (type `!`). `e` must not be a lazy boolean expression or end with `}`. The bindings are in scope after the statement and are not visible in `else`.
  - Bindings come into scope after the statement, so `let x = x + 1;` shadows correctly.
- **Expression statement** `ExprWithoutBlock ;` or `ExprWithBlock ;?`: evaluate, then drop the result (temporaries die at the end of the statement).
  - A block-like expression (`{}`, `if`, `match`, `loop`, `while`, `for`, `unsafe {}`, labelled block) **at statement start** is parsed as a statement and may omit `;`. If it omits `;` its type must be `()`.
  - Consequences: `{ 1 } - 1` at statement start parses as block `{1}` followed by `-1`, and `[42];` is an array, not an index.
  - In tail position (the last thing in a block, without `;`) it is the tail expression.
- **Macro statement** `m!(…);` or `m!{…}`: expand, then treat as statements, items or an expression.

## 6. Patterns

- Grammar:
  ```
  Pattern = |? PWA (| PWA)*        (PWA = pattern without alternation)
  ```
  Top-level `|` is forbidden in `let` and in fn and closure parameters (wrap it in parentheses). All alternatives must bind the same names with the same types and binding modes.

| Kind | Syntax | Matches / binds | Refutable? |
|---|---|---|---|
| Identifier | `ref? mut? x (@ P)?` | binds the value (after `P` matches, if given). If the name resolves to a const, unit struct or unit variant, it is a path pattern instead | irrefutable unless `@ P` is refutable |
| Wildcard | `_` | anything; **does not bind, move or read** | no |
| Rest | `..` | zero or more elements in a tuple, tuple-struct or slice (at most once); `x @ ..` in a slice binds the subslice or subarray | no |
| Literal | `-`? literal (bool, char, byte, int, float, str, byte string) | equality with the literal (no NaN) | yes |
| Range | `a..=b`, `a..b`, `a..`, `..=b` (`...` is obsolete, `..=`). Bounds are char, byte or numeric literals, `-lit`, or paths to consts | `lo <= v <= hi`, `lo <= v < hi`, `v >= lo` | only if it covers the whole type (e.g. `0u8..=255`) |
| Reference | `&P`, `&mut P` | deref the matched reference, then match `P` (the mutability must match) | as `P` |
| Tuple | `(P, Q, ..)` | positional. After `..`, positions count from the end | if any subpattern is |
| Tuple struct | `Path(P, .., Q)` | variant or struct check, then positional fields | yes if the enum has more than one variant, or a subpattern is |
| Struct | `Path { f: P, g, ref mut h, 0: P, .. }` | variant check, then named fields; without `..` all fields must be listed | same as tuple struct |
| Slice | `[P, Q, rest @ .., Z]` | arrays: fixed length. Slices: if there is no `..`, the length must be exactly the number of patterns; if there is `..`, the length must be ≥ the number of non-rest patterns. Elements after `..` index from the end | slices are refutable unless the pattern is `[..]` or `[x @ ..]`; arrays are refutable only via their elements |
| Path / const | `Enum::V`, `CONST`, `<T>::C` | equality with the const value, which must have structural equality (derived `PartialEq`, no NaN). Behaves as if the const's value had been written as a pattern | yes unless it is a single-variant enum, unit struct, or a const of an irrefutable type |
| Parenthesized | `(P)` | as `P` | as `P` |

- **Matching algorithm** (recursive over pattern and value):
  - An or-pattern tries alternatives left to right.
  - Identifier: match `@ P` first, then bind according to the binding mode.
  - Struct patterns check the variant and then match fields in **source order of the subpatterns**; the first failure short-circuits.
  - Only the bindings of the successful alternative exist.
- **Binding modes / default binding mode (match ergonomics):**
  - The default binding mode (DBM) starts as `move`.
  - Walking the pattern outside-in, whenever a **reference value** is matched by a **non-reference pattern**, auto-deref the value and update the DBM. Non-reference patterns are everything except bindings, `_`, `&`/`&mut` patterns, and consts of reference type.
    - A `&T` sets DBM to `ref`.
    - A `&mut T` sets DBM to `ref mut`, unless DBM is already `ref`, in which case it stays `ref`.
    - Repeat while the value is still a reference.
  - A binding without an explicit `ref`, `ref mut` or `mut` uses the DBM: `move` binds by value, `ref` binds `&v`, `ref mut` binds `&mut v`.
  - Explicit `ref x` binds `&v` and `ref mut x` binds `&mut v`, regardless of whether the type is `Copy`.
  - **Edition 2024**: explicit `ref`, `ref mut` or `mut` on a binding, and `&` patterns, are errors unless DBM is `move`. Before 2024, `mut x` reset DBM to `move` (by value), and a `&` pattern reset DBM to `move`.
  - Example: `let x: &Option<i32> = &Some(3); if let Some(y) = x {…}` gives `y: &i32`.
  - Default binding modes do **not** apply in destructuring assignment.
- **By-value binding**: copy if the type is `Copy`, else move, which can cause a **partial move** of the scrutinee. Mixing `ref` and move bindings is allowed: `let Person { name, ref age } = p;`.
- `mut x` makes the binding mutable. It does not make the source mutable (the FLS phrasing about requiring a mutable context value describes the binding's own place).
- **Refutability**: `let` without `else`, `for`, and fn and closure parameters require irrefutable patterns. `if let`, `while let`, `let … else` and `match` arms accept refutable ones.

## 7. Closures and capture

- **Capture targets**: variables and **places** (edition 2021: *disjoint/precise capture* by **capture path** = variable plus projections: field, tuple index, deref, index, pattern destructuring).
- **Capture mode per path** is the lowest mode compatible with use in the body:
  `ImmBorrow (&) < UniqueImmBorrow < MutBorrow (&mut) < ByValue (move)`
  - `UniqueImmBorrow` is used when the body mutates through an `&mut` held in a non-`mut` variable (`*r = …` where `r: &mut T`).
  - **Copy** values used by value in a non-`move` closure are captured by `ImmBorrow` (copied at use).
  - A `move` closure or `async move` block captures **every** path by value (move, or copy for `Copy` types). Paths are truncated before the first deref of a reference, so a reference itself is moved in.
- **Precision and truncation rules**:
  - If an ancestor path and a descendant path are both captured, capture only the ancestor, using the max mode (recursively).
  - Truncate at the **rightmost deref of a shared reference**.
  - Truncate before the first deref of a **raw pointer**.
  - Union fields: capture the union.
  - Fields of `repr(packed)` structs: capture the struct up to the packed field.
  - Arrays and slices are captured **whole** (no per-index capture).
  - Moving out of `*box` in a non-move closure, or reading `*box` in a move closure, captures the whole `Box`.
  - `_` patterns and `..` rest **do not capture**. Destructuring a struct, tuple or single-variant enum alone does not read it.
  - Matching a multi-variant enum variant (or a `#[non_exhaustive]` enum), a range pattern, or a slice pattern other than `[..]` reads the discriminant or length, which captures `ImmBorrow`. Matching an array against a slice pattern does not capture.
- **Edition ≤ 2018**: whole variables are captured, never fields.
- **Drop order consequence** (2021): captured paths are dropped when the closure is dropped. The remaining fields of the variable are dropped at the variable's scope end.
- **Fn trait inference**, from what the body *does* with the captures (not how they were captured):
  - Every closure implements `FnOnce`.
  - It implements `FnMut` if it does not move out of any capture.
  - It implements `Fn` if it neither mutates nor moves out of any capture.
  - A `move` closure can still be `Fn`.
  - Non-capturing, non-async closures coerce to `fn` pointers.
  - Async closures implement `AsyncFn*`. They are not `Fn`/`FnMut` if they are "lending" (the returned future borrows captured state mutably or moves from it).
- **Calling** (rint):
  - `Fn` passes the environment by `&`.
  - `FnMut` passes it by `&mut`, so the closure binding must be mutable.
  - `FnOnce` consumes the closure: by-value captures may be moved out, and a second call is a static error.
- **Async blocks** capture exactly like closures, and async fns are desugared to them (§8).

## 8. Functions

- Grammar:
  ```
  const? async? unsafe? (extern "abi")? fn name<G>(params) (-> T)? where… { body }
  ```
- `self` parameter forms:

  | Form | Type |
  |---|---|
  | `self` | `Self` |
  | `&self` | `&Self` |
  | `&mut self` | `&mut Self` |
  | `mut self` | `Self` (mutable binding) |
  | `self: Type` | the stated type, e.g. `Box<Self>`, `Rc<Self>`, `Pin<&mut Self>` |
- Parameters are irrefutable patterns (no top-level `|`) with no shadowing between parameters.
- **Call semantics**:
  1. Argument values are moved or copied in.
  2. Each argument is matched against its pattern, in order.
  3. The body block runs. Its value (or the `return` value) is the result.
  4. Drop order: body locals, then parameter pattern bindings, then the parameter values (later parameters first).
- The default return type is `()`. The body is a control-flow boundary.
- `async fn f(p: &P) -> R { body }` is `fn f<'a>(p: &'a P) -> impl Future<Output=R> + 'a { async move { body } }`. **All parameters are moved into the future, even unused ones.** Nothing runs until it is polled.
- `const fn` is callable in const contexts and behaves normally at runtime. `async` and `const` together are not allowed.
- `unsafe fn` calls require `unsafe`.
- Variadics apply only to `extern "C"`-family foreign functions.

## 9. Never type and divergence

- These have type `!`:
  - `break`, `continue`, `return`
  - `loop` without a `break` targeting it
  - `panic!`, `unreachable!`, `todo!`
  - calls to `-> !` functions
  - `match` on an empty enum with zero arms
- `!` coerces to any type.
- A block, `if` or `match` **diverges** when every reachable path contains a diverging expression (except an unread place of type `!`). A block with no tail expression that diverges has type `!`.
- Expressions of other types can also diverge, for example `Some(loop {})`.
- Uninhabited types like `enum Empty {}` are **not** automatically diverging.
- Inference fallback: a type variable unified only with diverging expressions falls back to `!` (edition 2024; earlier editions use `()`).
- Interpreter view: a divergent expression never produces a value; it produces a control signal (`Break(label, v)`, `Continue(label)`, `Return(v)`, `Panic`). Panics unwind and run drops, unless panic=abort.

## 10. FLS vs Reference notes and gotchas

- FLS 1.98 `IfLetExpression` and `WhileLetLoopExpression` show only a single `let`. The Reference (edition 2024) has **let chains** in `if` and `while`. Both have chains in match guards. Implement chains.
- The FLS lists method call at a higher precedence than field access. The Reference orders paths, then method calls, then fields (left-assoc) with `.await` alongside fields, then calls/index, then `?`. They are compatible: all postfix operators, applied left to right.
- The FLS overflow rule permits panic **or** truncation. rustc's debug and release split is the de facto behavior. `/` and `%` by zero and `MIN/-1` always panic.
- The FLS says shift overflow "results in arithmetic overflow". rustc release behavior **masks** the shift amount; it does not produce zero.
- The FLS gives an RHS-first order for primitive compound assignment and LHS-first for overloaded compound assignment. Basic assignment is always RHS-first.
- The `match` scrutinee and the `for` iterator keep their temporaries alive across the whole construct (common drop-order surprises).
- Edition switches rint must track:
  - closure capture precision (2021)
  - `array.into_iter()` (2021)
  - `if let` and tail-expression temporary scopes (2024)
  - match-ergonomics restrictions (2024)
  - let chains (2024)
  - `!` fallback (2024)
  - `...` range patterns (removed in 2021)
