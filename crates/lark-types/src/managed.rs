//! What a module holds that is managed.
//!
//! A `managed struct` carries an object header and a field map. Rule O-2 says
//! when the marker is required. Rule M-5 says the field map lists the byte
//! offset of every managed field, so heap tracing is precise.

use std::collections::BTreeMap;

use lark_span::Span;
use lark_syntax::SyntaxKind::{
    DECL_SPECIFIERS, DECLARATOR, ENUM_BODY, ENUM_DEF, ENUM_KW, FIELD_DECL, GENERIC_PARAMS, IDENT,
    IMPL_DEF, NAME, NAME_REF, POINTER, STRUCT_BODY, STRUCT_DEF, STRUCT_KW, UNION_DEF, UNION_KW,
};
use lark_syntax::{SyntaxNode, child_tokens};

/// One field of a record.
#[derive(Clone, Debug)]
pub struct Field {
    /// The name of the field.
    pub name: String,
    /// Whether the field holds a managed pointer.
    pub managed: bool,
}

/// Which keyword introduced a record. Rule X-8 emits the same one again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keyword {
    /// `struct`
    Struct,
    /// `union`
    Union,
    /// `enum`
    Enum,
}

impl Keyword {
    /// Returns the word as C spells it.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Union => "union",
            Self::Enum => "enum",
        }
    }
}

/// One struct, union, or enum that a module declares.
#[derive(Clone, Debug)]
pub struct Record {
    /// The tag name.
    pub name: String,
    /// The keyword that introduced it. See rule X-8.
    pub keyword: Keyword,
    /// Whether the declaration carries the `managed` marker.
    pub marked: bool,
    /// Whether the declaration carries `export`.
    pub exported: bool,
    /// Whether the record takes generic parameters.
    pub generic: bool,
    /// Every field, in declaration order.
    pub fields: Vec<Field>,
    /// Where the tag name is written.
    pub span: Span,
    /// Where the `struct` keyword is written.
    pub keyword_span: Span,
}

impl Record {
    /// Reports whether the record needs an object header.
    ///
    /// Rule O-2 requires the marker when the record holds a managed field, or
    /// when an implementation targets it.
    #[must_use]
    pub fn needs_header(&self, has_impl: bool) -> bool {
        has_impl || self.fields.iter().any(|field| field.managed)
    }

    /// Returns the names of the managed fields, in declaration order.
    pub fn managed_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|field| field.managed)
    }
}

/// What one module holds that is managed.
#[derive(Clone, Debug, Default)]
pub struct Managed {
    /// Every record, by tag name.
    pub records: BTreeMap<String, Record>,
    /// The type that each `impl` targets.
    pub implemented: Vec<String>,
}

impl Managed {
    /// Reports whether an implementation targets a type.
    #[must_use]
    pub fn has_impl(&self, name: &str) -> bool {
        self.implemented.iter().any(|target| target == name)
    }

    /// Reports whether a type name needs an object header.
    #[must_use]
    pub fn needs_header(&self, name: &str) -> bool {
        self.records
            .get(name)
            .is_some_and(|record| record.needs_header(self.has_impl(name)))
    }

    /// Returns every record that needs an object header.
    pub fn headed_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .values()
            .filter(|record| record.needs_header(self.has_impl(&record.name)))
    }

    /// Returns every record that needs a descriptor.
    ///
    /// Rule M-5a. `lark_new` reads the payload size from the descriptor, so a
    /// record needs one whether or not it holds a managed field. A record with
    /// no managed field gets a descriptor with an empty field map.
    ///
    /// An enum is never allocated, so it needs no descriptor.
    pub fn described_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .values()
            .filter(|record| record.keyword != Keyword::Enum)
    }

    /// Reports whether the module declares a managed record by that name.
    #[must_use]
    pub fn has_record(&self, name: &str) -> bool {
        self.records.contains_key(name)
    }
}

/// Reads every record and every implementation target of one module.
#[must_use]
pub fn collect(root: &SyntaxNode) -> Managed {
    let mut found = Managed::default();

    for node in root.descendants() {
        match node.kind() {
            STRUCT_DEF | UNION_DEF | ENUM_DEF => {
                if let Some(record) = read_record(&node) {
                    found.records.insert(record.name.clone(), record);
                }
            }
            IMPL_DEF => {
                // `impl Iface for Type`, so the second name is the target.
                let names: Vec<String> = node
                    .children()
                    .filter(|child| child.kind() == NAME_REF)
                    .filter_map(|child| child.first_token())
                    .map(|token| token.text().to_owned())
                    .collect();
                if let Some(target) = names.get(1) {
                    found.implemented.push(target.clone());
                }
            }
            _ => {}
        }
    }

    found
}

