// src/core/connectable.rs
//
// Replaces the conflicting blanket impls:
//
//   impl<T: Producer> Connectable for T { .. }
//   impl<T: Consumer> Connectable for T { .. }
//   impl<T: Processor> Connectable for T { .. }
//
// Those can never compile together because a single type could implement
// more than one of these traits -> overlapping impls (E0119).

use super::{Consumer, Producer}; use crate::core::processor::Processor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectRole {
    Producer,
    Processor,
    Consumer,
}

/// A unified adapter trait that lets Flow/Graph treat nodes uniformly
/// without using overlapping blanket impls.
///
/// Design rule:
/// - Implement this explicitly for your concrete node types.
/// - Use the macros below to keep it painless.
pub trait Connectable: Send + Sync {
    /// Stable role used by connect logic (routing rules, validation, etc.)
    fn role(&self) -> ConnectRole;

    /// Human-readable identifier (used for logs, registry keys, errors, UI).
    fn name(&self) -> &str;

    /// Role-specific views. Exactly one of these should typically be Some(..).
    fn as_producer(&self) -> Option<&dyn Producer> {
        None
    }
    fn as_producer_mut(&mut self) -> Option<&mut dyn Producer> {
        None
    }

    fn as_processor(&self) -> Option<&dyn Processor> {
        None
    }
    fn as_processor_mut(&mut self) -> Option<&mut dyn Processor> {
        None
    }

    fn as_consumer(&self) -> Option<&dyn Consumer> {
        None
    }
    fn as_consumer_mut(&mut self) -> Option<&mut dyn Consumer> {
        None
    }
}

/// Convenience helpers for connect logic.
/// Keep your Flow/Graph code readable.
impl dyn Connectable {
    #[inline]
    pub fn is_producer(&self) -> bool {
        self.role() == ConnectRole::Producer
    }
    #[inline]
    pub fn is_processor(&self) -> bool {
        self.role() == ConnectRole::Processor
    }
    #[inline]
    pub fn is_consumer(&self) -> bool {
        self.role() == ConnectRole::Consumer
    }
}

/* ---------- Macros to implement Connectable for concrete types ----------

Assumptions for macro use:
- Your type implements the matching role trait (Producer/Processor/Consumer).
- Your type has a method `name(&self) -> &str`.

If your type does NOT have `name()`, just implement Connectable manually.
*/

#[macro_export]
macro_rules! impl_connectable_producer {
    ($ty:ty) => {
        impl $crate::core::connectable::Connectable for $ty {
            fn role(&self) -> $crate::core::connectable::ConnectRole {
                $crate::core::connectable::ConnectRole::Producer
            }

            fn name(&self) -> &str {
                <Self as $crate::core::Producer>::name(self)
            }

            fn as_producer(&self) -> Option<&dyn $crate::core::Producer> {
                Some(self)
            }

            fn as_producer_mut(&mut self) -> Option<&mut dyn $crate::core::Producer> {
                Some(self)
            }
        }
    };
}

#[macro_export]
macro_rules! impl_connectable_processor {
    ($ty:ty) => {
        impl $crate::core::connectable::Connectable for $ty {
            fn role(&self) -> $crate::core::connectable::ConnectRole {
                $crate::core::connectable::ConnectRole::Processor
            }

            fn name(&self) -> &str {
                <Self as $crate::core::processor::Processor>::name(self)
            }

            fn as_processor(&self) -> Option<&dyn $crate::core::processor::Processor> {
                Some(self)
            }

            fn as_processor_mut(&mut self) -> Option<&mut dyn $crate::core::processor::Processor> {
                Some(self)
            }
        }
    };
}

#[macro_export]
macro_rules! impl_connectable_consumer {
    ($ty:ty) => {
        impl $crate::core::connectable::Connectable for $ty {
            fn role(&self) -> $crate::core::connectable::ConnectRole {
                $crate::core::connectable::ConnectRole::Consumer
            }

            fn name(&self) -> &str {
                <Self as $crate::core::Consumer>::name(self)
            }

            fn as_consumer(&self) -> Option<&dyn $crate::core::Consumer> {
                Some(self)
            }

            fn as_consumer_mut(&mut self) -> Option<&mut dyn $crate::core::Consumer> {
                Some(self)
            }
        }
    };
}
