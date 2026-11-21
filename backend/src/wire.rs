//! Enums whose values travel as fixed strings: in SQLite, in events, over the API, in what the
//! app reads. Each variant's string is written once, in `wire_enum!`; `as_str`, `parse`, `ALL`,
//! `Display`, `Serialize` and `Deserialize` all come from it, so the stored form and the parsed
//! form cannot drift apart when a variant is added.

/// Declare an enum with the string each variant is stored and sent as:
///
/// ```ignore
/// wire_enum! {
///     /// What a run is.
///     #[derive(Default)]
///     pub enum RunKind {
///         #[default]
///         Goal = "goal",
///         Chat = "chat",
///     }
/// }
/// ```
///
/// The enum derives `Clone, Copy, Debug, PartialEq, Eq, Hash`; other derives (`Default`) go in
/// the attributes. A string that names no variant does not parse (`None`, or a serde error).
macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $wire:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The string this value is stored and sent as.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }

            /// The value a stored or sent string names.
            pub fn parse(s: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|v| v.as_str() == s)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<$name, D::Error> {
                let s = <String as serde::Deserialize>::deserialize(d)?;
                $name::parse(&s).ok_or_else(|| serde::de::Error::unknown_variant(&s, &[$($wire),+]))
            }
        }
    };
}

pub(crate) use wire_enum;

#[cfg(test)]
mod tests {
    wire_enum! {
        #[derive(Default)]
        enum Sample {
            #[default]
            One = "one",
            TwoWords = "two_words",
        }
    }

    /// The one string per variant is what `as_str`, `parse`, `Display` and serde all use.
    #[test]
    fn one_string_per_variant_everywhere() {
        assert_eq!(Sample::ALL, &[Sample::One, Sample::TwoWords]);
        for v in Sample::ALL {
            assert_eq!(Sample::parse(v.as_str()), Some(*v));
            assert_eq!(v.to_string(), v.as_str());
            assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(v.as_str()));
            assert_eq!(serde_json::from_value::<Sample>(serde_json::json!(v.as_str())).unwrap(), *v);
        }
        assert_eq!(Sample::default(), Sample::One);
        assert_eq!(Sample::parse("TwoWords"), None);
        assert!(serde_json::from_value::<Sample>(serde_json::json!("three")).is_err());
    }
}
