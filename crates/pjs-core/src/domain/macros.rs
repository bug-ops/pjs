//! GAT macro utilities for domain port declarations
//!
//! This module provides macros that simplify the declaration of GAT-based
//! domain ports by eliminating boilerplate while preserving zero-cost abstractions.
//!
//! # Overview
//!
//! GAT (Generic Associated Types) traits require verbose type declarations
//! for each async method. The `gat_port!` macro converts ergonomic async fn
//! syntax into proper GAT trait definitions.
//!
//! # Example
//!
//! ```ignore
//! gat_port! {
//!     /// My domain port with async methods
//!     pub trait MyPort {
//!         /// Process an item
//!         async fn process(&self, item: Item) -> ();
//!         /// Fetch a result
//!         async fn fetch(&self, id: u64) -> Option<Item>;
//!     }
//! }
//! ```
//!
//! Expands to:
//!
//! ```ignore
//! pub trait MyPort: Send + Sync {
//!     type ProcessFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
//!     where Self: 'a;
//!     fn process(&self, item: Item) -> Self::ProcessFuture<'_>;
//!
//!     type FetchFuture<'a>: Future<Output = DomainResult<Option<Item>>> + Send + 'a
//!     where Self: 'a;
//!     fn fetch(&self, id: u64) -> Self::FetchFuture<'_>;
//! }
//! ```

/// Macro for declaring GAT-based domain ports with async fn syntax
///
/// Converts ergonomic async fn declarations into proper GAT traits with
/// associated future types. All methods return `DomainResult<T>`.
///
/// # Syntax
///
/// ```ignore
/// gat_port! {
///     $(#[doc = "..."])*
///     pub trait TraitName {
///         $(#[doc = "..."])*
///         async fn method_name(&self, arg: Type, ...) -> ReturnType;
///         // or
///         async fn method_name(&mut self, arg: Type, ...) -> ReturnType;
///     }
/// }
/// ```
///
/// # Generated Code
///
/// For each `async fn method_name(...)` the macro generates:
/// 1. An associated type `MethodNameFuture<'a>` (PascalCase) with proper bounds
/// 2. A method `fn method_name(...)` returning that future type
///
/// # Features
///
/// - Supports both `&self` and `&mut self` receivers
/// - Preserves all doc comments and attributes
/// - Automatically adds `Send + Sync` supertrait to the trait
/// - Uses unique lifetime `'gat_lt` to avoid conflicts
/// - All futures are `Send + 'a` bounded
#[macro_export]
macro_rules! gat_port {
    (
        $(#[$trait_meta:meta])*
        $vis:vis trait $trait_name:ident {
            $(
                $(#[$method_meta:meta])*
                async fn $method:ident( $($args:tt)* ) -> $ret:ty;
            )*
        }
    ) => {
        $(#[$trait_meta])*
        $vis trait $trait_name: ::std::marker::Send + ::std::marker::Sync {
            $(
                ::paste::paste! {
                    $(#[$method_meta])*
                    #[doc = ""]
                    #[doc = concat!("Future type for `", stringify!($method), "` method")]
                    type [<$method:camel Future>]<'a>: ::std::future::Future<
                        Output = $crate::domain::DomainResult<$ret>
                    > + ::std::marker::Send + 'a
                    where
                        Self: 'a;
                }

                $(#[$method_meta])*
                fn $method( $($args)* ) -> ::paste::paste! { Self::[<$method:camel Future>]<'_> };
            )*
        }
    };
}

// Re-export macro at crate level for convenience
pub use gat_port;

#[cfg(test)]
mod tests {
    use crate::domain::DomainResult;
    use std::future::Future;

    // Test that gat_port! generates valid trait definitions
    gat_port! {
        /// Test trait for unit return type
        pub trait TestUnitPort {
            /// Method returning unit
            async fn do_something(&self) -> ();
        }
    }

    // Test with arguments
    gat_port! {
        /// Test trait with arguments
        pub trait TestArgsPort {
            /// Method with multiple arguments
            async fn process(&self, id: u64, name: String) -> String;
        }
    }

    // Test with mut self
    gat_port! {
        /// Test trait with mutable receiver
        pub trait TestMutPort {
            /// Method requiring mutable access
            async fn mutate(&mut self, value: i32) -> ();
        }
    }

    // Test mixed methods
    gat_port! {
        /// Test trait with both &self and &mut self
        pub trait TestMixedPort {
            /// Immutable method
            async fn read(&self) -> String;
            /// Mutable method
            async fn write(&mut self, data: String) -> ();
        }
    }

    // Mock implementation to verify generated trait is implementable
    struct MockUnitPort;

    impl TestUnitPort for MockUnitPort {
        type DoSomethingFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn do_something(&self) -> Self::DoSomethingFuture<'_> {
            async move { Ok(()) }
        }
    }

    struct MockArgsPort;

    impl TestArgsPort for MockArgsPort {
        type ProcessFuture<'a>
            = impl Future<Output = DomainResult<String>> + Send + 'a
        where
            Self: 'a;

        fn process(&self, _id: u64, name: String) -> Self::ProcessFuture<'_> {
            async move { Ok(name) }
        }
    }

    struct MockMutPort {
        value: i32,
    }

    impl TestMutPort for MockMutPort {
        type MutateFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn mutate(&mut self, value: i32) -> Self::MutateFuture<'_> {
            async move {
                self.value = value;
                Ok(())
            }
        }
    }

    struct MockMixedPort {
        data: String,
    }

    impl TestMixedPort for MockMixedPort {
        type ReadFuture<'a>
            = impl Future<Output = DomainResult<String>> + Send + 'a
        where
            Self: 'a;

        fn read(&self) -> Self::ReadFuture<'_> {
            async move { Ok(self.data.clone()) }
        }

        type WriteFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn write(&mut self, data: String) -> Self::WriteFuture<'_> {
            async move {
                self.data = data;
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_gat_port_unit() {
        let port = MockUnitPort;
        port.do_something().await.unwrap();
    }

    #[tokio::test]
    async fn test_gat_port_args() {
        let port = MockArgsPort;
        let result = port.process(42, "test".to_string()).await.unwrap();
        assert_eq!(result, "test");
    }

    #[tokio::test]
    async fn test_gat_port_mut() {
        let mut port = MockMutPort { value: 0 };
        port.mutate(42).await.unwrap();
        assert_eq!(port.value, 42);
    }

    #[tokio::test]
    async fn test_gat_port_mixed() {
        let mut port = MockMixedPort {
            data: "initial".to_string(),
        };

        let value = port.read().await.unwrap();
        assert_eq!(value, "initial");

        port.write("updated".to_string()).await.unwrap();

        let value = port.read().await.unwrap();
        assert_eq!(value, "updated");
    }
}
