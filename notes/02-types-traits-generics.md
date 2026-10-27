# rint digest: Types, Traits, Generics, Impls, Associated Items, Values (FLS 1.98.0 + Reference)

Sources: the FLS files (`types-and-traits.rst`, `generics.rst`, `implementations.rst`, `associated-items.rst`, `values.rst`, plus the "Constant Expressions" section of `expressions.rst`), cross-checked against the Rust Reference. **[FLS]** and **[Ref]** mark claims specific to one source. **[!]** marks a typo in the spec or a place where the two disagree. The implementer should follow rustc's behaviour there.

---

## 1. Type grammar (top level)

```
TypeSpecification ::= ImplTraitTypeSpecification | TraitObjectTypeSpecification
                    | TypeSpecificationWithoutBounds
TypeSpecificationWithoutBounds ::= ArrayType | FnPtrType | `impl` OneBound | `_`
    | MacroInvocation | `!` | `(` Type `)` | QualifiedTypePath | RawPtrType
    | RefType | SliceType | `dyn` OneBound | TupleType | TypePath
TypeAscription ::= `:` TypeSpecification
```
- The `WithoutBounds` split exists so that `&dyn A + B` is rejected. It must be written `&(dyn A + B)`. Raw pointers and references take `WithoutBounds`.
- A *local type* is a type defined in the current crate.
- Classification:
  - Scalar: `bool`, `char`, integers, floats.
  - Sequence: array, slice, `str`, tuple.
  - ADT: enum, struct, union.
  - Function types: closures and fn items.
  - Pointers: raw pointers and references.
  - fn pointers.
  - Trait types: `impl Trait` and `dyn Trait`.
  - Other: inferred `_`, never `!`, parenthesized types.

## 2. Type kinds

### Scalars
- **bool**: `false` = `0x00`, `true` = `0x01`. Any other bit pattern is invalid (validity invariant). Supported ops: `! & | ^ == > ...`, with `false < true`. Derived comparisons: `a!=b ≡ !(a==b)`, `a>=b ≡ a==b | a>b`, `a<b ≡ !(a>=b)`.
- **char**: a 32-bit unsigned value in `0..=0xD7FF ∪ 0xE000..=0x10FFFF`. A surrogate or a value above `0x10FFFF` is invalid (validity invariant).
- **Integers**:
  - `u8/16/32/64/128` range over `0..2^N-1`. `i8..i128` range over `-2^(N-1)..2^(N-1)-1`.
  - `usize`/`isize` have pointer width, at least 16 bits. They have the same size and alignment as each other.
  - `align(uN) == align(iN)`.
- **Floats**: `f32` and `f64` are IEEE 754-2008 binary32 and binary64. NaN sign bits need not be preserved by operations.

### Sequences
- **Array** `[T; N]`:
  - `T` must be `Sized`.
  - `N` is a const expression of type `usize`, or the inferred const `_`.
  - It is a static error if `N` depends on generic parameters. Use a const generic `N` directly instead.
- **Slice** `[T]`: a DST. `T` must be `Sized`.
- **str**: a DST with the layout of `[u8]`. It must be valid UTF-8; this is a *safety* invariant, not a validity invariant.
- **Tuple** `( T, ... )`:
  - A one-element tuple needs a trailing comma: `(T,)`.
  - `()` is the unit type.
  - Only the last field may be a DST.

### ADTs
```
EnumDeclaration ::= `enum` Name Generics? Where? `{` EnumVariantList? `}`
EnumVariant ::= Attr* Vis? Name (`=` Expr | RecordStructFieldList | TupleStructFieldList)?
StructDeclaration ::= `struct` Name Generics? Where? `{`fields`}`
                    | `struct` Name Generics? `(`fields`)` Where? `;`
                    | `struct` Name Generics? Where? `;`
UnionDeclaration ::= `union` Name Generics? Where? RecordStructFieldList
```
- **Enum discriminants**:
  - The FLS allows explicit `= expr` only when *all* variants are fieldless. Current Rust also allows explicit discriminants on data-carrying enums that have a primitive repr; the FLS text does not say so **[!]**.
  - The expression type is the repr primitive type if one is given, otherwise `isize`. It is a type inference root and a const context.
  - Values: the explicit value; otherwise 0 for the first variant; otherwise the previous value + 1.
  - Duplicate values are an error, as is overflowing the discriminant type.
  - Generic parameters may not be used in a discriminant initializer.
  - A zero-variant enum has no values and is uninhabited.
- **Struct**:
  - Field names must be unique.
  - Only the last field may be a DST, which makes the whole struct a DST.
- **Union**:
  - It must have at least one field.
  - All fields share the same storage at offset 0.
  - Each field must be one of: a `Copy` type, `&mut T`, `ManuallyDrop<T>`, a tuple of valid union field types, or an array of a valid union field type. So union fields never need drop.
  - Reading a field is `unsafe`. Creating a union value from uninitialized memory is *not* UB; unions are the only exception.
- **Recursive types**: only ADTs may be recursive, and the size must be finite, so recursion needs indirection such as `Box`.

### Function types
- **Closure type**:
  - Unique and anonymous. It holds the captured environment.
  - Every closure implements `FnOnce`. It also implements `FnMut` if it does not move out of captures, and `Fn` if it neither moves out of nor mutates captures.
  - A non-capturing closure coerces to a `fn` pointer.
  - It is `Copy`/`Clone`/`Send` if all captured values are.
  - `Sync`: the FLS text says "if all captures are Send" **[!]**. This is a typo. The rule is that captures must be `Sync`, taking capture mode into account (Ref: a by-ref capture of `T` contributes `&T`).
  - Layout is tool-defined.
