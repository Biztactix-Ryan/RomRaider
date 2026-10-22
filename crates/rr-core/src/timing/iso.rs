//! Default timing parameters of the ISO diagnostic standards RomRaider's
//! links speak: K-line initialisation (ISO 9141-2), KWP2000 on K-line
//! (ISO 14230-2), and diagnostics on CAN (ISO 15765-2 network layer,
//! ISO 15765-3 application layer).
//!
//! One module per standard, named after it; each constant is named after the
//! standard's parameter (`P3min` is [`iso14230_2::P3_MIN`], `N_Bs` is
//! [`iso15765_2::N_BS_TIMEOUT`]) and cites the edition and clause its value
//! is printed in. Only values read in that edition are here; a parameter
//! whose clause could not be checked is left out rather than written from
//! memory (`fixtures/kat/timing-*.toml` say which).
//!
//! Each standard cited has later editions (ISO 14230-2:2013, ISO
//! 15765-2:2016 and :2024, ...). A later edition's value is a new constant
//! with its own citation, not an edit of these.

/// ISO 9141-2:1994 (first edition, 1994-02-01), *Road vehicles —
/// Diagnostic systems — Part 2: CARB requirements for interchange of digital
/// information*: the 5-baud initialisation timings W0 to W4.
///
/// Clause 7.4 ("W0 to W4 shall be as shown in figure 6"), Figure 6
/// ("Header"). The tester sends address 33h at 5 bit/s after the bus has
/// been idle for W0; the ECU answers with the synchronisation byte 55h
/// within W1, key word 1 within W2 of that and key word 2 within W3; the
/// tester returns the inverted key word 2 within W4, and the ECU the
/// inverted address within W4 of that (7.3.1).
///
/// The message timings P1 to P4 of ISO 9141-2 are in its clause 12, which
/// is not transcribed here.
pub mod iso9141_2 {
    use core::time::Duration;

    /// W0 minimum, 2 ms: the bus is idle at least this long before the
    /// tester sends the 5-baud address byte. ISO 9141-2:1994, 7.4,
    /// Figure 6 (`W0 ≥ 2 ms`); Figure 6 gives no maximum.
    pub const W0_MIN: Duration = Duration::from_millis(2);

    /// W1 minimum, 60 ms: end of the address byte's stop bit to the ECU's
    /// synchronisation byte (55h). ISO 9141-2:1994, 7.4, Figure 6
    /// (`60 ms ≤ W1 ≤ 300 ms`); its footnote derives the 60 ms from a 3 %
    /// tolerance on the 2 s address byte.
    pub const W1_MIN: Duration = Duration::from_millis(60);

    /// W1 maximum, 300 ms. ISO 9141-2:1994, 7.4, Figure 6.
    pub const W1_MAX: Duration = Duration::from_millis(300);

    /// W2 minimum, 5 ms: synchronisation byte to key word 1. ISO
    /// 9141-2:1994, 7.4, Figure 6 (`5 ms ≤ W2 ≤ 20 ms`).
    pub const W2_MIN: Duration = Duration::from_millis(5);

    /// W2 maximum, 20 ms. ISO 9141-2:1994, 7.4, Figure 6.
    pub const W2_MAX: Duration = Duration::from_millis(20);

    /// W3 minimum, 0 ms: key word 1 to key word 2. ISO 9141-2:1994, 7.4,
    /// Figure 6 (`0 ≤ W3 ≤ 20 ms`).
    pub const W3_MIN: Duration = Duration::from_millis(0);

    /// W3 maximum, 20 ms. ISO 9141-2:1994, 7.4, Figure 6.
    pub const W3_MAX: Duration = Duration::from_millis(20);

    /// W4 minimum, 25 ms: key word 2 to the tester's inverted key word 2,
    /// and that to the ECU's inverted address (7.3.1). ISO 9141-2:1994,
    /// 7.4, Figure 6 (`25 ms ≤ W4 ≤ 50 ms`).
    pub const W4_MIN: Duration = Duration::from_millis(25);

    /// W4 maximum, 50 ms. ISO 9141-2:1994, 7.4, Figure 6.
    pub const W4_MAX: Duration = Duration::from_millis(50);
}

/// ISO 14230-2:1999 (first edition, 1999-03-15), *Road vehicles —
/// Diagnostic systems — Keyword Protocol 2000 — Part 2: Data link layer*:
/// the default message timings P1 to P4.
///
/// Clause 4.5.1, Table 3 ("Normal Timing Parameters Set (for functional and
/// physical addressing)"), the *default* columns, in milliseconds. Figure 6
/// defines the parameters: P1 is the inter-byte time of an ECU response, P2
/// the time from a tester request to the ECU response (or between two ECU
/// responses), P3 the time from the end of the ECU responses to a new
/// tester request, P4 the inter-byte time of a tester request. Table 4 (the
/// extended set, physical addressing only) has the same defaults. Each may
/// be changed with the AccessTimingParameter service (5.4) within Table 3's
/// lower and upper limits, which are not transcribed here.
///
/// The initialisation timings of clause 5.2 (5-baud W1 to W5, fast-init
/// TiniL and TWuP) are not transcribed here.
pub mod iso14230_2 {
    use core::time::Duration;

    /// P1 minimum (default), 0 ms: inter-byte time of an ECU response.
    /// ISO 14230-2:1999, 4.5.1, Table 3.
    pub const P1_MIN: Duration = Duration::from_millis(0);

    /// P1 maximum (default), 20 ms. ISO 14230-2:1999, 4.5.1, Table 3.
    pub const P1_MAX: Duration = Duration::from_millis(20);

