# Logger definitions: legacy semantics

What the Java RomRaider logger does with a logger definition file
(`logger.xml`): which file and which protocol section it reads, what every
element and attribute means, how a parameter's bytes become a value, how the
ECU's init response and ECU ID decide which parameters exist, and how derived
(calculated) parameters work. Written from the source only (the Java was
never run, D-024). It is the spec for the rr-defs logger-definition reader,
the rr-live Signal and derived-signal graph, and their known-answer fixtures.
Part of the legacy behaviour catalogue, story US-RR-12, task US-RR-12-9.

Other pages hold what this one links to rather than repeats:
[`ssm.md`](ssm.md) has the SSM wire protocol, the init response layout and the
capability-bit rule in byte detail (section 3.2);
[`encoding-scaling.md`](encoding-scaling.md) has the JEP dialects, `BitWise`
and `DecimalFormat` display formatting (sections 7 and 8);
[`definitions.md`](definitions.md) has the ECU (editor) definitions;
[`csv.md`](csv.md) has the log file; [`settings.md`](settings.md) has the
settings that choose the logger file and protocol. Unit strings are read by
`crates/rr-core/src/units/legacy.rs` (US-RR-21-5), which already lists every
`units` spelling of the logger corpus.

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses: `logger/ecu/definition/xml/LoggerDefinitionHandler.java:344`
  is `legacy/src/main/java/com/romraider/logger/ecu/definition/xml/LoggerDefinitionHandler.java`,
  line 344. `LoggerDefinitionHandler.java:N` is short for that file. Files
  elsewhere in `legacy/` carry the `legacy/` prefix
  (`legacy/definitions/logger.dtd:89`). Every reference is `file:line` or
  `file:first-last`.
- **Corpus references.** `logger_STD_EN_v290.xml:N` is line N of
  `RomRaider/logger/logger_STD_EN_v290.xml` in the pinned definition tree
  (TD-D/SubaruDefs `0b0c0c42`, D-028), the logger file public CI uses.
  `logger_STD_EN_v370.xml:N` is line N of the file of that name inside the
  privately held `logger_v370.zip` (D-028): cited for metadata and short
  examples only, never copied into the repository. Corpus counts in this page
  were taken for it by reading those files as data.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-LOG-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Names.** The logger reader is a SAX handler that compares element names
  with `String.equals`, so element and attribute names are **case-sensitive**
  here (`LoggerDefinitionHandler.java:184-309`), unlike the DOM readers of
  `definitions.md` and `settings.md`.

### Source files

| Alias | Path under `legacy/src/main/java/com/romraider/` | Role |
|-------|---------------------------------------------------|------|
| `LoggerDefinitionHandler.java` | `logger/ecu/definition/xml/` | The SAX handler: every element and attribute read, protocol filtering, capability and ECU-ID gating, derived-parameter resolution |
| `EcuDataLoaderImpl.java` | `logger/ecu/definition/` | Opens the file, runs the parser, the protocol/transport fallback |
| `EcuAddressImpl.java` | `logger/ecu/definition/` | `<address>` text to the list of byte addresses |
| `EcuParameterConvertorImpl.java`, `EcuSwitchConvertorImpl.java`, `EcuDtcConvertorImpl.java` | `logger/ecu/definition/` | Bytes to value, and value to text, for parameters, switches and trouble codes |
| `EcuDerivedParameterImpl.java`, `EcuDerivedParameterConvertorImpl.java` | `logger/ecu/definition/` | Derived parameters: combined address, `[ID:units]` references, evaluation |
| `EcuParameterImpl.java`, `EcuSwitchImpl.java`, `Module.java`, `Transport.java` | `logger/ecu/definition/` | The parsed objects; selected conversion; module and transport identity |
| `ConverterMaxMinDefaults.java` | `logger/ecu/definition/xml/` | Gauge range defaults by unit string |
| `EcuDefinitionHandler.java` | `logger/ecu/definition/xml/` | The logger's read of the editor's ECU definitions: ECU ID to CAL ID and car |
| `ExternalDataImpl.java`, `logger/external/core/ExternalDataSourceLoaderImpl.java` | `logger/` | External sensors (plugins) as logger data |
| `EcuLogger.java` | `logger/ecu/` | When definitions are loaded and reloaded, module selection, the file-logging switch |
| `util/SaxParserFactory.java`, `util/HexUtil.java`, `util/ByteUtil.java` | `util/` | Parser settings, hex parsing, byte-to-int |
| `legacy/definitions/logger.dtd` | (outside `src/`) | The DTD, which describes most but not all of what the handler reads (section 2.3) |
| `legacy/version.properties` | (outside `src/`) | The forum URL for logger definitions, the declared minimum definition version |

## 1. Which file, which protocol

### 1.1 The file

The logger reads exactly **one** definition file, the path saved in
`settings.xml` as `<logger><definition path="...">`
(`EcuLogger.java:571-582`; [`settings.md`](settings.md) section 4.5). The
user sets it from the menu (`logger/ecu/ui/swing/menubar/action/LoggerDefinitionLocationAction.java:46-58`);
nothing searches for a file. If the path is empty, a dialog offers to open
the forum page that hosts the files, and the logger runs with external sensors
only (`EcuLogger.java:573-577,643-662`; URL `loggerdefsurl`,
`legacy/version.properties:17`).

The forum ships one file per unit system and language,
`logger_{STD,METRIC,IMP}_{EN,DE}_vNNN.xml`. The unit system is only the
**order of each parameter's conversions**: the first `<conversion>` is the
one shown until the user picks another (section 4.4). Coolant temperature
lists `F` then `C` in the STD file and `C` then `F` in the METRIC file, at
the same lines (`logger_STD_EN_v290.xml:48-49`; `logger_METRIC_EN_v290.xml:48-49`).
There is no unit-system setting in the logger.

