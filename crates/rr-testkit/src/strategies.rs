//! `proptest` strategies (PROJECT.md "Testing Strategy", tier T1).
//!
//! One submodule per concept, so every crate's property tests generate the
//! same shapes of input. Each submodule's docs name the story that fills it;
//! empty submodules are placeholders that fix the layout in advance.
//!
//! Conventions for strategies added here:
//!
//! * Expose functions returning `impl Strategy<Value = T>` (not `Arbitrary`
//!   impls), so a crate can pick a bounded or edge-biased variant by name.
//! * Only generate values the type's constructor accepts; a strategy for
//!   *invalid* input says so in its name (e.g. `empty_address_pair`).
//! * Keep strategies pure: no I/O, no real time, no global state.

pub mod capability;
pub mod clock;
pub mod encoding;
pub mod expr;
pub mod layered;
