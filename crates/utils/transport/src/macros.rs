//! Internal `macro_rules!` helpers shared across method-set modules.

/// Declare a complete RPC method-set in one table:
///
/// - emits a `Copy + Eq + Hash` enum with one variant per method
/// - implements `as_str`, `parse`, and [`std::fmt::Display`] on the enum
/// - emits a zero-sized type per method that implements [`crate::RpcMethod`]
///
/// # Example
///
/// ```ignore
/// define_method_set! {
///     /// Agent method set.
///     enum AgentMethod {
///         Initialize  = "initialize"     => AgentInitialize(InitializeParams) -> InitializeResponse,
///         Initialized = "initialized"    => AgentInitialized(()) -> (),
///         ProcessStart = "process/start" => AgentProcessStart(ProcessStartParams) -> ProcessStartResponse,
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_method_set {
    (
        $(#[$enum_meta:meta])*
        enum $enum_name:ident {
            $(
                $variant:ident = $literal:literal => $ty:ident ( $params:ty ) -> $result:ty
            ),+
            $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $enum_name {
            $( $variant ),+
        }

        impl $enum_name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $literal ),+
                }
            }

            pub fn parse(method: &str) -> Option<Self> {
                match method {
                    $( $literal => Some(Self::$variant), )+
                    _ => None,
                }
            }

            /// Every variant in declaration order — useful for tests and
            /// registry-style code that iterates the full set.
            pub const fn all() -> &'static [Self] {
                &[ $( Self::$variant ),+ ]
            }
        }

        impl ::std::fmt::Display for $enum_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        $(
            #[doc = concat!("Marker type for the `", $literal, "` method.")]
            pub struct $ty;

            impl $crate::RpcMethod for $ty {
                const METHOD: &'static str = $literal;
                type Params = $params;
                type Result = $result;
            }
        )+
    };
}
