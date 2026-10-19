//! Capability tokens that gate every write (SECURITY.md "Authorization",
//! decision D-013).
//!
//! The software is read-only by default. Anything that can change the user's
//! files or car needs a *capability token* — a value whose existence proves
//! that the user explicitly confirmed this exact action:
//!
//! | Token              | Permits                                        | Scope / lifetime                                   |
//! |--------------------|------------------------------------------------|----------------------------------------------------|
//! | [`RomWrite`]       | Overwriting a ROM file on disk                 | One save; taken **by value** and consumed          |
//! | [`EcuMemoryWrite`] | SSM/KWP memory writes (global adjust, RAM tune)| One session, one [`EcuId`], one [`AddressRange`]; taken **by reference** |
//! | [`EcuReset`]       | ECU reset, learning/adaptation reset           | One action; taken **by value** and consumed        |
//!
//! There is deliberately **no `EcuFlash` token**: reflashing is out of scope
//! until there is a HIL bench (VISION.md), so no API can be written that
//! accepts one.
//!
//! Every write API in `rr-rom` and `rr-comms` takes one of these tokens as a
//! parameter. The tokens have private fields and no public constructor, and
//! they implement none of `Default`, `Clone`, `Copy`, `Serialize` or
//! `Deserialize`, so the only way to obtain one is to *mint* it through a
//! [`Confirmation`] — the user-facing confirmation path.
//!
//! # How minting is sealed
//!
//! SECURITY.md ("Enforcement") requires that tokens "can only be built in
//! `rr-app`/`rr-cli` confirmation paths". `rr-core` sits at the bottom of the
//! dependency graph and cannot name those crates, so the seal is built from
//! two pieces:
//!
//! 1. **A sealed trait.** [`Confirmation`] has a supertrait `Sealed` that lives
//!    in a private module. Without help, no crate outside `rr-core` can
//!    implement [`Confirmation`], and therefore nobody can mint a token.
//! 2. **A cargo feature that opens the seal.** With the
//!    `confirmation-authority` feature enabled, the `authority` module appears.
//!    It re-exports `Sealed` (so the crate can implement [`Confirmation`]) and
//!    holds the only minting functions (`mint_rom_write`,
//!    `mint_ecu_memory_write`, `mint_ecu_reset`). Without the feature `rr-core`
//!    exposes *no* API that produces a token at all. Only `rr-app` and `rr-cli`
//!    may enable the feature, from their confirmation modules.
//!
//! ## Trade-off
//!
//! Cargo features are additive and unify across a build, so during a full
//! `cargo build --workspace` (once `rr-app`/`rr-cli` enable the feature) the
//! `authority` module is compiled for every crate, and a library crate
//! *could* name it. Rust alone cannot express "only crate X may implement
//! this trait" for a crate `rr-core` does not know about; the alternatives
//! are worse (`rr-core` depending on the binaries is a cycle; a
//! "proof value" parameter is itself forgeable by anyone who can construct
//! it; `unsafe` markers are a convention, not a check). The feature is
//! therefore the single, greppable, mechanically checkable gate, and the T6
//! safety-invariant suite closes the unification hole:
//!
//! * **Compile-fail tests** (trybuild, US-RR-7-1) build against `rr-core`
//!   *without* the feature and prove a token cannot be constructed, cloned,
//!   defaulted or deserialized, and that `Sealed`/the minting functions are
//!   unnameable.
//! * **Manifest check**: `cargo metadata` must show that only `rr-app` and
//!   `rr-cli` enable `rr-core/confirmation-authority`, and that no crate
//!   depends on `rr-app` or `rr-cli` as a library.
//! * **Isolated builds**: every other crate is checked on its own
//!   (`cargo check -p rr-rom`, `-p rr-comms`, ...). Feature resolution then
//!   leaves the feature off, so any use of `capability::authority` outside
//!   the two binaries fails to compile even if a workspace build would hide
//!   it.
//!
//! # Scoping
//!
//! Holding a token is necessary but not sufficient: every write path must
//! also ask the token whether *this particular* write is inside the scope the
//! user confirmed, immediately before touching the file or the ECU, and must
//! abort on a [`ScopeError`]. The checks are pure and cheap:
//!
//! | Token              | Check                          | Rejects                                                         |
//! |--------------------|--------------------------------|-----------------------------------------------------------------|
//! | [`EcuMemoryWrite`] | [`EcuMemoryWrite::check_write`]| another ECU; any byte outside the granted range; zero-length writes; `start + len` overflowing `u32` (never wrapped) |
//! | [`EcuReset`]       | [`EcuReset::check_reset`]      | another ECU; a different [`ResetKind`]                          |
//! | [`RomWrite`]       | [`RomWrite::check_save`]       | failing checksums unless the user confirmed the "save with failing checksum" override |
//!
//! A [`RomWrite`] also records whether the user overrode failing checksums
//! ([`RomWrite::checksum_override`]) so the save path can journal it.

use std::error::Error;
use std::fmt;