- **Fn item type**:
  - A unique zero-sized type per function and generic instantiation.
  - Always `Copy + Clone + Send + Sync`.
  - It implements `Fn`/`FnMut`/`FnOnce` unless the function is `unsafe`, has `#[target_feature]`, or has a non-Rust ABI.
  - It coerces to a `fn` pointer.

### Pointers
```
RawPointerType ::= `*` (`const` | `mut`) TypeWithoutBounds
ReferenceType  ::= `&` Lifetime? `mut`? TypeWithoutBounds
FnPtrType ::= (`for` Generics)? `unsafe`? (`extern` Abi?)? `fn` `(` Params? `)` (`->` TypeWithoutBounds)?
FnPtrParams ::= Param (`,` Param)* (`,` Attr* `...` | `,`?)     // Param ::= Attr* (Ident|_ `:`)? Type
```
- `&T` is `Copy`. `&mut T` is not `Copy`.
- References are never null (validity invariant).
- A `fn` pointer is never null (validity invariant). Its default return type is `()`. The variadic `...` form is for C-variadic signatures only.

### Trait types
```
ImplTraitType ::= `impl` UseCaptures? TypeBoundList
UseCaptures   ::= `use` `<` (Ident | Lifetime),* `>`
TraitObjectType ::= `dyn` TypeBoundList
```
- **`impl Trait`** is allowed only in function parameter types (anonymous generic parameter) and return types (opaque type, "anonymous return type").
  - No `?Sized`-style opt-out bounds.
  - The return position captures all in-scope type and const parameters. Lifetime parameters are captured implicitly in edition 2024 and for `async fn`. `use<..>` makes the capture list explicit.
  - In a trait method return (RPITIT), `impl Trait` is sugar for a fresh anonymous associated type. In the impl, it refers to that associated type.
  - Lifetimes cannot be implicitly elided on `impl` trait types.
- **`dyn Trait`**:
  - The first trait is the principal trait and must be dyn compatible. All other trait bounds must be auto traits.
  - At most one lifetime bound. No `?Trait` bounds.
  - It is a DST. Method calls dispatch dynamically through the vtable.
- **Never `!`**: has no values and unifies with any type. It coerces to any type.
- **Inferred `_`** is forbidden in item signatures:
  - type alias right-hand sides
  - function return types
  - const, static, and struct field ascriptions
  - const parameter types
  - function parameter types
  - impl self types
  - tuple struct fields
  - where-clause types
  - type parameter defaults

### Type aliases
```
TypeAliasDeclaration ::= `type` Name Generics? (`:` Bounds)? Where? (`=` Type Where?)? `;`
```
- Bounds are allowed only on associated types.
- Bounds on a free alias's generic parameters are not enforced.
- The trailing where-clause on a free alias is rejected.
- An alias unifies wherever its aliased type does.

## 3. Layout

**General rules**
- Every value has a size and an alignment. Alignment is a power of two, at least 1.
- Size is the array stride, so it is a multiple of the alignment. A ZST has size 0, and 0 is a multiple of any alignment.

**Sizes of primitives**

| Type | Size |
|---|---|
| `bool`, `u8`, `i8` | 1 |
| `u16`, `i16` | 2 |
| `u32`, `i32`, `f32`, `char` | 4 |
| `u64`, `i64`, `f64` | 8 |
| `u128`, `i128` | 16 |
| `!` | 0 |
| `usize`, `isize` | pointer width |

- Alignment is platform-specific and may be smaller than size. For example, `u128` often has align 8 (or 16 on newer x86_64 rustc), and `u64` can have align 4 on 32-bit targets.
- For rint, pick a target model and fix these values.

**Arrays, slices, str**
- `[T; N]`: size = `size_of::<T>() * N`, align = `align(T)`. Element *i* is at offset `i * size_of::<T>()`.
- `[T]` is laid out like the array it views. `str` is laid out like `[u8]`.

**Pointers**
- A thin pointer (`&T`, `*T`, `Box<T>` with `T: Sized`) and a `fn` pointer have the size and alignment of `usize`.
- A fat pointer to a slice, `str`, or `dyn` is `(data_ptr, metadata)`: size `2 * usize`, align `usize`.
  - Slice metadata is the length in elements.
  - `dyn` metadata is a vtable pointer. The vtable holds drop_in_place, size, align, then methods; the format is an implementation choice.
- Mutability does not change layout. References and raw pointers have the same layout.

**Tuples and closures**
- Tuples use the Rust (unspecified) representation. `()` has size 0 and align 1.
- Closures have no layout guarantees.

**`dyn Trait` value**: same layout as the concrete value behind it. Size and alignment come from the vtable at runtime (`size_of_val`).

**Representations** (`#[repr(...)]` on struct, enum, or union). A repr changes padding and field order, not the internal layout of the fields.
- **Rust (default)**:
  - Each field offset is a multiple of the field's alignment.
  - The type's alignment is at least the maximum field alignment.
  - Struct fields do not overlap, but may be reordered.
  - Structs with no fields, or only ZST fields, are ZSTs.
  - A zero-variant enum is a ZST.
  - A single-variant enum whose fields are all ZSTs is a ZST.
  - Nothing else is guaranteed, so the interpreter is free to choose (for example, reorder by decreasing alignment).
  - repr does not depend on generic arguments: `Foo<A>` and `Foo<B>` share the same repr kind.
