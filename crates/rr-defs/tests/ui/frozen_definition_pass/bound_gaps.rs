// US-RR-34-5: what the compile-time check in `rr_defs::resolve` does NOT
// rule out. std declares `Mutex`, `RwLock`, `OnceLock`, `LazyLock` and the
// atomics `Send + Sync + UnwindSafe + RefUnwindSafe`, so a field of one
// passes the bound. `cargo xtask check-globals` keeps them out instead: it
// rejects any of these named in the resolve module or in a module whose
// types a resolved definition embeds (xtask/src/globals.rs STATELESS_PATHS;
// xtask/tests/globals.rs plants each one). If std ever tightens one of
// these, this case fails and the docs of the check should say so.
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize};
use std::sync::{LazyLock, Mutex, OnceLock, RwLock};

const fn frozen<T: Send + Sync + UnwindSafe + RefUnwindSafe>() {}

#[allow(dead_code)]
struct Planted {
    cache: Mutex<Vec<u8>>,
    memo: RwLock<u8>,
    built: OnceLock<u8>,
    lazy: LazyLock<u8>,
    hits: AtomicUsize,
}

fn main() {
    frozen::<Mutex<u32>>();
    frozen::<RwLock<u32>>();
    frozen::<OnceLock<u32>>();
    frozen::<LazyLock<u32>>();
    frozen::<AtomicBool>();
    frozen::<AtomicU32>();
    frozen::<Planted>();
}
