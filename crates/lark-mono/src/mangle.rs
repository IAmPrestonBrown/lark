//! The name that a generic instantiation takes in the emitted C.
//!
//! Rule X-5a gives the scheme `lk_<module>__<name>__<argmangle>`. Rule X-7
//! places it in the identifier space that C reserves for the implementation, so
//! a well formed user program cannot collide with it.

/// Returns the mangled form of one type argument.
///
/// | Argument | Mangle |
/// |---|---|
/// | `int` | `i` |
/// | `unsigned int` | `j` |
/// | `char` | `c` |
/// | `T*` | `P` and the mangle of `T` |
/// | `gc T*` | `G` and the mangle of `T` |
/// | A user type | The length of the name, then the name |
#[must_use]
pub fn argument(text: &str) -> String {
    let trimmed = text.trim();

    if let Some(inner) = trimmed.strip_suffix('*') {
        let inner = inner.trim();
        if let Some(target) = inner.strip_prefix("gc ") {
            return format!("G{}", argument(target));
        }
        return format!("P{}", argument(inner));
    }
    if let Some(target) = trimmed.strip_prefix("gc ") {
        return format!("G{}", argument(target));
    }

    match trimmed {
        "void" => "v".to_owned(),
        "_Bool" => "b".to_owned(),
        "char" => "c".to_owned(),
        "signed char" => "a".to_owned(),
        "unsigned char" => "h".to_owned(),
        "short" => "s".to_owned(),
        "unsigned short" => "t".to_owned(),
        "int" => "i".to_owned(),
        "unsigned int" | "unsigned" => "j".to_owned(),
        "long" => "l".to_owned(),
        "unsigned long" => "m".to_owned(),
        "long long" => "x".to_owned(),
        "unsigned long long" => "y".to_owned(),
        "float" => "f".to_owned(),
        "double" => "d".to_owned(),
        "long double" => "e".to_owned(),
        name => {
            // A user type carries its length, so `Data` and `Dataset` differ.
            let clean: String = name
                .chars()
                .map(|item| {
                    if item.is_ascii_alphanumeric() {
                        item
                    } else {
                        '_'
                    }
                })
                .collect();
            format!("{}{clean}", clean.len())
        }
    }
}

/// Returns the C name of one instantiation. See rule X-5a.
#[must_use]
pub fn instance(module: &str, name: &str, arguments: &[String]) -> String {
    let mangled: String = arguments.iter().map(|text| argument(text)).collect();
    format!("lk_{module}__{name}__{mangled}")
}

#[cfg(test)]
mod tests {
    use super::{argument, instance};

    /// covers: X-5a
    #[test]
    fn a_builtin_type_takes_one_letter() {
        assert_eq!(argument("int"), "i");
        assert_eq!(argument("unsigned int"), "j");
        assert_eq!(argument("char"), "c");
        assert_eq!(argument("double"), "d");
    }

    /// covers: X-5a
    #[test]
    fn a_pointer_carries_its_marker() {
        assert_eq!(argument("int*"), "Pi");
        assert_eq!(argument("char* *"), "PPc");
        assert_eq!(argument("gc int*"), "Gi");
        assert_eq!(argument("gc Person*"), "G6Person");
    }

    /// covers: X-5a
    #[test]
    fn a_user_type_carries_its_length() {
        assert_eq!(argument("Person"), "6Person");
        assert_eq!(argument("Data"), "4Data");
    }

    /// covers: G-7, X-5a
    #[test]
    fn two_argument_sets_give_two_names() {
        let one = instance("app", "Data", &["int".to_owned()]);
        let other = instance("app", "Data", &["char".to_owned()]);
        assert_eq!(one, "lk_app__Data__i");
        assert_ne!(one, other);
    }

    /// covers: G-7
    #[test]
    fn the_same_argument_set_gives_the_same_name() {
        let one = instance("app", "Box", &["int".to_owned(), "gc Person*".to_owned()]);
        let other = instance("app", "Box", &["int".to_owned(), "gc Person*".to_owned()]);
        assert_eq!(one, other);
        assert_eq!(one, "lk_app__Box__iG6Person");
    }
}