- **`repr(C)` struct**: alignment = maximum field alignment (1 if no fields). Size and offsets:
  1. Start with `off = 0`.
  2. For each field in declaration order: `off = round_up(off, align_f)`, record the field offset, then `off += size_f`.
  3. Finally `size = round_up(off, align)`.
- **`repr(C)` union**: size = maximum field size rounded up to the alignment; alignment = maximum field alignment. All fields are at offset 0.
- **`repr(C)` fieldless enum**:
  - Same size and alignment as a C `enum` with those values (typically `c_int`).
  - FLS: the discriminant type is C `int`. Ref: discriminant values are `isize`, and the size is chosen like C would choose it.
  - `repr(C)` is not allowed on a zero-variant enum.
- **Primitive repr** (`u8`…`isize`) on an enum:
  - Enums only. Not on a zero-variant enum, and two primitive reprs cannot be combined.
  - Fieldless enum: the size and alignment of the primitive type.
  - Enum with fields: a `repr(C)` **union** of `repr(C)` structs, one per variant, each laid out as `{tag: prim, fields...}`.
- **`repr(C)` enum with fields (Ref)**: a `repr(C)` **struct** `{ tag: repr(C) fieldless enum, payload: repr(C) union of repr(C) variant structs }`.
  - `repr(C, u8)` is the same shape, with a `u8` tag.
  - These are *different* layouts from `repr(u8)`. For example, `enum {A(u8), B}` has size 8 with `repr(C)`, size 2 with `repr(C,u8)`, and size 4 with `repr(C,u16)`.
  - **[!]** The FLS says repr(C) and primitive-repr enums both use the union-of-structs layout. That is wrong for plain `repr(C)`. Follow the Reference.
- **`repr(transparent)`**: struct, or enum with exactly one variant.
  - It may have at most one field that is not (size 0 and align 1), plus any number of size-0, align-1 fields such as `PhantomData`.
  - Layout and ABI are those of that one field, or unit (size 0, align 1) if there is none.
  - It cannot be combined with another repr.
- **`align(N)` / `packed(N)`**:
  - Allowed on Rust/C struct and union. `align` is also allowed on enums, with the same effect as wrapping the enum in an aligned newtype.
  - `N` is a power of two in `1..=2^29`. Bare `packed` means `packed(1)`.
  - `align` only raises alignment. `packed` only lowers it.
  - Under `packed(N)`, each field is positioned using alignment `min(N, field_align)`, with minimal padding. `packed(1)` gives no padding.
  - `align` and `packed` cannot both apply to one type. A packed type cannot transitively contain an `align`ed type.
  - Taking a reference to an unaligned packed field is an error (UB). Use `&raw const` plus `read_unaligned`.
- **Niches / enum layout optimisation**: not specified by the FLS or the Reference; it falls under "tool-defined".
  - std guarantees `Option<T>` has the same size as `T` with `None` = all-zero for `T` in `&T`, `&mut T`, `Box<T>`, `NonNull<T>`, `NonZero*`, `fn` pointers.
  - Known invalid values usable as niches: bool 2..=255, char above `0x10FFFF` or in the surrogate range, null for references, out-of-range enum discriminants.
  - An interpreter can choose not to optimise, except that FFI or transmute code relying on the `Option` guarantees needs them.
- **`MaybeUninit<T>`**: same size, alignment, and ABI as `T`, and always valid. **`PhantomData<T>`**: ZST, align 1.

## 4. DSTs
- A type is a DST if it does not implement `Sized`: `[T]`, `str`, `dyn Trait`, and structs or tuples whose last field is a DST.
- A DST can only be used behind a pointer, which is a fat pointer.
- Type parameters have an implicit `Sized` bound, removed with `?Sized`. Associated types are also implicitly `Sized`. `Self` in a trait is *not* implicitly `Sized`.
- Constants, statics, `let` bindings, function parameters, and return values must be `Sized`.

## 5. Type inference (FLS model)

**Inference roots**
- Roots are inferred independently: function bodies, const/static initializers, discriminant initializers, const parameter defaults, const arguments, and array size operands.
- Expected type of each root:
  - fn body: the return type
  - const/static: the ascribed type
  - discriminant: the repr primitive type or `isize`
  - array size: `usize`
  - const argument: the const parameter's type

**Type variables**
- Kinds: *global* (any type), *integer* (from an unsuffixed int literal), *float*, *diverging* (from diverging expressions), and lifetime variables.

**Unification** (symmetric). Variable cases:
1. A global variable is assigned the other type.
2. Otherwise a diverging variable is assigned the other type.
3. An integer variable unifies only with an integer type or another integer variable.
4. A float variable unifies only with a float type or another float variable.

Concrete cases:
- A scalar type unifies only with itself. `!` unifies with anything.
- Arrays: element types unify and the lengths are equal. Slices: element types unify. `str` unifies only with itself.
- Tuples: same arity, fields unify pairwise.
- ADTs, closures, and fn items: same definition, and the generic arguments unify.
- fn pointers: same unsafety, ABI, and parameter count; parameter types unify; same variadic-ness; return types unify; lifetimes are variance-conformant.
- Raw pointers and references: same mutability, and the pointees unify.
- `dyn`: bounds and lifetime unify. An `impl` type unifies only with itself. An opaque return type unifies with a type that implements all of its bounds.

**Obligations**
- These are `T: Trait` and outlives requirements. They may be deferred and are re-attempted whenever new type information arrives.
- A reference to `<T as Tr>::Assoc` creates the obligation `T: Tr`.