Definitions are loaded at start-up (`EcuLogger.java:369`), again whenever an
init reports a **different ECU ID** (`EcuLogger.java:399-417`), and when the
user changes protocol or file (`logger/ecu/ui/swing/menubar/action/SelectProtocolAction.java:249-255`;
`LoggerDefinitionLocationAction.java:46`). The start-up load has no init
yet, so it has no ECU-specific parameters and no capability filtering
(sections 5 and 6); both appear after the first successful connection.

### 1.2 Parsing

The file is parsed with a **validating**, namespace-aware SAX parser with
XInclude on (`util/SaxParserFactory.java:33-38`;
`EcuDataLoaderImpl.java:98-102`). The handler does not override the SAX
`error` callback, so validity errors are ignored; only well-formedness errors
and a missing DTD stop the load. Validation therefore has two effects: the
DTD must be readable, and nothing else. The v290 files name an external
`logger.dtd` (`logger_STD_EN_v290.xml:2`), which must sit next to them; the
v370 files carry the DTD inline (`logger_STD_EN_v370.xml:30-131`).

### 1.3 The protocol section

The handler is built with the protocol name from settings (`SSM`, `OBD`,
`DS2`, `NCS`; `<logger><protocol name>`, default `SSM`)
(`EcuLogger.java:581-582`; `LoggerDefinitionHandler.java:167-173`). For each
`<protocol id>` it sets `parseProtocol` to a **case-insensitive** match
(`LoggerDefinitionHandler.java:186-188`). Only the matching section's
parameters, switches, trouble codes and ECU-specific parameters are read,
and character data is buffered only inside it (`LoggerDefinitionHandler.java:224,313-317`).

Transports and modules are read for **every** protocol, inside or outside the
match, into a map protocol → transport → modules
(`LoggerDefinitionHandler.java:204-223,320-324`). After parsing, the loader
checks that the settings' protocol is in that map and has a transport whose
`id` equals the settings' transport (ignoring case)
(`EcuDataLoaderImpl.java:112,143-163`). If not, it switches the settings to
the **first protocol and first transport of a `HashMap`**, then parses the
file again (`EcuDataLoaderImpl.java:114-118,137-140`). `HashMap` order is
not defined, so which protocol a user lands on is arbitrary (MF-67).

**The v290 files cannot be loaded by this version of legacy.** They have no
`<transports>` at all (none in `logger_STD_EN_v290.xml`), so every protocol
maps to an empty transport map, the check fails, and
`keySet().iterator().next()` on the empty map throws; the exception becomes a
`ConfigurationException` and an error dialog (`EcuDataLoaderImpl.java:114-117,130-132`;
`EcuLogger.java:621-624`; MF-67). Legacy declares 370 as its minimum definition
version but never checks it (`legacy/version.properties:27`;
`legacy/src/main/java/com/romraider/Version.java.template:37`). A commented-out
block once supplied the default SSM module (`ecu`, address `10`, tester `F0`,
fast poll on, transport `iso9141`) for files without transports
(`LoggerDefinitionHandler.java:440-451`). Public CI has only v290 (D-028), so
the port must load it: see section 12.

## 2. Element reference

### 2.1 Structure

```
logger [version]
└─ protocols
   └─ protocol id baud databits stopbits parity connect_timeout send_timeout [p1_max p3_min p4_min]
      ├─ transports
      │  └─ transport id name desc
      │     └─ module id address desc [tester] [fastpoll]
      ├─ parameters
      │  └─ parameter id name desc target [ecubyteindex ecubit] [group subgroup groupsize]
      │     ├─ address [length] [bit]   (one or more)   ── or ──   depends / ref parameter (derived)
      │     └─ conversions / conversion units expr format [storagetype] [endian] [gauge_min gauge_max gauge_step]
      │                                 └─ replace value with
      ├─ switches
      │  └─ switch id name desc byte bit target [ecubyteindex] [storagetype units] [group subgroup groupsize]
      ├─ dtcodes
      │  └─ dtcode id name desc tmpaddr memaddr bit
      └─ ecuparams
         └─ ecuparam id name desc target [group subgroup groupsize]
            ├─ ecu id                (one or more; comma-separated ECU IDs)
            │  └─ address [length] [bit]
            └─ conversions / conversion ...
```

Order matters: the handler resolves a derived parameter's references against
what it has already read (section 7), and the DTD puts `parameters` before
`switches`, `dtcodes` and `ecuparams` (`legacy/definitions/logger.dtd:89`).

### 2.2 Elements and attributes

