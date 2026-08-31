# 01 - Lexical Structure

## 1. Source text

Source files use the extension `.lark`. The encoding is UTF-8. Comments follow
C11 exactly: `/* ... */` and `// ...`.

Lark uses the C11 token set without change, and adds two token forms.

| Token | Text | Use |
|---|---|---|
| Directive | `@` followed by an identifier | `@import`, `@global`, `@init` |
| Scope | `::` | Namespace qualification |

**Rule L-1.** The character `@` does not appear in any valid C11 token outside a
string literal, a character constant, or a comment. A `@` directive therefore
never conflicts with C.

**Rule L-2.** The token `::` is invalid in C11 outside an attribute token
sequence. Lark reads `::` as the scope operator everywhere except inside
`[[ ... ]]`.

## 2. Contextual keywords

**Rule L-3.** Lark reserves no identifier. Each keyword below is recognized only
at its stated position. At every other position the same spelling is an ordinary
identifier.

| Keyword | Recognized when | Why C cannot parse it there |
|---|---|---|
| `gc` | It precedes a type specifier in a declaration or a cast. | Two type names in sequence violate C11 6.7.2p2. |
| `managed` | It precedes `struct`. | Same constraint violation. |
| `iface` | It starts a top level declaration and an identifier and `{` follow. | Not a C declaration form. |
| `impl` | It starts a top level declaration and `for` follows the next identifier. | Not a C declaration form. |
| `new` | It appears in expression position and a type name follows. | A type name cannot follow an identifier in a C expression. |
| `export` | It starts a top level declaration and a declaration follows. | Same constraint violation. |
| `init` | It precedes a function definition. | Same constraint violation. |
| `gc_leaf` | It precedes a declaration. | Same constraint violation. |
| `gc_safe` | It precedes a declaration. | Same constraint violation. |
| `auto` | See section 3. | See section 3. |
| `Self` | Inside an `impl` body only. | Ordinary identifier elsewhere. |

**Rule L-4.** A contextual keyword at a non-keyword position is an identifier. A
C program that declares `int new;` stays valid.

## 3. The `auto` rule

C11 section 6.7.2 paragraph 2 requires at least one type specifier in every
declaration. So `auto x = 5;` is invalid C11, and the spelling is available.

**Rule L-5.** In a declaration that starts with `auto`:

- If a type specifier follows, `auto` is the C storage class specifier. Lark
  ignores it, as C does.
- If a declarator follows, `auto` requests type inference. Chapter 02 defines
  the inference.

This matches the meaning that C23 gives to `auto`. Lark agrees with the standard
rather than diverging from it.

## 4. The generic disambiguation rule

Generic arguments use angle brackets. `struct Data<T>`, `gc Data<int>* p`, and
`swap<Person>(&a, &b)` all parse without ambiguity.

**Rule L-6 (the innermost binding rule).** When the parser reads an identifier
followed by `<`, it resolves the identifier in the innermost enclosing scope.

- If the identifier names a **type**, the parser reads a generic argument list.
- If the identifier names a **value**, the parser reads a relational expression.
- If the identifier is unbound, the answer depends on rule L-15.

**Rule L-7.** A generic argument is always a type. An expression is never a
generic argument.

Rules L-6 and L-7 together make the grammar unambiguous. The proof is short. A
C expression cannot contain a type name as an operand. So when the identifier
before `<` names a type, no valid C reading exists, and the generic reading is
the only one.

**Rule L-15.** The name table is **complete** for a translation unit when the
front end knows every name that the unit can see. A unit whose headers the front
end has not read has an incomplete table.

- With a complete table, an unbound identifier before `<` opens a generic
  argument list. The front end then reports an unknown type.
- With an incomplete table, an unbound identifier before `<` opens a relational
  expression, because a name from an unread header is almost always a value.

Rule L-15 exists because delivery phase A does not read headers. Chapter 00
section 4 gives the phases. In phase B the table becomes complete, and the first
branch applies.

The front end resolves names in a pass that runs before it parses any function
body. Every type name in the translation unit is therefore known. Unlike C++,
Lark does not depend on declaration order for this decision.

**Rule L-16.** A local declaration hides an outer binding of the same name, from
its own declarator to the end of its scope.

```c
void f(void) {
    g(swap<Person>(a));   /* Person is a type, so this is generic */
    int Person;
    g(swap<Person>(a));   /* Person is now a value, so this is a comparison */
}
```

A block, a function, and a `for` statement each open a scope. A parameter
belongs to the function that declares it, and reaches no other function.

## 5. Well formed tokens

**Rule L-10.** A block comment must end before the end of the file. An
unterminated block comment is diagnostic LK0103.

**Rule L-11.** A character constant and a string literal must end on the line
where they start. An unterminated literal is diagnostic LK0104.

**Rule L-12.** A character that cannot start a token is diagnostic LK0105. The
lexer keeps the character as an error token, so the text stays complete.

**Rule L-13.** The lexer keeps every byte of the source. Whitespace, a comment,
and a preprocessor line all become tokens. The tokens of a file join back into
the file, byte for byte.

Rule L-13 is invariant R from `docs/test-strategy.md`, at the token level. A
formatter, a rename, and an accurate hover all depend on it.

**Rule L-14.** A `>>` token that closes two generic argument lists splits into
two `>` tokens.

```c
gc Box<Data<int>>* nested;   /* the `>>` closes both lists */
int shifted = a >> b;        /* the `>>` stays one shift operator */
```

C needs `>>` for a shift, so the lexer produces one token. The parser splits it
where a generic list needs a close. The two halves stay in the tree, so rule
L-13 holds.

## 6. Two pass name resolution

**Rule L-8.** The front end processes a translation unit in two passes.

1. **Pass one** reads every top level declaration and records every name: types,
   functions, globals, interfaces, and modules.
2. **Pass two** parses function bodies and checks types.

A program can therefore reference any top level name from any point in the file,
with no forward declaration.

**Rule L-9.** Pass one respects C scoping for the C subset. A C program that
depends on declaration order keeps its meaning, because pass one never makes a
name visible earlier than C makes it visible *for the purpose of C parsing*.
Lark name visibility is what pass one extends.

## 7. LSP requirement

The parser produces a full fidelity tree. The tree keeps every token, including
whitespace and comments, and gives every node a source span. The parser recovers
from errors and always produces a tree. A single file reparses without a reparse
of its dependencies.

This requirement is normative for the implementation. It is not a language rule.
