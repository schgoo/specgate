//! Link-time operation and semantic-type metadata registry.

use super::*;

// ---------------------------------------------------------------------------
// Operation registry - populated at link time via #[distributed_slice].
// Discovery binaries iterate this to find all annotated operations.
// ---------------------------------------------------------------------------

/// Metadata about one annotated operation or setup.
#[expect(
    clippy::struct_excessive_bools,
    reason = "link-time metadata preserves independent operation properties"
)]
#[derive(Debug, Clone, Copy)]
pub struct OpMeta {
    /// Semantic operation or setup name.
    pub(crate) name: &'static str,
    /// Native module path.
    pub(crate) module_path: &'static str,
    /// Native function or method name.
    pub(crate) fn_name: &'static str,
    /// Whether this declaration is a setup producer.
    pub(crate) is_setup: bool,
    /// Whether this declaration is asynchronous.
    pub(crate) is_async: bool,
    /// Whether this declaration is a method.
    pub(crate) is_method: bool,
    /// Whether this declaration is public.
    pub(crate) is_public: bool,
    /// Raw parameter names and types.
    pub(crate) params: &'static [FieldMeta],
    /// Raw return type.
    pub(crate) return_type: &'static str,
    /// For setups: the operation parameter this setup fills, when explicitly selected.
    /// Used to disambiguate when several params share the setup's output type.
    pub(crate) fills: Option<&'static str>,
    /// The component (declared via `spec_component!` or a per-item `spec = "..."`
    /// override) that owns this operation. Extraction groups by component and
    /// derives cross-component `depends_on` from it.
    pub(crate) component: &'static str,
}

/// One named field with its (stringified) Rust type. Used for both operation
/// parameters and `SpecEvent` struct/enum-variant fields.
#[derive(Debug, Clone, Copy)]
pub struct FieldMeta {
    name: &'static str,
    rust_type: &'static str,
}

impl FieldMeta {
    /// Construct static field metadata for generated registration.
    #[must_use]
    pub const fn new(name: FieldName, rust_type: RustType) -> Self {
        Self {
            name: name.0,
            rust_type: rust_type.0,
        }
    }

    /// Return the semantic field or parameter name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Return the stringified native Rust type.
    #[must_use]
    pub const fn rust_type(self) -> &'static str {
        self.rust_type
    }
}