// ---------------------------------------------------------------------------
// Scope value types (freely constructible; they grant nothing on their own)
// ---------------------------------------------------------------------------

/// Identifier an ECU reports about itself (e.g. the 5-byte SSM ECU ID).
///
/// Stored as raw bytes and displayed as upper-case hex, which is how the
/// confirmation prompt names the ECU to the user.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EcuId(Box<[u8]>);

impl EcuId {
    /// Longest ECU ID accepted, in bytes.
    pub const MAX_LEN: usize = 16;

    /// Builds an ECU ID from the raw bytes the ECU reported.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeError::EmptyEcuId`] for no bytes and
    /// [`ScopeError::EcuIdTooLong`] for more than [`EcuId::MAX_LEN`] bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ScopeError> {
        if bytes.is_empty() {
            return Err(ScopeError::EmptyEcuId);
        }
        if bytes.len() > Self::MAX_LEN {
            return Err(ScopeError::EcuIdTooLong { len: bytes.len() });
        }
        Ok(Self(bytes.into()))
    }

    /// The raw ID bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for EcuId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter() {
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

/// A non-empty, half-open range of ECU addresses `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AddressRange {
    start: u32,
    end: u32,
}

impl AddressRange {
    /// Builds the range `start..end`.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeError::EmptyRange`] unless `start < end`.
    pub fn new(start: u32, end: u32) -> Result<Self, ScopeError> {
        if start >= end {
            return Err(ScopeError::EmptyRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// First address in the range.
    #[must_use]
    pub fn start(&self) -> u32 {
        self.start
    }

    /// One past the last address in the range.
    #[must_use]
    pub fn end(&self) -> u32 {
        self.end
    }

    /// Number of addresses in the range (always at least 1).
    #[must_use]
    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    /// Always `false`: an `AddressRange` cannot be empty. Provided for API
    /// symmetry with [`AddressRange::len`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether `address` lies inside the range.
    #[must_use]
    pub fn contains(&self, address: u32) -> bool {
        (self.start..self.end).contains(&address)
    }
}

impl fmt::Display for AddressRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:06X}..0x{:06X}", self.start, self.end)
    }
}

/// Which kind of reset an [`EcuReset`] permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResetKind {
    /// Reset the ECU itself.
    Ecu,
    /// Clear learned values / adaptations.
    Adaptation,
}

impl fmt::Display for ResetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ecu => "ECU reset",
            Self::Adaptation => "adaptation reset",
        })
    }
}

/// Invalid scope values, or a write that falls outside the scope a capability
/// token was granted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeError {
    /// The ECU ID had no bytes.
    EmptyEcuId,
    /// The ECU ID was longer than [`EcuId::MAX_LEN`].
    EcuIdTooLong {
        /// Length that was supplied.
        len: usize,
    },
    /// An address range with `start >= end`.
    EmptyRange {
        /// Requested start.
        start: u32,
        /// Requested end.
        end: u32,
    },
    /// The write or reset targets a different ECU than the token was granted
    /// for.
    WrongEcu {
        /// ECU named in the token.
        granted: EcuId,
        /// ECU the caller tried to act on.
        requested: EcuId,
    },
    /// A memory write of zero bytes. Rejected rather than treated as a
    /// no-op: a write path asking to write nothing is a caller bug, and
    /// accepting it would make `start == range.end()` look authorised.
    EmptyWrite {
        /// Requested start address.
        start: u32,
    },
    /// `start + len` does not fit in a `u32`. The address is rejected, never
    /// wrapped around to the bottom of the address space.
    AddressOverflow {
        /// Requested start address.
        start: u32,
        /// Requested length in bytes.
        len: u32,
    },
    /// At least one byte of the write lies outside the granted range.
    OutOfRange {
        /// Range named in the token.
        granted: AddressRange,
        /// Bytes the caller tried to write.
        requested: AddressRange,
    },
    /// The reset is of a different kind than the token was granted for.
    WrongResetKind {
        /// Kind named in the token.
        granted: ResetKind,
        /// Kind the caller tried to perform.
        requested: ResetKind,
    },
    /// The ROM's checksums fail and the user did not confirm saving with
    /// failing checksums.
    ChecksumOverrideMissing,
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEcuId => f.write_str("ECU ID is empty"),
            Self::EcuIdTooLong { len } => write!(
                f,
                "ECU ID is {len} bytes long; at most {} are allowed",
                EcuId::MAX_LEN
            ),
            Self::EmptyRange { start, end } => {
                write!(f, "address range 0x{start:X}..0x{end:X} is empty")
            }
            Self::WrongEcu { granted, requested } => write!(
                f,
                "ECU {requested} is not the ECU {granted} the user authorised"
            ),
            Self::EmptyWrite { start } => {
                write!(f, "zero-length write at 0x{start:X} is not allowed")
            }
            Self::AddressOverflow { start, len } => write!(
                f,
                "write of {len} bytes at 0x{start:X} overflows the 32-bit address space"
            ),
            Self::OutOfRange { granted, requested } => write!(
                f,
                "write to {requested} is outside the authorised range {granted}"
            ),
            Self::WrongResetKind { granted, requested } => write!(
                f,
                "{requested} was requested but the user authorised an {granted}"
            ),
            Self::ChecksumOverrideMissing => f.write_str(
                "ROM checksums fail and the user did not confirm saving with failing checksums",
            ),
        }
    }
}