    /// P2 minimum (default), 25 ms: tester request to ECU response, or
    /// between two ECU responses. ISO 14230-2:1999, 4.5.1, Table 3.
    pub const P2_MIN: Duration = Duration::from_millis(25);

    /// P2 maximum (default), 50 ms. ISO 14230-2:1999, 4.5.1, Table 3. A
    /// server may extend it with negative response code 78h (4.5.2).
    pub const P2_MAX: Duration = Duration::from_millis(50);

    /// P3 minimum (default), 55 ms: end of the ECU responses to the start of
    /// a new tester request. ISO 14230-2:1999, 4.5.1, Table 3.
    ///
    /// Legacy's bare `sleep(55L)` after the NCS ECU-ID request
    /// (`logger/ecu/comms/io/connection/NCSLoggerConnection.java:111`) is
    /// this wait (PROJECT.md MF-28).
    pub const P3_MIN: Duration = Duration::from_millis(55);

    /// P3 maximum (default), 5000 ms. ISO 14230-2:1999, 4.5.1, Table 3.
    pub const P3_MAX: Duration = Duration::from_millis(5000);

    /// P4 minimum (default), 5 ms: inter-byte time of a tester request.
    /// ISO 14230-2:1999, 4.5.1, Table 3. The table's lower limit, the least
    /// it may be set to, is 0 ms; 5 ms is the default.
    pub const P4_MIN: Duration = Duration::from_millis(5);

    /// P4 maximum (default), 20 ms. ISO 14230-2:1999, 4.5.1, Table 3.
    pub const P4_MAX: Duration = Duration::from_millis(20);
}

/// ISO 15765-2:2004 (first edition, 2004-10-15), *Road vehicles —
/// Diagnostics on Controller Area Networks (CAN) — Part 2: Network layer
/// services*: the network-layer timeouts and the SeparationTime (STmin)
/// range.
///
/// Clause 6.7.1, Table 16 ("Network layer timing parameter values"). A
/// specified timeout is a lower limit: "the real timeout shall occur no
/// later than at the specified timeout value + 50 %". N_Br and N_Cs have no
/// timeout, only performance requirements relative to the timeouts
/// (`(N_Br + N_Ar) < 0.9 × N_Bs timeout`, `(N_Cs + N_As) < 0.9 × N_Cr
/// timeout`), so they have no constant here.
pub mod iso15765_2 {
    use core::time::Duration;

    /// N_As timeout, 1000 ms: transmission of a CAN frame on the sender
    /// side (L_Data.request to L_Data.confirm). ISO 15765-2:2004, 6.7.1,
    /// Table 16.
    pub const N_AS_TIMEOUT: Duration = Duration::from_millis(1000);

    /// N_Ar timeout, 1000 ms: transmission of a CAN frame on the receiver
    /// side. ISO 15765-2:2004, 6.7.1, Table 16.
    pub const N_AR_TIMEOUT: Duration = Duration::from_millis(1000);

    /// N_Bs timeout, 1000 ms: the sender waiting for the next FlowControl
    /// frame. ISO 15765-2:2004, 6.7.1, Table 16.
    pub const N_BS_TIMEOUT: Duration = Duration::from_millis(1000);

    /// N_Cr timeout, 1000 ms: the receiver waiting for the next
    /// ConsecutiveFrame. ISO 15765-2:2004, 6.7.1, Table 16.
    pub const N_CR_TIMEOUT: Duration = Duration::from_millis(1000);

    /// The longest STmin, 127 ms: FlowControl byte #3 values 00h to 7Fh are
    /// 0 to 127 ms. ISO 15765-2:2004, 6.5.5.5, Table 15. A sender that
    /// receives a reserved STmin value (80h to F0h, FAh to FFh) uses this
    /// one instead (6.5.5.6).
    pub const STMIN_MAX: Duration = Duration::from_millis(127);

    /// The shortest sub-millisecond STmin, 100 µs: value F1h. ISO
    /// 15765-2:2004, 6.5.5.5, Table 15 (F1h to F9h are 100 µs to 900 µs in
    /// steps of 100 µs).
    pub const STMIN_SUB_MS_MIN: Duration = Duration::from_micros(100);

    /// The longest sub-millisecond STmin, 900 µs: value F9h. ISO
    /// 15765-2:2004, 6.5.5.5, Table 15.
    pub const STMIN_SUB_MS_MAX: Duration = Duration::from_micros(900);
}

/// ISO 15765-3:2004 (first edition, 2004-10-15), *Road vehicles —
/// Diagnostics on Controller Area Networks (CAN) — Part 3: Implementation
/// of unified diagnostic services (UDS on CAN)*: the server response times
/// of the default session.
///
/// Clause 6.3.2, Table 2 ("Application layer timing parameter definitions
/// for the defaultSession"). The client's own timeouts (P2CAN_Client,
/// P2*CAN_Client) are these plus a network-dependent ΔP2CAN, so they have
/// no constant here.
pub mod iso15765_3 {
    use core::time::Duration;

    /// P2CAN_Server maximum, 50 ms: the server starts its response within
    /// this long of receiving a request. ISO 15765-3:2004, 6.3.2, Table 2.
    pub const P2_CAN_SERVER_MAX: Duration = Duration::from_millis(50);

    /// P2*CAN_Server maximum, 5000 ms: after a negative response with code
    /// 78h (response pending), the server starts its response within this
    /// long. ISO 15765-3:2004, 6.3.2, Table 2.
    pub const P2_STAR_CAN_SERVER_MAX: Duration = Duration::from_millis(5000);
}