**Algorithm**
1. Walk statements and expressions in program order.
2. Unresolved integer variables become `i32`.
3. Unresolved float variables become `f64`.
4. Unresolved diverging variables become `()`.
5. Any remaining global variable is an error (type annotations needed).
6. Any unresolved or failing obligation is an error.

**Expected types**
- If the expression is a coercion site or coercion-propagating expression, try to coerce to the expected type. Otherwise (or if no coercion applies), unify with it.
- Statements:
  - An expression statement without `;` gets expected type `()`.
  - `let`: unify the ascription with the pattern type, then infer the initializer against the pattern type.
  - `let-else`: the else block is expected to have type `!`.

**Expression rules**
- `if` / `if let`:
  - The condition is `bool`. The branches get the if-expression's expected type.
  - A nested `else if` is unified with the outer if.
  - In practice, branch types are combined by least upper bound (LUB) coercion.
- `match`:
  - Unify all arm pattern types, then infer the scrutinee against that type.
  - Guards are `bool`. Arm bodies get the match's expected type.
- `for pat in e`: `pat : <typeof(e) as IntoIterator>::Item`.
- Arrays: LUB-coerce the elements to the expected type, falling back to unifying all elements.
- Unary operators:
  - `!e` has type `<T as Not>::Output`; `-e` has type `<T as Neg>::Output`.
- Binary arithmetic and bit operators:
  - The type is `<L as OpTrait<R>>::Output`.
  - If the expected type is numeric, both operands are unified with it. For shifts, only the left operand is (when the expected type is an integer).
  - **[!]** The FLS table names `core::ops::Mod`. The correct trait is `Rem` / `RemAssign`.
- Compound assignment: obligation `L: OpAssign<R>`; the type is `()`.
- Comparisons:
  - The FLS says obligation `L: PartialEq<R>`, type `bool`. The real rule is `PartialOrd` for `< > <= >=` **[!]**.
  - Primitive operands are built in.
- Assignment: unify the assignee type with the value type.
- Closures:
  - The signature comes from the expected type if it is a `fn` pointer.
  - Otherwise it comes from a pending `FnOnce`/`FnMut`/`Fn` obligation on the closure type, or a trait with `FnOnce` as a supertrait.
  - Otherwise the signature is inferred normally.
  - The body is inferred against the return type.
- Diverging expressions:
  - `break`, `return`, `loop` without a `break`, calls returning `!`, `panic!`, and any expression that diverges on all paths.
  - Each gets a fresh diverging type variable.

**Subtyping checks** run after inference on the inferred types:
- `let` initializer ≤ pattern type
- assignment value ≤ assignee
- `as` operand ≤ target
- call arguments ≤ parameter types
- `return` operand ≤ function return type
- `break` value ≤ the type of the loop or block
- Operator expressions follow the rules of the corresponding trait method call.

## 6. Coercions

**Coercion sites**
- `let` with an explicit type, `const`, `static`
- call and method-call arguments (except the receiver, which uses autoref/autoderef)
- struct, union, and enum variant field initializers
- function results: the tail expression and `return` operands
- [Ref] the value operand of an assignment

**Propagating expressions** (sub-expressions become coercion sites)
- array elements, including the repeat operand for target type `[U; n]`
- tuple fields, element-wise
- parenthesized expressions
- block tail expressions, including the branches of `if`/`match` when the type is known

**Coercion kinds** (`T → U`)
- subtyping (reflexive)
- transitive chains (only partly supported by rustc)
- `&mut T → &T`
- `*mut T → *const T`, `&T → *const T`, `&mut T → *mut T`
- **Deref coercion**: `&T` or `&mut T` → `&U` if `T: Deref<Target=U>`; `&mut T → &mut U` if `T: DerefMut<Target=U>`. These apply repeatedly, e.g. `&String → &str`, `&Box<Vec<T>> → &[T]`.
- **Pointer unsizing**: `C<T> → C<U>`, where `C` is `&`, `&mut`, `*const`, `*mut`, or `Box` (generally anything implementing `CoerceUnsized`), and `T: Unsize<U>`
- fn item → fn pointer, and non-capturing closure → fn pointer, when the signature is a subtype; also `unsafe fn`
- `!` → any type
- `dyn` upcasting:
  - `dyn A + Auto1 + Auto2` may drop auto traits.
  - `dyn Sub → dyn Super`, where `Super` is a supertrait.
  - Auto traits may be *added* if the principal trait has them as supertraits.

**`Unsize` (built in)**
- `[T; N] → [T]`. The metadata is `N`.
- `T → dyn Tr` if `T: Tr + Sized` and `Tr` is dyn compatible. The metadata is a vtable pointer.
- `dyn T → dyn U` (upcasting). The vtable changes.
- Struct tail: `S<.., T, ..> → S<.., U, ..>` if `T: Unsize<U>`, the last field's type involves `T` (and if it is `W<T>`, then `W<T>: Unsize<W<U>>`), and `T` appears in no other field.

**Least upper bound (LUB) coercion**
- Used for: `if`/`else` branches, `match` arms, array elements, labeled-block and `loop` `break` values plus the tail, and a closure or function with multiple `return`s.
- Algorithm (start with `U = T1`), for each `Ti`:
  1. If `Ti` coerces to `U`, keep `U`.
  2. Else if `U` coerces to `Ti`, set `U = Ti`.
  3. Else if both are fn items, non-capturing closures, or fn pointers, and some fn pointer type accepts both, set `U` to that fn pointer type. Two distinct fn items both become `fn(..)`.
  4. Otherwise there is no coercion; fall back to unification, which reports an error.