impl Error for ScopeError {}

// ---------------------------------------------------------------------------
// Requests (what the confirmation prompt shows the user)
// ---------------------------------------------------------------------------

/// Whether the ROM about to be saved passes its checksums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChecksumStatus {
    /// All checksums verify.
    Verified,
    /// At least one checksum fails; saving needs an explicit override that
    /// the save path records in the sidecar journal.
    Failing,
}

/// A request to overwrite a ROM file, shown to the user for confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomWriteRequest {
    checksums: ChecksumStatus,
}

impl RomWriteRequest {
    /// Request to save a ROM whose checksums are in the given state.
    #[must_use]
    pub fn new(checksums: ChecksumStatus) -> Self {
        Self { checksums }
    }

    /// Checksum state the user is being asked about.
    #[must_use]
    pub fn checksums(&self) -> ChecksumStatus {
        self.checksums
    }
}

/// A request for a session of ECU memory writes, naming the ECU and the
/// address range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcuMemoryWriteRequest {
    ecu_id: EcuId,
    range: AddressRange,
}

impl EcuMemoryWriteRequest {
    /// Request write access to `range` on the ECU identified by `ecu_id`.
    #[must_use]
    pub fn new(ecu_id: EcuId, range: AddressRange) -> Self {
        Self { ecu_id, range }
    }

    /// ECU the writes would go to.
    #[must_use]
    pub fn ecu_id(&self) -> &EcuId {
        &self.ecu_id
    }

    /// Addresses the writes would touch.
    #[must_use]
    pub fn range(&self) -> AddressRange {
        self.range
    }
}

/// A request to perform one reset on one ECU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcuResetRequest {
    ecu_id: EcuId,
    kind: ResetKind,
}

impl EcuResetRequest {
    /// Request a reset of `kind` on the ECU identified by `ecu_id`.
    #[must_use]
    pub fn new(ecu_id: EcuId, kind: ResetKind) -> Self {
        Self { ecu_id, kind }
    }

    /// ECU to reset.
    #[must_use]
    pub fn ecu_id(&self) -> &EcuId {
        &self.ecu_id
    }

    /// What kind of reset.
    #[must_use]
    pub fn kind(&self) -> ResetKind {
        self.kind
    }
}

// ---------------------------------------------------------------------------
// Confirmation (sealed)
// ---------------------------------------------------------------------------

/// The user's answer to a confirmation prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decision {
    /// The user explicitly confirmed the action.
    Confirmed,
    /// The user declined, cancelled or did not answer.
    Declined,
}

mod seal {
    /// Seal for [`super::Confirmation`]. Nameable outside `rr-core` only via
    /// the `confirmation-authority` feature.
    pub trait Sealed {}
}

/// A user-facing confirmation path: asks the user to approve one write and
/// reports their [`Decision`].
///
/// This trait is **sealed**. Only crates that enable `rr-core`'s
/// `confirmation-authority` feature — `rr-app` and `rr-cli`, and nobody else
/// (enforced by the T6 suite) — can implement it, and only an implementation
/// can be passed to the minting functions in the `authority` module. See the
/// [module docs](self) for the mechanism and its trade-off.
///
/// Implementations must ask the user every time a method is called and must
/// return [`Decision::Declined`] for anything other than an explicit "yes".
pub trait Confirmation: seal::Sealed {
    /// Ask whether to overwrite a ROM file. When the request reports
    /// [`ChecksumStatus::Failing`] the prompt must say so and ask for the
    /// explicit "save with failing checksum" override.
    fn confirm_rom_write(&mut self, request: &RomWriteRequest) -> Decision;

    /// Ask whether to allow memory writes to the named ECU and address range
    /// for this session.
    fn confirm_ecu_memory_write(&mut self, request: &EcuMemoryWriteRequest) -> Decision;

    /// Ask whether to perform this one reset.
    fn confirm_ecu_reset(&mut self, request: &EcuResetRequest) -> Decision;
}

/// Which capability a confirmation was declined for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityKind {
    /// [`RomWrite`].
    RomWrite,
    /// [`EcuMemoryWrite`].
    EcuMemoryWrite,
    /// [`EcuReset`].
    EcuReset,
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RomWrite => "ROM write",
            Self::EcuMemoryWrite => "ECU memory write",
            Self::EcuReset => "ECU reset",
        })
    }
}

/// The user declined to grant a capability; no token was minted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declined {
    capability: CapabilityKind,
}

impl Declined {
    /// The capability that was refused.
    #[must_use]
    pub fn capability(&self) -> CapabilityKind {
        self.capability
    }
}

