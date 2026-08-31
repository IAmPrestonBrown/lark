//! Type representation and the type store.
//!
//! Every type is a value in one store. A [`TypeId`] is an index into it, so a
//! type is one word to copy and holds no cycle. See decision D040.

use std::collections::HashMap;
use std::fmt::Write as _;

/// A handle to one type in a [`TypeStore`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TypeId(u32);

/// The width of an integer type.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum IntWidth {
    /// `_Bool`.
    Bool,
    /// `char`.
    Char,
    /// `short`.
    Short,
    /// `int`.
    Int,
    /// `long`.
    Long,
    /// `long long`.
    LongLong,
}

impl IntWidth {
    /// Returns the rank that the usual arithmetic conversions use.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Bool => 0,
            Self::Char => 1,
            Self::Short => 2,
            Self::Int => 3,
            Self::Long => 4,
            Self::LongLong => 5,
        }
    }
}

/// The width of a floating type.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum FloatWidth {
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `long double`.
    LongDouble,
}

/// What a named type names.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NamedKind {
    /// A `struct` tag.
    Struct,
    /// A `union` tag.
    Union,
    /// An `enum` tag.
    Enum,
    /// A name that `typedef` introduces.
    Alias,
    /// An interface. Rule T-12 makes an interface name a type.
    Iface,
    /// A name that the front end cannot classify yet.
    Unknown,
}

/// One type.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeKind {
    /// The type is not known. It silences every later diagnostic.
    Error,
    /// `void`.
    Void,
    /// An integer type.
    Int {
        /// The width.
        width: IntWidth,
        /// Whether the type is signed.
        signed: bool,
    },
    /// A floating type.
    Float(FloatWidth),
    /// A pointer.
    Pointer {
        /// What the pointer refers to.
        target: TypeId,
        /// Whether the pointer carries the `gc` qualifier. See rule T-1.
        managed: bool,
    },
    /// An array.
    Array {
        /// The element type.
        element: TypeId,
        /// The number of elements, when the declaration gives one.
        length: Option<u64>,
    },
    /// A function.
    Function {
        /// The result type.
        result: TypeId,
        /// The parameter types.
        params: Vec<TypeId>,
        /// Whether the parameter list ends with `...`.
        variadic: bool,
    },
    /// A struct, a union, an enum, a type alias, or an interface.
    Named {
        /// The name as written.
        name: String,
        /// What the name names.
        kind: NamedKind,
        /// The generic arguments, when the use gives any.
        args: Vec<TypeId>,
    },
    /// A generic parameter that no instantiation replaced.
    Param(String),
}

/// Every type that one compiler run builds.
#[derive(Debug)]
pub struct TypeStore {
    types: Vec<TypeKind>,
    index: HashMap<TypeKind, TypeId>,
}

/// The types that every run needs.
#[derive(Clone, Copy, Debug)]
pub struct Common {
    /// The unknown type.
    pub error: TypeId,
    /// `void`.
    pub void: TypeId,
    /// `_Bool`.
    pub bool: TypeId,
    /// `char`.
    pub char: TypeId,
    /// `int`.
    pub int: TypeId,
    /// `unsigned int`.
    pub uint: TypeId,
    /// `long`.
    pub long: TypeId,
    /// `unsigned long`, the type of `sizeof`.
    pub size: TypeId,
    /// `double`.
    pub double: TypeId,
}

impl Default for TypeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeStore {
    /// Builds a store that holds the common types.
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Returns the handles for the types that every run needs.
    pub fn common(&mut self) -> Common {
        Common {
            error: self.intern(TypeKind::Error),
            void: self.intern(TypeKind::Void),
            bool: self.int(IntWidth::Bool, false),
            char: self.int(IntWidth::Char, true),
            int: self.int(IntWidth::Int, true),
            uint: self.int(IntWidth::Int, false),
            long: self.int(IntWidth::Long, true),
            size: self.int(IntWidth::Long, false),
            double: self.intern(TypeKind::Float(FloatWidth::Double)),
        }
    }