**Method call receiver** [Ref] (not covered by the coercion rules above)
1. Build the candidate list from the receiver type `T`: `T, *T, **T, …` (repeated built-in or `Deref` deref), then append an array→slice unsize at the end.
2. After each candidate `X`, insert `&X` and `&mut X`. For example, `Box<[i32;2]>` gives: `Box<..>, &Box, &mut Box, [i32;2], &[i32;2], &mut [i32;2], [i32], &[i32], &mut [i32]`.
3. For each candidate in order, look for a visible method whose receiver type equals that candidate. First check inherent methods, then methods of in-scope traits implemented by it. For a type parameter, its bound traits come first.
4. The first match wins, so a `&self` trait method can beat an inherent `&mut self` method.

**Allowed `self` types** [FLS]: `Self`, `&T`, `&mut T`, `Pin<T>`, recursively. [Ref]: also `Box<Self>`, `Rc<Self>`, `Arc<Self>`.

## 7. Subtyping and variance
- Subtyping exists only through lifetimes: `'long ≤ 'short`, so `&'static T <: &'a T`.
- Higher-ranked types are subtypes of their instantiations: `for<'a> fn(&'a T) <: fn(&'static T)`, and similarly for `dyn for<'a> Fn(..)`.
- After erasing lifetimes, subtyping is equality.

| Type | Variance in `'a` | Variance in `T` |
|---|---|---|
| `&'a T` | covariant | covariant |
| `&'a mut T` | covariant | invariant |
| `*const T` | | covariant |
| `*mut T` | | invariant |
| `[T]`, `[T; N]` | | covariant |
| `fn(A) -> R` | | contravariant in `A`, covariant in `R` |
| `UnsafeCell<T>` | | invariant |
| `PhantomData<T>` | | covariant |
| `dyn Trait<T> + 'a` | covariant | invariant |

- Trait parameters, including `Self`, are invariant.
- **ADT and tuple variance**: for each parameter, combine its variance across all field uses. Start at "any". The first use sets it; any conflicting use makes it invariant.
  - If a parameter stays at "any" (unused), that is an error. Every type or lifetime parameter must be used in a field or variant; use `PhantomData` to satisfy this.

## 8. Traits
```
TraitDeclaration ::= `unsafe`? `trait` Name Generics? (`:` TypeBoundList?)? Where? `{` InnerAttr* AssociatedItem* `}`
```
- Inside a trait, `Self` is an implicit type parameter (not implicitly `Sized`).
- **Supertraits**:
  - `trait T: B` is the same as `trait T where Self: B`.
  - The relation is transitive, and cycles are an error.
  - A bound `X: T` also gives access to the supertraits' items, and `dyn T` can call supertrait methods.
- **`unsafe trait`**: implementations must be `unsafe impl`, and only implementations of unsafe traits may be `unsafe impl`.

**Auto traits**: exactly `Send`, `Sync`, `Unpin`, `UnwindSafe`, `RefUnwindSafe`.
- A type implements an auto trait implicitly when all of its constituents do:
  - `&T`, `&mut T`, `*const T`, `*mut T`, `[T; n]`, `[T]` follow `T`.
  - fn items and fn pointers always implement them.
  - ADTs and tuples need all fields.
  - Closures need all captures, taking capture mode into account.
  - Field-less types implement every auto trait.
- A cycle (the answer for `T` depends on itself) is assumed to hold (coinductive).
- Explicit and negative implementations override the automatic one. std has `impl !Send for *mut T`, and `&T: Send` iff `T: Sync`. If a generic explicit impl exists, the automatic rule is not used for that type constructor.
- Auto traits are the only extra bounds allowed on `dyn`.

**Dyn compatibility** (formerly "object safety"). Merged FLS + Ref rules; a trait must have:
- All supertraits dyn compatible.
- No `Sized` supertrait.
- No associated consts.
- No generic associated types.
- Every associated function either:
  - **dispatchable**:
    - no type or const generics (lifetimes are fine)
    - a method that uses `Self` only in the receiver
    - a receiver of `&Self`, `&mut Self`, `Box<Self>`, `Rc<Self>`, `Arc<Self>`, or `Pin<P>` of these
    - no `async fn` and no return-position `impl Trait`
    - no `where Self: Sized`
    - not C-variadic
    - FLS adds: no `Self: Trait` predicates other than `Send`, `Sync`, `Unpin`
  - or **non-dispatchable**: has `where Self: Sized` (a by-value `self` receiver implies this). Such methods are left out of the vtable.
- `AsyncFn*` are not dyn compatible.
- **[!]** The FLS wording "associated types specify Sized for Self" is garbled. Follow the Reference.
- `dyn Trait` automatically implements `Trait` and its supertraits.

### Special traits and types (lang items an interpreter must know)
- **`Sized`**: implemented only by the compiler. It is an implicit bound on type parameters and associated types, relaxed with `?Sized`.
- **`Copy`** (requires `Clone`):
  - A use of a `Copy` value is a bitwise copy and leaves the source valid; otherwise the value is moved.
  - It is only allowed if all fields (all variants, all union fields) are `Copy` and the type has no `Drop` impl.
  - Built in for: scalars, `&T`, raw pointers, `!`, fn items, fn pointers, tuples and arrays of `Copy`, closures with only `Copy` captures.
  - `&mut T` is never `Copy`.
