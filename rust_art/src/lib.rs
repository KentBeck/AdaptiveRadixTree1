//! Adaptive Radix Tree (ART) — an ordered key-value map.
//!
//! Uses tagged pointers to distinguish leaf vs inner node types, and adaptive
//! node sizes (4, 16, 48, 256) for memory efficiency. Path compression
//! collapses single-child chains into node prefixes.
//!
//! Keys are byte slices (`&[u8]`), values are generic `V`.

mod inner;
mod iter;
mod map;
mod prefix;
mod raw;

pub use crate::iter::{Iter, RangeIter};
pub use crate::map::ARTMap;

#[cfg(test)]
mod tests;