    /// Adds a type and returns its handle.
    ///
    /// A type that the store already holds keeps its first handle, so two equal
    /// types compare equal by handle.
    pub fn intern(&mut self, kind: TypeKind) -> TypeId {
        if let Some(id) = self.index.get(&kind) {
            return *id;
        }
        let id = TypeId(u32::try_from(self.types.len()).unwrap_or(u32::MAX));
        self.index.insert(kind.clone(), id);
        self.types.push(kind);
        id
    }

    /// Returns the type for a handle.
    ///
    /// A handle from another store yields the error type.
    #[must_use]
    pub fn kind(&self, id: TypeId) -> &TypeKind {
        self.types.get(id.0 as usize).unwrap_or(&TypeKind::Error)
    }

    /// Adds an integer type.
    pub fn int(&mut self, width: IntWidth, signed: bool) -> TypeId {
        self.intern(TypeKind::Int { width, signed })
    }

    /// Adds a pointer type.
    pub fn pointer(&mut self, target: TypeId, managed: bool) -> TypeId {
        self.intern(TypeKind::Pointer { target, managed })
    }

    /// Adds an array type.
    pub fn array(&mut self, element: TypeId, length: Option<u64>) -> TypeId {
        self.intern(TypeKind::Array { element, length })
    }

    /// Reports whether a type is the unknown type.
    #[must_use]
    pub fn is_error(&self, id: TypeId) -> bool {
        matches!(self.kind(id), TypeKind::Error)
    }

    /// Reports whether a type is a pointer.
    #[must_use]
    pub fn is_pointer(&self, id: TypeId) -> bool {
        matches!(self.kind(id), TypeKind::Pointer { .. })
    }

    /// Reports whether a pointer carries the `gc` qualifier.
    #[must_use]
    pub fn is_managed(&self, id: TypeId) -> bool {
        matches!(self.kind(id), TypeKind::Pointer { managed: true, .. })
    }

    /// Reports whether a type is an integer or a floating type.
    #[must_use]
    pub fn is_arithmetic(&self, id: TypeId) -> bool {
        matches!(self.kind(id), TypeKind::Int { .. } | TypeKind::Float(_))
    }

    /// Applies the decay that C performs on an array or a function.
    ///
    /// An array becomes a pointer to its element. A function becomes a pointer
    /// to itself. Every other type stays as it is.
    pub fn decay(&mut self, id: TypeId) -> TypeId {
        match self.kind(id).clone() {
            TypeKind::Array { element, .. } => self.pointer(element, false),
            TypeKind::Function { .. } => self.pointer(id, false),
            _ => id,
        }
    }

    /// Returns the type in the form that a diagnostic prints.
    #[must_use]
    pub fn display(&self, id: TypeId) -> String {
        let mut out = String::new();
        self.write(&mut out, id);
        out
    }

