# SSM over K-line: legacy protocol flow

What the Java RomRaider does when it talks Subaru Select Monitor (SSM) to an
ECU or TCU, written from the source only (the Java was never run, D-024). It
is the spec for the SSM codec (US-RR-47), the ECU simulator (US-RR-36) and
the first real-car captures on a KKL cable (US-RR-33). Part of the legacy
behaviour catalogue, story US-RR-12, task US-RR-12-8.

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses. `iso9141/X.java` is short for
  `io/protocol/ssm/iso9141/X.java`, and `iso15765/X.java` for
  `io/protocol/ssm/iso15765/X.java`. The protocol notes shipped with legacy
  are cited as `legacy/docs/ssm_info.txt:N`. Every reference is
  `file:line` or `file:first-last`.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-SSM-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Byte notation.** Bytes are hex without `0x` unless quoted from a source.
  That includes the value of a length byte quoted as a byte: `LL = 39` is
  the byte `39`, decimal 57. Counts and sizes (`N`, `count`, `n`, "40
  bytes", "N ≤ 84") are decimal, and so is the arithmetic that defines
  `LL` (`LL = N + 1`). Where both appear together the decimal is given too.
  Every checksum in this page was recomputed; frames marked
  *hand-composed* were built for this page, not taken from a source.

### Source files

| Alias | Path under `legacy/` | Role |
|-------|---------------------|------|
| `iso9141/SSMProtocol.java` | `src/main/java/com/romraider/io/protocol/ssm/iso9141/SSMProtocol.java` | K-line frame builder, command constants, response-type checks, default serial settings |
| `iso9141/SSMResponseProcessor.java` | `.../io/protocol/ssm/iso9141/SSMResponseProcessor.java` | Echo stripping, response validation, data extraction |
| `iso9141/SSMChecksumCalculator.java` | `.../io/protocol/ssm/iso9141/SSMChecksumCalculator.java` | The 8-bit sum |
| `iso9141/SSMLoggerProtocol.java` | `.../io/protocol/ssm/iso9141/SSMLoggerProtocol.java` | Logger-facing packing: dedupe, address lists, expected response size, fan-out of answers |
| `logger/ecu/comms/io/connection/SSMLoggerConnection.java` | same path under `src/main/java/com/romraider/` | Init, reads, writes, reset: the request/response cycles |
| `io/serial/connection/SerialConnectionManager.java` | same | Echo-aware read loop, timeouts, line break, continuous-mode frame check |
| `io/serial/connection/SerialConnectionImpl.java` | same | Serial port settings, stale-data drain |
| `io/j2534/api/J2534ConnectionISO9141.java`, `io/j2534/api/J2534Impl.java` | same | The same flow over a J2534 pass-through device |
| `logger/ecu/comms/manager/QueryManagerImpl.java`, `PollingStateImpl.java`, `TransmissionManagerImpl.java` | same | Connect/init/retry loop and the fast-poll (continuous) state machine |
| `logger/ecu/comms/query/SSMEcuInit.java` | same | ECU ID extraction from the init response |
| `logger/ecu/definition/xml/LoggerDefinitionHandler.java` | same | Module addresses, serial settings and capability bits from the logger definition |
| `ramtune/test/**` | same | RamTune test app: the only user of A0 and B0 |
| `iso15765/*.java`, `logger/ecu/comms/io/connection/SSMLoggerCANSubQuery.java` | same | SSM over CAN (summarised separately) |
| `io/serial/connection/TestSerialConnection.java`, `TestSerialConnection2.java` | same | Legacy's own fake ECU: canned init frames, simulator behaviour |
| `legacy/docs/ssm_info.txt` | `docs/ssm_info.txt` | Protocol notes: frame format, commands, worked examples, init capability bits, parameter and switch addresses |

The logger definition file (`logger.xml`) that supplies the real module
addresses and serial timeouts is **not** in `legacy/` (only its DTD,
`legacy/definitions/logger.dtd`, is). Values that only that file holds are
marked *not in legacy/* below and must be read from the corpus copy or
measured during US-RR-33.

## 1. Physical layer

### 1.1 Serial settings

| Setting | Value | Source |
|---------|-------|--------|
| Baud | 4800 | `iso9141/SSMProtocol.java:168-170`; `legacy/docs/ssm_info.txt:1` ("4800 bps n, 8, 1") |
| Data bits | 8 | `iso9141/SSMProtocol.java:176-178` |
| Stop bits | 1 | `iso9141/SSMProtocol.java:180-182` |
| Parity | 0 (none) | `iso9141/SSMProtocol.java:184-186` |
| Connect timeout | 2000 ms | `iso9141/SSMProtocol.java:188-190` |
| Send timeout | 55 ms | `iso9141/SSMProtocol.java:192-194` |
| Flow control | disabled | `io/serial/connection/SerialConnectionImpl.java:201-202` |
| Port read timeouts | semi-blocking, 0, 0 | `io/serial/connection/SerialConnectionImpl.java:205-206` |
| RTS | cleared after open; DTR is not touched | `io/serial/connection/SerialConnectionImpl.java:207-208` |

Those are the protocol *defaults*, and only RamTune uses them
(`ramtune/test/RamTuneTestApp.java:301`). The logger instead takes baud,
data bits, stop bits, parity, `connect_timeout` and `send_timeout` from the
`<protocol>` element of the logger definition
(`logger/ecu/definition/xml/LoggerDefinitionHandler.java:186-202`; all six
attributes are `#REQUIRED`, `legacy/definitions/logger.dtd:91-96`). The SSM
values in the shipped `logger.xml` are *not in legacy/*.

### 1.2 Addresses

| Byte | Meaning | Source |
|------|---------|--------|
| `10` | ECU | `legacy/docs/ssm_info.txt:16-17`; `TestSerialConnection2.java:57` (`8010F0...`) |
| `18` | TCU | `io/serial/connection/SerialConnectionManager.java:92` (accepted as a response source); `TestSerialConnection2.java:61` (`8018F0...`, `80F018...`) |
| `F0` | Diagnostic tool (tester) | `legacy/docs/ssm_info.txt:18` |

The actual bytes come from each `<module address=".." tester="..">` of the
logger definition (`LoggerDefinitionHandler.java:211-223`;
`legacy/definitions/logger.dtd:75-80`). A commented-out fallback shows the
ECU as address `10`, tester `F0`, fast poll on
(`LoggerDefinitionHandler.java:440-451`). The frame builder writes the whole
address and tester arrays (`iso9141/SSMProtocol.java:209-210`), so on
K-line each must be one byte. The validator compares only the first byte of
each (`iso9141/SSMResponseProcessor.java:66-68`).

### 1.3 No wake-up

SSM on K-line has no 5-baud or fast init. Legacy opens the port and sends the
`BF` init request straight away: `SSMLoggerConnection.open` and
`SerialConnectionManager.open` are empty
(`logger/ecu/comms/io/connection/SSMLoggerConnection.java:61-63`,
`io/serial/connection/SerialConnectionManager.java:56-58`), and `ecuInit`
builds and sends the frame directly (`SSMLoggerConnection.java:78-87`). The
ISO 9141-2 initialisation timings W0–W4 (`rr_core::timing::iso::iso9141_2`)
do not apply to SSM.

### 1.4 Echo

A KKL cable has one wire for transmit and receive, so the tester receives
every byte it sends before the ECU's reply. Legacy assumes the echo is always
there and is exactly the request:

- **Request/response (known length)**: the receive buffer is sized for echo
  plus reply, `(N·1 + 6) + (3·N + 7)` bytes for an `A8` read of `N`
  addresses. The source writes the reply term first: `N·1 + 6` is the reply
  (`N` data bytes + 6 non-data bytes) and `3·N + 7` the echoed request
  (`3N` address bytes + 7 non-data bytes: header, two IDs, `LL`, command,
  `PP`, checksum). On the wire the echo comes first
  (`iso9141/SSMLoggerProtocol.java:70-72`; constants
  `RESPONSE_NON_DATA_BYTES = 6`, `REQUEST_NON_DATA_BYTES = 7`,
  `iso9141/SSMProtocol.java:57-60`).
- **Stripping**: `filterRequestFromResponse` drops the first
  `request.length` bytes whatever they are, for every command except an `A8`
  read in continuous state (`iso9141/SSMResponseProcessor.java:46-60`). The
  echo is **never compared** with what was sent (EC-16).
- **Continuous state**: no request is written, so nothing is stripped
  (`iso9141/SSMResponseProcessor.java:55-58`;
  `SerialConnectionManager.java:72-75`).
- **J2534 devices** emulate the echo: the channel is opened with
  `LOOPBACK = 1` (`io/j2534/api/J2534ConnectionISO9141.java:166`), and the
  unknown-length read asks for one extra message when loopback is on and
  concatenates them (`io/j2534/api/J2534Impl.java:1000-1024`), so the
  protocol layer sees the same echo-then-reply layout.

An interface that does not echo makes the strip remove reply bytes. With a
reply shorter than the request, Java throws `NegativeArraySizeException`
(`iso9141/SSMResponseProcessor.java:52`; EC-16).

### 1.5 Timing

Legacy has no timing model. What it does, and the nearest ISO parameter in
`rr_core::timing::iso`:

| What | Legacy behaviour | Source | Nearest ISO parameter |
|------|------------------|--------|-----------------------|
| Request inter-byte gap | None. The whole frame goes out in one `write` + `flush`, so the bytes are back to back at the UART rate | `io/serial/connection/SerialConnectionImpl.java:69-76` | P4 (tester inter-byte), `iso14230_2::P4_MIN` = 5 ms default. Legacy uses 0 |
| J2534 inter-byte and idle | `P1_MAX = 1`, `P3_MIN = 1`, `P4_MIN = 0` raw config values. SAE J2534-1 counts these in 0.5 ms units; that unit is not in legacy/ | `io/j2534/api/J2534ConnectionISO9141.java:162-173` | P1, P3, P4 |
| Reply wait, known length (`A8` polling) | Loop `sleep(1)` and decrement a counter that starts at `connect_timeout`, until `available() >= expected`. The counter counts iterations, not milliseconds (**MF-27**) | `io/serial/connection/SerialConnectionManager.java:76-86` | P2 (`iso14230_2::P2_MAX` = 50 ms default); legacy waits up to `connect_timeout` |
| Reply wait, unknown length (init, write, reset; all of RamTune) | Poll every 2 ms; stop when `available()` has not changed for `connect_timeout` ms (wall clock). Every such exchange takes at least `connect_timeout` (2000 ms by default) after the last byte (MF-82) | `io/serial/connection/SerialConnectionManager.java:107-129` | None. This is end-of-reply detection by silence |
| Gap between requests | None enforced. Before each written request, stale input is drained for up to 100 ms, and only if bytes are waiting. The logger loop waits at least 1 ms per cycle | `io/serial/connection/SerialConnectionImpl.java:124-135`; `SerialConnectionManager.java:72-74,113`; `logger/ecu/comms/manager/QueryManagerImpl.java:297,347-350` | P3 (`iso14230_2::P3_MIN` = 55 ms default). Legacy does not wait for it |
| `send_timeout` (55 ms) | Parsed and carried in the connection properties, but never read on the serial path. `SerialConnectionManager` reads only `getConnectTimeout()`. RamTune's "Send Timeout" field (default "55") has no effect on a serial port (MF-28) | `LoggerDefinitionHandler.java:198-201`; `iso9141/SSMProtocol.java:192-194`; `SerialConnectionManager.java:49`; `ramtune/test/RamTuneTestApp.java:481-485`; `ramtune/test/io/RamTuneTestAppConnectionProperties.java:57-58` | Equals `iso14230_2::P3_MIN` (see MF-28 for the same 55 ms on NCS) |
| Line break (stops continuous mode) | Break held for `(10000 / baud) * (dataBits + stopBits + parity + 1)` ms in integer arithmetic: 20 ms at 4800 8N1. Then drain, sleeping 2 ms + 10 ms per round, until nothing is waiting | `io/serial/connection/SerialConnectionManager.java:131-147`; `SerialConnectionImpl.java:169-175` | None (not an ISO 9141 or 14230 mechanism) |
| J2534 "break" | Writes 20 zero bytes with a 100 ms timeout, then reads with 100 ms timeouts until a read is empty | `io/j2534/api/J2534ConnectionISO9141.java:103-124` | None |
| J2534 timeouts | Timeouts throw `J2534Exception` | `io/j2534/api/J2534Impl.java:932-936,1007-1011` | n/a |

## 2. Frame format (K-line)

### 2.1 Layout

Request (tester to module) and response (module to tester) have the same
layout (`legacy/docs/ssm_info.txt:6-21`):

| Offset | Request | Response | Source |
|--------|---------|----------|--------|
| 0 | `80` header | `80` header | `iso9141/SSMProtocol.java:43,208`; `iso9141/SSMResponseProcessor.java:65` |
| 1 | destination (module, `10` / `18`) | destination (tester, `F0`) | `iso9141/SSMProtocol.java:209`; `iso9141/SSMResponseProcessor.java:66` |
| 2 | source (tester, `F0`) | source (module) | `iso9141/SSMProtocol.java:210`; `iso9141/SSMResponseProcessor.java:67-68` |
| 3 | `LL` data length | `LL` data length | `iso9141/SSMProtocol.java:211`; `iso9141/SSMResponseProcessor.java:69` |
| 4 | command | response code | `iso9141/SSMProtocol.java:212`; `iso9141/SSMResponseProcessor.java:70` |
| 5 … n-2 | parameters | data | `iso9141/SSMProtocol.java:213-219`; `iso9141/SSMResponseProcessor.java:74-81` |
| n-1 | checksum | checksum | `iso9141/SSMProtocol.java:220-223`; `iso9141/SSMResponseProcessor.java:71` |

- **Data length `LL`** counts the command/response byte and everything after
  it, up to but not including the checksum. So frame length = `LL + 5`. The
  builder writes `content + 1`, or `content + 2` when the mode byte is added
  (`iso9141/SSMProtocol.java:201-211`); the validator expects
  `response.length - 5` (`iso9141/SSMResponseProcessor.java:69`, as
  `length - RESPONSE_NON_DATA_BYTES + 1`); the continuous-mode check does the
  same (`SerialConnectionManager.java:93`). The builder takes the low 8 bits
  of the length with no range check (MF-79).
- **Response code = command + `40`** for every K-line command: `A0`→`E0`,
  `A8`→`E8`, `B0`→`F0`, `B8`→`F8`, `BF`→`FF`
  (`iso9141/SSMProtocol.java:47-56`). Legacy uses the constants, not the
  arithmetic.
- **Minimum response**: the validator needs more than 6 bytes, so a response
  must carry at least one data byte (`iso9141/SSMResponseProcessor.java:64`).

### 2.2 Checksum

The checksum is the low 8 bits of the sum of every byte before it, header
included (`legacy/docs/ssm_info.txt:20-21`).
`SSMChecksumCalculator.calculateChecksum(bytes)` sums `bytes[0 ..
length-2]`, so it ignores the last element, which is the checksum slot
(`iso9141/SSMChecksumCalculator.java:31-38`). Two details are needed to port
it exactly:

- It adds **signed** bytes: `ByteUtil.asInt` is `Byte.intValue()`
  (`util/ByteUtil.java:69-71`). It then keeps `total mod 2^16` and narrows
  that to a byte (`iso9141/SSMChecksumCalculator.java:37`;
  `util/ByteUtil.java:60-62`). Sums of signed and unsigned bytes agree
  modulo 256, so the result is the plain 8-bit sum. Port it as
  `wrapping_add` over `u8`.
- The builder writes a `00` placeholder, computes the sum over everything
  before it, then overwrites the placeholder
  (`iso9141/SSMProtocol.java:220-223`). The validator computes the sum over
  the received frame minus its last byte and compares that with the last
  byte (`iso9141/SSMResponseProcessor.java:71`).

## 3. Commands

| Cmd | Name | Request parameters (after the command byte) | Response | Used by | Source |
|-----|------|----------------------------------------------|----------|---------|--------|
| `BF` | ECU init | none | `FF` + init data | logger, RamTune | `iso9141/SSMProtocol.java:55-56,65-70`; `legacy/docs/ssm_info.txt:43,83-85` |
| `A8` | Read addresses | `PP` mode, then `N` × 3-byte address | `E8` + `N` bytes, one per address, in request order | logger (all polling, DTCs, learning tables, global adjust), RamTune (single bytes) | `iso9141/SSMProtocol.java:49-50,101-107`; `legacy/docs/ssm_info.txt:40,61-67` |
| `A0` | Read memory block | `PP` mode, 3-byte start address, `count - 1` | `E0` + `count` bytes | RamTune only | `iso9141/SSMProtocol.java:46-48,92-99`; `legacy/docs/ssm_info.txt:39,53-59` |
| `B8` | Write one address | 3-byte address, 1 data byte | `F8` + the byte written | logger (ECU reset, global adjustments), RamTune | `iso9141/SSMProtocol.java:53-54,81-90`; `legacy/docs/ssm_info.txt:42,76-81` |
| `B0` | Write memory block | 3-byte start address, data bytes | `F0` + the bytes written | RamTune only | `iso9141/SSMProtocol.java:51-52,72-79`; `legacy/docs/ssm_info.txt:41,69-74` |

Addresses are 24-bit **big-endian** (`ADDRESS_SIZE = 3`,
`iso9141/SSMProtocol.java:57`; `legacy/docs/ssm_info.txt:55-59`; RamTune
builds them big-endian, `ramtune/test/command/generator/WriteCommandGenerator.java:65-72`).
No other K-line command appears in legacy. ECU reset is a `B8` write
(section 3.6).

### 3.1 Mode byte `PP` (A8 and A0)

`PP = 00` reads once; `PP = 01` reads continuously, so the ECU repeats the
response until it gets another command (`legacy/docs/ssm_info.txt:45-49`;
`iso9141/SSMProtocol.java:44-45`). The builder adds `PP` only for `A8` and
`A0` (`padContent`, `iso9141/SSMProtocol.java:98,106,213-216`). The source
calls this byte "padding" (`padContent`, and the frame comments at
`iso9141/SSMProtocol.java:97,105`); it is the same byte as ssm_info's read
mode, and it is counted in `LL`. The K-line constant
`READ_MEMORY_PADDING = 00` (`iso9141/SSMProtocol.java:46`) is never used:
an `A0`'s `PP` comes from the poll state like an `A8`'s. (Only the CAN
variant writes that constant, `iso15765/SSMProtocol.java:214`.) It chooses
`01` when its polling state says fast poll is on, and `00` otherwise. That
state is static and shared by the whole JVM (MF-78). For `A0`, this means
a RamTune block read can go out as continuous whenever the logger's fast
poll flag is set.

### 3.2 ECU init (`BF`)

Request, always 6 bytes: `80 <dst> F0 01 BF <cs>`. For the ECU,
`80 10 F0 01 BF 40` (`iso9141/SSMProtocol.java:68`;
`legacy/docs/ssm_info.txt:27`).

Exchange (`SSMLoggerConnection.java:78-87`):

1. Build the request (`iso9141/SSMProtocol.java:65-70`).
2. `manager.send(request)`, the unknown-length read: drain stale input,
   write, then collect bytes until the line has been quiet for
   `connect_timeout` (`SerialConnectionManager.java:107-129`).
3. Strip the echo (`request.length` = 6 bytes) using a fresh
   `PollingStateImpl`, which also resets the shared poll state (MF-78;
   `SSMLoggerConnection.java:83`).
4. Validate the frame (section 4) and check that byte 4 is `FF`
   (`iso9141/SSMProtocol.java:118-128`; `iso9141/SSMLoggerProtocol.java:84-90`).
5. Parse (`iso9141/SSMProtocol.java:130-132`). **Init data** is the frame
   from offset 5 up to, but not including, the checksum: `LL - 1` bytes
   (`iso9141/SSMResponseProcessor.java:74-81`).

Init data layout (`iso9141/SSMProtocol.java:119`, "response_header
3_unknown_bytes 5_ecu_id_bytes readable_params_switches... checksum"):

| Init-data index (0-based) | ssm_info "BYTE" (1-based) | Content | Source |
|---------------------------|---------------------------|---------|--------|
| 0–2 | 1–3 | Three bytes legacy does not interpret. Neither ssm_info nor the code names them. Every frame in legacy starts `A1 10`, `A2 10` or `A6 10`, and the third byte varies (`02`, `09`, `0B`, `0F`, `11`, `14`, `22`), so a simulator may send any value | `iso9141/SSMProtocol.java:119`; `legacy/docs/ssm_info.txt:30`; `TestSerialConnection2.java:57-69` |
| 3–7 | 4–8 | **ECU ID**, 5 bytes. Shown as 10 upper-case hex digits | `logger/ecu/comms/query/SSMEcuInit.java:35-41`; `util/HexUtil.java:41-42` |
| 8 … | 9 … | Capability bit flags ("readable params switches"): bit = 1 means the parameter or switch can be read | `legacy/docs/ssm_info.txt:138-618` |

The data lengths seen in legacy's canned frames are 40 (`LL = 29`, decimal
41), 56 (`LL = 39`, decimal 57) and 104 (`LL = 69`, decimal 105) bytes
(`TestSerialConnection2.java:57-69`, checked for this page).

**Minimum length.** The frame validator accepts one byte of init data
(section 2.1), but the ECU ID copy needs 8: `SSMEcuInit` copies init-data
bytes 3–7 without a length check (`logger/ecu/comms/query/SSMEcuInit.java:38-39`),
so init data of fewer than 8 bytes throws
`ArrayIndexOutOfBoundsException`. The init loop catches any exception as an
init failure ("INITFAIL", `QueryManagerImpl.java:249-251`; section 6). A
valid init reply therefore has `LL ≥ 09` (14-byte frame); the port should
reject a shorter one as a typed error. `ReadCodesManagerImpl` uses the length as a model
hint: below 104 bytes it stops the DTC list at `D488`
(`logger/ecu/comms/readcodes/ReadCodesManagerImpl.java:66-74`; its `< 56`
branch can never run, MF-83).

**ECU ID use.** The ID string selects the logger's per-ECU definitions
(`ecuparam` addresses, `LoggerDefinitionHandler.java:366-384`) and the
CAL ID / car lookup (`logger/ecu/EcuLogger.java:399-433`). A changed ID
reloads the logger parameters (`EcuLogger.java:404-416`).

**Capability bits.** A `<parameter>` that has both `ecubyteindex` and
`ecubit`, or a `<switch>` that has `ecubyteindex`, is kept only if
`isSupportedParameter` says so (`LoggerDefinitionHandler.java:229-230,274-275,344-354,386-403`):

```
supported = index < initData.length && (initData[index] & (1 << bit)) != 0
```

(`LoggerDefinitionHandler.java:462-472`). In detail:

- `ecubyteindex` is a **0-based index into the init data** (offset 5 of the
  frame onward), so ssm_info's 1-based "BYTE n" is `ecubyteindex = n - 1`.
  This follows from ssm_info placing BYTE 9 "immediately after the 5 byte
  ECU ID" (`legacy/docs/ssm_info.txt:139`) and from the ID being at
  init-data 3–7 (`SSMEcuInit.java:39`).
- Bit 0 is the least significant bit, and bit 7 works despite Java's signed
  bytes: `b & 0x80` is still non-zero.
- An index past the end of the init data means **not supported**.
- With no init yet (`ecuInit == null`), or with either attribute missing,
  the parameter is kept.
- For a `<switch>`, the capability bit is the switch's own `bit` attribute
  (`LoggerDefinitionHandler.java:275`, `ATTR_BIT`, not `ecubit`). That is
  by design: switch byte `0x000061` and init BYTE 20 have the same bit
  layout (`legacy/docs/ssm_info.txt:250-258` vs `1171-1179`).

ssm_info lists the meaning of each bit for BYTE 9 to BYTE 56
(`legacy/docs/ssm_info.txt:138-618`), parameter addresses and scalings
(`:621-1161`) and switch bytes (`:1165-1279`). Those belong to the logger
definition page, not to the codec.

### 3.3 Read addresses (`A8`)

Request: `80 <dst> F0 LL A8 PP A1 A1 A1 … AN AN AN <cs>`, with
`LL = 3·N + 2` (`iso9141/SSMProtocol.java:101-107`; `legacy/docs/ssm_info.txt:61-67`).

Packing (`iso9141/SSMLoggerProtocol.java:56-59,132-156`):

- Queries are de-duplicated by their **full address byte string**, keeping
  the first occurrence and its order (`iso9141/SSMLoggerProtocol.java:132-140`;
  `EcuQueryImpl` equality is by address hex,
  `logger/ecu/comms/query/EcuQueryImpl.java:64-70`). Overlapping but
  different queries are not merged.
- Each query adds every one of its addresses in order. A multi-byte
  parameter (`<address length="2">`) expands to consecutive addresses
  (`logger/ecu/definition/EcuAddressImpl.java:76-95`), for example RPM is
  `00000E`, `00000F` (`legacy/docs/ssm_info.txt:621-622,669-674`).
- **No per-request limit.** The one-byte `LL` caps a valid request at
  `3·N + 2 ≤ 255`, so **N ≤ 84**. Legacy does not enforce this: 85 or more
  addresses wrap `LL` (MF-79). The only caller that batches is DTC
  reading, 150 queries per request before de-duplication
  (`ReadCodesManagerImpl.java:96-106`).

Response: `80 F0 <src> LL E8 D1 … DN <cs>`, `LL = N + 1`. Data byte `i` is
the value at address `i` of the request. The fan-out walks the de-duplicated
queries in order, takes `addresses-per-query` bytes for each, and gives every
original query the bytes for its address key
(`iso9141/SSMLoggerProtocol.java:98-114`). Expected receive size:
`(N + 6) + (3N + 7)` with echo in single-shot state, `N + 6` in continuous
state (`iso9141/SSMLoggerProtocol.java:61-78`).

### 3.4 Read memory block (`A0`)

Request: `80 <dst> F0 06 A0 PP A2 A1 A0 CC <cs>`, `CC = count - 1`
(`iso9141/SSMProtocol.java:92-99`; `legacy/docs/ssm_info.txt:53-59`).
`LL` is always 6. Response: `80 F0 <src> LL E0 D1 … Dcount <cs>`,
`LL = count + 1`.

- `count` must be > 0 (`checkGreaterThanZero`, `iso9141/SSMProtocol.java:95`).
  `CC` is `asByte(count - 1)`, so a count above 256 wraps silently
  (MF-79). A single response frame can carry at most 254 data bytes,
  because `LL = count + 1 ≤ 255`. What an ECU sends for a count of 255 or
  256 (a reply whose `LL` cannot be written) is not in legacy; the port
  rejects such counts before sending (LEG-SSM-06).
- RamTune splits a range into blocks of `blocksize` (default 128, "Set to
  value allowed by the ECU"; nothing validates it), and reads a 1-byte
  remainder, or every byte when block mode is off, with a single-address
  `A8` (`ramtune/test/command/generator/ReadCommandGenerator.java:40-77`;
  `ramtune/test/RamTuneTestApp.java:242-244`). The address increment is
  big-endian 24-bit (`ReadCommandGenerator.java:85-110`).
- The logger never sends `A0`. RamTune forces the K-line protocol because
  "Read Address blocks only seems to work with ISO9141"
  (`ramtune/test/RamTuneTestApp.java:144-151`).

### 3.5 Write one address (`B8`)

Request: `80 <dst> F0 05 B8 A2 A1 A0 VV <cs>` (`iso9141/SSMProtocol.java:81-90`;
`legacy/docs/ssm_info.txt:76-81`). Response: `80 F0 <src> 02 F8 VV <cs>`, the
value the ECU now holds (`legacy/docs/ssm_info.txt:128-134`).

- The logger sends each write through the unknown-length read, strips the
  echo, and accepts the write if byte 4 is `F8` and byte 5 equals the value
  sent (`SSMLoggerConnection.java:159-185`;
  `iso9141/SSMProtocol.java:152-163`). That check **does not run
  `validateResponse`**, so header, IDs, length and checksum of a write
  acknowledgement are never checked (MF-80).
- `sendAddressWrites` silently skips any query whose address is not exactly
  3 bytes. Multi-byte parameters are never written
  (`SSMLoggerConnection.java:163-164`; MF-85).
- **Gating.** There is no write-enable model. The logger writes only from
  two user actions: the only callers of the SSM write paths are
  `ResetManagerImpl` (`logger/ecu/comms/reset/ResetManagerImpl.java:50`,
  `SSMLoggerConnection.java:66-76`) and `SSMGlobalAdjustManager`
  (`logger/ecu/comms/globaladjust/SSMGlobalAdjustManager.java:109`):
  - ECU reset, after a confirmation dialog (section 3.6).
  - Global adjustments: the user confirms values in a panel, then the
    timing byte and two RPM bytes (`(rpm + 3200) / 25`) are written with
    `B8` (`logger/ecu/comms/globaladjust/SSMGlobalAdjustManager.java:72-121,146-150`).

  RamTune asks "Are you sure you want to write to ECU memory?"
  (`ramtune/test/RamTuneTestApp.java:419-423`) and checks only the address
  and data text (`RamTuneTestApp.java:377-417`). With block mode off it
  writes one `B8` per byte (`WriteCommandGenerator.java:50-57`).

### 3.6 ECU reset (a `B8` write)

Reset writes value `40` to address `000060`: `80 10 F0 05 B8 00 00 60 40 DD`
(`iso9141/SSMProtocol.java:134-139`). The expected answer is
`80 F0 10 02 F8 40 BA`, checked in full (`validateResponse`, then byte 4 =
`F8` and byte 5 = `40`) (`iso9141/SSMProtocol.java:141-150`). It is
triggered from the menu after a confirmation, with code `0x40` for SSM
(`logger/ecu/ui/swing/menubar/action/ResetEcuAction.java:40-51`), on its own
connection (`logger/ecu/comms/reset/ResetManagerImpl.java:42-61`). A running
logger is stopped first and restarted after (`ResetEcuAction.java:42-50`).

### 3.7 Write memory block (`B0`)

Request: `80 <dst> F0 LL B0 A2 A1 A0 D1 … Dn <cs>`, `LL = n + 4`
(`iso9141/SSMProtocol.java:72-79`; `legacy/docs/ssm_info.txt:69-74`).
Response: `80 F0 <src> LL F0 D1 … Dn <cs>`, `LL = n + 1`, the data written
(`legacy/docs/ssm_info.txt:118-124`; legacy's fake ECU echoes the request
data, `TestSerialConnection2.java:188-200`). RamTune only, when the "block"
box is ticked (`WriteCommandGenerator.java:46-49`).

- **Limit.** `LL = n + 4 ≤ 255`, so one `B0` carries at most **251** data
  bytes (and its acknowledgement, `LL = n + 1`, then fits). Legacy does not
  enforce this: unlike reads, RamTune does not split a block write by
  `blocksize`, it sends the whole range as one `B0`
  (`WriteCommandGenerator.java:40-49`; `blocksize` is accepted and unused),
  and the builder wraps `LL` (MF-79, `iso9141/SSMProtocol.java:211`).
  The port rejects `n > 251` as a typed error.

The reply passes
`parseResponseData` (generic validation) but is **not** compared with the
data sent (`ramtune/test/RamTuneTestApp.java:307-312`).

## 4. Response validation

`validateResponse(frame)` runs on an echo-stripped frame. It is used by
every read, the init and the reset, but not by the write acknowledgement
(section 3.5, MF-80). Checks, in order; the first failure throws
`InvalidResponseException` with the message shown
(`iso9141/SSMResponseProcessor.java:62-72,84-111`):

| # | Check | Message |
|---|-------|---------|
| 1 | `length > 6` | "Invalid response length" |
| 2 | `frame[0] == 80` | "Invalid header" |
| 3 | `frame[1] == tester[0]` | "Invalid diagnostic tool id" |
| 4 | `frame[2] == module.address[0]` | "Invalid <module> id" |
| 5 | `frame[3] == low8(length - 5)` | "Invalid response data length" |
| 6 | `frame[4]` ∈ {`FF`, `E8`, `E0`, `F8`, `F0`} | "Invalid response code" |
| 7 | `frame[last] == sum8(frame[0..last])` | "Invalid checksum" |

Check 6 does not tie the response code to the command sent: an `F8` frame
of the right length passes as the answer to an `A8` read. Only init (`FF`),
reset and write (`F8`) check the code against their command
(`iso9141/SSMProtocol.java:123-127,146-149,157-162`).

In continuous state the serial manager runs its own check before the
protocol layer does (`SerialConnectionManager.java:89-104`). It needs:
header `80`, destination `F0`, source `10` **or `18`** (hard-coded, not the
module), `frame[3] == length - 5` compared as a signed byte against an int
(fails for `LL ≥ 80`, MF-79), and the checksum. A failure goes to the
MF-08 path (section 6).

## 5. Polling cycle and continuous mode

The logger runs one connection per session. `QueryManagerImpl.run` loops:
init on each candidate interface (J2534 libraries first, then the serial
port), then poll until stopped or until an error
(`logger/ecu/comms/manager/QueryManagerImpl.java:162-198,200-258,260-368`).
Fast poll is on by default (`Settings.java:236`), but a module with
`fastpoll` false disables it (`logger/ecu/EcuLogger.java:1619-1633`).

Each cycle calls `sendAddressReads(queries, module, pollState)`
(`SSMLoggerConnection.java:89-147`; `TransmissionManagerImpl.java:55-60`):

- **STATE_0 (single-shot, or the first frame of a continuous run).** Drain
  stale input, write the `A8` request (`PP` = `01` if fast poll, else `00`),
  read exactly `echo + reply` bytes, strip the echo, validate, fan out
  (`SerialConnectionManager.java:72-87`). If the previous state was STATE_1,
  send a line break first to stop the stream (`SerialConnectionManager.java:67-70`).
- **STATE_1 (continuous).** Write nothing; read exactly one reply frame
  (`N + 6` bytes) of the stream the ECU is already sending
  (`SerialConnectionManager.java:76-104`). The backlog is never drained, so
  if the host falls behind it reads ever older frames (MF-81).

State machine (`QueryManagerImpl.java:283-344`):

| Condition after a cycle | Next state |
|-------------------------|------------|
| Fast poll on, STATE_0 | STATE_1 |
| Fast poll on, STATE_1, query set changed (`newQuery`) or a bad frame was seen | STATE_0 with `lastState = STATE_1`, so the next send begins with a break |
| Fast poll off | STATE_0 every cycle (one request per cycle) |
| Fast poll switched off while streaming, or the query list emptied | `endQueries`, which is a line break (`TransmissionManagerImpl.java:62-66`) |

Stopping the stream is done with a line break, not with a command
(`SerialConnectionManager.java:131-147`), even though ssm_info says the ECU
streams "until it receives another command" (`legacy/docs/ssm_info.txt:45`).
Whether the ECU also needs the break is to be measured in US-RR-33
(LEG-SSM-18).

## 6. Error paths

No SSM path retries a request. Recovery means tearing down the connection
and starting again: any exception leaves `runLogger`, which stops the
transmission manager, and the `run` loop re-inits
(`QueryManagerImpl.java:172-185,359-367`).

| Condition | What legacy does | Source |
|-----------|------------------|--------|
| No byte at all (init, write, reset) | The empty array fails `checkNotNullOrEmpty`: `IllegalArgumentException` | `iso9141/SSMResponseProcessor.java:48`; `util/ParamChecker.java:72-76` |
| Echo only, ECU silent (init) | Stripped reply is empty: `IllegalArgumentException` | `iso9141/SSMResponseProcessor.java:52-53`; `iso9141/SSMLoggerProtocol.java:86` |
| Fewer bytes than the echo (non-echoing interface) | `NegativeArraySizeException` | `iso9141/SSMResponseProcessor.java:52` |
| Reply cut short, then silence (init, reset, write: the unknown-length path) | Whatever arrived after the echo is processed as the frame. Init and reset run `validateResponse`: 6 bytes or fewer fail check 1 "Invalid response length", longer ones check 5 "Invalid response data length" (`LL` ≠ length − 5). A `B8` acknowledgement is not validated: 1 to 5 bytes throw `ArrayIndexOutOfBoundsException` (none at all: `IllegalArgumentException`, `iso9141/SSMLoggerProtocol.java:128`), and a 6-byte stub whose bytes 4 and 5 match is taken as success (MF-80). An init reply that passes validation but has fewer than 8 init-data bytes throws in `SSMEcuInit` (section 3.2) | `iso9141/SSMResponseProcessor.java:62-72`; `iso9141/SSMProtocol.java:118-128,141-150,152-163`; `logger/ecu/comms/query/SSMEcuInit.java:38-39` |
| Any init failure | Caught: "INITFAIL" message, connection closed, then the next interface is tried. When all fail, sleep 1000 ms and retry forever | `QueryManagerImpl.java:176-184,200-258` |
| Known-length read times out (STATE_0 or STATE_1) | Read whatever arrived, log it, discard it, and return with the zero-filled buffer. The timeout counter is not reset, and counts iterations (MF-27). The zeros then fail "Invalid header" | `SerialConnectionManager.java:76-85` |
| Invalid frame in STATE_0, or timeout in either state | `InvalidResponseException` leaves `runLogger`: error reported, sleep 500 ms, the TX manager stops (line break, close), then `run` re-inits from scratch | `QueryManagerImpl.java:359-367,172-185`; `TransmissionManagerImpl.java:68-75` |
| Invalid frame in STATE_1 | **MF-08**: the previous good frame is copied over the bad one and shown as live, and `newQuery` is set so the next cycle re-sends from STATE_0. If no good STATE_1 frame has been seen yet, `lastResponse` is null and the copy throws `NullPointerException`, which ends in the re-init path. The same code is duplicated for J2534 | `SerialConnectionManager.java:89-104`; `io/j2534/api/J2534ConnectionISO9141.java:77-92` |
| Wrong response code (any of the five) on a read | Accepted (MF-84) | `iso9141/SSMResponseProcessor.java:70` |
| Write acknowledgement does not match (`F8`/value mismatch) | `InvalidResponseException`. Global adjust reports "NOCONNECTION" and returns 0; reset reports failure and returns false | `iso9141/SSMProtocol.java:152-163`; `SSMGlobalAdjustManager.java:115-120`; `ResetManagerImpl.java:56-60` |
| Extra bytes after the reply | Unknown-length path: the length check fails. Known-length path: the extra bytes stay buffered until the next STATE_0 drain; in STATE_1 they misalign the next frame, which then takes the MF-08 path | `iso9141/SSMResponseProcessor.java:69`; `SerialConnectionImpl.java:124-135` |
| Corrupt echo (collision on the line) | Not detected; the bytes are dropped unseen (EC-16) | `iso9141/SSMResponseProcessor.java:51-54` |
| Negative response | K-line SSM has none in legacy. An ECU that ignores a request leads to the timeout path. Only the CAN variant decodes a negative response (section 7) | `iso9141/SSMResponseProcessor.java:70` (the accepted codes; no NRC case); `iso15765/SSMResponseProcessor.java:52-54` |
| RamTune, any failure | Exception shown in the trace pane; the port is closed | `ramtune/test/RamTuneTestApp.java:315-320,496-505` |

Shared state that affects every path: the module of the last built request
lives in a public static field used by validation (**MF-29**,
`iso9141/SSMProtocol.java:61,67,76,87,96,104`;
`iso9141/SSMResponseProcessor.java:31,66-68`), and the poll state is
static too (MF-78).

## 7. SSM over CAN (ISO 15765), briefly

Separate classes, chosen by transport name (`io/protocol/ProtocolFactory.java:29-44`).
Only J2534 devices carry it. Not used on the KKL cable.

- **Frame.** A 4-byte CAN ID, then the command and its parameters. No `80`
  header, no length, no checksum (`iso15765/SSMProtocol.java:204-223`). The
  request starts with the module's **`tester`** bytes and the response is
  checked against the module's **`address`** bytes. The comments give
  `000007E0` out and `000007E8` back (`iso15765/SSMProtocol.java:63,211`;
  `iso15765/SSMResponseProcessor.java:50-51,88-96`; `iso15765/SSMProtocol.java:128`).
- **Commands.** Init is `AA` → `EA`, not `BF`
  (`iso15765/SSMProtocol.java:51-52`). `A8` always uses mode byte `00`
  (`:213-215`). `B8` works as on K-line, including reset `000060` ← `40`
  (`:79-89,141-156`). `A0` and `B0` throw `UnsupportedProtocolException`
  (`:67-77,91-101`).
- **Init data.** The same layout after `EA`: 3 unknown bytes, then a 5-byte
  ECU ID (`iso15765/SSMProtocol.java:125-128,137-139`). Data starts at
  offset 5 (`iso15765/SSMResponseProcessor.java:58-65`).
- **Negative response.** `7F <cmd> <code>`: `13` "invalid format or
  length", `22` "address not allowed", anything else "unsupported
  parameter"; each throws (`iso15765/SSMResponseProcessor.java:52-54,74-86`).
- **No echo** to strip (`iso15765/SSMResponseProcessor.java:43-46`).
- **TCU on CAN.** Each query is sent alone, and a multi-address query is
  split into one-address requests whose answers are stitched together, "so
  max CAN data packet contains 8 or less bytes"
  (`SSMLoggerConnection.java:95-127`;
  `logger/ecu/comms/io/connection/SSMLoggerCANSubQuery.java:50-89`).

## 8. Worked examples

All checksums below were recomputed for this page.

### 8.1 From legacy sources

**Init, ECU** (`legacy/docs/ssm_info.txt:25-33`):

```
TX  80 10 F0 01 BF 40
RX  80 F0 10 39 FF  A2 10 0F  1B 14 40 05 05  73 FA EB 80 2B C1 02 AA 00 10 00 60
    CE 54 F8 B0 60 00 00 E0 00 00 00 00 00 DC 00 00 55 10 00 00 02 00 00 00 00
    00 00 00 00 00 00 00 00 00 00 00 00 00 1F
```

`LL = 39`: 62-byte frame, 56 bytes of init data, checksum `1F`. ECU ID
`1B14400505`. Init BYTE 9 (`ecubyteindex = 8`) is `73` = `0111 0011`:
Coolant Temperature (bit 6), A/F Correction #1 (5), A/F Learning #1 (4),
MAP (1) and Engine Speed (0) are supported; Engine Load (7), A/F Correction
#2 (3) and A/F Learning #2 (2) are not (`legacy/docs/ssm_info.txt:140-148`).

**Init with echo, as the serial port delivers it**
(`io/serial/connection/TestSerialConnection2.java:57`; the same frame
without echo is in the comment at `iso9141/SSMProtocol.java:120`):

```
80 10 F0 01 BF 40 | 80 F0 10 39 FF A2 10 11 31 52 58 40 06 73 FA CB 84 2B 83 FE A8
00 00 00 60 CE D4 FD B0 60 00 0F 20 00 00 00 00 00 DC 00 00 55 1E 30 C0 F2 22 00
00 40 FB 00 E1 00 00 00 00 00 00 00 00 59
```

ECU ID `3152584006`. The other canned inits in that file
(`TestSerialConnection2.java:57-69`) give ECU IDs `1954580505`
(`LL = 29`, 40 data bytes), `2F12795606`, `4352584006`, `1B04490605`,
`7442594007`, `2E12495106`, `1B14400605` (`LL = 39`), and `7144345007`,
`5112187007`, `6644D87207`, `6644D87407` (`LL = 69`, 104 data bytes). All
response checksums are correct. Do not reuse their echo halves: some are
not real exchanges (legacy test data only; see
[Ledgered issues](#ledgered-issues)).

**A0 read 128 bytes at `200000`, once** (`legacy/docs/ssm_info.txt:89-104`):

```
TX  80 10 F0 06 A0 00 20 00 00 7F C5
RX  80 F0 10 81 E0 <128 × 00> E1
```

**A8 continuous read of `000008` and `00001C`** (`legacy/docs/ssm_info.txt:108-114`):

```
TX  80 10 F0 08 A8 01 00 00 08 00 00 1C 55
RX  80 F0 10 03 E8 7D B1 99   80 F0 10 03 E8 7D B1 99   80 F0 10 03 E8 7D B1 99 ...
```

**B0 write 4 bytes at `200000`** (`legacy/docs/ssm_info.txt:118-124`):

```
TX  80 10 F0 08 B0 20 00 00 01 02 03 04 62
RX  80 F0 10 05 F0 01 02 03 04 7F
```

**B8 write `02` to `00006F`** (`legacy/docs/ssm_info.txt:128-134`):

```
TX  80 10 F0 05 B8 00 00 6F 02 AE
RX  80 F0 10 02 F8 02 7C
```

**ECU reset** (`iso9141/SSMProtocol.java:135,142`):

```
TX  80 10 F0 05 B8 00 00 60 40 DD
RX  80 F0 10 02 F8 40 BA
```

**A8 replies from legacy's fake ECU** (`TestSerialConnection2.java:130,136-145,150`):
`80 F0 10 06 E8 3F 60 00 00 00 0D` (5 values), `80 F0 10 02 E8 44 AE`,
`80 F0 10 02 E8 60 CA`, `80 F0 10 02 E8 80 EA`.

### 8.2 Hand-composed (not from a source)

**Init, TCU.** Destination `18`; checksum `80+18+F0+01+BF = 248` → `48`:

```
TX  80 18 F0 01 BF 48
```

**A8 single-shot, Coolant Temperature (`000008`)**, with the echo as the
receive buffer holds it (`(1+6)+(3+7) = 17` bytes). Value `7D` is 85 °C
under ssm_info's "minus 40" (`legacy/docs/ssm_info.txt:632-637`):

```
TX              80 10 F0 05 A8 00 00 00 08 35
RX (17 bytes)   80 10 F0 05 A8 00 00 00 08 35 | 80 F0 10 02 E8 7D E7
```

**A8 single-shot, Engine Speed (`00000E`, `00000F`)**. Value `0BB8` = 3000,
/4 = 750 rpm (`legacy/docs/ssm_info.txt:669-674`):

```
TX  80 10 F0 08 A8 00 00 00 0E 00 00 0F 4D
RX  80 F0 10 03 E8 0B B8 2E
```

**A0 read 4 bytes at `FF6000`, once:**

```
TX  80 10 F0 06 A0 00 FF 60 00 03 88
RX  80 F0 10 05 E0 DE AD BE EF 9D
```

## 9. Mapping to the port

How the behaviour above lands in the Rust design. These are pointers, not
decisions.

- The codec (US-RR-47) is pure: build and parse frames, the sum8, response
  code = command + `40`, `LL` and the `N ≤ 84` limit as typed errors, never
  wrap-around.
- The echo, timing and break belong to the K-line Link/LinkProfile. The
  timing values to use are measured on the car (LEG-SSM-17/18), with
  `rr_core::timing::iso::iso14230_2` defaults (P1–P4) as the starting
  reference, never legacy's iteration counters (MF-27).
- A bad or missing frame gives no value plus a link-health event, never the
  previous frame (MF-08).
- Codec and session state are owned per session, never static (MF-29,
  MF-78).
- Writes (`B8`, `B0`, reset) stay behind the write gate; the US-RR-33 work is
  read-only (`BF`, `A8`, `A0` only).

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-SSM-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID.

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-SSM-01 | MF-78 | `PollingStateImpl` keeps all its fields `static` (the code's own TODO says so), and its constructor resets them. Every `new PollingStateImpl()` in init, write, reset, DTC read, learning tables, global adjust or RamTune resets the logger's global poll state. The `A8`/`A0` mode byte is read from that state, so a request's `PP` depends on whatever touched it last. Sibling of MF-29 | `logger/ecu/comms/manager/PollingStateImpl.java:23-37`; `iso9141/SSMProtocol.java:62,214-215`; `SSMLoggerConnection.java:71,83,178`; `ReadCodesManagerImpl.java:104`; `SSMGlobalAdjustManager.java:83`; `ramtune/test/RamTuneTestApp.java:108` |
| UL-SSM-02 | MF-79 | Data-length overflow. `LL` is `Integer.byteValue()` with no cap, so an `A8` of 85 or more addresses, or a `B0` of more than 251 data bytes (section 3.7), sends a wrong `LL`. The continuous-mode check compares the signed byte `frame[3]` with an int, so any reply with `LL ≥ 80` (127 or more addresses) always fails and loops on MF-08 | `iso9141/SSMProtocol.java:201-211`; `SerialConnectionManager.java:93` |
| UL-SSM-03 | MF-79 | `A0` count byte `asByte(count - 1)` wraps for counts above 256, and RamTune never validates its block size | `iso9141/SSMProtocol.java:98`; `ramtune/test/RamTuneTestApp.java:242-244,377-417` |
| UL-SSM-04 | MF-80 | The write acknowledgement check skips `validateResponse`: a corrupt frame (bad checksum, wrong IDs or length) whose bytes 4 and 5 happen to match is taken as a successful write. A reply shorter than 6 bytes throws `ArrayIndexOutOfBoundsException`. RamTune's `B0` reply is not compared with the data at all | `iso9141/SSMProtocol.java:152-163`; `ramtune/test/RamTuneTestApp.java:307-312` |
| UL-SSM-05 | EC-16 | The echo is stripped by length and never compared with the request, so a collision on the K-line or an adapter that does not echo misframes silently | `iso9141/SSMResponseProcessor.java:46-60` |
| UL-SSM-06 | MF-81 | Continuous mode reads one frame per cycle and never drains the backlog, so displayed values can lag real time without bound, with no timestamp. Related to MF-08 and MF-26 (staleness), which cover other paths | `SerialConnectionManager.java:76-87` |
| UL-SSM-07 | MF-28 | `send_timeout` (55 ms, ISO 14230 P3min) is configured in three places but never used on the serial path; RamTune's "Send Timeout" field does nothing there. Related to MF-28 | `SerialConnectionManager.java:49`; `iso9141/SSMProtocol.java:192-194`; `ramtune/test/RamTuneTestApp.java:481-485` |
| UL-SSM-08 | MF-82 | Unknown-length exchanges (init, every write, reset, all of RamTune) end only after `connect_timeout` (2000 ms) of silence, with no end-of-frame detection from `LL`. Slow, and any late byte extends it | `SerialConnectionManager.java:107-129` |
| UL-SSM-09 | MF-27 | After a known-length timeout `readTimeout` is not reset, so the next read on the same manager times out after one iteration unless the data is already there. In the logger this is hidden because a timeout rebuilds the connection | `SerialConnectionManager.java:76-86` |
| UL-SSM-10 | not a bug: legacy test data, never sent to an ECU | Legacy's canned test frames are not all valid exchanges, which matters only to anyone reusing them as fixtures. The TCU and `EP5D004L`/`EZ1E401G`/`JZ2F401A`/`JZ2F422A` init echoes carry checksum `40` (the right value is `48`), and the last four pair a `18` destination with replies from `10`. The fake ECU reads the `A0` count as a signed byte (counts of 129 or more break). Use only their reply halves | `io/serial/connection/TestSerialConnection2.java:61,66-69,111,176`; `io/serial/connection/TestSerialConnection.java:58` |
| UL-SSM-11 | MF-83 | DTC range by init length: `if (len < 104) … else if (len < 56)`; the second branch can never run, so 40-byte-init ECUs get the `D488` range rather than `D256` | `logger/ecu/comms/readcodes/ReadCodesManagerImpl.java:69-74` |

Ledger entries this page cross-references: **MF-08** (stale frame shown as
live), **MF-27** (iteration-counted timeout), **MF-28** (55 ms P3min,
related), **MF-29** (static `SSMProtocol.module`). The table above adds
**MF-78** to **MF-83**, and files the uncompared echo under edge case
**EC-16**; sections 3.5 and 6 add **MF-85** (multi-byte writes skipped)
and **MF-84** (reply code not matched to the request).

## Fixtures to write

Stable IDs for US-RR-12-2. Each one becomes a KAT file (`fixtures/kat/`), a
simulator script (US-RR-36), or a capture (private tier `RR_FIXTURES_DIR`,
US-RR-33). Expected bytes come from `legacy/docs/ssm_info.txt` or are
hand-computed (D-024), never from running Java.

The first simulator fixtures written from this page (US-RR-12-4) are in
[`fixtures/public/ssm/`](../../fixtures/public/ssm/): init (ECU and TCU),
`A8` with one and three addresses and a continuous run, `A0`, `B8`, `B0`,
reset, and the error cases bad checksum, short reply, silent ECU, corrupt
echo and no echo. They cover part of LEG-SSM-05, -07 to -10, -12, -14 to -16;
`crates/rr-testkit/tests/ssm_fixtures.rs` checks them against the rules of
sections 1.4 and 2 to 4.

| ID | Kind | What it pins down | Source of truth |
|----|------|-------------------|-----------------|
| LEG-SSM-01 | KAT `fixtures/kat/ssm-checksum.toml` | sum8 over every frame in section 8, including all bytes ≥ `80` | section 8; ssm_info:20-21 |
| LEG-SSM-02 | KAT `fixtures/kat/ssm-encode.toml` | Request encoding: `BF` (ECU `10`, TCU `18`); `A8` once and continuous with 1, 2 and 84 addresses; `A0` with counts 1, 128, 254; `B8`; `B0`; reset | sections 3, 8 |
| LEG-SSM-03 | KAT `fixtures/kat/ssm-init-decode.toml` | Init reply → init data, ECU ID, data length for `LL` = 29, 39, 69 (ssm_info frame and the reply halves of `TestSerialConnection2`) | section 8.1 |
| LEG-SSM-04 | KAT `fixtures/kat/ssm-capability.toml` | `(ecubyteindex, ecubit)` → supported: bit 0 and bit 7, index = length − 1, index ≥ length → false, the BYTE 9 = `73` example | section 3.2 |
| LEG-SSM-05 | KAT `fixtures/kat/ssm-decode-reject.toml` | One corrupt reply per validation rule (length ≤ 6, header, tester, module, `LL`, unknown code, checksum), each with its typed error; plus a reply code that does not match the command (rr rejects it, legacy accepts it, MF-84) | section 4 |
| LEG-SSM-06 | KAT `fixtures/kat/ssm-limits.toml` | `A8` with 84 addresses encodes; 85 is an error (no wrap, MF-79); `A0` count 0 and 255 are errors (MF-79) | sections 3.3, 3.4 |
| LEG-SSM-07 | Sim script `ssm-init-echo` | KKL init exchange byte-exact with echo: TX 6, RX 6 + 62; ECU and TCU modules | sections 1.4, 3.2 |
| LEG-SSM-08 | Sim script `ssm-a8-single-shot` | A polling cycle with `PP = 00`: echo + reply, buffer size `(N+6)+(3N+7)`, fan-out to de-duplicated queries | section 3.3 |
| LEG-SSM-09 | Sim script `ssm-a8-continuous` | `PP = 01` stream; the host reads frame by frame; a line break stops the stream; stale bytes are drained before the next request | section 5 |
| LEG-SSM-10 | Sim fault `ssm-silent-ecu` | Echo only, then nothing: init fails with a typed timeout, no panic, retry policy per LinkProfile (legacy: retry every 1 s forever) | section 6 |
| LEG-SSM-11 | Sim fault `ssm-corrupt-midstream` | One bad checksum frame in continuous mode: that sample yields no value plus a link-health event, never the previous frame (MF-08) | section 6 |
| LEG-SSM-12 | Sim fault `ssm-partial-timeout` | Half a reply, then silence: error after the LinkProfile deadline on a VirtualClock (MF-27), no zero-filled frame | section 6 |
| LEG-SSM-13 | Sim fault `ssm-trailing-bytes` | Extra bytes after a reply, and bytes waiting before a request: drained, no misframing | section 6 |
| LEG-SSM-14 | Sim fault `ssm-no-echo` | An interface without echo: detected and reported as a link-profile mismatch, not misframed (EC-16) | section 1.4 |
| LEG-SSM-15 | Sim script `ssm-a0-block` | The ssm_info 128-byte `A0` read; RamTune-style range splitting with a remainder | sections 3.4, 8.1 |
| LEG-SSM-16 | Sim script `ssm-writes` (write-gated) | `B8` with matching and mismatching acknowledgement, a corrupt-checksum acknowledgement (rr rejects it, MF-80), reset `000060` ← `40`, `B0` with a data check | sections 3.5–3.7 |
| LEG-SSM-17 | Capture (US-RR-33, 2004 Liberty) | Init exchange as received, with timestamps: the echo, the first-reply latency (P2-like) and the inter-byte gap (P1-like) | sections 1.5, 3.2 |
| LEG-SSM-18 | Capture (US-RR-33) | `A8` single-shot cycles and a continuous run: the gap between streamed frames, and whether a break, a new command, or both stop the stream | section 5 |
| LEG-SSM-19 | Capture (US-RR-33, read-only) | The ECU's reaction to an `A8` of an unmapped address and to a request with a bad checksum: silence or a reply? This decides the NAK-less timeout policy | section 6 |
| LEG-SSM-20 | Doc + KAT `fixtures/kat/timing-ssm-kline.toml` | SSM K-line LinkProfile timings: measured values from LEG-SSM-17/18 next to the `iso14230_2` P1–P4 defaults, and the break length (legacy: 20 ms at 4800 8N1) | section 1.5 |
| LEG-SSM-21 | KAT `fixtures/kat/ssm-can.toml` (low priority) | CAN variant: `AA`/`EA` init, `A8`/`B8` framing with a 4-byte ID, `7F` negative responses `13` and `22`; `A0`/`B0` unsupported | section 7 |