macro_rules! static_text {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(&'static str);

        impl $name {
            /// Construct a typed static metadata value.
            #[must_use]
            pub const fn new(value: &'static str) -> Self {
                Self(value)
            }

            /// Return the registered text.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

static_text!(OpName, "A semantic operation name embedded in link-time metadata.");
static_text!(ModulePath, "A native Rust module path embedded in link-time metadata.");
static_text!(FnName, "A native Rust function name embedded in link-time metadata.");
static_text!(FieldName, "A semantic field or parameter name embedded in link-time metadata.");
static_text!(RustType, "A stringified native Rust type embedded in link-time metadata.");
static_text!(ComponentName, "A component identifier embedded in link-time metadata.");
static_text!(TypeName, "A semantic type name embedded in link-time metadata.");
static_text!(VariantName, "A semantic enum-variant name embedded in link-time metadata.");

/// Required immutable operation metadata supplied when starting an [`OpMeta`] builder.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::exhaustive_structs,
    reason = "macro expansions construct this fixed registration protocol directly"
)]
pub struct OpMetaDeps {
    /// Semantic operation or setup name.
    pub name: OpName,
    /// Native module path.
    pub module_path: ModulePath,
    /// Native function or method name.
    pub fn_name: FnName,
    /// Raw parameter names and types.
    pub params: &'static [FieldMeta],
    /// Raw return type.
    pub return_type: RustType,
    /// Owning component identifier.
    pub component: ComponentName,
}

/// Const builder for link-time operation metadata.
#[derive(Debug, Clone, Copy)]
pub struct OpMetaBuilder {
    meta: OpMeta,
}

impl OpMeta {
    /// Return the semantic operation or setup name.
    #[must_use]
    pub const fn name(self) -> OpName {
        OpName::new(self.name)
    }

    /// Return the native module path.
    #[must_use]
    pub const fn module_path(self) -> ModulePath {
        ModulePath::new(self.module_path)
    }

    /// Return the native function or method name.
    #[must_use]
    pub const fn fn_name(self) -> FnName {
        FnName::new(self.fn_name)
    }

    /// Report whether this declaration is a setup producer.
    #[must_use]
    pub const fn is_setup(self) -> bool {
        self.is_setup
    }

    /// Report whether this declaration is asynchronous.
    #[must_use]
    pub const fn is_async(self) -> bool {
        self.is_async
    }

    /// Report whether this declaration is a method.
    #[must_use]
    pub const fn is_method(self) -> bool {
        self.is_method
    }

    /// Report whether this declaration is public.
    #[must_use]
    pub const fn is_public(self) -> bool {
        self.is_public
    }

    /// Return the raw parameter metadata.
    #[must_use]
    pub const fn params(self) -> &'static [FieldMeta] {
        self.params
    }

    /// Return the raw native return type.
    #[must_use]
    pub const fn return_type(self) -> RustType {
        RustType::new(self.return_type)
    }

    /// Return the explicitly selected setup-filled parameter, when present.
    #[must_use]
    pub const fn fills(self) -> Option<FieldName> {
        match self.fills {
            Some(name) => Some(FieldName::new(name)),
            None => None,
        }
    }

    /// Return the owning component.
    #[must_use]
    pub const fn component(self) -> ComponentName {
        ComponentName::new(self.component)
    }

    /// Start a const builder with required operation identity and signature metadata.
    ///
    /// # Examples
    /// ```
    /// use specgate_runtime::registry::{
    ///     ComponentName, FnName, ModulePath, OpMeta, OpMetaDeps, OpName, RustType,
    /// };
    ///
    /// const META: OpMeta = OpMeta::builder(OpMetaDeps {
    ///     name: OpName::new("run"),
    ///     module_path: ModulePath::new("example"),
    ///     fn_name: FnName::new("run"),
    ///     params: &[],
    ///     return_type: RustType::new("()"),
    ///     component: ComponentName::new("example.component"),
    /// })
    /// .public(true)
    /// .build();
    ///
    /// # let _ = META;
    /// ```
    #[must_use]
    pub const fn builder(deps: OpMetaDeps) -> OpMetaBuilder {
        OpMetaBuilder {
            meta: Self {
                name: deps.name.0,
                module_path: deps.module_path.0,
                fn_name: deps.fn_name.0,
                is_setup: false,
                is_async: false,
                is_method: false,
                is_public: false,
                params: deps.params,
                return_type: deps.return_type.0,
                fills: None,
                component: deps.component.0,
            },
        }
    }
}

impl OpMetaBuilder {
    /// Mark setup-producer metadata.
    #[must_use]
    pub const fn setup(mut self, fills: Option<FieldName>) -> Self {
        self.meta.is_setup = true;
        self.meta.fills = match fills {
            Some(fills) => Some(fills.0),
            None => None,
        };
        self
    }

    /// Record whether the declaration is asynchronous.
    #[must_use]
    pub const fn asynchronous(mut self, value: bool) -> Self {
        self.meta.is_async = value;
        self
    }

    /// Record whether the declaration is a method.
    #[must_use]
    pub const fn method(mut self, value: bool) -> Self {
        self.meta.is_method = value;
        self
    }

    /// Record whether the declaration is public.
    #[must_use]
    pub const fn public(mut self, value: bool) -> Self {
        self.meta.is_public = value;
        self
    }

    /// Complete the const metadata value.
    #[must_use]
    pub const fn build(self) -> OpMeta {
        self.meta
    }
}