| Element / attribute | Read as | Meaning | Source |
|---------------------|---------|---------|--------|
| `logger version` | string | Shown in the load message as "def version" (`na` if absent); never compared | `LoggerDefinitionHandler.java:184-185`; `EcuLogger.java:606-619` |
| `protocol id` | string, compared ignoring case | Selects the section (1.3) | `LoggerDefinitionHandler.java:186-188` |
| `protocol baud databits stopbits parity connect_timeout send_timeout` | `Integer.parseInt`, all required | Serial settings of the link; how SSM uses them is in `ssm.md` sections 1.1 and 1.5 | `LoggerDefinitionHandler.java:197-202`; `legacy/definitions/logger.dtd:90-96` |
| `protocol p1_max p3_min p4_min` | `Integer.parseInt`, only when `id` is `NCS` (any case) | ISO 14230 timing for NCS (MF-28). Not in the DTD | `LoggerDefinitionHandler.java:190-195` |
| `transport id name desc` | strings; `id` upper-cased | A physical transport (`iso9141`, `iso14230`, `iso15765`) | `LoggerDefinitionHandler.java:205-210`; `logger/ecu/definition/Transport.java:33` |
| `module id address desc tester fastpoll` | `address`, `tester`: hex via `HexUtil.asBytes` (spaces and `0x` removed; absent → empty); `fastpoll`: `"true"` ignoring case, else false | One addressable unit. `id` is the module's name (`ecu`, `tcu`) that the settings' target module selects | `LoggerDefinitionHandler.java:211-223`; `util/HexUtil.java:51-60`; `logger/ecu/definition/Module.java:32-45`; `EcuLogger.java:1588-1590,1616-1635` |
| `parameter id name desc` | strings | Identity and labels. `name` is the CSV column name (csv.md) | `LoggerDefinitionHandler.java:226-228` |
| `parameter ecubyteindex ecubit` | strings, parsed only when tested | Capability bit (section 5) | `LoggerDefinitionHandler.java:229-230,344-345` |
| `parameter group subgroup groupsize` | lower-cased strings; `groupsize` `Integer.parseInt`, absent → 0 | DS2 request grouping only | `EcuParameterImpl.java:55-57`; `io/protocol/ds2/iso9141/DS2LoggerProtocol.java:154,198` |
| `parameter target`, `switch target`, `ecuparam target` | string | **Parsed and never used** (MF-71). The corpus uses 1 (ECU), 2 (TCU) and 3 (both); legacy offers every parameter whatever module is selected | `LoggerDefinitionHandler.java:234,281,291` (no other reader) |
| `address` text | hex string, `0x` optional | Start address; its **string width** sets the byte width on the wire (section 3) | `LoggerDefinitionHandler.java:326-327`; `EcuAddressImpl.java:76-122` |
| `address length` | `Integer.valueOf`, absent → 1 | Number of consecutive bytes | `LoggerDefinitionHandler.java:237-238` |
| `address bit` | `Integer.valueOf`, absent → -1 | Read one bit (0–31) of the value instead of the value (4.2) | `LoggerDefinitionHandler.java:239-240` |
| `depends` / `ref parameter` | string IDs, de-duplicated in order | Makes the parameter derived (section 7) | `LoggerDefinitionHandler.java:242-246` |
| `conversion units expr format` | strings, all required (`IllegalArgumentException` if empty) | Unit label, JEP expression in `x`, `DecimalFormat` pattern | `LoggerDefinitionHandler.java:247-250`; `EcuParameterConvertorImpl.java:58-64` |
| `conversion storagetype` | string, absent → `uint8` | Signed unless it starts with `uint` (ignoring case); `float` reads IEEE single. The width is **not** taken from it (4.1) | `LoggerDefinitionHandler.java:251`; `EcuParameterConvertorImpl.java:66,82-110` |
| `conversion endian` | `little` (ignoring case) → little, anything else or absent → big | Byte order of the value | `LoggerDefinitionHandler.java:252-258` |
| `conversion gauge_min gauge_max gauge_step` | `Double.parseDouble`; absent or empty → default by unit (4.5) | Dashboard gauge range | `LoggerDefinitionHandler.java:259-262,474-490` |
| `replace value with` | strings | Text substitution of the formatted value (4.3) | `LoggerDefinitionHandler.java:268-269` |
| `switch byte bit` | `byte` hex string; `bit` `Integer.valueOf`, required | One bit of one byte | `LoggerDefinitionHandler.java:282` |
| `switch ecubyteindex` | string | Capability byte; the capability **bit is the switch's own `bit`** | `LoggerDefinitionHandler.java:274-275` |
| `switch storagetype units` | strings; defaults `uint8`, `On/Off` | Units label of the switch; storage type is carried but not used to decode | `LoggerDefinitionHandler.java:279-280`; `EcuSwitchConvertorImpl.java:32-37` |
| `dtcode tmpaddr memaddr bit` | hex strings; `bit` `Integer.valueOf` | Trouble code: "temporary" and "memorised" flag bytes (4.6) | `LoggerDefinitionHandler.java:297-307` |
| `ecuparam id name desc group subgroup groupsize` | as `parameter` | ECU-specific parameter (section 6) | `LoggerDefinitionHandler.java:284-294` |
| `ecu id` | string, split on `,` with no trimming | ECU IDs that use the enclosed address | `LoggerDefinitionHandler.java:295-296,378-384` |

### 2.3 Where the DTD and the handler disagree

- In the DTD but never read: `parameters mode` (`legacy/definitions/logger.dtd:41`);
  `ref ecuparam` (`legacy/definitions/logger.dtd:11`). A `<ref ecuparam="E1"/>`
  adds a `null` dependency that never resolves, so its derived parameter is
  dropped (`LoggerDefinitionHandler.java:246,331-336`).
- Read but not in the DTD: `protocol p1_max p3_min p4_min`; `parameter group
  subgroup groupsize`; `switch storagetype units group subgroup groupsize`
  (compare `LoggerDefinitionHandler.java:190-195,231-233,276-280` with
  `legacy/definitions/logger.dtd:32-50,89-96`). Validity errors are ignored
  (1.2), so these load.
- The DTD declares `id` as `ID` (unique in the document), but the same
  parameter ID appears in several protocol sections, and inside one section
  of v370: 99 IDs repeat as DS2 `<ecuparam>`s with disjoint ECU lists, and
  NCS has `P31` twice (`logger_STD_EN_v370.xml:39253,39333`). Legacy keeps
  both objects in its list, and the ID map keeps the last
  (`LoggerDefinitionHandler.java:352-353,375-376`).

## 3. Addresses

`EcuAddressImpl` turns the `<address>` texts of one element into a list of
byte addresses (`EcuAddressImpl.java:45-51,76-95`):

