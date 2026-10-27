# rint implementer digest: FLS (rustc 1.98.0) on lexing, macros, crate structure, cfg and attributes

**Sources:** FLS `general.rst`, `lexical-elements.rst`, `macros.rst`, `program-structure-and-compilation.rst` and `attributes.rst` under `/home/ggvgc/stuff/projects/rint/spec/fls/src/`. Gaps were filled from the Rust Reference in `/home/ggvgc/stuff/projects/rint/spec/reference/src/`: `tokens.md`, `keywords.md`, `identifiers.md`, `input-format.md`, `shebang.md`, `macros-by-example.md`, `conditional-compilation.md`, `attributes.md`, `names/name-resolution.md`.

Tags used below:
- **[FLS]** means the rule is stated in the FLS.
- **[Ref]** means the rule comes only from the Reference.
- **(!)** marks a place where the FLS and the Reference disagree, or where the FLS is incomplete.

---

## 0. Spec conventions

- **Scope.** The FLS describes the **2021 edition** as implemented by rustc 1.98.0. Where in doubt, it documents what rustc does rather than claiming what is correct. It is not meant to support conformance between different compilers.
- **Normative vs informative.** Everything is normative except "Examples" headings, subchapters whose titles start with "Example", and all appendices (A–D).
- **Subchapter segments:**
  - **Syntax**: the context-free grammar.
  - **Legality Rules**: compile-time checks and facts. A construct is legal if it obeys all of them. **They are checked after macro expansion.**
  - **Dynamic Semantics**: run-time effects.
  - **Undefined Behavior**: "unbounded errors". The spec does not define their effect, and a tool is not required to detect them.
  - **Implementation Requirements**: requirements on the tool.
  - **Examples**: informative.
- **Rules vs facts.**
  - A *rule* is a requirement on the programmer ("shall", "must"). Breaking one is a static error that a conforming tool must detect.
  - A *fact* is a requirement on the tool ("is", "can").
- **Conformity.** A conforming tool must:
  - translate legal programs correctly,
  - reject programs that exceed its capacity,
  - reject programs with errors that the spec requires it to detect.
- **Observable behaviour** is defined only by external interactions: calls to external functions, interactions with external statics, the result of the entry point, and results returned from exported functions to external callers. Their order and timing must be consistent with the spec. This gives an interpreter an "as-if" freedom.
- A tool may add attributes, provided their names do not clash with built-in attributes.
- **Paragraph IDs.**
  - Every FLS paragraph has a stable ID `:dp:`fls_xxxxxxxxxxxx``, and every section has an anchor `.. _fls_xxx:`. Cite these in code and tests (e.g. `fls_4vicosdeaqmp`).
  - The Reference uses rule IDs `r[lex.token.literal.suffix]`.
  - In the RST markup, `:t:` is a term, `:dt:` a term definition, `:s:` a syntax category, `:ds:` a syntax-category definition, `:c:` a code or attribute name, `:dc:` a code-name definition, and `:std:` a library item.
- **Syntax notation (BNF variant).**
  - `PascalCase` is a nonterminal. **bold** (`$$x$$` in source) is a literal keyword or character. `\t` is an escaped character.
  - `X?` means optional, `X*` zero or more, `X+` one or more, and `X1-6` a bounded repetition.
  - `[a b]` is a character set, `~[a b]` a negated set, `[a-f]` a range, `|` an alternative and `( )` a group.
  - The Reference grammar also uses `!X` (negative lookahead), `^` (commit point), `X{n:1..=255}` (counted, bound repetition) and `_except …_`.

---

## 1. Source-file input pipeline

Steps 1–4 come from [Ref] `input-format.md`. The FLS only gives `SourceFile ::= ZeroWidthNoBreakSpace? Shebang? InnerAttributeOrDoc* Item*`.

1. **Decode UTF-8.** Invalid UTF-8 is an error. A `CHAR` is a Unicode scalar value, so surrogates are excluded.
2. **Remove a leading BOM** (`U+FEFF`).
3. **CRLF normalization.** Each `CR LF` becomes `LF`, in a single pass, so `CR CR LF LF` becomes `CR LF LF`. Any other `CR` is whitespace.
   - The FLS states the CRLF→LF replacement separately for string, byte string and C string literals.
   - A bare CR is forbidden inside comments and string literals, and inside raw strings (`~[\r]`).
4. **Remove the shebang.**
   - Grammar: `#!` followed by everything up to the next LF or EOF.
   - It is *not* a shebang if, after `#!`, the next significant token is `[`. "Significant" here skips whitespace and *non-doc* comments only. So `#![attr]` is an inner attribute, while `#! /*! */ [x]` *is* a shebang.
   - It may only appear at the file start or after the BOM. It has no effect on compilation [FLS].
5. **Tokenize** using maximal munch, with the special cases described below.

**Whitespace** (is a separator): U+0009, 000A, 000B, 000C, 000D, 0020, 0085, 200E, 200F, 2028, 2029. These are the `Pattern_White_Space` characters.

A program's meaning depends only on its sequence of lexical elements, **excluding non-doc comments**. Doc comments are tokens: they become `#[doc]` attributes.

```
LexicalElement ::= Comment | Identifier | Keyword | Literal | Punctuation
```

