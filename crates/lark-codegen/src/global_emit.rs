//! The C that a global block becomes.
//!
//! | Source | Emitted C |
//! |---|---|
//! | `@global name { ... }` | One file scope variable per declaration, plus an initializer |
//! | `@init name;` | One call to that initializer |
//! | `@global(f)` | The call goes at the start of `f`. Rule I-12. |
//!
//! Rule I-7 makes every such variable zero at program start, which C already
//! gives a file scope variable with no initializer. Rule I-10 guards the
//! initializer with a flag, so a second call does nothing.

/// Returns the C name of the initializer for a block. See rule X-5a.
pub fn init_name(module: &str, block: &str) -> String {
    format!(
        "{}__{block}__init",
        lark_mono::mangle::module_prefix(module)
    )
}

/// Returns the C name of the guard flag for a block. See rule I-10.
pub fn guard_name(module: &str, block: &str) -> String {
    format!(
        "{}__{block}__done",
        lark_mono::mangle::module_prefix(module)
    )
}