/// Reads one record definition.
fn read_record(node: &SyntaxNode) -> Option<Record> {
    // An enum has an `ENUM_BODY` and no field of its own.
    let body = node
        .children()
        .find(|child| matches!(child.kind(), STRUCT_BODY | ENUM_BODY))?;
    let name_node = node.children().find(|child| child.kind() == NAME)?;
    let name_token = name_node.first_token()?;
    let keyword =
        child_tokens(node).find(|token| matches!(token.kind(), STRUCT_KW | UNION_KW | ENUM_KW))?;
    let word = match keyword.kind() {
        UNION_KW => Keyword::Union,
        ENUM_KW => Keyword::Enum,
        _ => Keyword::Struct,
    };

    let marked = child_tokens(node).any(|token| token.kind() == IDENT && token.text() == "managed");

    let exported = node
        .parent()
        .and_then(|specifiers| specifiers.parent())
        .is_some_and(|item| {
            child_tokens(&item)
                .find(|token| !token.kind().is_trivia())
                .is_some_and(|token| token.kind() == IDENT && token.text() == "export")
        });

    let generic = node.children().any(|child| child.kind() == GENERIC_PARAMS);

    let mut fields = Vec::new();
    for field in body.children().filter(|child| child.kind() == FIELD_DECL) {
        let managed = field_is_managed(&field);
        for declarator in field.children().filter(|child| child.kind() == DECLARATOR) {
            let Some(field_name) = declarator_name(&declarator) else {
                continue;
            };
            fields.push(Field {
                name: field_name,
                managed,
            });
        }
    }

    Some(Record {
        name: name_token.text().to_owned(),
        keyword: word,
        marked,
        exported,
        generic,
        fields,
        span: span_of(&name_token),
        keyword_span: span_of(&keyword),
    })
}

/// Reports whether a field declaration holds a managed pointer.
///
/// Rule T-1a puts the `gc` marker in the specifiers or after a `*`.
fn field_is_managed(field: &SyntaxNode) -> bool {
    field
        .descendants_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
        .any(|token| {
            token.kind() == IDENT
                && token.text() == "gc"
                && token
                    .parent()
                    .is_some_and(|parent| matches!(parent.kind(), DECL_SPECIFIERS | POINTER))
        })
}

/// Returns the name that a declarator introduces.
fn declarator_name(declarator: &SyntaxNode) -> Option<String> {
    for child in declarator.children() {
        match child.kind() {
            NAME => return child.first_token().map(|token| token.text().to_owned()),
            DECLARATOR => {
                if let Some(name) = declarator_name(&child) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

/// Returns the span of a token.
fn span_of(token: &lark_syntax::SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

#[cfg(test)]
mod tests {
    use lark_syntax::{NoNames, parse};

    use super::collect;

    fn managed_of(source: &str) -> super::Managed {
        collect(&parse(source, &NoNames).syntax())
    }

    /// covers: O-2
    #[test]
    fn a_record_with_a_managed_field_needs_a_header() {
        let found = managed_of("managed struct Person { gc char* name; int age; }");
        let record = found.records.get("Person").expect("Person must exist");
        assert!(record.marked);
        assert!(record.needs_header(false));
        assert_eq!(record.fields.len(), 2);
        assert_eq!(record.managed_fields().count(), 1);
    }

    /// covers: O-2
    #[test]
    fn a_record_with_no_managed_field_needs_no_header() {
        let found = managed_of("struct Point { int x; int y; }");
        let record = found.records.get("Point").expect("Point must exist");
        assert!(!record.marked);
        assert!(!record.needs_header(false));
    }

    /// covers: O-2
    #[test]
    fn an_implementation_target_needs_a_header() {
        let found = managed_of(
            "struct Point { int x; }\n\
             iface Greet { void say_hi(Self this); }\n\
             impl Greet for Point { void say_hi(Point this) { } }\n",
        );
        assert!(found.has_impl("Point"));
        assert!(found.needs_header("Point"));
    }

    #[test]
    fn a_gc_after_a_star_marks_the_field() {
        let found = managed_of("managed struct Holder { char* gc name; }");
        let record = found.records.get("Holder").expect("Holder must exist");
        assert_eq!(record.managed_fields().count(), 1);
    }

    #[test]
    fn an_export_marker_reaches_the_record() {
        let found = managed_of("export managed struct Person { gc char* name; }");
        let record = found.records.get("Person").expect("Person must exist");
        assert!(record.exported);
    }
}