/// One enum variant: its name plus either named fields or an ordered tuple
/// payload. Unit variants carry neither.
#[expect(
    clippy::exhaustive_structs,
    reason = "macro expansions construct this fixed link-time metadata protocol directly"
)]
#[derive(Debug, Clone, Copy)]
pub struct VariantMeta {
    /// Semantic variant name.
    pub name: VariantName,
    /// Named payload fields.
    pub fields: &'static [FieldMeta],
    /// Ordered tuple payload types.
    pub tuple: Option<&'static [RustType]>,
}

/// The Rust data shape represented by [`TypeMeta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeKind {
    /// A struct with named semantic fields.
    Struct,
    /// An enum with semantic variants.
    Enum,
}

impl TypeKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
        }
    }
}

/// Required immutable type metadata supplied when starting a [`TypeMeta`] builder.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::exhaustive_structs,
    reason = "macro expansions construct this fixed registration protocol directly"
)]
pub struct TypeMetaDeps {
    /// Semantic type name.
    pub name: TypeName,
    /// Native module path.
    pub module_path: ModulePath,
    /// Rust data shape.
    pub kind: TypeKind,
    /// Owning component identifier.
    pub component: ComponentName,
}

/// Const builder for link-time semantic-type metadata.
#[derive(Debug, Clone, Copy)]
pub struct TypeMetaBuilder {
    meta: TypeMeta,
}

/// Metadata about a struct or enum that derives `SpecEvent`.
///
/// Structs populate `fields` (only
/// `#[spec_event]`-tagged fields, honoring `#[spec_event(name = "...")]`); enums
/// populate `variants`.
#[derive(Debug, Clone, Copy)]
pub struct TypeMeta {
    name: TypeName,
    module_path: ModulePath,
    kind: TypeKind,
    fields: &'static [FieldMeta],
    variants: &'static [VariantMeta],
    component: ComponentName,
}

impl TypeMeta {
    /// Start a const builder with required semantic-type identity metadata.
    ///
    /// # Examples
    /// ```
    /// use specgate_runtime::registry::{
    ///     ComponentName, ModulePath, TypeKind, TypeMeta, TypeMetaDeps, TypeName,
    /// };
    ///
    /// const META: TypeMeta = TypeMeta::builder(TypeMetaDeps {
    ///     name: TypeName::new("Request"),
    ///     module_path: ModulePath::new("example"),
    ///     kind: TypeKind::Struct,
    ///     component: ComponentName::new("example.component"),
    /// })
    /// .fields(&[])
    /// .build();
    ///
    /// # let _ = META;
    /// ```
    #[must_use]
    pub const fn builder(deps: TypeMetaDeps) -> TypeMetaBuilder {
        TypeMetaBuilder {
            meta: Self {
                name: deps.name,
                module_path: deps.module_path,
                kind: deps.kind,
                fields: &[],
                variants: &[],
                component: deps.component,
            },
        }
    }
}

impl TypeMetaBuilder {
    /// Set named struct fields.
    #[must_use]
    pub const fn fields(mut self, fields: &'static [FieldMeta]) -> Self {
        self.meta.fields = fields;
        self
    }

    /// Set enum variants.
    #[must_use]
    pub const fn variants(mut self, variants: &'static [VariantMeta]) -> Self {
        self.meta.variants = variants;
        self
    }

    /// Complete the const metadata value.
    ///
    /// # Panics
    /// Panics when the selected kind carries metadata for the other data shape.
    #[must_use]
    pub const fn build(self) -> TypeMeta {
        assert!(
            match self.meta.kind {
                TypeKind::Struct => self.meta.variants.is_empty(),
                TypeKind::Enum => self.meta.fields.is_empty(),
            },
            "semantic type metadata kind disagrees with its fields or variants"
        );
        self.meta
    }
}

#[linkme::distributed_slice]
/// Link-time inventory of annotated operations and setups.
pub static OPERATIONS: [OpMeta];

#[linkme::distributed_slice]
/// Link-time inventory of semantic types.
pub static TYPES: [TypeMeta];

