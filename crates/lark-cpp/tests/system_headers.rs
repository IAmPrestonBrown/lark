//! Reads the real system headers. Test type T3.
//!
//! The test proves rule C-1 on the machine that runs it. A header set that the
//! platform ships must parse and yield the names that the C standard names.
//!
//! covers: C-1, C-1a, C-1c, C-1e

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::Path;

use lark_cpp::{Headers, Options, read};

/// Reads one header set, and returns the names it declares.
fn headers_for(include: &str) -> Headers {
    let source = format!("{include}\nint main(void) {{ return 0; }}\n");
    match read(&source, Path::new("probe.lark"), &Options::default()) {
        Ok(headers) => headers,
        Err(error) => panic!("cannot read {include}: {error}"),
    }
}

#[test]
fn stdio_declares_printf_and_file() {
    let headers = headers_for("#include <stdio.h>");
    assert!(headers.is_value("printf"), "printf is missing");
    assert!(headers.is_value("fopen"), "fopen is missing");
    assert!(headers.is_type("FILE"), "FILE is not a type");
    assert!(headers.is_type("size_t"), "size_t is not a type");
    assert!(headers.value("printf").is_some_and(|d| d.is_function));
}

#[test]
fn stdlib_declares_allocation_and_exit() {
    let headers = headers_for("#include <stdlib.h>");
    for name in ["malloc", "free", "exit", "abort", "qsort", "strtol"] {
        assert!(headers.is_value(name), "{name} is missing");
    }
    assert!(headers.is_type("div_t"));
}

#[test]
fn string_declares_the_copy_functions() {
    let headers = headers_for("#include <string.h>");
    for name in ["memcpy", "strlen", "strcmp", "strncpy", "memset"] {
        assert!(headers.is_value(name), "{name} is missing");
    }
}

#[test]
fn several_headers_merge_into_one_set() {
    let headers = headers_for("#include <stdio.h>\n#include <string.h>\n#include <math.h>");
    assert!(headers.is_value("printf"));
    assert!(headers.is_value("strlen"));
    assert!(headers.is_value("sqrt"));
}

#[test]
fn stdio_defines_the_stream_macros() {
    // `stdout` is a macro for `__stdoutp`, not a declaration. A name table
    // without the macros would call `stdout` unknown.
    let headers = headers_for("#include <stdio.h>");
    for name in ["stdout", "stderr", "stdin", "EOF", "NULL"] {
        assert!(headers.is_macro(name), "{name} is not a macro");
    }
}

#[test]
fn stdbool_defines_bool_as_a_type() {
    // `bool` is a macro for `_Bool`, so a table that calls every macro a value
    // reads `bool ready = 1;` as an expression. Rule C-1e.
    let headers = headers_for("#include <stdbool.h>");
    assert!(headers.is_type("bool"), "bool is not a type");
    assert!(!headers.is_value("bool"));
    assert!(headers.is_macro("true"), "true is not a macro");
}

#[test]
fn stdint_declares_the_sized_types() {
    let headers = headers_for("#include <stdint.h>");
    for name in ["int8_t", "uint8_t", "int32_t", "uint64_t", "intptr_t"] {
        assert!(headers.is_type(name), "{name} is not a type");
    }
}

#[test]
fn a_missing_header_reports_an_error() {
    let source = "#include <no_such_header_exists.h>\n";
    let result = read(source, Path::new("probe.lark"), &Options::default());
    assert!(result.is_err());
}

#[test]
fn a_source_with_no_include_reads_nothing() {
    let result = read(
        "int main(void) { return 0; }",
        Path::new("p.lark"),
        &Options::default(),
    );
    assert!(result.is_ok_and(|headers| headers.is_empty()));
}

#[test]
fn a_wide_header_set_parses_without_an_error() {
    // Rule C-4 and rule C-6 together. Every header below uses a compiler
    // extension that Lark reads but gives no meaning.
    let list = [
        "stdio",
        "stdlib",
        "string",
        "math",
        "time",
        "errno",
        "ctype",
        "stdint",
        "signal",
        "locale",
        "wchar",
        "inttypes",
        "stdarg",
        "complex",
        "pthread",
        "unistd",
        "fcntl",
        "dirent",
        "regex",
        "termios",
        "sys/stat",
        "sys/types",
        "sys/socket",
        "sys/time",
    ];
    for name in list {
        let headers = headers_for(&format!("#include <{name}.h>"));
        assert!(!headers.is_empty(), "{name}.h declared no name");
    }
}

/// A name from a header enters the global namespace, so a program writes it
/// with no prefix at all.
/// covers: N-12, C-1b
#[test]
fn a_header_name_needs_no_prefix() {
    let headers = headers_for("#include <string.h>");
    // The header declares `strlen`, and rule N-12 puts it in the global
    // namespace rather than in a namespace of its own.
    assert!(headers.is_value("strlen"));
    // Nothing carries a module prefix, because a header is not a module.
    for name in headers.values().take(200) {
        assert!(!name.contains("::"), "`{name}` carries a prefix");
    }
}

/// Rule C-6. A header that uses an extension the parser does not model still
/// yields its names. A header must never stop a build.
/// covers: C-6, S-3
#[test]
fn an_unknown_extension_does_not_stop_a_header() {
    // Every one of these headers uses a compiler extension somewhere.
    for name in ["stdio", "pthread", "signal", "unistd", "dirent", "regex"] {
        let headers = headers_for(&format!("#include <{name}.h>"));
        assert!(!headers.is_empty(), "{name}.h yielded no name");
    }
}

/// Rule C-7. A declaration with no body names a function that lives
/// elsewhere, and the header set is where those declarations come from.
/// covers: C-7
#[test]
fn a_header_declaration_has_no_body() {
    let headers = headers_for("#include <stdlib.h>");
    let Some(malloc) = headers.value("malloc") else {
        panic!("malloc is missing");
    };
    assert!(malloc.is_function);
    // A prototype ends at a semicolon, so no body follows it.
    assert!(
        !malloc.text.contains('{'),
        "malloc carries a body: {}",
        malloc.text
    );
}

/// Rule C-2. The front end keeps the source that the programmer wrote, so a
/// diagnostic names a line of that file rather than a line of the
/// preprocessed text.
/// covers: C-2
#[test]
fn a_diagnostic_names_the_line_the_programmer_wrote() {
    use lark_resolve::{FileLoader, resolve_with};

    // `<stdio.h>` expands to hundreds of lines. The error sits on line 4 of
    // the file the programmer wrote, and the report must say 4.
    let source = "#include <stdio.h>\n\
                  \n\
                  managed struct Person { gc char* name; }\n\
                  static gc Person* bad_global;\n";
    let directory = std::env::temp_dir();
    let path = directory.join("lark-cpp-c2-probe.lark");
    let loader = FileLoader::new(vec![directory]);
    let reader = lark_cpp::Reader::new(Options::default());
    let resolution = resolve_with(&loader, &reader, "probe", &path, source);
    let found = lark_types::check_resolution(&resolution);

    let items = found.items();
    assert!(
        !items.is_empty(),
        "the file must report the misplaced global"
    );
    for item in items {
        // Every span lies inside the source that the programmer wrote, not in
        // the thousands of lines that the header adds.
        assert!(
            item.primary.span.end as usize <= source.len(),
            "a span at {:?} runs past the source of {} bytes",
            item.primary.span,
            source.len()
        );
    }
}
