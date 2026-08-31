# 15 - The Formatter, the Debugger, and the Editor

Three tools, one chapter, because each reads what the compiler already
produces: the lossless tree of invariant R, the object header of rule M-4, and
the language server. None of them needs a parser or a metadata format of its
own.

## 1. The formatter

**Rule Z-1.** There is one canonical style and nothing to configure. An option
set turns every project into an argument about the option set, and the value of
a formatter is that the argument stops.

| Choice | The style |
|---|---|
| Indent | Four spaces. Never a tab. |
| A brace | On the line of the construct that opens it. |
| A statement | One per line. |
| A binary operator | One space on each side. |
| A comma | No space before, one space after. |
| A call | No space between the name and the parenthesis. |
| A keyword before a group | One space, as in `if (x)`. |
| A pointer star | Bound to the type, as in `gc char* name`. |
| A generic list | Bound to the name, as in `Box<Data<int>>`. |
| An initializer | On one line, as in `new Person { .age = 1 }`. |
| A blank line | At most one, anywhere. |
| The end of a line | No trailing space. |
| The end of a file | Exactly one newline. |

A pointer star and a generic angle both need the tree to decide. A `*` is a
pointer when its parent is a `POINTER` node and a product otherwise. A `<` is a
generic list when its parent is a `GENERIC_ARGS` or a `GENERIC_PARAMS` node and
a comparison otherwise. Rule L-6 already made that decision once, and the
formatter reads the answer rather than guessing again.

## 2. What the formatter must never do

**Rule Z-2.** Formatting changes no token. The sequence of tokens that are not
whitespace or a comment is the same before and after, so the program means the
same thing.

That rule is not free. Two tokens that a style would write next to each other
sometimes read as different tokens when they touch.

```c
a++ + ++a      /* the source */
a++ ++a        /* without the space: `a ++ ++ a`, a different program */
```

So a space goes between two tokens whenever writing them together would lex
differently. The check lexes the pair, which is exact and needs no table of
operators. The same check covers `/` before `*`, which would open a comment.

A generic list closes with `>`, and the parser splits a `>>` into two tokens to
close two lists at once. A space between them would leave text that lexes as
two tokens rather than one, so the list binds tight for that reason as well as
for looks.

**Rule Z-3.** Formatting twice equals formatting once. A style that does not
settle would make every save a change and every difference noise.

A layout choice therefore never reads the whitespace of the source. The choice
looks at the next token that is not trivia, so the second pass decides what the
first pass decided.

**Rule Z-4.** A file that does not parse still formats. The tree keeps the text
that the parser could not read, so the parts that read are laid out and the
rest stands. An editor holds a file in that state most of the time.

## 3. The commands

```
lark fmt <file.lark> ...          rewrite each file in the canonical style
lark fmt --check <file.lark> ...  name each file that is not formatted
```

`--check` changes nothing and exits non zero when a file differs. That is the
form that a gate runs.

## 4. The debugger

Rule X-3 already emits `#line`, so a debugger shows Lark source and Lark line
numbers with no help. What it cannot show is a managed value: a `gc Person*`
prints as an address.

**Rule Z-5.** Lark ships no debugger. It ships a formatter script for `lldb`
and one for `gdb`, and a build writes both beside the program. Each script
reads the object header that rule M-4 puts at a negative offset, and the
descriptor that rule M-5 fills in, so neither needs metadata that the compiler
does not already emit.

```text
(lldb) frame variable one
(Person *) one = 0x986800250 Person at 0x986800250

(lldb) frame variable team
(Person *) team = 0x986810150 Person[3] at 0x986810150

(lldb) gc-stats
collector          precise-marksweep
total_allocations  2
collections        0
```

The count in `Person[3]` comes from the header, so an array prints its length
without a cast.

**Rule Z-6.** A build emits debug information by default. A debugger needs it
to name a local, and a program that cannot be debugged is harder to trust than
one that is larger. `build.debug = false` turns it off.

## 5. Loading a script

```text
lldb:  command script import build/lark_lldb.py
gdb:   source build/lark_gdb.py
```

The name carries an underscore rather than a dash, because a Python module name
cannot hold a dash and `lldb` refuses one.

## 6. The editor

**Rule Z-7.** The editor extension gives two things that do not depend on each
other. A TextMate grammar colours a file with no compiler installed at all. A
language server client gives completion, hover, go to definition, and
diagnostics when `lark-lsp` is on the path.

A person who installs the extension and nothing else still reads Lark
correctly. A server that does not start is reported once, and the editor stays
usable.

**Rule Z-8.** The grammar names every keyword that the lexer knows. A grammar
is a second list of the keywords, and a second list drifts, so a test compares
it against the lexer.

Rule L-3 makes every Lark keyword contextual, and the grammar gives each one a
scope of its own, so a reader tells `managed` from `static` by colour.

```text
editors/vscode/
  package.json                    the manifest and the settings
  language-configuration.json     brackets, comments, indentation
  syntaxes/lark.tmLanguage.json   the grammar
  src/extension.js                the client, in plain JavaScript
```

The client is plain JavaScript with no build step, so the file a reader opens
is the file that runs.