    fn write(&self, out: &mut String, id: TypeId) {
        match self.kind(id) {
            TypeKind::Error => out.push_str("<unknown>"),
            TypeKind::Void => out.push_str("void"),
            TypeKind::Int { width, signed } => {
                let name = match width {
                    IntWidth::Bool => "_Bool",
                    IntWidth::Char => "char",
                    IntWidth::Short => "short",
                    IntWidth::Int => "int",
                    IntWidth::Long => "long",
                    IntWidth::LongLong => "long long",
                };
                if !signed && *width != IntWidth::Bool {
                    out.push_str("unsigned ");
                }
                out.push_str(name);
            }
            TypeKind::Float(width) => out.push_str(match width {
                FloatWidth::Float => "float",
                FloatWidth::Double => "double",
                FloatWidth::LongDouble => "long double",
            }),
            TypeKind::Pointer { target, managed } => {
                if *managed {
                    out.push_str("gc ");
                }
                self.write(out, *target);
                out.push('*');
            }
            TypeKind::Array { element, length } => {
                self.write(out, *element);
                match length {
                    Some(count) => {
                        let _ = write!(out, "[{count}]");
                    }
                    None => out.push_str("[]"),
                }
            }
            TypeKind::Function {
                result,
                params,
                variadic,
            } => {
                self.write(out, *result);
                out.push('(');
                for (index, param) in params.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    self.write(out, *param);
                }
                if *variadic {
                    out.push_str(if params.is_empty() { "..." } else { ", ..." });
                }
                out.push(')');
            }
            TypeKind::Named { name, kind, args } => {
                match kind {
                    NamedKind::Struct => out.push_str("struct "),
                    NamedKind::Union => out.push_str("union "),
                    NamedKind::Enum => out.push_str("enum "),
                    NamedKind::Alias | NamedKind::Iface | NamedKind::Unknown => {}
                }
                out.push_str(name);
                if !args.is_empty() {
                    out.push('<');
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            out.push_str(", ");
                        }
                        self.write(out, *arg);
                    }
                    out.push('>');
                }
            }
            TypeKind::Param(name) => out.push_str(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FloatWidth, IntWidth, NamedKind, TypeKind, TypeStore};

    #[test]
    fn an_equal_type_keeps_one_handle() {
        let mut store = TypeStore::new();
        let first = store.int(IntWidth::Int, true);
        let second = store.int(IntWidth::Int, true);
        assert_eq!(first, second);
        let other = store.int(IntWidth::Int, false);
        assert_ne!(first, other);
    }

    #[test]
    fn a_handle_from_no_store_reads_as_the_error_type() {
        let store = TypeStore::new();
        let common = TypeStore::new().common();
        assert!(matches!(store.kind(common.int), TypeKind::Error));
    }

    /// covers: T-1
    #[test]
    fn a_managed_pointer_prints_its_marker() {
        let mut store = TypeStore::new();
        let char_type = store.int(IntWidth::Char, true);
        let managed = store.pointer(char_type, true);
        let plain = store.pointer(char_type, false);
        assert_eq!(store.display(managed), "gc char*");
        assert_eq!(store.display(plain), "char*");
        assert!(store.is_managed(managed));
        assert!(!store.is_managed(plain));
    }

    #[test]
    fn an_array_decays_to_a_pointer_to_its_element() {
        let mut store = TypeStore::new();
        let int_type = store.int(IntWidth::Int, true);
        let array = store.array(int_type, Some(4));
        let decayed = store.decay(array);
        assert_eq!(store.display(array), "int[4]");
        assert_eq!(store.display(decayed), "int*");
    }

    #[test]
    fn a_function_decays_to_a_pointer_to_itself() {
        let mut store = TypeStore::new();
        let common = store.common();
        let function = store.intern(TypeKind::Function {
            result: common.int,
            params: vec![common.char],
            variadic: true,
        });
        assert_eq!(store.display(function), "int(char, ...)");
        let decayed = store.decay(function);
        assert!(store.is_pointer(decayed));
    }

    #[test]
    fn a_generic_use_prints_its_arguments() {
        let mut store = TypeStore::new();
        let common = store.common();
        let data = store.intern(TypeKind::Named {
            name: "Data".to_owned(),
            kind: NamedKind::Struct,
            args: vec![common.int],
        });
        assert_eq!(store.display(data), "struct Data<int>");
    }

    #[test]
    fn every_common_type_prints_its_c_spelling() {
        let mut store = TypeStore::new();
        let common = store.common();
        assert_eq!(store.display(common.void), "void");
        assert_eq!(store.display(common.int), "int");
        assert_eq!(store.display(common.uint), "unsigned int");
        assert_eq!(store.display(common.bool), "_Bool");
        assert_eq!(store.display(common.double), "double");
        let float_type = store.intern(TypeKind::Float(FloatWidth::Float));
        assert_eq!(store.display(float_type), "float");
    }
}