/// Escape a string for inclusion as a JSON string literal. Handles the control
/// and structural characters that can appear in stringified Rust types (quotes,
/// backslashes); other characters pass through. Kept dependency-free so the
/// runtime stays lean.
fn write_string(out: &mut String, value: impl AsRef<str>) {
    // JSON requires every code point below U+0020 to use an escape sequence.
    const FIRST_UNESCAPED: u32 = 0x20;
    out.push('"');
    for character in value.as_ref().chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < FIRST_UNESCAPED => {
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

/// Write a JSON array of `[name, type]` field pairs into `out`.
fn write_fields(out: &mut String, fields: impl AsRef<[FieldMeta]>) {
    out.push('[');
    for (index, field) in fields.as_ref().iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('[');
        write_string(out, field.name);
        out.push(',');
        write_string(out, field.rust_type);
        out.push(']');
    }
    out.push(']');
}

fn write_property(out: &mut String, prefix: impl AsRef<str>, value: impl AsRef<str>) {
    out.push_str(prefix.as_ref());
    write_string(out, value);
}

/// Borrow the complete link-time discovery document.
///
/// Formatting the returned value emits the stable JSON consumed by discovery.
///
/// # Examples
///
/// ```
/// let document = specgate_runtime::registry::discovery();
/// let json = document.to_string();
/// assert!(json.starts_with("{\"operations\":["));
/// ```
#[must_use]
pub fn discovery() -> Discovery {
    Discovery {
        operations: &OPERATIONS,
        types: &TYPES,
    }
}

/// Typed borrowed view of registered operation and semantic-type metadata.
#[derive(Debug, Clone, Copy)]
pub struct Discovery {
    operations: &'static [OpMeta],
    types: &'static [TypeMeta],
}

impl Discovery {
    /// Encode the complete link-time inventory as deterministic discovery JSON.
    #[must_use]
    pub fn json(&self) -> String {
        let mut out = String::from("{\"operations\":[");
        for (index, op) in self.operations.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_property(&mut out, "{\"name\":", op.name);
            write_property(&mut out, ",\"module_path\":", op.module_path);
            write_property(&mut out, ",\"fn_name\":", op.fn_name);
            let _ = write!(
                out,
                ",\"is_setup\":{},\"is_async\":{},\"is_method\":{},\"is_public\":{}",
                op.is_setup, op.is_async, op.is_method, op.is_public
            );
            write_property(&mut out, ",\"return_type\":", op.return_type);
            write_property(&mut out, ",\"fills\":", op.fills.unwrap_or(""));
            write_property(&mut out, ",\"component\":", op.component);
            out.push_str(",\"params\":");
            write_fields(&mut out, op.params);
            out.push('}');
        }
        out.push_str("],\"types\":[");
        for (index, ty) in self.types.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_property(&mut out, "{\"name\":", ty.name);
            write_property(&mut out, ",\"module_path\":", ty.module_path);
            write_property(&mut out, ",\"kind\":", ty.kind.as_str());
            write_property(&mut out, ",\"component\":", ty.component);
            out.push_str(",\"fields\":");
            write_fields(&mut out, ty.fields);
            out.push_str(",\"variants\":[");
            for (variant_index, variant) in ty.variants.iter().enumerate() {
                if variant_index > 0 {
                    out.push(',');
                }
                write_property(&mut out, "{\"name\":", variant.name);
                out.push_str(",\"fields\":");
                write_fields(&mut out, variant.fields);
                out.push_str(",\"tuple\":");
                if let Some(tuple) = variant.tuple {
                    out.push('[');
                    for (field_index, field_type) in tuple.iter().enumerate() {
                        if field_index > 0 {
                            out.push(',');
                        }
                        write_string(&mut out, field_type);
                    }
                    out.push(']');
                } else {
                    out.push_str("null");
                }
                out.push('}');
            }
            out.push_str("]}");
        }
        out.push_str("]}");
        out
    }
}

impl std::fmt::Display for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.json())
    }
}