- **One `<address>`, `length` n**: n consecutive addresses from the start,
  each re-padded to the start string's width. RPM is `<address
  length="2">0x00000E</address>`, so `0x00000E`, `0x00000F`
  (`logger_STD_EN_v290.xml:87-88`).
- **More `<address>` elements than `length`**: each address is used as
  written, in order, and `length` is ignored. Steering angle reads `0x00008D`
  then `0x00015A` as one `int16` (`logger_STD_EN_v290.xml:991-997`).
- **Several `<address>` elements but no more than `length`**: only the
  **first** is used and expanded by `length`; the others are dropped silently
  (MF-70).
- `length` and `bit` come from the **last** `<address>` of the element
  (`LoggerDefinitionHandler.java:236-241`). The address object is built when
  the first `<conversion>` starts (`LoggerDefinitionHandler.java:264-267`).
- Byte width on the wire is the hex digit count of the string
  (`EcuAddressImpl.java:97-122`; `util/HexUtil.java:51-60`): `0x000007` is 3
  bytes (SSM), `0x0000DA2A` is 4 (DS2, `logger_STD_EN_v370.xml:36302-36303`).
  How SSM sends them is in `ssm.md` section 3.3.
- A derived parameter's address is the concatenation of its dependencies'
  addresses (section 7.2).

## 4. Decoding a value

### 4.1 Parameters

`EcuParameterConvertorImpl.convert(bytes)` gets the bytes read for the
parameter's addresses, in address order (`EcuParameterConvertorImpl.java:72-114`):

1. Byte order: little if the conversion says `endian="little"`, else big
   (`:73-76`). This is per conversion, so two conversions of one parameter
   could read its bytes in different orders.
2. **Bit**: if the address has `bit` 0–31, the value is 1 if that bit of the
   bytes read as one **big-endian** unsigned integer is set, else 0; no
   expression is applied, and `endian` is ignored (`:79-81`;
   `util/ByteUtil.java:49-58`).
3. **Float**: `storagetype="float"` reads a 4-byte IEEE single in the chosen
   order and passes it to the expression (`:82-83`). Fewer than 4 bytes throw.
4. **Integer**: the width is the **number of bytes read**, not the storage
   type: 1, 2 or 4 bytes read as signed, then masked to unsigned if the
   storage type starts with `uint` (`:86-110`). Any other byte count (3, or
   more than 4) gives **0** (the logger's version of MF-02). So
   `storagetype="int8"` on a 2-byte address reads a signed 16-bit value.
   Absent storage type means `uint8`, so unsigned.
5. The expression is evaluated with JEP's single-variable entry point, with
   `x` bound to the value: the editor's dialect, with the standard functions
   and `BitWise` (`:83,111`; [`encoding-scaling.md`](encoding-scaling.md)
   sections 7.2 and 7.3; the shared cache is MF-25).
6. A NaN or infinite result becomes **0.0** (`:113`): an invalid value is
   shown and logged as a real zero (same family as MF-09).

Contrast with the editor's codec in `encoding-scaling.md` section 2.4: no
float hack, no static endian.

### 4.2 Bit-flag parameters

A parameter or ECU-specific parameter whose `<address>` has a `bit` decodes
to 0 or 1 and then is **formatted** through its conversion's `format` and
`replace` (4.3), but its `expr` is never evaluated. The MerpMod parameters
use this (`logger_STD_EN_v290.xml:34334-34340`). See MF-68 for how an
`<ecuparam>` picks its bit.

### 4.3 Value to text

`format(value)` is `new DecimalFormat(format).format(value)`, then, if that
text is a key of the conversion's `<replace>` map, the replacement text
(`EcuParameterConvertorImpl.java:64,128-135`). The `DecimalFormat` is built
with the **default locale** when the definition is loaded, and rounds
`HALF_EVEN` ([`encoding-scaling.md`](encoding-scaling.md) section 8). So:

- Engine speed (`x/4`, format `0`, `logger_STD_EN_v290.xml:90`) shows raw
  `0BBA` (3002, 750.5) as `750` and raw `0BBE` (3006, 751.5) as `752`.
- In a decimal-comma locale the text is `14,36`, and a `<replace
  value="0.5">` can never match (the text is `0,5`; locale-formatted text,
  as MF-30). Neither the v290 nor the
  v370 STD_EN file uses `<replace>`.

The DTD restricts `format` to `0`, `0.0` … `0.000000`
(`legacy/definitions/logger.dtd:22`), but any `DecimalFormat` pattern is
accepted (validation is ignored, 1.2). The CSV log writes this text
([`csv.md`](csv.md) section 6.3).

### 4.4 Which conversion is used

Every parameter keeps all its conversions; one is **selected**, index 0
(the first in the file) unless changed (`EcuParameterImpl.java:39,96-117`).
The user's choice is saved per parameter in the logger profile as a `units`
string (`logger/ecu/profile/xml/UserProfileHandler.java:39`;
`legacy/definitions/profile.dtd:9-14`), not in the definition or in
`settings.xml`. A parameter with no `<conversion>` gets a default one: units
`Raw data`, expression `x`, format `0`, storage type `uint` (unsigned, width
from the bytes read), big endian
(`LoggerDefinitionHandler.java:346-348,368-370`;
`EcuParameterConvertorImpl.java:49-52`). Changing the selection notifies the
CSV writer, which rewrites the header ([`csv.md`](csv.md) section 9).

### 4.5 Unit strings and gauge defaults

The `units` text is free text, shown as-is (CSV header, gauges;
`logger/ecu/ui/handler/file/FileUpdateHandlerImpl.java:194-195`). Which
registry unit each corpus spelling means is `crates/rr-core/src/units/legacy.rs`
(US-RR-21-5), which covers every `units` attribute of the pinned and v370
logger files.

When a conversion has no `gauge_min`, `gauge_max` or `gauge_step`, the
default comes from a table keyed by the **lower-cased** unit string
(`ConverterMaxMinDefaults.java:33-80`), else 0, 100, 10
(`ConverterMaxMinDefaults.java:28-31`). `add` lower-cases the key as `get`
does, so the mixed-case keys `MPa` and `mmHg` are found like the others
(`ConverterMaxMinDefaults.java:54,61,64-67,93-96`). (The module docs of
`crates/rr-core/src/units/legacy.rs` say those two keys were never found;
reading `add` shows they are, so that note should be corrected.) Nothing checks the values: NCS coolant
temperature has `gauge_min="120" gauge_max="-20"`
(`logger_STD_EN_v370.xml:39236`).

### 4.6 Switches and trouble codes

- **Switch**: 1 if bit `bit` of the first byte is set, else 0; formatted as
  `1` or `0` (`EcuSwitchConvertorImpl.java:39-41,55-58`). `bit` must be 0–7,
  else `IllegalArgumentException` and the whole load fails
  (`EcuSwitchConvertorImpl.java:33`; `util/ParamChecker.java:78-98`; MF-87).
- **Trouble code**: two bytes, `tmpaddr` then `memaddr`; the value is 1 if
  the bit is set in the temporary byte, plus 2 if set in the memorised byte;
  both bytes `FF` means **-1** (not supported)
  (`EcuDtcConvertorImpl.java:36-46`). Formatted `1`/`0` like a switch, so -1
  prints as `0` (`EcuDtcConvertorImpl.java:61-64`). Example:
  `logger_STD_EN_v290.xml:1878`.

## 5. Capability gating (init response)

A `<parameter>` with both `ecubyteindex` and `ecubit`, or a `<switch>` with
`ecubyteindex`, is kept only if the ECU's init response says it is
supported: `index < initData.length && (initData[index] & (1 << bit)) != 0`,
with the switch using its own `bit` (`LoggerDefinitionHandler.java:344-345,386-387,462-472`).
`ecubyteindex` is a 0-based index into the init data after the response
header; the byte layout, the bit order and worked examples are in
[`ssm.md`](ssm.md) section 3.2. Further rules:

- No init yet (start-up, or external-sensor-only mode): **everything is
  kept** (`LoggerDefinitionHandler.java:344`; `EcuLogger.java:582`).
- An index past the end of the init data means **not supported**
  (`LoggerDefinitionHandler.java:465-471`).
- A non-numeric `ecubyteindex` or `ecubit` throws and fails the whole load
  (`LoggerDefinitionHandler.java:464,468`; MF-87).
- Derived parameters and `<ecuparam>`s have no capability bit; derived ones
  inherit gating through their dependencies (section 7.3), ECU-specific ones
  are gated by ECU ID (section 6).
- The same rule applies to NCS, whose init data is longer
  (`logger_STD_EN_v370.xml:39233`, `ecubyteindex="40"`). The largest index in
  the v290 SSM section is 76.

## 6. ECU-specific parameters (`<ecuparam>`)

Each `<ecu id="A,B,C">` maps every listed ECU ID to the address inside it
(`LoggerDefinitionHandler.java:378-384`). At `</ecuparam>`, the parameter is
created only if there is an init **and** its ECU ID is in the map, using
that ECU's address (`LoggerDefinitionHandler.java:366-377`). The ECU ID is
the init's 5 ID bytes as 10 upper-case hex digits (`ssm.md` section 3.2).
Example: `E1` IAM with six address variants (`logger_STD_EN_v290.xml:2469-2492`).
The v290 STD_EN SSM section has 204 `<ecuparam>`s.

- **Exact string match, no trimming.** `ecuIds.split(",")` keeps spaces, so
  `<ecu id="2A04446305, 2A04446405">` (`logger_STD_EN_v290.xml:3400`) maps
  `" 2A04446405"`, which no init ever matches (MF-68).
- **Conversions use the last `<ecu>`'s bit.** The conversion objects are
  built once per `<ecuparam>`, and take `address.getBit()` from whatever
  `<ecu>` was read last (`LoggerDefinitionHandler.java:356-365,378-380`). The
  address itself is per ECU, but the bit used to decode is not. Four v290
  parameters mix bits across ECUs: `E1400` has bit 7 for some ECUs and bit 0
  for others (`0xFF659F`, bit 0), and its last `<ecu>` has bit 7
  (`logger_STD_EN_v290.xml:34334-34428`), so legacy reads bit 7 at the bit-0
  addresses (MF-68).
- An ECU ID listed in two `<ecu>` elements of one parameter takes the later
  address (`HashMap.put`, `LoggerDefinitionHandler.java:382`).
- DS2 uses `<ecuparam>`s with `group`/`subgroup` and 7-digit ECU IDs
  (`logger_STD_EN_v370.xml:36301-36307`), and repeats IDs across disjoint ECU
  lists (2.3).
- The editor's ECU definitions are also read by the logger, only to show the
  CAL ID and car for the ECU ID (`EcuLogger.java:419-431,541-569`;
  `EcuDefinitionHandler.java:124-161`; requires `ecuid`, `internalidstring`,
  `year`, `market`, `make`, `model`, `submodel`, `transmission`).

## 7. Derived parameters

A `<parameter>` with `<depends>` instead of `<address>` is derived: its
value is computed from other parameters (`LoggerDefinitionHandler.java:236-244,329-343`).

### 7.1 Resolution

At `</parameter>`, each `ref` ID is looked up in the map of data read **so
far** in this protocol section (parameters, switches, trouble codes and
ECU-specific parameters are all added to it as they are created)
(`LoggerDefinitionHandler.java:329-342`; map filled at `:353,376,397,410`).
If **any** reference is missing, the parameter is dropped with no message
(**MF-24**). In practice a reference is missing when:

- it names a parameter defined **later** in the file;
- it names a parameter removed by capability gating (section 5), so derived
  parameters follow their dependencies' support;
- it names an `<ecuparam>`, a switch or a trouble code, because those
  sections come after `<parameters>` (`legacy/definitions/logger.dtd:89`);
- it names an external sensor, which is never in the map (**MF-24**);
- it uses `ref ecuparam=` (2.3).

The `<depends>` flag is only reset by a later `<address>` or `<ecuparam>`
(`LoggerDefinitionHandler.java:241,244,294`), so the handler's state relies
on the DTD's content model.

### 7.2 Bytes

The resolved dependencies are kept in a `HashSet`, so their order is
arbitrary but fixed for the run (`LoggerDefinitionHandler.java:330-338`). The
derived parameter's address is all their addresses concatenated in that order
(`EcuDerivedParameterImpl.java:132-142`), and `convert(bytes)` slices the
response back into per-dependency byte runs in the same order and runs each
dependency's own conversion on its slice
(`EcuDerivedParameterConvertorImpl.java:53-65`). A dependency can itself be
derived (its address is already a concatenation).

### 7.3 The expression

Each `<conversion>` of a derived parameter has an expression over parameter
IDs (`EcuDerivedParameterConvertorImpl.java:40-51,107-126`):

- **`[ID:units]`** uses dependency `ID` converted with **its conversion whose
  `units` equals the text exactly**. At evaluation the text `[ID:units]` is
  replaced by the variable `_ID_units_`, with every non-word character of the
  units changed to `_` (`EcuDerivedParameterConvertorImpl.java:63,128-130,156-163`).
  `[P7:psi]-[P24:psi]` becomes `_P7_psi_-_P24_psi_`
  (`logger_STD_EN_v290.xml:1362`); `[P229:µs]` becomes `_P229__s_`
  (Java's `\w` is ASCII only).
- **Bare `ID`** uses the dependency's **currently selected** conversion
  (`EcuDerivedParameterConvertorImpl.java:123-124`), so the derived value
  changes when the user switches that dependency's units. `(P12*60)/P8`
  (`logger_STD_EN_v290.xml:1338-1346`) is safe only because P12 and P8 have
  one conversion each. Checked for this page across all six v290 files and
  the STD_EN v370 file: every bare reference is to a single-conversion
  dependency, every `[ID:units]` names a conversion that exists, and no
  expression names two units of one dependency.
- Only the **first** `[ID:` of each dependency is looked at
  (`indexOf`, `EcuDerivedParameterConvertorImpl.java:110`). A second
  `[ID:other]` in the same expression is never rewritten, so JEP fails to
  parse it and the value is NaN, then 0.0 (MF-69).
- An `[ID:units]` whose units the dependency does not have leaves the
  convertor `null`, and the `checkNotNull` in `ExpressionInfo` throws while
  the definition is still being parsed: the **whole file** fails to load
  (`EcuDerivedParameterConvertorImpl.java:115-122,137-141`; MF-69).
- The formula is evaluated with JEP's **map** entry point: no standard
  functions, no `BitWise`, one variable per dependency key
  (`EcuDerivedParameterConvertorImpl.java:66`; `encoding-scaling.md`
  section 7.2). The string replacement runs on every sample (**MF-25**).
- NaN or infinite becomes 0.0 (`EcuDerivedParameterConvertorImpl.java:67`).
- Derived parameters have no group, subgroup or group size
  (`EcuDerivedParameterImpl.java:117-130`).

Example, injector duty cycle `(P8*[P21:ms])/1200`
(`logger_STD_EN_v290.xml:1347-1355`): with P8 = 3000 rpm and P21 = 10 ms,
3000 × 10 / 1200 = 25.00 %.

## 8. Switches as controls: the file-logging switch

Settings name one switch, default `S20` (rear defogger,
`logger_STD_EN_v290.xml:1723`), as the file-logging controller
(`Settings.java:216`; [`settings.md`](settings.md) section 4.5). When the
handler creates a switch with that ID (ignoring case) it keeps a second copy
for the controller (`LoggerDefinitionHandler.java:398-402`). If the setting
is active and the module is the ECU, the switch is polled every cycle and
each change of its value starts or stops the CSV log
(`EcuLogger.java:590-598,706-724`; `logger/ecu/comms/manager/QueryManagerImpl.java:370-376,393-395`).
The switch is part of the polled set, so it shares the ECU's request; see
[`csv.md`](csv.md) section 1.

## 9. External sensors

Wideband and other external sensors are **not** in the logger definition.
Each `./plugins/*.plugin` file (relative to the working directory) is a Java
properties file naming a `datasource.class` on the application's own class
path (`logger/external/core/ExternalDataSourceLoaderImpl.java:39-76`;
`legacy/plugins/aem2.plugin:1`). The class supplies its own items, units and
conversions. Each item becomes logger data with ID `"X_"` + its name with
spaces as underscores (`ExternalDataImpl.java:98-100`; **MF-33**). Serial
ports come from `settings.xml` (`<plugins><plugin id port>`,
[`settings.md`](settings.md) section 4.5). External data has no address and
cannot be a derived dependency (**MF-24**); its values carry no timestamp
(**MF-26**), and selecting one opens its port (**MF-32**).

## 10. Other consumers that fix parts of the schema

- **Hard-coded parameter IDs.** The MAF, injector and dyno tabs look up
  `P2`, `P3`, `P4`, `P8`, `P11`, `P12`, `P18`, `P58`, `E3`, `E33`, `E23`,
  `E54` by ID (`logger/ecu/ui/tab/maf/MafControlPanel.java:74-85`;
  `logger/ecu/ui/tab/injector/InjectorControlPanel.java:77-84`; **MF-31**). These
  SSM IDs are therefore part of what a "compatible" definition means today.
- **Dyno car file.** `cars_def.xml` is looked for next to the logger
  definition file, then next to the profile, then in the working directory
  (`logger/ecu/ui/tab/dyno/DynoControlPanel.java:105,1560-1575`).
- **Learning-table readers** and DTC reading use the parsed parameters and
  `dtcodes` list (`EcuLogger.java:587`).

## 11. Error handling

| Condition | Legacy | Source |
|-----------|--------|--------|
| File missing, unreadable, not well-formed, or DTD missing | `ConfigurationException`, error dialog, no parameters | `EcuDataLoaderImpl.java:97-132`; `EcuLogger.java:621-624` |
| Any bad number or missing required value **in the selected section** (`baud`, `length`, `bit`, `ecubyteindex`, a module's hex address, an empty `units`/`expr`/`format`) | The exception leaves the SAX handler: the **whole file** fails as above (MF-87) | `LoggerDefinitionHandler.java:191-202,238-240,282,464`; `EcuParameterConvertorImpl.java:58-60` |
| Settings' protocol or transport not in the file | Settings switched to an arbitrary protocol and transport, file re-parsed (MF-67) | `EcuDataLoaderImpl.java:112-118,137-140` |
| No `<transports>` (v290) | Load fails (1.3, MF-67) | `EcuDataLoaderImpl.java:114-117` |
| Derived parameter with a missing dependency | Dropped silently (**MF-24**) | `LoggerDefinitionHandler.java:336` |
| `[ID:units]` naming a missing unit | Whole file fails (MF-69) | `EcuDerivedParameterConvertorImpl.java:115-122,137-141` |
| ECU ID not in any `<ecu>` | Parameter absent, no message (EC-20) | `LoggerDefinitionHandler.java:367` |
| Capability bit clear | Parameter absent, no message | `LoggerDefinitionHandler.java:344-345` |
| Duplicate ID in one section | Both kept in the list; the ID map keeps the last | `LoggerDefinitionHandler.java:352-353` |
| 3-byte or >4-byte integer value | Decodes as 0 (MF-02) | `EcuParameterConvertorImpl.java:87-97` |
| NaN or infinite result | Shown and logged as 0 (MF-09) | `EcuParameterConvertorImpl.java:113`; `EcuDerivedParameterConvertorImpl.java:67` |

## 12. Mapping to the port

Pointers, not decisions.

- **Reader.** rr-defs reads the logger XML into a typed definition per
  protocol, keeping every protocol (not just the selected one), with
  Diagnostics instead of a whole-file failure for one bad parameter
  (section 11). Element names stay case-sensitive as in legacy.
- **v290 and v370 both load.** A protocol with no `<transports>` gets the
  documented default module set (SSM: `ecu` `10`/`F0` on K-line,
  `LoggerDefinitionHandler.java:440-451`) with an Info Diagnostic, so the
  public-CI v290 files work. No `HashMap`-order fallback: an unknown protocol
  or transport is a Diagnostic and the user chooses (MF-67).
- **Addresses.** Byte width from the hex string as legacy, but an `<address>`
  list that `length` would truncate is a Diagnostic (MF-70); `bit` is part
  of each ECU's address, not of the conversion (MF-68); ECU ID lists are
  trimmed (MF-68).
- **Decoding** goes through rr-core Encoding: the storage width comes from
  `storagetype` when given, checked against the address length; 3-byte values
  decode (MF-02); NaN/∞ is `None`, never 0 (MF-09).
- **Derived signals** form a graph over Signals resolved after the whole file
  is read, so order does not matter, external sensors can be inputs, and a
  missing input is a Diagnostic (MF-24, EC-25). Expressions are parsed once
  into the Expr AST with one dialect (MF-25). `[ID:units]` resolves to a
  conversion at load (missing unit: Diagnostic for that signal only), and a
  bare `ID` is fixed to a definite conversion at load (the first, or the
  unit-registry conversion of the dependency's canonical unit), never the
  UI's current choice.
- **Units** go through `rr_core::units::legacy` (US-RR-21-5); gauge defaults
  come from the unit registry, and an inverted `gauge_min`/`gauge_max` is a
  Warning.
- **Capability gating and ECU-ID selection** are rules of the profile layer
  (C2 Pattern / C13 Roles): unknown ECU ID gives only the base set (EC-20).
- **`target`** is mapped to the module (1 ECU, 2 TCU, 3 both) and used to
  filter, which legacy never did; the hard-coded tab IDs become Roles
  (MF-31).

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-LOG-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID.

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-LOG-01 | MF-67 | When the settings' protocol or transport is not in the file, legacy switches to the first entry of a `HashMap` (undefined order) and re-parses, silently changing the user's protocol; a file whose protocols have no transports throws instead | `logger/ecu/definition/EcuDataLoaderImpl.java:112-118,137-140,143-163` |
| UL-LOG-02 | MF-68 | ECU ID lists are split on `,` without trimming, so an ID after `", "` never matches and that ECU silently loses the parameter; the v290 corpus has one (`E31`) | `LoggerDefinitionHandler.java:381-383`; `logger_STD_EN_v290.xml:3400` |
| UL-LOG-03 | MF-68 | An `<ecuparam>`'s conversions take the bit of the **last** `<ecu>`, so ECUs listed earlier with a different `bit` decode the wrong bit; four v290 MerpMod parameters (`E1400`–`E1403`) mix bits 0, 1, 2, 3 and 7 | `LoggerDefinitionHandler.java:356-365,378-380`; `logger_STD_EN_v290.xml:34334-34686` |
| UL-LOG-04 | MF-69 | Derived-expression references: only the first `[ID:` of a dependency is rewritten (a second unit of the same dependency evaluates to NaN, logged as 0), and a reference to a unit the dependency lacks throws during parsing, so one typo makes the whole logger definition fail to load | `logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:107-126,137-141` |
| UL-LOG-05 | MF-70 | A parameter with several `<address>` elements but `length` ≥ their count keeps only the first address (expanded by `length`) and drops the rest silently | `logger/ecu/definition/EcuAddressImpl.java:76-95` |
| UL-LOG-06 | MF-71 | The `target` attribute (ECU, TCU, both) is parsed and never used, so TCU parameters are offered and polled against the ECU and vice versa | `LoggerDefinitionHandler.java:234,281,291` |

Ledger entries this page cross-references: **MF-02** (unsupported widths read
0), **MF-09** (missing or invalid values become 0), **MF-24** (derived
parameters: missing dependencies vanish, no external inputs), **MF-25** (two
JEP dialects, per-sample `String.replace`), **MF-26**, **MF-32**, **MF-33**
(external sensors), **MF-28** (NCS timing), **MF-31** (hard-coded IDs); edge
cases **EC-20** (unknown ECU ID) and **EC-25** (derived input missing). The
table above adds **MF-67** to **MF-71**; sections 4.6, 5 and 11 add
**MF-87** (one bad value fails the whole file).

## Fixtures to write

Stable IDs for US-RR-12-2. Each becomes a logger-definition reader KAT
(proposed `fixtures/defs-kat/logger/*.toml`: a minimal hand-written logger
XML and the typed result it must give, like the existing
`fixtures/defs-kat/romraider/` files), a byte KAT under `fixtures/kat/`, or a
test where noted. Expected values are hand-computed from this page (D-024),
never from running Java. Where a fixture pins a legacy bug, the expected
result is the port's fixed behaviour and the legacy result goes in its
`notes`.

| ID | Kind | What it pins down | Section |
|----|------|-------------------|---------|
| LEG-LOG-01 | defs-kat `logger/structure.toml` | One file with SSM and OBD sections: both protocols, their transports and modules (hex `address`/`tester`, `fastpoll`), serial attributes, `version`; element names case-sensitive | 1.3, 2.2 |
| LEG-LOG-02 | defs-kat `logger/no-transports.toml` + corpus test over the pinned v290 files | A protocol without `<transports>` loads with the default SSM module and an Info Diagnostic (legacy fails to load); every v290 file of the pinned tree loads | 1.3 |
| LEG-LOG-03 | defs-kat `logger/addresses.toml` | `length="2"` expansion with padding; two `<address>` elements without `length` (the steering-angle shape); two with `length="2"` (Diagnostic, MF-70); 3- and 4-byte address widths | 3 |
| LEG-LOG-04 | KAT `fixtures/kat/logger-decode.toml` | Conversion decode: `uint8`, `int8` on 2 bytes (signed 16), `uint16` little endian, `float` both orders, `bit="7"` on 2 bytes (big-endian bit numbering), 3-byte value (port decodes; legacy 0, MF-02), NaN result (port `None`; legacy 0, MF-09) | 4.1, 4.2 |
| LEG-LOG-05 | Test (rr-live formatting) | `format="0"` on 750.5 → `750`, 751.5 → `752`; `0.00` on 14.3647 → `14.36`; `<replace>` match on the `.`-decimal text | 4.3 |
| LEG-LOG-06 | defs-kat `logger/conversions.toml` | Selected conversion is the first; no `<conversions>` gives `Raw data` / `x` / `0`; gauge defaults by unit and the 0/100/10 fallback; inverted `gauge_min`/`gauge_max` gives a Warning | 4.4, 4.5 |
| LEG-LOG-07 | defs-kat `logger/capability.toml` | Parameter with `ecubyteindex`/`ecubit` kept or dropped against a given init-data blob (bit 0, bit 7, index past the end); switch gated by its own `bit`; no init keeps everything. Shares vectors with LEG-SSM-04 | 5 |
| LEG-LOG-08 | defs-kat `logger/ecuparam.toml` | `<ecuparam>` selection by ECU ID; ID list with `", "` (port trims, MF-68); per-ECU `bit` (port decodes each ECU's own bit, MF-68); unknown ECU ID gives no parameter (EC-20); DS2-style duplicate IDs with disjoint ECU lists | 6 |
| LEG-LOG-09 | defs-kat `logger/derived.toml` + rr-live derived-graph test | `[ID:units]` and bare `ID` references; forward reference resolves (legacy drops); missing dependency gives a Diagnostic (MF-24); external-sensor input (MF-24); two units of one dependency in one formula (MF-69); missing unit is a per-signal Diagnostic, not a whole-file failure | 7 |
| LEG-LOG-10 | KAT `fixtures/kat/logger-derived.toml` | Values: `(P12*60)/P8` with P12 = 30.00 g/s, P8 = 3000 rpm → 0.60 g/rev; `(P8*[P21:ms])/1200` with 3000, 10 → 25.00; `[P7:psi]-[P24:psi]` with 99 and 100 raw → (99 − 100) × 37/255 = −0.15 psi (−0.1451) | 7.3 |
| LEG-LOG-11 | KAT `fixtures/kat/logger-switch-dtc.toml` | Switch bit 0–7 incl. bit 7 of `80`; trouble code temp/mem combinations 0–3 and `FF FF` → not supported (legacy −1) | 4.6 |
| LEG-LOG-12 | defs-kat `logger/dtd-drift.toml` | Attributes outside the DTD load (`p1_max`, switch `storagetype`/`units`, parameter `group`); `ref ecuparam` gives a Diagnostic (legacy drops silently); `target` is kept and maps to a module (MF-71) | 2.3, 2.2 |
| LEG-LOG-13 | Corpus test (`RR_CORPUS_DIR`, and the private v370 package when present) | Every logger file of the pinned tree and `logger_v370.zip` loads with no Error Diagnostics; counts per protocol of parameters, switches, trouble codes, ECU-specific and derived parameters are reported | 1, 6, 7 |
| LEG-LOG-14 | Test (rr-defs, error policy) | One bad number in one parameter yields a Diagnostic and the rest of the file loads (legacy fails whole file) | 11 |