- **`Clone`**: built in for `Copy` types, tuples, arrays, and closures whose captures are `Clone`.
- **`Drop`**: user-defined destructor `drop(&mut self)`.
  - Drop glue runs it, then drops the fields in declaration order.
  - Statics are never dropped. Unions do not drop their fields.
  - A `Drop` type cannot be `Copy`.
  - Drop order is specified in the FLS ownership chapter (not part of this digest).
- **`Deref`/`DerefMut`**:
  - Overload `*`.
  - Used in deref coercion and method autoderef.
  - `Box` deref is built in and can be moved out of (`*b` moves the contents).
- **`Fn`/`FnMut`/`FnOnce`** (`FnOnce` ⊃ `FnMut` ⊃ `Fn`):
  - Calls desugar to `call`, `call_mut`, or `call_once`.
  - The sugar `Fn(A, B) -> R` is `Fn<(A, B), Output = R>`.
  - Function lifetime elision applies to the arguments of this sugar.
- **Operator traits**: everything in `core::ops` and `core::cmp`; `Index`/`IndexMut` for `[]`.
- **`Unsize`/`CoerceUnsized`**: drive unsizing coercions.
- **`UnsafeCell<T>`**:
  - The only source of interior mutability. A type has interior mutability if it contains an `UnsafeCell`.
  - Invariant in `T`.
  - Blocks constant promotion and read-only placement of statics.
- **`PhantomData<T>`**: a ZST that acts as if it owns a `T` for variance, drop check, and auto traits.
- **`Pin`**: allowed as a receiver, and a fundamental type.
- **`Box`**: deref can be moved out of; allowed as a receiver; fundamental for coherence.
- **`Termination`**: the return type of `main`.
- **Structural equality** (const patterns): built in for `bool`, `char`, integers, `str`, raw pointers, and fn pointers. ADTs qualify when `PartialEq` and `Eq` are *derived*. Arrays, slices, references, and tuples qualify when their components do.

## 9. Bounds, lifetimes, HRTB
```
TypeBoundList ::= TypeBound (`+` TypeBound)* `+`?
TypeBound ::= Lifetime | `'_` | `'static` | `(` TraitBound `)` | TraitBound | UseCaptures
TraitBound ::= (`?` | `for` GenericParameterList)? TypePath
Lifetime ::= `'` (NonKeywordIdentifier | RawIdentifierKeyword)
```
- **Opt-out bounds**: `?Sized` is the only one. It is not allowed in `impl Trait` or `dyn`.
- **HRTB**: `for<'a>` binds *lifetimes only* (no type or const parameters). It means "for all `'a`".
  - `where for<'a> F: Fn(&'a T)` puts the binder on the whole predicate.
  - `F: for<'a> Fn(&'a T)` scopes the binder to the trait.
- **Outlives bounds**: `'a: 'b` means `'a` outlives `'b`. `T: 'a` means every lifetime in `T` outlives `'a`.
- **Implied bounds** (not written, but assumed): `&'a T` in a parameter, return type, or field implies `T: 'a`. Outlives bounds on field types carry over to the ADT, and those on parameter and return types carry over to the function.
- **Trivial predicates**: a predicate that uses no generic or higher-ranked parameters must hold, or it is an error.

## 10. Generics
```
GenericParameterList ::= `<` (GenericParameter (`,` GenericParameter)* `,`?)? `>`
GenericParameter ::= Attr* (ConstantParameter | LifetimeParameter | TypeParameter)
LifetimeParameter ::= Lifetime (`:` LifetimeIndicationList)?
TypeParameter ::= Name (`:` TypeBoundList?)? (`=` Type)?
ConstantParameter ::= `const` Name `:` Type (`=` (BlockExpr | Ident | `-`? Literal))?
WhereClause ::= `where` (LifetimeBoundPredicate | TypeBoundPredicate),* `,`?
LifetimeBoundPredicate ::= LifetimeIndication `:` LifetimeIndicationList?
TypeBoundPredicate ::= (`for` Generics)? Type `:` TypeBoundList?
GenericArgumentList ::= `<` (GenericArgument),* `>`
GenericArgument ::= Ident `=` Type          // binding (assoc type equality)
                  | Ident `:` TypeBoundList // binding bound (assoc type bounds)
                  | BlockExpr | `-`? Literal | Ident   // const arg
                  | LifetimeIndication | Type
```
**Parameters**
- Lifetime parameters must come before type and const parameters.
- `<X: B>` is the same as `where X: B`.
- Type parameters are implicitly `Sized`.
- A type parameter default must satisfy the parameter's bounds.

**Const generics**
- The parameter type must be a scalar type (integer, `bool`, `char`).
- Allowed uses: as a const argument in signatures and fields, in associated const initializers, in associated type definitions, and as a value or const argument anywhere in a function body.
- A const argument is a literal, an optional negated literal, a single-segment path, or a `{ block }`.
- Inferred const `_`: not allowed in signatures, not inside braces `{_}`, but parentheses are allowed.
- Lifetime parameters (other than `'static`) cannot be used in const contexts.

**Implicit lifetime bounds on type parameters**
- Use inside `&'a` adds `T: 'a`.
- Use as an argument to an ADT inherits that ADT parameter's lifetime bounds.
- This applies to both ADTs and function signatures.

