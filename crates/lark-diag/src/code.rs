use std::fmt;

/// How serious a diagnostic is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Severity {
    /// The compiler rejects the program.
    Error,
    /// The compiler accepts the program and reports a problem.
    Warning,
    /// Extra context for the diagnostic above.
    Note,
    /// A suggested change.
    Help,
}

impl Severity {
    /// Returns the word that the renderer prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
            Self::Help => "help",
        }
    }

    /// Reports whether the severity stops the compiler from accepting the input.
    #[must_use]
    pub const fn is_fatal(self) -> bool {
        matches!(self, Self::Error)
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A stable diagnostic code, such as `LK0301`.
///
/// A test asserts the code, never the message text. See rule P-1 in
/// `docs/test-strategy.md`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code(u16);

impl Code {
    /// Returns the numeric part of the code.
    #[must_use]
    pub const fn number(self) -> u16 {
        self.0
    }

    /// Returns the catalogue entry for the code.
    ///
    /// Returns `None` for a code that the catalogue does not list.
    #[must_use]
    pub fn info(self) -> Option<&'static CodeInfo> {
        CATALOG
            .binary_search_by_key(&self.0, |entry| entry.code.0)
            .ok()
            .map(|index| &CATALOG[index])
    }

    /// Parses a code from its printed form, such as `LK0301`.
    ///
    /// Returns `None` when the text does not have the shape of a code, or when
    /// the catalogue does not list it.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix("LK")?;
        if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let code = Self(digits.parse().ok()?);
        code.info().map(|_| code)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LK{:04}", self.0)
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// One entry in the diagnostic catalogue.
#[derive(Clone, Copy, Debug)]
pub struct CodeInfo {
    /// The code itself.
    pub code: Code,
    /// The severity that the specification gives the code.
    pub severity: Severity,
    /// The specification rule that the code enforces, or `n/a`.
    pub rule: &'static str,
    /// The message from chapter 12 of the specification.
    pub message: &'static str,
}

/// Declares every code as a constant and builds the catalogue.
///
/// The list mirrors `docs/spec/12-diagnostics.md`. A test checks that the two
/// stay the same.
macro_rules! catalog {
    ($( $name:ident = $number:literal, $severity:ident, $rule:literal, $message:literal; )*) => {
        $(
            #[doc = $message]
            pub const $name: Code = Code($number);
        )*

        /// Every diagnostic code, in numeric order.
        pub static CATALOG: &[CodeInfo] = &[
            $(
                CodeInfo {
                    code: Code($number),
                    severity: Severity::$severity,
                    rule: $rule,
                    message: $message,
                },
            )*
        ];
    };
}

catalog! {
    LK0100 = 100, Error, "L-6", "unresolved name before `<`, cannot decide generic or comparison";
    LK0101 = 101, Error, "L-3", "`@` directive is unknown";
    LK0102 = 102, Error, "n/a", "unterminated generic argument list";
    LK0103 = 103, Error, "L-10", "the block comment does not end before the end of the file";
    LK0104 = 104, Error, "L-11", "the literal does not end on the line where it starts";
    LK0105 = 105, Error, "L-12", "the character cannot start a token";
    LK0110 = 110, Error, "n/a", "the parser expected a different token";
    LK0200 = 200, Error, "T-2", "`gc` applies only to a pointer type";
    LK0210 = 210, Error, "T-9", "`auto` declaration needs an initializer";
    LK0211 = 211, Error, "T-11", "`auto` is not valid in this position";
    LK0301 = 301, Error, "T-5", "no implicit conversion between a managed pointer and a raw pointer";
    LK0310 = 310, Error, "M-2", "a managed pointer cannot live here";
    LK0311 = 311, Error, "M-3", "a managed struct cannot live in unmanaged memory";
    LK0320 = 320, Error, "M-8", "the selected collector does not support an interior pointer";
    LK0330 = 330, Error, "M-15", "`longjmp` crosses a frame that holds a managed local";
    LK0340 = 340, Error, "M-22", "a `gc_leaf` function cannot take a managed parameter";
    LK0400 = 400, Error, "O-2, G-11", "this struct needs the `managed` marker";
    LK0410 = 410, Error, "O-13", "the implementation is missing a function that the interface declares";
    LK0411 = 411, Error, "O-13", "the implementation declares a function that the interface does not";
    LK0412 = 412, Error, "O-14", "an interface applies only to a managed struct";
    LK0413 = 413, Error, "O-15", "an implementation must live with its interface or with its type";
    LK0420 = 420, Error, "O-18", "the address of a stack object is not a managed pointer";
    LK0421 = 421, Error, "O-21", "the method name is ambiguous across two interfaces";
    LK0430 = 430, Error, "O-12", "an interface function needs a receiver";
    LK0440 = 440, Error, "C-9", "an exported signature has no C form";
    LK0500 = 500, Error, "G-8", "the instantiation depth limit is reached";
    LK0501 = 501, Error, "G-6a", "a call to a generic function needs a type argument list";
    LK0502 = 502, Error, "G-2, L-7", "a generic argument must be a type";
    LK0600 = 600, Error, "N-3", "the module is not found on the search path";
    LK0610 = 610, Error, "N-10", "an exported declaration names a private type";
    LK0611 = 611, Error, "N-11", "the name is not exported from that module";
    LK0612 = 612, Error, "N-2", "a module reference needs the `name::` prefix";
    LK0613 = 613, Error, "N-2", "no module with that name is imported here";
    LK0700 = 700, Error, "I-1", "no function carries the `init` marker";
    LK0701 = 701, Error, "I-1", "more than one function carries the `init` marker";
    LK0710 = 710, Error, "I-11", "`@init` names an unknown global block";
    LK0711 = 711, Error, "I-17", "this initializer reads a global that is not initialized yet";
    LK0800 = 800, Error, "C-9", "this type has no C representation, so C cannot call this function";
    LK0801 = 801, Warning, "C-6", "an unknown extension is skipped. Warning, not an error";
    LK0900 = 900, Error, "F-1", "the configuration field is unknown";
    LK0901 = 901, Error, "R-1", "the collector name is unknown";
}

#[cfg(test)]
mod tests {
    use super::{CATALOG, Code, LK0301, Severity};

    #[test]
    fn the_catalogue_is_sorted_and_holds_no_duplicate() {
        for pair in CATALOG.windows(2) {
            assert!(
                pair[0].code.number() < pair[1].code.number(),
                "the catalogue is out of order at {} and {}",
                pair[0].code,
                pair[1].code
            );
        }
    }

    #[test]
    fn prints_a_code_with_four_digits() {
        assert_eq!(LK0301.to_string(), "LK0301");
    }

    #[test]
    fn parses_a_printed_code() {
        assert_eq!(Code::parse("LK0301"), Some(LK0301));
    }

    #[test]
    fn rejects_text_that_is_not_a_code() {
        assert_eq!(Code::parse("LK301"), None);
        assert_eq!(Code::parse("XX0301"), None);
        assert_eq!(Code::parse("LK03o1"), None);
        assert_eq!(Code::parse("LK9999"), None);
    }

    #[test]
    fn every_entry_carries_a_message() {
        for entry in CATALOG {
            assert!(!entry.message.is_empty(), "{} has no message", entry.code);
            assert!(!entry.rule.is_empty(), "{} has no rule", entry.code);
        }
    }

    #[test]
    fn only_the_listed_codes_are_warnings() {
        let warnings: Vec<_> = CATALOG
            .iter()
            .filter(|entry| entry.severity == Severity::Warning)
            .map(|entry| entry.code.to_string())
            .collect();
        assert_eq!(warnings, vec!["LK0801"]);
    }
}