[Ref] has a richer token taxonomy:

```
Token ::= RESERVED_TOKEN | RAW_IDENTIFIER | CHAR_LITERAL | STRING_LITERAL
        | RAW_STRING_LITERAL | BYTE_LITERAL | BYTE_STRING_LITERAL | RAW_BYTE_STRING_LITERAL
        | C_STRING_LITERAL | RAW_C_STRING_LITERAL | FLOAT_LITERAL | INTEGER_LITERAL
        | LIFETIME_TOKEN | PUNCTUATION | IDENTIFIER_OR_KEYWORD
```

---

## 2. Punctuation and delimiters

- **Delimiters:** `{ } [ ] ( )`. They must balance into token trees.
- **Simple punctuators:** `+ - * / % ^ ! & | = > < @ _ . , ; : # $ ? { } [ ] ( )`
- **Compound punctuators:** `&& || << >> += -= *= /= %= ^= &= |= <<= >>= == != >= <= .. ... ..= :: -> =>`
- **Flexible compound punctuators:** `&& || << >>`. The parser may split each into two simple punctuators, e.g. `>>` closing two generic lists, or `&&x` as `& &x`, or `||` as an empty closure parameter list.
  - Implementation tip: store a `joint` flag (like rustc's `Spacing::Joint`) on single-char punctuation tokens and glue or split them in the parser. Proc macros need that flag anyway.
  - rustc also splits `>=`, `>>=` and `<-`-like tokens in generic contexts. The FLS lists only the four above.
- `_` is a punctuator in the FLS. The Reference treats it as a strict keyword. In both, it is **not** an identifier.
- Names you may want in diagnostics: Plus, Minus, Star, Slash, Percent, Caret, Not, And, Or, AndAnd, OrOr, Shl, Shr, PlusEq, …, Eq, EqEq, Ne, Gt, Lt, Ge, Le, At, Underscore, Dot, DotDot, DotDotDot, DotDotEq, Comma, Semi, Colon, PathSep (`::`), RArrow (`->`), FatArrow (`=>`), Pound (`#`), Dollar, Question.

---

## 3. Identifiers

```
Identifier          ::= NonKeywordIdentifier | RawIdentifier
NonKeywordIdentifier::= PureIdentifier | WeakKeyword
PureIdentifier      ::= XID_Start XID_Continue* | `_` XID_Continue+
RawIdentifier       ::= `r#` (PureIdentifier | RawIdentifierKeyword)
RawIdentifierKeyword::= any Keyword except crate, self, Self, super
IdentifierOrUnderscore ::= Identifier | `_`
Renaming            ::= `as` IdentifierOrUnderscore
IdentifierList      ::= Identifier (`,` Identifier)* `,`?
```

- **Unicode rules.** Follow UAX #31 for Unicode 17.0, with Start = XID_Start plus `_`, Continue = XID_Continue, and Medial empty.
  - ZWNJ/ZWJ (U+200C/200D) are forbidden.
  - Identifiers are **NFC-normalized**. Equality is equality after NFC, and macros receive normalized identifiers.
- **ASCII-only identifiers are required for:**
  - `extern crate` names,
  - crate names after a leading `::`,
  - names of outline modules that have no `#[path]`,
  - names of `#[no_mangle]` items,
  - names of items in `extern` blocks.
- **Raw identifiers.** `r#foo` has the same name as `foo`, so the `r#` is not part of the name. `r#_`, `r#crate`, `r#self`, `r#Self` and `r#super` are errors (`RESERVED_RAW_IDENTIFIER`) [Ref].
- The `ident` fragment specifier matches a *pure* identifier (FLS). The Reference says `IDENTIFIER_OR_KEYWORD` except `_`, `RAW_IDENTIFIER`, or `$crate`, so in practice keywords **are** accepted by `$i:ident` in rustc. **(!)** Follow rustc here.

---

## 4. Keywords

Keywords are case-sensitive.

- **Strict** (never usable as identifiers except with `r#`): `as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while`. The Reference also lists `_`. `async`, `await` and `dyn` became keywords in 2018+.
- **Reserved** (same restrictions as strict): `abstract become box do final macro override priv try typeof unsized virtual yield`.
  - [Ref] adds **`gen`**, which is reserved in the **2024** edition. **(!)** The FLS targets 2021.
  - `try` has been reserved since 2018.
- **Weak** (keywords only in context):
  - `macro_rules`: only in a `MacroRulesDeclaration`.
  - `'static`: only as a lifetime. It cannot be a generic parameter or a label.
  - `union`: only in a union declaration.
  - `safe`: only as a qualifier of a function or static inside an `extern` block.
  - [Ref] also lists `raw`, which is a keyword in `&raw const` / `&raw mut`.
- **Context-sensitive words that are not keywords at all:** `auto` (historical), `default` (specialization, unstable), and `macro_rules` as an identifier elsewhere.

---

## 5. Lifetimes and labels ([Ref]; the FLS lexical chapter omits these)

```
LIFETIME_TOKEN    ::= RAW_LIFETIME | `'` IDENTIFIER_OR_KEYWORD !`'`
LIFETIME_OR_LABEL ::= RAW_LIFETIME | `'` NON_KEYWORD_IDENTIFIER !`'`
RAW_LIFETIME      ::= `'r#` IDENTIFIER_OR_KEYWORD !`'`     // 2021+
RESERVED_RAW_LIFETIME ::= `'r#` (_ | crate | self | Self | super)   // error
```

- **Disambiguation:** after `'`, lex an identifier. If the next character is `'`, the whole thing is a char literal (`'a'`). Otherwise it is a lifetime (`'a`).
- `'_` is the anonymous lifetime. `'static` is the weak keyword.
- The lexer accepts any `LIFETIME_TOKEN`, including `'fn`, so it can be passed to macros. Only `LIFETIME_OR_LABEL` is valid as a parameter or label.
- The `lifetime` fragment matches `'_` or `'` followed by an identifier [FLS].

---

## 6. Literals

```
Literal ::= BooleanLiteral | ByteLiteral | ByteStringLiteral | CStringLiteral
          | CharacterLiteral | NumericLiteral | StringLiteral
```

### Suffixes [Ref]

- **Every** literal token may carry an arbitrary identifier-like suffix: `SUFFIX ::= _ XID_Continue+ | XID_Start XID_Continue*`. The token is lexically valid, so `"x"foo` and `1bar` can be passed to macros, and `$l:literal` matches them.
- When the token is *used* as a literal expression or pattern:
  - any suffix on a non-numeric literal is an error,
  - integers accept only `u8 i8 u16 i16 u32 i32 u64 i64 u128 i128 usize isize`,
  - floats accept only `f32 f64`.
- The FLS grammar builds in only the valid suffixes. Implement the Reference model: lex any suffix, and validate later.

### Integers

```
IntegerLiteral ::= (BinaryLiteral | DecimalLiteral | HexadecimalLiteral | OctalLiteral) IntegerSuffix?
DecimalLiteral ::= DecDigit (DecDigit | _)*
BinaryLiteral  ::= `0b` (BinDigit|_)* BinDigit (BinDigit|_)*      // likewise 0o [0-7], 0x [0-9a-fA-F]
```

- Underscores are allowed anywhere after the first decimal digit, and after the radix prefix.
- A radix literal needs at least one real digit: `0b_` and `0x` are errors.
- Uppercase radix prefixes (`0B`, `0X`) are not allowed.
- **Type of an unsuffixed integer:** inferred from context. If under-constrained it defaults to **`i32`**; if over-constrained it is an error.
- Gotchas [Ref]:
  - A suffix may not start with `e`/`E` on a decimal literal, because that is an exponent. `2em` is an error, not `2` followed by `em`.
  - `0b101e` is an error.
  - An unsuffixed `0b`/`0o` literal followed directly by an out-of-range decimal digit is an error (`0b12`).
  - An unsuffixed `0b`/`0o`/`0x` literal directly followed by `.` (in float position) is an error.
  - Tuple indices (`x.0`) are integer literals with no suffix and no underscores. `x.0.1` lexes `0.1` as a float, so the parser must split it.
  - `-1` is two tokens. Negation is an operator, but the `literal` fragment and literal patterns accept an optional leading `-`.

### Floats

```
FloatLiteral ::= DecimalLiteral `.`                    // only if next char is not `.`, `_`, or XID_Start
               | DecimalLiteral FloatExponent
               | DecimalLiteral `.` DecimalLiteral FloatExponent?
               | DecimalLiteral (`.` DecimalLiteral)? FloatExponent? FloatSuffix
FloatExponent ::= [eE] [+-]? (DecDigit|_)* DecDigit (DecDigit|_)*
```

- There is no leading-dot form (`.5`) and no hexadecimal float.
- `1.` is a float, but `1..2` is a range, `1.foo()` is a method call and `1._x` is a field access.
- `2.f64` is a method call: a suffix cannot follow a trailing `.`.
- **Type of an unsuffixed float:** inferred, defaulting to **`f64`**.

### Characters and strings

```
CharacterLiteral ::= `'` (CharLitChar | AsciiEscape | UnicodeEscape) `'`
   CharLitChar = any char except TAB, LF, CR, `'`, `\`
AsciiEscape  ::= \0 \" \' \t \n \r \\ | `\x` OctalDigit HexDigit      // \x00..\x7F only
UnicodeEscape::= `\u{` HexDigit{1-6} `}`   // ≤ 10FFFF, not D800–DFFF; [Ref] allows `_` inside after first digit
StringContinuation ::= `\` LF   // [Ref]: also skips following whitespace (SP TAB LF CR)
SimpleStringLiteral ::= `"` (SimpleStringChar | AsciiEscape | UnicodeEscape | StringContinuation)* `"`
   SimpleStringChar = any char except CR, `"`, `\`        // literal LF allowed (multi-line strings)
RawStringLiteral ::= `r` `#`{n} `"` ~[CR]* `"` `#`{n}      // n = 0..=255, no escapes
ByteLiteral ::= `b'` (ByteChar | ByteEscape) `'`           // ASCII except TAB LF CR ' \ ; ByteEscape = \xHH (any 00-FF) + simple escapes
SimpleByteStringLiteral ::= `b"` (ASCII except CR " \ | ByteEscape | StringContinuation)* `"`
RawByteStringLiteral ::= `br` `#`{n} `"` ASCII* `"` `#`{n}
SimpleCStringLiteral ::= `c"` (char except CR " \ NUL | AsciiEscape(≠\0 effectively) | UnicodeEscape | StringContinuation)* `"`
RawCStringLiteral ::= `cr` `#`{n} `"` ~[CR NUL]* `"` `#`{n}
BooleanLiteral ::= `true` | `false`
```

- **(!)** The FLS writes `ByteEscape` as `\x OctalDigit HexDigit`, which looks like a copy-paste error. In rustc, byte escapes allow `\x00`–`\xFF`, while char/str `\x` escapes are limited to `\x00`–`\x7F`.
- **(!)** For C strings, rustc forbids any escape that produces NUL (`\0`, `\x00`, `\u{0}`). It allows `\xHH` up to FF and Unicode escapes, which are encoded as UTF-8.
- **Types:**

  | Literal | Type |
  |---|---|
  | char | `char` |
  | `"…"` / `r"…"` | `&'static str` |
  | `b'…'` | `u8` |
  | `b"…"` / `br"…"` | `&'static [u8; N]` |
  | `c"…"` / `cr"…"` | `&'static core::ffi::CStr` (implicit trailing 0x00) |
  | `true` / `false` | `bool` |

- Raw strings: up to 255 `#`s, the closing delimiter must use the same count, and escapes are not processed.

### Reserved token forms [Ref] (lexer errors in 2021+)

- `ident"…"`, unless the prefix is `b`, `c`, `r`, `br` or `cr`.
- `ident'…`, unless the prefix is `b`.
- `ident#`, unless the prefix is `r`, `br` or `cr`.
- `'ident#` (reserved lifetime prefix).
- Raw identifiers or lifetimes using `_`, `crate`, `self`, `Self` or `super`.
- **2024 edition only:** guarded strings `#"…"` / `#…"…"` and `##` (two or more `#`). Before 2024 these lex as separate tokens.
- These are tokens that the lexer recognises and then rejects. Implement them so that `a#b` errors rather than silently splitting. Note that `r#let#foo` lexes as `r#let` `#` `foo`.

---

## 7. Comments

```
LineComment   ::= `//` | `//` (~[! /] | `//`) ~[LF]*        // `////…` is a plain comment
InnerLineDoc  ::= `//!` ~[LF CR]*
OuterLineDoc  ::= `///` (~[/] ~[LF CR]*)?
BlockComment  ::= `/*` (~[! *] | `**` | BlockCommentOrDoc) (BlockCommentOrDoc | ~[*/])* `*/` | `/**/` | `/***/`
InnerBlockDoc ::= `/*!` (BlockCommentOrDoc | ~[*/ CR])* `*/`
OuterBlockDoc ::= `/**` (~[*] | BlockCommentOrDoc) (BlockCommentOrDoc | ~[*/ CR])* `*/`
```

- **Block comments nest.** Keep a depth counter.
- **CR is forbidden** in doc comments. The FLS says it is forbidden in all comments; the Reference restricts this to doc comments.
- **Desugaring:**
  - `//! x` / `/*! x */` become `#![doc = " x"]`.
  - `/// x` / `/** x */` become `#[doc = " x"]`.
  - The content is the text after the opener and before `*/`, keeping the leading space.
- An outer doc comment must be followed by something it can attach to. An inner doc comment must be in an inner-attribute position.
- Before macro matching (declarative or proc), doc comments inside the input are replaced by their `doc` attribute form. So `#[doc = "…"]` token sequences are what `$m:meta` or `tt` will see: three tokens `#`, `[…]`, or with `!` for inner docs.

---

## 8. Declarative macros (`macro_rules!`)

### Grammar

```
MacroRulesDeclaration ::= `macro_rules` `!` Name MacroRulesDefinition
MacroRulesDefinition  ::= `(` MacroRuleList `)` `;` | `[` MacroRuleList `]` `;` | `{` MacroRuleList `}`
MacroRuleList         ::= MacroRule (`;` MacroRule)* `;`?
MacroRule             ::= MacroMatcher `=>` MacroTranscriber
MacroMatcher          ::= `(` MacroMatch* `)` | `[` … `]` | `{` … `}`
MacroTranscriber      ::= DelimitedTokenTree
MacroMatch            ::= MacroMatcher | MacroMatchToken            // any token but `$` / delimiters
                        | `$` MacroMetavariable `:` FragSpec
                        | `$` `(` MacroMatch* `)` Separator? RepOp
MacroMetavariable     ::= Keyword | NonKeywordIdentifier           // [Ref]: IDENT_OR_KEYWORD except `crate`, or raw ident
RepOp                 ::= `*` | `+` | `?`
Separator             ::= any token except delimiters and `+ * ?`   // `?` repetition may not have a separator [Ref]
MacroRepetitionTranscriber ::= `$` `(` TokenTree* `)` Separator? RepOp

MacroInvocation       ::= SimplePath `!` DelimitedTokenTree
TerminatedMacroInvocation ::= SimplePath `!` (`(`…`)` `;` | `[`…`]` `;` | `{`…`}`)   // statement/item position
DelimitedTokenTree    ::= `(` TokenTree* `)` | `[` … `]` | `{` … `}`
TokenTree             ::= DelimitedTokenTree | NonDelimitedToken
```

### Fragment specifiers: what they match

| spec | matches |
|---|---|
| `block` | a block expression (Ref: without inner attributes) |
| `expr` | an expression. Under 2024 it also matches `_` and `const {}` at top level. The FLS says neither `expr` nor `expr_2021` matches `_` or `const` blocks. |
| `expr_2021` | an expression except underscore expressions and const blocks |
| `ident` | an identifier (Ref: any identifier or keyword except `_`; raw identifiers allowed; not `$crate`) |
| `item` | an item, with its attributes and visibility |
| `lifetime` | a `LIFETIME_TOKEN` (`'_`, `'a`) |
| `literal` | `-`? followed by a literal expression (any suffix) |
| `meta` | attribute content: `SimplePath AttrInput?` |
| `pat` | a pattern including top-level `\|` (2021+; the edition of the macro definition decides) |
| `pat_param` | a pattern without top-level alternation |
| `path` | a TypePath (with `::<>` / generics) |
| `stmt` | a statement without its trailing `;`, except items that require `;` |
| `tt` | one token tree |
| `ty` | a type |
| `vis` | a possibly empty visibility |

- **Opaque forwarding** [Ref]: once a fragment is captured as anything other than `ident`, `lifetime` or `tt`, it becomes an opaque AST node.
  - When passed to another macro, it can only be matched by the same fragment kind, or by `tt`.
  - When transcribed into an expression, it acts as if parenthesized: `$e * 2` with `$e = 1+1` gives 4.
  - Model this with a `Nonterminal` / invisible-delimiter token.

### Follow-set restrictions (checked at definition time)

| Fragment | May only be followed by |
|---|---|
| `expr`, `expr_2021`, `stmt` | `=>` `,` `;` |
| `pat_param` | `=>` `,` `=` `\|` `if` `in` |
| `pat` | Reference (2021+): `=>` `,` `=` `if` `in`, with no `\|`. **(!)** The FLS also lists `\|`; rustc follows the Reference in 2021+. |
| `path`, `ty` | `=>` `,` `=` `\|` `;` `:` `>` `>>` `[` `{` `as` `where`, or a `$x:block` |
| `vis` | `,`, any identifier except non-raw `priv`, any token that can begin a type, or a metavariable of kind `ident`, `ty` or `path`. **(!)** The FLS says `ident`, `ty` or `block`. |
| all others | anything |

**Repetitions** are checked for every possible expansion count:
- the separator must be able to follow the content;
- for `*` and `+`, the content must be able to follow itself;
- the content must be able to follow what precedes it, and what comes next must be able to follow the content;
- for `*` and `?`, what comes next must be able to follow what precedes the repetition.

The formal FIRST/LAST computation is in the Reference's `macro-ambiguity.md`.

### Matching algorithm

1. Before matching, replace doc comments in the input with `#[doc=…]`.
2. Try rules **in declaration order**. The first matcher that fully matches wins.
3. If a rule fails with a **hard error**, stop. Do not try later rules. A hard error is a fragment parser failing *after consuming at least one token*: no backtracking out of a fragment parse.
4. Matching is left to right with **no lookahead**. When a fragment could start at a position where another alternative could also continue, the match is "locally ambiguous" and is an error. Example: `$($i:ident)* $j:ident` errors.
   - rustc implements this with an Earley-like multi-position matcher, `mbe::macro_parser`. Multiple "threads" are allowed for literal tokens, but if more than one thread wants a fragment parser (or a fragment and a token compete), that is ambiguity.
5. The matcher's outer delimiters match **any** delimiter kind. Inner delimiters must match exactly.
6. Other tokens match literally. `$` cannot be matched literally.
7. Leftover input tokens mean that rule did not match.
8. If no rule matches, it is a static error.

### Transcription

- Replace `$name` with its bound fragment. A `$name` that is not bound is emitted **verbatim**, as `$` plus the identifier.
- `$crate` expands to a special path root meaning "the crate where this macro was defined".
- Repetition depth rules:
  - A metavariable must be used at the same repetition depth, and inside the same nesting of repetitions, as in the matcher.
  - Every transcriber repetition must contain at least one metavariable that drives the iteration count.
  - All metavariables iterated together must have equal counts at every level. For `+`/`*`/`?` the count comes from the bindings.
- The output is parsed according to the **context of the invocation**:
  - item position (module, associated item, extern block, or as the argument of another macro per the FLS): zero or more items;
  - expression-without-block: an expression;
  - pattern-without-range: a pattern;
  - statement: zero or more statements;
  - type (without bounds): a type.

  If the parse fails or tokens are left over, it is an error.
- `recursion_limit` bounds the expansion depth. The default is 128.

### Hygiene

- `macro_rules` has **mixed-site** hygiene. Local variables, labels and `$crate` resolve at the **definition** site; everything else (items, functions, types, fields, methods) resolves at the **invocation** site.
- A `let x` introduced by one expansion is invisible to tokens from the call site, and to other expansions.
- Implementation: give every identifier a `SyntaxContext` (an expansion mark chain). Locals and labels compare by name and context; items compare by name and are looked up from the call site.
- Proc macros choose per token: `Span::call_site()` is unhygienic, `Span::mixed_site()` behaves like `macro_rules`, and `def_site` is unstable.

### Scoping [Ref]

- **Textual scope.** A macro is visible after its `macro_rules!` definition until the end of the enclosing module or block. It flows into child modules, including outline `mod` files. A later definition shadows an earlier one. A macro can call itself recursively because lookup happens at the use site.
- **Path-based scope.** A macro gets one via `#[macro_export]` (placed at the crate root, `pub`) or via `pub(crate) use m;` re-export. The implicit visibility of a macro is `pub(crate)`.
- **Lookup order.** An unqualified `m!` first tries textual scope, then path-based scope, and textual shadows path-based. A qualified `a::m!` uses path-based scope only.
- `#[macro_use] mod m { … }` extends the macro's textual scope past the module's end.
- `#[macro_use] extern crate c;` (optionally with `(a, b)`) imports `c`'s exported macros into the **macro_use prelude**.
- The FLS is thin on all of this. Use the Reference's `macros-by-example.md` §scoping and `names/*`.

---

## 9. Procedural macros (brief)

- They must be defined in a crate whose `crate_type` is `proc-macro`. Such a crate may only export proc macros publicly from its root.
- The implementation function must be `pub`, with no qualifiers and no generics, and use `proc_macro::TokenStream` for parameters and return type:

| Kind | Attribute | Signature | Invoked by | Input | Output handling |
|---|---|---|---|---|---|
| Function-like | `#[proc_macro]` | `fn(TokenStream) -> TokenStream` | `m!(…)` | contents without outer delimiters | parsed by invocation context |
| Derive | `#[proc_macro_derive(Name, attributes(h1, h2))]` | `fn(TokenStream) -> TokenStream` | `#[derive(Name)]` on struct/enum/union | the item, minus this and earlier `derive` attributes | **appended** next to the item; must be items |
| Attribute | `#[proc_macro_attribute]` | `fn(attr: TokenStream, item: TokenStream) -> TokenStream` | `#[path]` or `#[path(…)]` | (1) the attribute arguments (empty if none); (2) the item including its other outer attributes, without the invoking one | **replaces** the item; must be zero or more items |

- **Derive helper attributes** (`h1`, `h2`) are inert attributes that are in scope on the ADT.
- **Outline modules:** the bodies of `mod foo;` inside the input are *not* loaded into the token stream.
- An error or panic in the implementation function is a static error.
- An interpreter will probably support built-in derives natively (`Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Default`). Generated impls carry `#[automatically_derived]`. Real proc macros need a host ABI, or an interpreted `proc_macro` crate.

---

## 10. Program structure

```
SourceFile ::= `\u{FEFF}`? Shebang? InnerAttributeOrDoc* Item*
ModuleDeclaration ::= `unsafe`? `mod` Name (`{` InnerAttributeOrDoc* Item* `}` | `;`)
                      // `unsafe mod` parses (macros may consume it) but is rejected
ExternalCrateImport ::= `extern` `crate` (Identifier | `self`) Renaming? `;`   // `self` requires `as`
```

- **Crate.** A crate is the unit of compilation and linking: a tree of modules rooted at the **crate root module**, which is the compilation root and comes from a tool-defined file.
- **Crate types:** `bin` (the default), `lib`, `rlib`, `staticlib`, `dylib`, `cdylib`, `proc-macro`. The type comes from `#![crate_type]` or `--crate-type`. A crate with several types is treated as separate crates.
- **Outline module.** `mod foo;` loads a file and treats its contents as inlined.
  - The location is tool-defined unless `#[path = "…"]` is given.
  - rustc's rule: `foo.rs` or `foo/mod.rs` relative to the current module's directory. For a non-`mod.rs` file `a.rs`, children live in `a/`.
  - `#[path]` inside an inline module is relative to that module's implied directory.
- **`extern crate`.**
  - At the crate root, it adds the crate to the **extern prelude**.
  - With `as name`, it binds a local name in the enclosing scope.
  - How a crate name resolves to a crate is tool-defined (`--extern`).
- **Entry point.** A binary crate without `#![no_main]` must have `main` in scope at the crate root with a "main function signature": not `async` or `unsafe`, Rust ABI, no generics, no parameters, no where clause, and a body.
  - **(!)** The FLS says *no return type*. The Reference and rustc allow any `T: std::process::Termination`, such as `()`, `!`, `Result<T: Termination, E: Debug>` or `ExitCode`. Implement the rustc rule.
  - `main` may also be imported with `use`.
- **Expansion and resolution order.** The FLS only says that legality rules apply after expansion. In practice ([Ref] `names/name-resolution.md` plus rustc):
  1. Parse the root file.
  2. Run a fixpoint loop over the AST, repeating until no unexpanded invocations remain:
     - strip `cfg`-false nodes and expand `cfg_attr` (eagerly, at collection);
     - resolve `use` and macro paths (*expansion-time / early resolution*);
     - expand attribute macros and derives (an item's attributes are processed in order: `cfg`/`cfg_attr` first, then attribute macros and derives);
     - expand bang macros;
     - load `mod foo;` files when they are reached;
     - register newly produced `macro_rules`.
  3. After expansion, no unresolved invocation may remain.
  4. Then comes **primary (late) resolution**, then **type-relative resolution**, then type checking, borrow checking and evaluation.
  - Textual macro scope means that the **source order** of `macro_rules` definitions and `mod` declarations matters during expansion.

---

## 11. Conditional compilation

```
CfgContent ::= `cfg` `(` ConfigurationPredicate `)`
ConfigurationPredicate ::= Identifier (`=` StringLiteral)?            // option: `unix`, `feature = "x"`
                         | `all` `(` List? `)` | `any` `(` List? `)` | `not` `(` Pred `)`
                         | `true` | `false`
List ::= Pred (`,` Pred)* `,`?
CfgAttrContent ::= `cfg_attr` `(` Pred `,` AttributeContentList `)`    // Ref: list may be empty
```

- `all()` is true. `any()` is false. Evaluating an option is tool-defined: it is a set lookup over name and name/value pairs, and keys may have several values (e.g. `feature`, `target_feature`).
- A construct whose `cfg` is false is removed entirely. That applies to items, fields, variants, match arms, statements, expressions where allowed, parameters, generic parameters, and so on.
- A false `#![cfg]` on the crate root leaves an **empty crate**, but it keeps the attributes that came *before* that `cfg`. So `#![no_std] #![cfg(foo)]` keeps `no_std`.
- `cfg_attr(true, a, b)` becomes `#[a] #[b]`. It works recursively, so a nested `cfg_attr` is evaluated too. A false `cfg_attr` disappears.
- `cfg` and `cfg_attr` are **active** attributes: they are removed from the item.
- The `cfg!(pred)` macro expands to the literal `true` or `false`.
- **Options the Reference defines:** `target_arch`, `target_feature`, `target_os`, `target_family`, `unix`, `windows`, `target_env`, `target_abi`, `target_endian`, `target_pointer_width`, `target_vendor`, `target_has_atomic`, `test`, `debug_assertions`, `proc_macro`, `panic` (`"unwind"`/`"abort"`), plus the conventional `feature = "…"` from Cargo.
- For an interpreter, pick a host-like target triple and expose a `--cfg` flag.

---

## 12. Attributes

```
InnerAttributeOrDoc ::= `#![` AttributeContent `]` | InnerBuiltin | InnerBlockDoc | InnerLineDoc
OuterAttributeOrDoc ::= `#[`  AttributeContent `]` | OuterBuiltin | OuterBlockDoc | OuterLineDoc
AttributeContent    ::= SimplePath AttributeInput?
AttributeInput      ::= `(` TokenTree* `)` | `=` Expression       // Ref also allows `[..]`/`{..}` delimiters; `=` expr is usually a literal or macro call
AttributeContentList::= AttributeContent (`,` AttributeContent)* `,`?
```

- An inner attribute applies to the enclosing construct. An outer attribute applies to the following one.
- **Where attributes may appear** [Ref]:
  - all items take outer attributes;
  - modules, functions, impls, extern blocks and the crate take inner attributes;
  - also: statements, block expressions in limited positions, fields, variants, match arms, generic parameters, function, closure and fn-pointer parameters, and a few expression positions.
- **Active vs inert.**
  - Active (consumed, removed from the item): attribute macros, `cfg`, `cfg_attr`, and `test` when *not* compiling tests.
  - Everything else is inert and stays on the item.
- **Tool attributes** such as `#[rustfmt::skip]` and `#[clippy::…]` are inert. A tool may define extra attributes as long as their names do not clash with built-ins.
- **`unsafe(...)` wrapper** (2024 requires it): `#[unsafe(no_mangle)]`, `#[unsafe(export_name = …)]`, `#[unsafe(link_section = …)]`, `#[unsafe(naked)]`. It has no other effect. [Ref] also lists the unsafe-extern-block related forms.

### Built-in attributes by category

**Code generation**
- `cold`: the function is unlikely to be called.
- `inline`, `inline(always)`, `inline(never)`: hints only; applies to functions and closures.
- `naked`: no prologue or epilogue; the body must be a single `naked_asm!`; cannot be combined with `inline`, `track_caller` or `#[test]`.
- `no_builtins` (crate): do not replace code patterns with intrinsic calls.
- `target_feature(enable = "avx2,…")`: x86 feature list in the FLS.
  - A safe `#[target_feature]` function may only be called without `unsafe` from a function that enables a superset of its features.
  - **UB:** running the code on hardware that lacks the feature.
- `track_caller`: `Location::caller()` reports the topmost untracked caller.
  - Applies to non-`main`, Rust-ABI functions.
  - On a trait method, it applies to all implementations.
  - **UB:** a mismatch between the extern declaration and the definition.
  - This matters for an interpreter: panics inside `unwrap` and similar should report the caller's location.
- [Ref] `instruction_set`.

**Conditional compilation:** `cfg`, `cfg_attr` (§11).

**Derivation**
- `derive(Path, …)`: on struct, enum or union; invokes derive macros.
- `automatically_derived`: marker added to derive-generated impls of built-in traits.

**Diagnostics** (not defined by the FLS; they affect lints only)
- `allow`, `warn`, `deny`, `forbid`, `expect`
- `deprecated`, `must_use`
- [Ref] `diagnostic::on_unimplemented`, `diagnostic::do_not_recommend`.

**Documentation:** `doc = "…"`, `doc = macro!(…)`, `doc(…)`. Doc comments desugar to this.

**FFI, linkage and symbols**
- `crate_name = "…"`, `crate_type = "bin|lib|rlib|dylib|cdylib|staticlib|proc-macro"` (crate root).
- `export_name = "sym"`: function or static symbol name.
- `link_section = ".sec"`.
- `no_mangle`: use the plain name as the symbol and export it publicly. Defines "exported function" and "exported static", which count as external interactions.
- `link(name = "…", kind = "dylib|static|framework|raw-dylib", wasm_import_module = "…")` on extern blocks.
  - The kind defaults to `dylib`. `framework` is macOS-only.
  - `raw-dylib` needs a file extension in the name and forbids `no_mangle` items in the block.
- `link_name = "…"` and `link_ordinal(N)` on extern functions and statics. They are mutually exclusive, and `link_ordinal` requires `raw-dylib`.
- `no_link`: on `extern crate`; do not link it.
- `no_main` (crate): do not emit the entry symbol.
- `used`: on statics; keep in the object file.
- `repr(...)`: on ADTs. Combinations of `C`, `Rust`, `transparent`, a primitive integer type (`u8` … `isize`, for enums), `align(N)`, `packed` / `packed(N)`. This controls layout, which matters to an interpreter if it models memory byte-precisely: transmute, raw pointers, FFI.

**Limits** (crate root)
- `recursion_limit = "N"`: maximum macro-expansion and auto-deref depth (rustc default 128).
- `type_length_limit = "N"`.

**Macros**
- `macro_export`: makes a `macro_rules` macro `pub` at the crate root (path-based). [Ref] also has `macro_export(local_inner_macros)`.
- `macro_use`, `macro_use(a, b)`:
  - on `extern crate`: import its exported macros into the macro_use prelude;
  - on `mod`: extend textual scope.
- `proc_macro`, `proc_macro_attribute`, `proc_macro_derive(Name, attributes(...))`: on public root functions of a proc-macro crate.
- `collapse_debuginfo(no|external|yes)`: debug info location for macro expansions.

**Modules:** `path = "file"`: the module's file. The format is tool-defined.

**Preludes**
- `no_std` (crate): no std prelude; uses the core prelude; imports core's exported macros into the macro_use prelude.
- `no_implicit_prelude` (crate or module): disables the extern, std/core and tool preludes in that subtree.

**Runtime**
- `global_allocator`: on a static whose type implements `GlobalAlloc`.
- `panic_handler`: `fn(&PanicInfo) -> !`. The crate graph needs exactly one; std provides it.
- `windows_subsystem = "console|windows"` (binary crate root; default `console`).

**Testing**
- `test`: on a function that is not async, not unsafe, and has no generics or parameters.
  - It passes if it returns `()` without panicking, or returns `Ok(())` for `Result<(), E>`.
  - It is inert under `--test`. Otherwise it is active, and the function is removed.
- `ignore` / `ignore = "reason"`.
- `should_panic` / `should_panic(expected = "substr")`: passes only if it panics and the message contains the substring.

**Type system:** `non_exhaustive` on structs, enums and variants.
- It cannot be constructed outside the defining crate.
- Outside the defining crate, matches need `..` / a wildcard, and a non-exhaustive variant does not count toward exhaustiveness.

**Other (Reference only)**
- `debugger_visualizer`
- `feature(...)`: unstable features, crate root, nightly only.
- `crate-level` `no_core`: unstable.

---

## 13. Places where the FLS and the Reference or rustc disagree

The FLS is **edition-2021**; the Reference includes 2024 behaviour. For each item below, rustc behaves as the Reference says. Decide which edition rint targets; edition 2021 is simplest and matches the FLS.

1. **`main`'s return type.** FLS: no return type. Reference/rustc: any `T: Termination`.
2. **Follow set of `pat`.** The FLS includes `|`; for 2021+ rustc does not.
3. **Follow set of `vis`.** The FLS says the next metavariable may be `ident`, `ty` or `block`; the Reference says `ident`, `ty` or `path`.
4. **`$x:ident` and keywords.** The FLS says a pure identifier; the Reference accepts keywords and raw identifiers, but not `_`.
5. **Byte escapes.** The FLS grammar allows `\x[0-7][hex]` everywhere. Real rule: bytes allow `\x00`–`\xFF`; chars and strings allow only `\x00`–`\x7F`.
6. **2024 edition additions.** `gen` is a reserved keyword, guarded strings and `##` are reserved, `expr` also matches `_` and `const {}`, `unsafe(...)` attributes are required, and `raw` is a weak keyword.
7. **Missing from the FLS lexical chapter.** Lifetime tokens, reserved prefixes, arbitrary literal suffixes at token level, and the input pipeline (UTF-8, CRLF normalization, shebang `[` exception). Use the Reference's `tokens.md`, `input-format.md` and `shebang.md`.
8. **Macro scoping.** Textual vs path-based scope, shadowing, and `macro_use` order are only lightly covered by the FLS. Use the Reference's `macros-by-example.md` and `names/`.
