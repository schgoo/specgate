//! Unrestricted semantic identities used by discovery.
//!
//! These types prevent accidental interchange without imposing lexical rules
//! beyond the current CTSC contracts.

use std::fmt;
use std::ops::Deref;

macro_rules! string_identity {
    ($name:ident, $doc:literal) => {
        #[doc = concat!(
                                    $doc,
                                    "\n\nValues preserve producer spelling exactly and intentionally perform no lexical validation. ",
                                    "The distinct wrapper prevents accidental interchange with other semantic identities.\n\n",
                                    "# Examples\n\n",
                                    "```\n",
                                    "use specgate_discovery::identity::", stringify!($name), ";\n",
                                    "fn consume(value: &", stringify!($name), ") { println!(\"{value}\"); }\n",
                                    "let value = ", stringify!($name), "::from(\"example\");\n",
                                    "consume(&value);\n",
                                    "```\n"
                                )]
        #[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        pub struct $name(String);

        impl $name {
            /// Borrow the identity text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the identity and return its text.
            #[must_use]
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }
    };
}

string_identity!(ComponentId, "A CTSC component identity.");
string_identity!(OperationName, "A semantic operation name.");
string_identity!(TargetName, "A binding target name.");
string_identity!(TypeName, "A named semantic type.");
string_identity!(FieldName, "A semantic field or input name.");
string_identity!(ErrorName, "A declared operation error name.");
string_identity!(VariantName, "A named semantic enum variant.");
string_identity!(TypeExpression, "A source or normalized semantic type expression.");
string_identity!(PackageName, "A Cargo package name.");
string_identity!(PackageVersion, "A Cargo package version or requirement.");
string_identity!(RegistryName, "A Cargo registry name or index URL.");
string_identity!(ModulePath, "A native module path.");
string_identity!(FunctionName, "A native function or method name.");
string_identity!(KindName, "A raw producer type-kind name.");

#[cfg(test)]
mod tests {
    use super::ComponentId;
    use std::borrow::Borrow;

    #[test]
    fn identity_conversions() {
        let identity = ComponentId::from(String::from("example.orders"));

        assert_eq!(identity.as_str(), "example.orders");
        assert_eq!(identity.as_ref(), "example.orders");
        assert_eq!(Borrow::<str>::borrow(&identity), "example.orders");
        assert_eq!(&*identity, "example.orders");
        assert_eq!(identity.to_string(), "example.orders");
        assert_eq!(identity, "example.orders");
        assert_eq!(identity.clone().into_string(), "example.orders");
    }
}