**Argument order and conformance**
- Order: lifetimes, then const and type arguments in parameter order, then bindings.
- Conformance:
  - Type arguments must satisfy the parameter's bounds.
  - Const argument types must unify with the parameter type.
  - Lifetime arguments must outlive the bound.
  - Binding arguments must satisfy the associated type's bounds.
  - Missing arguments are allowed only if the parameter has a default, is a lifetime that is elided or inferred, or is a type that is inferred.
- Const parameter value: the argument, else the default, else an error.
- Binding bounds (`Ident: Bounds`) are allowed only in where-clause bounds, `impl Trait` bounds, associated type bounds, and supertraits.

**Impl parameter constraint rule**
- An impl's type and const parameters must *constrain* the impl. A parameter constrains it if it appears in the implementing type, in the trait reference, or as an associated-type binding in a bound on a type that contains another constraining parameter.
- An unconstrained lifetime is an error only if it is used in an associated type.

**Where clauses**: a construct is well-formed iff all of its predicates hold for the given arguments.

## 11. Implementations
```
InherentImpl ::= `impl` Generics? Type Where? ImplBody
TraitImpl    ::= `unsafe`? `impl` Generics? `!`? TypePath `for` Type Where? ImplBody
ImplBody     ::= `{` InnerAttr* AssociatedItem* `}`
```
- Inside an impl, `Self` is an alias for the implementing type.

**Inherent impls**
- Must be in the same crate as the type.
- Across all inherent impls of a type, at most one item per name per namespace.

**Coherence**
- The impl must not overlap with another impl of the same trait (the intersection of the implementing types must be empty). There is no specialization.
- **Orphan rule** [Ref, precise form]: `impl<P..> Trait<T1..Tn> for T0` is allowed iff either:
  - `Trait` is local, or
  - some `Ti` is a local type (let `Ti` be the first such), and no *uncovered* type parameter `P` appears in `T0..Ti-1`. "Uncovered" means not inside a non-fundamental type constructor.
- The FLS gives an equivalent-in-spirit version:
  - The trait is fundamental or local, or
  - some `Ti` is fundamental or local and no bare type parameter appears in non-local, non-fundamental positions.
- **Fundamental** types: `&T`, `&mut T`, `Pin`, and `Box` (an FLS comment notes `Box`). `Box<Local>` and `&Local` count as local.
- **Fundamental** traits [FLS]: `Fn`, `FnMut`, `FnOnce`, `Sized`.

**Conformance** (trait impl ↔ trait)
- Every associated type must be provided and must satisfy the trait's bounds on it.
- Every const and function without a default must be provided.
- Const types must be subtypes of the trait's types.
- An impl function's signature must be a subtype of the trait function's signature, and its bounds must be no stricter (bounds can only be more general).

## 12. Associated items
```
AssociatedItem ::= Attr* (Vis? (ConstantDeclaration | FunctionDeclaration | TypeAliasDeclaration)
                         | TerminatedMacroInvocation)
```
- **In a trait**:
  - Consts may omit an initializer; one that is given is the default.
  - Functions may omit a body; one that is given is the default.
  - Trait functions cannot be `const` (no stable const traits).
  - Associated types have no `= Type`, only optional bounds. `type X: B;` is the same as `where Self::X: B`.
  - Associated types are implicitly `Sized`.
- **In an impl**: consts need an initializer, functions need a body, types need `= Type`.
- Visibility modifiers on trait items and trait-impl items are rejected.
- An associated type cannot be used as the path of a struct expression.
- **GATs** (`type Item<'x> where Self: 'x;`): rustc requires `where Self: 'x` (or `T: 'x`) bounds when the trait's methods use the GAT with lifetimes that imply them. For example, `LendingIterator::Item<'a>` together with `fn next<'a>(&'a mut self) -> Self::Item<'a>` requires `Self: 'a`.
- **Method**: an associated function with a `self` parameter. Without one it is called with path syntax (`Type::f`, `<T as Tr>::f`).
- Projection `<T as Trait>::Assoc` resolves by selecting the impl for `T: Trait`.

## 13. Lifetime elision
- `'_` is explicit elision. Omitting the lifetime entirely is implicit elision.
- The rules come from the innermost enclosing construct. Eliding where no rules apply is an error.

**Functions, fn pointers, and `Fn*` sugar**
1. Each elided input lifetime becomes a fresh, distinct lifetime parameter.
2. If there is exactly one input lifetime (elided or not), it is used for all elided output lifetimes.
3. Otherwise, if the receiver is `&Self` or `&mut Self` (including `&self` with a single self lifetime), the self lifetime is used for all elided outputs.
4. Otherwise eliding an output lifetime is an error.

**Const and static**: elided lifetimes in the type default to `'static`. If the type contains fn or closure signatures, the function rules apply inside them first. An associated trait const cannot elide its lifetime.

**Default trait-object lifetime** (`dyn Tr` with no lifetime written; `'_` uses the normal rules instead)
- Inside `&'a dyn Tr`, it is `'a`.
- As a generic argument `C<dyn Tr>`: if the parameter has exactly one lifetime bound, use it. More than one bound is an error.
- Otherwise use the trait's own lifetime bound: a single bound is used; any `'static` gives `'static`; no bound gives `'static` outside expressions and inferred inside expressions.
- The innermost object decides: `&'a Box<dyn Foo>` is `&'a Box<dyn Foo + 'static>`.
- `type T<'a> = &'a dyn Tr` means `&'a (dyn Tr + 'a)`.

**`impl` headers**
- `'_`, and an implicitly elided reference lifetime, each become a fresh anonymous impl lifetime parameter.
- Implicit elision anywhere else (for example a path such as `Strukt` with a hidden lifetime) is an error.