impl fmt::Display for Declined {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} was not confirmed by the user", self.capability)
    }
}

impl Error for Declined {}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// Permission to overwrite a ROM file once.
///
/// Save APIs take it **by value**, so one confirmation authorises one save.
/// No public constructor; no `Clone`/`Copy`/`Default`/serde.
#[derive(Debug)]
#[must_use = "a capability token authorises a write; dropping it discards the user's confirmation"]
pub struct RomWrite {
    checksum_override: bool,
}

impl RomWrite {
    /// `true` when the user explicitly chose to save despite failing
    /// checksums. The save path must record this in the sidecar journal.
    #[must_use]
    pub fn checksum_override(&self) -> bool {
        self.checksum_override
    }

    /// Checks that a ROM whose checksums are currently in state `checksums`
    /// may be saved with this token. Save paths call this immediately before
    /// writing the file, with the checksum state of the bytes about to be
    /// written.
    ///
    /// Verified checksums are always accepted. Failing checksums are accepted
    /// only when the user confirmed the "save with failing checksum" override
    /// (the token was minted from a [`ChecksumStatus::Failing`] request) — so
    /// a ROM that started failing after a "verified" confirmation is rejected.
    ///
    /// # Errors
    ///
    /// [`ScopeError::ChecksumOverrideMissing`] for failing checksums without
    /// the override.
    pub fn check_save(&self, checksums: ChecksumStatus) -> Result<(), ScopeError> {
        match checksums {
            ChecksumStatus::Verified => Ok(()),
            ChecksumStatus::Failing if self.checksum_override => Ok(()),
            ChecksumStatus::Failing => Err(ScopeError::ChecksumOverrideMissing),
        }
    }
}

/// Permission to write ECU memory, for one session, on one ECU, within one
/// address range.
///
/// Write APIs take it **by reference** and must reject any target outside
/// [`EcuMemoryWrite::ecu_id`] / [`EcuMemoryWrite::range`]. No public
/// constructor; no `Clone`/`Copy`/`Default`/serde.
#[derive(Debug)]
#[must_use = "a capability token authorises a write; dropping it discards the user's confirmation"]
pub struct EcuMemoryWrite {
    ecu_id: EcuId,
    range: AddressRange,
}

impl EcuMemoryWrite {
    /// The ECU the user granted write access to.
    #[must_use]
    pub fn ecu_id(&self) -> &EcuId {
        &self.ecu_id
    }

    /// The address range the user granted write access to.
    #[must_use]
    pub fn range(&self) -> AddressRange {
        self.range
    }

    /// Checks that writing `len` bytes at `start` on `ecu` is inside the
    /// scope the user confirmed. Write paths call this before every write,
    /// immediately before touching the ECU, and abort on error.
    ///
    /// The write covers the half-open span `start..start + len`; every byte
    /// of it must lie in [`EcuMemoryWrite::range`] (also half-open), so a
    /// write may end exactly at `range().end()` but not one byte past it.
    ///
    /// On success returns that span as an [`AddressRange`].
    ///
    /// # Errors
    ///
    /// * [`ScopeError::WrongEcu`] if `ecu` is not the granted ECU;
    /// * [`ScopeError::EmptyWrite`] if `len == 0` (zero-length writes are
    ///   rejected, not treated as no-ops);
    /// * [`ScopeError::AddressOverflow`] if `start + len` exceeds `u32` — the
    ///   address is rejected, never wrapped;
    /// * [`ScopeError::OutOfRange`] if any byte falls outside the granted
    ///   range.
    pub fn check_write(
        &self,
        ecu: &EcuId,
        start: u32,
        len: u32,
    ) -> Result<AddressRange, ScopeError> {
        if *ecu != self.ecu_id {
            return Err(ScopeError::WrongEcu {
                granted: self.ecu_id.clone(),
                requested: ecu.clone(),
            });
        }
        if len == 0 {
            return Err(ScopeError::EmptyWrite { start });
        }
        let Some(end) = start.checked_add(len) else {
            return Err(ScopeError::AddressOverflow { start, len });
        };
        // `len > 0` and no overflow, so `start < end`: the span is non-empty.
        let requested = AddressRange { start, end };
        if start < self.range.start || end > self.range.end {
            return Err(ScopeError::OutOfRange {
                granted: self.range,
                requested,
            });
        }
        Ok(requested)
    }
}

/// Permission to perform one reset on one ECU.
///
/// Reset APIs take it **by value**, so one confirmation authorises one
/// reset. No public constructor; no `Clone`/`Copy`/`Default`/serde.
#[derive(Debug)]
#[must_use = "a capability token authorises a reset; dropping it discards the user's confirmation"]
pub struct EcuReset {
    ecu_id: EcuId,
    kind: ResetKind,
}

impl EcuReset {
    /// The ECU the user agreed to reset.
    #[must_use]
    pub fn ecu_id(&self) -> &EcuId {
        &self.ecu_id
    }

    /// The kind of reset the user agreed to.
    #[must_use]
    pub fn kind(&self) -> ResetKind {
        self.kind
    }

