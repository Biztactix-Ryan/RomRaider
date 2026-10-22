// US-RR-34-5: what the compile-time check in `rr_defs::resolve` rules out.
// `frozen` is the bound every resolved type is asserted to meet there
// (keep the two in step). A cell is not `Sync`, and `UnsafeCell`, under
// every cell, is not `RefUnwindSafe`; `Rc` is neither `Send` nor `Sync`.
// So a field of any of these in a resolved type fails the build. (What the
// bound does not rule out is pinned in
// `frozen_definition_pass/bound_gaps.rs`.)
use std::cell::{Cell, OnceCell, RefCell, UnsafeCell};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::rc::Rc;

const fn frozen<T: Send + Sync + UnwindSafe + RefUnwindSafe>() {}

fn main() {
    frozen::<Cell<u32>>();
    frozen::<RefCell<u32>>();
    frozen::<OnceCell<u32>>();
    frozen::<UnsafeCell<u32>>();
    frozen::<Rc<u32>>();
}