**`impl Trait`**: lifetimes cannot be implicitly elided. Without a lifetime bound, the type is not bound by any lifetime.

## 14. Values, constants, statics, pointers

**Values and allocated objects**
- An allocated object has a base address and a size.
- UB if the base is null, if `base + size > usize::MAX`, or if `size > isize::MAX`.
- UB to create a value of any non-union type from uninitialized memory.
- Overlap: a value with its own fields or elements, or elements of the same array.

**Constants**
```
ConstantDeclaration ::= `const` (Name | `_`) `:` Type (`=` Expr)? `;`
```
- `const _` is an unnamed constant (useful for static checks).
- The type must be `Sized` with `'static` lifetimes.
- The initializer is required except for trait constants, and must be a constant expression.
- Any pointer in the result must be non-dangling and well-formed.
- **Every use is replaced by the value**: a fresh copy per use, with no fixed address, so `&CONST` gives a new temporary (often promoted).
- Evaluation is "elaboration". Evaluate lazily and memoize per (item, generic arguments).

**Statics**
```
StaticDeclaration ::= ItemSafety? `static` `mut`? Name `:` Type (`=` Expr)? `;`
```
- `ItemSafety` (`safe`/`unsafe`) is allowed only for an extern static in an `unsafe extern` block.
- A static is one memory location. All paths to it refer to the same place; a use is a place expression.
- A static inside a generic function exists once, not once per instantiation.
- The type must be `Sized` with `'static` lifetimes. An immutable static's type must be `Sync`.
- The initializer is required except for extern statics, and must be a constant expression.
- Statics are never dropped.
- `static mut`: any access needs `unsafe`.
- Mutating an immutable static without interior mutability is UB, and such a static may live in read-only memory.

**Pointers**
- A *dangling* pointer is null or does not point entirely within one allocation.
- *Original* pointers come from allocation. *Derived* pointers come from borrow, copy, load, arithmetic, or cast, and inherit provenance.
- **Provenance** restricts where, when, and how (read or write) a pointer may access memory. Accessing memory without permitting provenance is UB.
- A *well-formed* pointer either has no provenance on any byte, or every byte comes from the same single pointer with provenance.
- An interpreter should track provenance per byte, Miri-style, to detect UB.
- Raw pointer `==` compares addresses (for fat pointers, also the metadata).
- Copying a shared reference is a shallow copy. Dropping or releasing a reference does nothing to the referent.

**Variables**: bindings and temporaries live on the stack, start uninitialized, and must be definitely initialized on all paths before use.

**Constant promotion** (`&expr` becomes `&'static`) happens when all of these hold:
- the operand is a constant expression
- its type has no destructor
- it constructs no `UnsafeCell`
- all operations always succeed (no overflow, division by zero, or out-of-bounds indexing)
- it is the operand of an *immutable* borrow
- `&mut []` is always promotable.

Mechanism: create an anonymous const, then borrow it with `'static`.

## 15. Const evaluation

**Constant expressions**: only these forms qualify, their operands must also be constant expressions, and they must not involve types that need drop.
- literals; paths to consts, associated consts, const parameters, functions, statics, tuple structs, unit structs
- arithmetic, bit, comparison, negation, and lazy boolean operators on scalars only (no user operator traits)
- array, tuple, and struct expressions; parenthesized expressions; ranges
- blocks; `unsafe` blocks; `let` and expression statements; assignment and compound assignment
- `if`, `if let`, `match`, `loop`, `while`, `while let`
- calls and method calls to `const fn`, with no implicit `Deref`
- field access without `Deref`
- indexing an array or slice with `usize`
- dereference
- non-capturing closures
- `as` casts, except pointer→address casts, fn-pointer→address casts, and unsizing to `dyn`
- borrows, except `&mut` or interior-mutable borrows that would be extended to `'static`
- `addr_of!`
- `panic!()` with no arguments or with a single string literal that has no format captures

Not constant expressions:
- explicit calls to trait methods
- operators on non-built-in types
- `for` loops (they use `IntoIterator`)

**Const contexts** (these require constant expressions):
- const and associated const initializers
- const arguments and const parameter defaults
- enum discriminants
- array type lengths and array-repeat lengths
- static initializers
- `const { ... }` blocks

**Semantics**
- A constant expression is evaluated at compile time whenever its value is needed.
- Arithmetic overflow panics. A panic, or reaching `panic!`, during const evaluation is a *static error*.
- An unaligned result is a static error.
- A `const fn` called at runtime behaves like a normal function. The interpreter can share the evaluator between compile time and runtime, applying the restricted operation set and error-on-panic in const contexts.
- An array length must not depend on generic parameters, except as a bare const parameter `N`.

---
**Spec defects to remember**:
- FLS uses `Mod`/`ModAssign`; the correct names are `Rem`/`RemAssign`.
- FLS says comparisons use `PartialEq`; ordering comparisons use `PartialOrd`.
- FLS says closure `Sync` requires `Send` captures; it should be `Sync`.
- FLS treats `repr(C)` enums with fields as union-of-structs; the Reference's struct-of-tag-plus-union layout is correct.
- FLS's associated-type rule for dyn compatibility is garbled.
- FLS writes `copy::marker::Copy`, meaning `core::marker::Copy`.
- FLS says discriminant initializers need all variants fieldless; current Rust also allows them on fieldful enums with a primitive repr.
- Niche layout is entirely unspecified.