    /// Checks that performing a reset of `kind` on `ecu` is exactly what the
    /// user confirmed. Reset paths call this immediately before sending the
    /// reset, then consume the token.
    ///
    /// # Errors
    ///
    /// [`ScopeError::WrongEcu`] if `ecu` is not the granted ECU;
    /// [`ScopeError::WrongResetKind`] if `kind` differs from the granted kind.
    pub fn check_reset(&self, ecu: &EcuId, kind: ResetKind) -> Result<(), ScopeError> {
        if *ecu != self.ecu_id {
            return Err(ScopeError::WrongEcu {
                granted: self.ecu_id.clone(),
                requested: ecu.clone(),
            });
        }
        if kind != self.kind {
            return Err(ScopeError::WrongResetKind {
                granted: self.kind,
                requested: kind,
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Minting
// ---------------------------------------------------------------------------

/// The only code that constructs tokens. Compiled only for this crate's own
/// tests and when the `authority` module is enabled.
#[cfg(any(test, feature = "confirmation-authority"))]
mod mint {
    use super::{
        CapabilityKind, ChecksumStatus, Confirmation, Decision, Declined, EcuMemoryWrite,
        EcuMemoryWriteRequest, EcuReset, EcuResetRequest, RomWrite, RomWriteRequest,
    };

    pub(super) fn rom_write<C: Confirmation + ?Sized>(
        confirmation: &mut C,
        request: &RomWriteRequest,
    ) -> Result<RomWrite, Declined> {
        match confirmation.confirm_rom_write(request) {
            Decision::Confirmed => Ok(RomWrite {
                checksum_override: request.checksums() == ChecksumStatus::Failing,
            }),
            Decision::Declined => Err(Declined {
                capability: CapabilityKind::RomWrite,
            }),
        }
    }

    pub(super) fn ecu_memory_write<C: Confirmation + ?Sized>(
        confirmation: &mut C,
        request: &EcuMemoryWriteRequest,
    ) -> Result<EcuMemoryWrite, Declined> {
        match confirmation.confirm_ecu_memory_write(request) {
            Decision::Confirmed => Ok(EcuMemoryWrite {
                ecu_id: request.ecu_id().clone(),
                range: request.range(),
            }),
            Decision::Declined => Err(Declined {
                capability: CapabilityKind::EcuMemoryWrite,
            }),
        }
    }

    pub(super) fn ecu_reset<C: Confirmation + ?Sized>(
        confirmation: &mut C,
        request: &EcuResetRequest,
    ) -> Result<EcuReset, Declined> {
        match confirmation.confirm_ecu_reset(request) {
            Decision::Confirmed => Ok(EcuReset {
                ecu_id: request.ecu_id().clone(),
                kind: request.kind(),
            }),
            Decision::Declined => Err(Declined {
                capability: CapabilityKind::EcuReset,
            }),
        }
    }
}

/// Minting authority — present only with the `confirmation-authority`
/// feature, which only `rr-app` and `rr-cli` may enable.
///
/// A confirmation module implements
/// [`Sealed`](crate::capability::authority::Sealed) and [`Confirmation`] for
/// its prompt type, then calls the `mint_*` functions; each one asks the user
/// via the [`Confirmation`] and returns a token only on
/// [`Decision::Confirmed`].
#[cfg(feature = "confirmation-authority")]
pub mod authority {
    pub use super::seal::Sealed;
    use super::{
        Confirmation, Declined, EcuMemoryWrite, EcuMemoryWriteRequest, EcuReset, EcuResetRequest,
        RomWrite, RomWriteRequest, mint,
    };

    /// Ask the user to confirm a ROM save; mint a [`RomWrite`] if they do.
    ///
    /// # Errors
    ///
    /// [`Declined`] if the user did not confirm.
    pub fn mint_rom_write<C: Confirmation + ?Sized>(
        confirmation: &mut C,
        request: &RomWriteRequest,
    ) -> Result<RomWrite, Declined> {
        mint::rom_write(confirmation, request)
    }

    /// Ask the user to confirm a session of ECU memory writes; mint an
    /// [`EcuMemoryWrite`] scoped to the request's ECU and range if they do.
    ///
    /// # Errors
    ///
    /// [`Declined`] if the user did not confirm.
    pub fn mint_ecu_memory_write<C: Confirmation + ?Sized>(
        confirmation: &mut C,
        request: &EcuMemoryWriteRequest,
    ) -> Result<EcuMemoryWrite, Declined> {
        mint::ecu_memory_write(confirmation, request)
    }

    /// Ask the user to confirm one reset; mint an [`EcuReset`] if they do.
    ///
    /// # Errors
    ///
    /// [`Declined`] if the user did not confirm.
    pub fn mint_ecu_reset<C: Confirmation + ?Sized>(
        confirmation: &mut C,
        request: &EcuResetRequest,
    ) -> Result<EcuReset, Declined> {
        mint::ecu_reset(confirmation, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted confirmation: answers with `answer` and records every prompt.
    struct Scripted {
        answer: Decision,
        prompts: Vec<CapabilityKind>,
    }

    impl Scripted {
        fn new(answer: Decision) -> Self {
            Self {
                answer,
                prompts: Vec::new(),
            }
        }
    }

    impl seal::Sealed for Scripted {}

    impl Confirmation for Scripted {
        fn confirm_rom_write(&mut self, _: &RomWriteRequest) -> Decision {
            self.prompts.push(CapabilityKind::RomWrite);
            self.answer
        }
        fn confirm_ecu_memory_write(&mut self, _: &EcuMemoryWriteRequest) -> Decision {
            self.prompts.push(CapabilityKind::EcuMemoryWrite);
            self.answer
        }
        fn confirm_ecu_reset(&mut self, _: &EcuResetRequest) -> Decision {
            self.prompts.push(CapabilityKind::EcuReset);
            self.answer
        }
    }

    fn ecu() -> EcuId {
        EcuId::from_bytes(&[0x2F, 0x12, 0x78, 0x52, 0x06]).unwrap()
    }

    #[test]
    fn ecu_id_validates_and_displays_hex() {
        assert_eq!(ecu().to_string(), "2F12785206");
        assert_eq!(ecu().as_bytes(), &[0x2F, 0x12, 0x78, 0x52, 0x06]);
        assert_eq!(EcuId::from_bytes(&[]), Err(ScopeError::EmptyEcuId));
        assert_eq!(
            EcuId::from_bytes(&[0; 17]),
            Err(ScopeError::EcuIdTooLong { len: 17 })
        );
        assert!(EcuId::from_bytes(&[0; EcuId::MAX_LEN]).is_ok());
    }

    #[test]
    fn address_range_is_half_open_and_non_empty() {
        let r = AddressRange::new(0xFF_8000, 0xFF_8010).unwrap();
        assert_eq!((r.start(), r.end(), r.len()), (0xFF_8000, 0xFF_8010, 16));
        assert!(!r.is_empty());
        assert!(r.contains(0xFF_8000));
        assert!(r.contains(0xFF_800F));
        assert!(!r.contains(0xFF_8010));
        assert!(!r.contains(0xFF_7FFF));
        assert_eq!(r.to_string(), "0xFF8000..0xFF8010");
        assert_eq!(
            AddressRange::new(5, 5),
            Err(ScopeError::EmptyRange { start: 5, end: 5 })
        );
        assert!(AddressRange::new(6, 5).is_err());
        assert!(AddressRange::new(u32::MAX - 1, u32::MAX).is_ok());
    }

    #[test]
    fn confirmed_rom_write_records_checksum_override() {
        let mut c = Scripted::new(Decision::Confirmed);
        let ok = mint::rom_write(&mut c, &RomWriteRequest::new(ChecksumStatus::Verified)).unwrap();
        assert!(!ok.checksum_override());
        let forced =
            mint::rom_write(&mut c, &RomWriteRequest::new(ChecksumStatus::Failing)).unwrap();
        assert!(forced.checksum_override());
        assert_eq!(c.prompts, [CapabilityKind::RomWrite; 2]);
    }

    /// US-RR-7-3: a minted `EcuMemoryWrite` carries the ECU ID and address
    /// range the user confirmed.
    #[test]
    fn ecu_memory_write_token_carries_granted_scope() {
        let mut c = Scripted::new(Decision::Confirmed);
        let range = AddressRange::new(0x20_0000, 0x20_0100).unwrap();
        let token =
            mint::ecu_memory_write(&mut c, &EcuMemoryWriteRequest::new(ecu(), range)).unwrap();
        assert_eq!(token.ecu_id(), &ecu());
        assert_eq!(token.range(), range);
        assert_eq!(c.prompts, [CapabilityKind::EcuMemoryWrite]);
    }

    #[test]
    fn ecu_reset_token_carries_ecu_and_kind() {
        let mut c = Scripted::new(Decision::Confirmed);
        let token =
            mint::ecu_reset(&mut c, &EcuResetRequest::new(ecu(), ResetKind::Adaptation)).unwrap();
        assert_eq!(token.ecu_id(), &ecu());
        assert_eq!(token.kind(), ResetKind::Adaptation);
        assert_eq!(c.prompts, [CapabilityKind::EcuReset]);
    }

    #[test]
    fn declined_confirmation_mints_nothing() {
        let mut c = Scripted::new(Decision::Declined);
        let range = AddressRange::new(0, 1).unwrap();
        let rom = mint::rom_write(&mut c, &RomWriteRequest::new(ChecksumStatus::Verified));
        let mem = mint::ecu_memory_write(&mut c, &EcuMemoryWriteRequest::new(ecu(), range));
        let reset = mint::ecu_reset(&mut c, &EcuResetRequest::new(ecu(), ResetKind::Ecu));
        assert_eq!(rom.unwrap_err().capability(), CapabilityKind::RomWrite);
        assert_eq!(
            mem.unwrap_err().capability(),
            CapabilityKind::EcuMemoryWrite
        );
        let err = reset.unwrap_err();
        assert_eq!(err.capability(), CapabilityKind::EcuReset);
        assert_eq!(err.to_string(), "ECU reset was not confirmed by the user");
        // Every attempt asked the user exactly once.
        assert_eq!(c.prompts.len(), 3);
    }

    #[test]
    fn confirmation_is_object_safe() {
        let mut c = Scripted::new(Decision::Confirmed);
        let dyn_c: &mut dyn Confirmation = &mut c;
        assert!(mint::ecu_reset(dyn_c, &EcuResetRequest::new(ecu(), ResetKind::Ecu)).is_ok());
    }

    #[test]
    fn scope_errors_display() {
        assert_eq!(ScopeError::EmptyEcuId.to_string(), "ECU ID is empty");
        assert_eq!(
            ScopeError::EmptyRange { start: 16, end: 16 }.to_string(),
            "address range 0x10..0x10 is empty"
        );
    }

    // -- Scope enforcement (US-RR-7-7). Tokens are built directly here (private
    // access inside the crate) so these run in the default, feature-less build.

    fn other_ecu() -> EcuId {
        EcuId::from_bytes(&[0x2F, 0x12, 0x78, 0x52, 0x07]).unwrap()
    }

    /// Token for `0x1000..0x1100` on [`ecu`].
    fn mem_token() -> EcuMemoryWrite {
        EcuMemoryWrite {
            ecu_id: ecu(),
            range: AddressRange::new(0x1000, 0x1100).unwrap(),
        }
    }

    /// US-RR-7-3: a write inside the granted range on the granted ECU passes.
    #[test]
    fn t6_memory_write_inside_range_is_authorised() {
        let t = mem_token();
        assert_eq!(
            t.check_write(&ecu(), 0x1010, 4),
            Ok(AddressRange::new(0x1010, 0x1014).unwrap())
        );
    }

    /// US-RR-7-3: the first byte, last byte and whole granted range pass.
    #[test]
    fn t6_memory_write_exact_boundaries_are_authorised() {
        let t = mem_token();
        // First byte of the range.
        assert!(t.check_write(&ecu(), 0x1000, 1).is_ok());
        // Last byte of the range (end is exclusive).
        assert!(t.check_write(&ecu(), 0x10FF, 1).is_ok());
        // The whole range in one write.
        assert_eq!(t.check_write(&ecu(), 0x1000, 0x100), Ok(t.range()));
    }

    /// US-RR-7-3: use outside the granted range is rejected at runtime, down
    /// to a single byte past either end.
    #[test]
    fn t6_memory_write_one_byte_outside_range_is_rejected() {
        let t = mem_token();
        let granted = t.range();
        // Starts at the exclusive end.
        assert_eq!(
            t.check_write(&ecu(), 0x1100, 1),
            Err(ScopeError::OutOfRange {
                granted,
                requested: AddressRange::new(0x1100, 0x1101).unwrap(),
            })
        );
        // Starts inside but runs one byte past the end.
        assert_eq!(
            t.check_write(&ecu(), 0x10FE, 3),
            Err(ScopeError::OutOfRange {
                granted,
                requested: AddressRange::new(0x10FE, 0x1101).unwrap(),
            })
        );
        // Whole range plus one.
        assert!(t.check_write(&ecu(), 0x1000, 0x101).is_err());
        // One byte before the start, alone and straddling.
        assert!(t.check_write(&ecu(), 0x0FFF, 1).is_err());
        assert!(t.check_write(&ecu(), 0x0FFF, 2).is_err());
        // Far away.
        assert!(t.check_write(&ecu(), 0, 1).is_err());
    }

    /// US-RR-7-3: use on any ECU other than the granted one is rejected.
    #[test]
    fn t6_memory_write_to_other_ecu_is_rejected() {
        let t = mem_token();
        let err = t.check_write(&other_ecu(), 0x1000, 1).unwrap_err();
        assert_eq!(
            err,
            ScopeError::WrongEcu {
                granted: ecu(),
                requested: other_ecu(),
            }
        );
        assert_eq!(
            err.to_string(),
            "ECU 2F12785207 is not the ECU 2F12785206 the user authorised"
        );
    }

    /// US-RR-7-3: an end address that overflows `u32` is rejected, never
    /// wrapped back into the granted range.
    #[test]
    fn t6_memory_write_address_overflow_is_rejected_not_wrapped() {
        // A token whose range reaches the top of the address space, so a
        // wrapped end could otherwise look in range.
        let t = EcuMemoryWrite {
            ecu_id: ecu(),
            range: AddressRange::new(0, u32::MAX).unwrap(),
        };
        assert_eq!(
            t.check_write(&ecu(), u32::MAX - 1, 4),
            Err(ScopeError::AddressOverflow {
                start: u32::MAX - 1,
                len: 4,
            })
        );
        assert_eq!(
            t.check_write(&ecu(), u32::MAX, 1),
            Err(ScopeError::AddressOverflow {
                start: u32::MAX,
                len: 1,
            })
        );
        assert!(mem_token().check_write(&ecu(), 0x1000, u32::MAX).is_err());
        // Ending exactly at u32::MAX (exclusive) does not overflow.
        assert!(t.check_write(&ecu(), u32::MAX - 1, 1).is_ok());
    }

    /// US-RR-7-3: a zero-length write (even at the exclusive end) is rejected.
    #[test]
    fn t6_zero_length_memory_write_is_rejected() {
        let t = mem_token();
        assert_eq!(
            t.check_write(&ecu(), 0x1000, 0),
            Err(ScopeError::EmptyWrite { start: 0x1000 })
        );
        // Including at the exclusive end, which must not look authorised.
        assert_eq!(
            t.check_write(&ecu(), 0x1100, 0),
            Err(ScopeError::EmptyWrite { start: 0x1100 })
        );
    }

    #[test]
    fn t6_reset_checks_ecu_and_kind() {
        let t = EcuReset {
            ecu_id: ecu(),
            kind: ResetKind::Adaptation,
        };
        assert_eq!(t.check_reset(&ecu(), ResetKind::Adaptation), Ok(()));
        let err = t.check_reset(&ecu(), ResetKind::Ecu).unwrap_err();
        assert_eq!(
            err,
            ScopeError::WrongResetKind {
                granted: ResetKind::Adaptation,
                requested: ResetKind::Ecu,
            }
        );
        assert_eq!(
            err.to_string(),
            "ECU reset was requested but the user authorised an adaptation reset"
        );
        assert_eq!(
            t.check_reset(&other_ecu(), ResetKind::Adaptation),
            Err(ScopeError::WrongEcu {
                granted: ecu(),
                requested: other_ecu(),
            })
        );
    }

    #[test]
    fn t6_rom_save_requires_override_for_failing_checksums() {
        let plain = RomWrite {
            checksum_override: false,
        };
        let forced = RomWrite {
            checksum_override: true,
        };
        assert_eq!(plain.check_save(ChecksumStatus::Verified), Ok(()));
        assert_eq!(
            plain.check_save(ChecksumStatus::Failing),
            Err(ScopeError::ChecksumOverrideMissing)
        );
        assert_eq!(forced.check_save(ChecksumStatus::Failing), Ok(()));
        assert_eq!(forced.check_save(ChecksumStatus::Verified), Ok(()));
    }

    /// US-RR-7-3: a token minted through the confirmation path enforces the
    /// confirmed range.
    #[test]
    fn t6_minted_tokens_enforce_the_confirmed_scope() {
        let mut c = Scripted::new(Decision::Confirmed);
        let range = AddressRange::new(0x20_0000, 0x20_0010).unwrap();
        let mem =
            mint::ecu_memory_write(&mut c, &EcuMemoryWriteRequest::new(ecu(), range)).unwrap();
        assert!(mem.check_write(&ecu(), 0x20_0000, 0x10).is_ok());
        assert!(mem.check_write(&ecu(), 0x20_0000, 0x11).is_err());
        let rom = mint::rom_write(&mut c, &RomWriteRequest::new(ChecksumStatus::Verified)).unwrap();
        assert!(rom.check_save(ChecksumStatus::Failing).is_err());
        let reset = mint::ecu_reset(&mut c, &EcuResetRequest::new(ecu(), ResetKind::Ecu)).unwrap();
        assert!(reset.check_reset(&ecu(), ResetKind::Ecu).is_ok());
    }

    #[test]
    fn t6_scope_violation_errors_display() {
        assert_eq!(
            ScopeError::EmptyWrite { start: 0x10 }.to_string(),
            "zero-length write at 0x10 is not allowed"
        );
        assert_eq!(
            ScopeError::AddressOverflow {
                start: 0xFFFF_FFFE,
                len: 4
            }
            .to_string(),
            "write of 4 bytes at 0xFFFFFFFE overflows the 32-bit address space"
        );
        assert_eq!(
            ScopeError::OutOfRange {
                granted: AddressRange::new(0x1000, 0x1100).unwrap(),
                requested: AddressRange::new(0x1100, 0x1101).unwrap(),
            }
            .to_string(),
            "write to 0x001100..0x001101 is outside the authorised range 0x001000..0x001100"
        );
        assert_eq!(
            ScopeError::ChecksumOverrideMissing.to_string(),
            "ROM checksums fail and the user did not confirm saving with failing checksums"
        );
    }

    /// With the feature on, the authority surface is the same seal and the
    /// same minting path.
    #[cfg(feature = "confirmation-authority")]
    #[test]
    fn authority_reexports_seal_and_mints() {
        fn assert_sealed<T: authority::Sealed>() {}
        assert_sealed::<Scripted>();
        let mut c = Scripted::new(Decision::Confirmed);
        let token =
            authority::mint_rom_write(&mut c, &RomWriteRequest::new(ChecksumStatus::Verified))
                .unwrap();
        assert!(!token.checksum_override());
    }
}
