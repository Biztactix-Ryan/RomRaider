# Logger CSV: legacy format

What the Java RomRaider logger writes when it logs to a file: when a file is
opened and closed, its name and location, encoding and line endings, the
header row, the time column, how a row is assembled, how numbers are
formatted and which delimiter is used under which locale, what happens when
the parameter set changes mid-log, and which values in the file are not real
readings. Also what legacy can read back (a playback stub and the dyno
tab's log import). Written from the source only (the Java was never run,
D-024). It is the spec for import compatibility of the rr-live CSV writer
and the CSV playback producer (ARCHITECTURE.md, "CSV logs"), and for their
fixtures. Part of the legacy behaviour catalogue, story US-RR-12, task
US-RR-12-9.

Where a parameter's value and its text come from (conversions, `format`,
`<replace>`) is [`logger.md`](logger.md) section 4; `DecimalFormat` rounding
is [`encoding-scaling.md`](encoding-scaling.md) section 8; the settings this
page names are in [`settings.md`](settings.md) section 4.5.

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses. `file/X.java` is short for
  `logger/ecu/ui/handler/file/X.java`. Files elsewhere in `legacy/` carry
  the `legacy/` prefix. Corpus lines (`logger_STD_EN_v290.xml:N`) are as in
  [`logger.md`](logger.md). Every reference is `file:line` or
  `file:first-last`.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-CSV-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Notation.** `⏎` is the platform line separator, `␣` a space where it
  matters. Sample files marked *hand-composed* were written for this page from
  the rules below; no legacy-written file was available.
- **JDK behaviour.** A few rules come from the Java class library, not from
  `legacy/` (`DecimalFormat`, `SimpleDateFormat`, `String.getBytes`). They are
  marked *(JDK)* and are the documented behaviour of those classes.

### Source files

| Alias | Path under `legacy/src/main/java/com/romraider/` | Role |
|-------|---------------------------------------------------|------|
| `file/FileUpdateHandlerImpl.java` | `logger/ecu/ui/handler/file/` | Which parameters, column order, row assembly, header text, the delimiter and its locale list |
| `file/FileLoggerImpl.java` | `logger/ecu/ui/handler/file/` | File name, open/close, time column, encoding, line separator |
| `logger/ecu/comms/query/ResponseImpl.java` | same | The row timestamp; missing values |
| `logger/ecu/comms/manager/QueryManagerImpl.java` | same | When a response (row candidate) is built; the file-logging switch |
| `logger/ecu/EcuLogger.java` | same | The log-to-file button, the file-name text field, handler wiring |
| `logger/ecu/definition/EcuParameterConvertorImpl.java` | same | Value text (`format`, `replace`) |
| `Settings.java`, `logger/ecu/ui/swing/menubar/action/LogFileNumberFormatAction.java` | same | Output directory, time mode, switch ID, the "US number format" locale switch |
| `logger/ecu/ui/playback/PlaybackManagerImpl.java` | same | The unfinished playback |
| `logger/ecu/ui/tab/dyno/DynoControlPanel.java` | same | The only CSV reader: dyno log import |
| `logger/ecu/ui/swing/tools/ReadCodesResultsPanel.java`, `SSMLearningTableValuesResultsPanel.java` | same | Other CSV files (trouble codes, learning tables) |

## 1. When a file is open

A log file exists only while file logging is on and the logger is
connected:

- **Button.** "Log to file" starts file logging only if the logger is
  running; pressing it again stops it (`logger/ecu/EcuLogger.java:1471-1489`).
- **File-logging switch.** If `<filelogging active>` is true and the
  definition has the configured switch (default `S20`, rear defogger), the
  switch is polled every cycle and each **change** of its value starts (1) or
  stops (0) file logging (`logger/ecu/EcuLogger.java:706-724`;
  `logger/ecu/comms/manager/QueryManagerImpl.java:370-376,393-395`;
  [`logger.md`](logger.md) section 8). Each switch-on opens a new file.
- **Start** opens a new file, reports its name in the title bar, and writes
  the header (`file/FileUpdateHandlerImpl.java:129-136`;
  `file/FileLoggerImpl.java:57-75`). **Stop** closes it
  (`file/FileLoggerImpl.java:77-88`). Logger shutdown stops it
  (`file/FileUpdateHandlerImpl.java:112-117`).
- Output is buffered (`BufferedOutputStream`, `file/FileLoggerImpl.java:63`)
  and flushed only on close, so a crash or power loss loses the unflushed
  tail (MF-74).
- A write error stops logging and throws `FileLoggerException`
  (`file/FileLoggerImpl.java:109-119`).

## 2. File name and location

```
<dir>/romraiderlog_[<text>_]<yyyyMMdd>_<HHmmss>.csv
```

(`file/FileLoggerImpl.java:132-145`):

- `<dir>` is `<logger><filelogging path>` of settings, default the user's
  home directory (`Settings.java:215`); a separator is added if missing.
- `<text>` is the "Logfile Text" field next to the button, included when
  non-empty. It is free text, not sanitised, and **not saved** in settings;
  its popup can fill in the ECU ID or one of a fixed list of labels with
  spaces turned into `_` (`logger/ecu/EcuLogger.java:1398-1450`). A `/` or
  other path character in it changes the path (MF-74).
- The stamp is `SimpleDateFormat("yyyyMMdd_HHmmss")` of the local time when
  the file is opened (`file/FileLoggerImpl.java:44,143`). Two files started
  within the same second get the same name, and `FileOutputStream`
  **truncates** the first (MF-74).

Example: `romraiderlog_2004Liberty_20261022_141503.csv` *(hand-composed)*.

## 3. Encoding and line endings

- Every line is written with `String.getBytes()`, the **platform default
  charset** *(JDK)*: windows-1252 on a western Windows of the Java 8 era,
  UTF-8 on Linux and macOS (`file/FileLoggerImpl.java:109-114`). No byte
  order mark. Unit strings in the corpus include `°`, `µ`, `²`, `³`, and the
  German files have non-ASCII names (`Hinterräder`)
  (`logger_STD_EN_v290.xml:1043`). The same log is therefore different bytes
  on different machines, and nothing in the file says which (MF-73).
- The line separator is `System.getProperty("line.separator")`: `CR LF` on
  Windows, `LF` elsewhere (`file/FileLoggerImpl.java:41,112-114`). One is
  appended after every header and row (unless the text already ends with
  one, which never happens).

## 4. Header row

```
Time (msec)<d><name1>␣(<units1>)<d><name2>␣(<units2>)…⏎
```

- The first cell is `Time` in absolute-time mode, `Time (msec)` otherwise
  (`file/FileLoggerImpl.java:95-102`).
- Each column is the delimiter `<d>`, the parameter's `name`, a space, and
  its **selected** conversion's `units` in parentheses
  (`file/FileUpdateHandlerImpl.java:191-198`). Nothing is escaped or quoted
  (section 8). A switch's units default to `On/Off`; a parameter with no
  conversions shows `Raw data` ([`logger.md`](logger.md) sections 4.4, 4.6).
- External sensors use their plugin's item name and units
  (`logger/ecu/definition/ExternalDataImpl.java:45,53-55`;
  `logger/ecu/definition/ExternalDataConvertorImpl.java:57-59`).
- There is no blank after the delimiter, and no trailing delimiter: each
  column is the delimiter followed by its text
  (`file/FileUpdateHandlerImpl.java:194,204`).

Example (`logger_STD_EN_v290.xml:39-90`, STD file, first conversions):
`Time (msec),Engine Load (Relative) (%),Coolant Temperature (F),Engine Speed (rpm)`.
Note `name` can itself end in a parenthesis, so a header cell is not
reliably split at its last `(` (`Actual Common Rail Pressure (Time
Synchronized) (…)`, `logger_STD_EN_v290.xml:1381`).

## 5. Time column

The first cell of each row (`file/FileLoggerImpl.java:104-130`):

| Setting `<filelogging absolutetimestamp>` | Cell | Example |
|------------------------------------------|------|---------|
| `false` (default) | Milliseconds since the first row of **this file**, as an integer (`String.valueOf(long)`) | `0`, `52`, `104` |
| `true` | Local wall-clock time `HH:mm:ss.SSS` *(JDK `SimpleDateFormat`, default time zone)*, no date | `14:15:03.512` |

- The timestamp is `System.currentTimeMillis()` taken when the cycle's
  response object is built, **after** all reads of the cycle
  (`logger/ecu/comms/query/ResponseImpl.java:32-34`;
  `logger/ecu/comms/manager/QueryManagerImpl.java:393-407`). It is wall-clock
  time, so an NTP step or DST change shifts or reverses relative times
  (EC-26), and it says when the host finished the cycle, not when the ECU
  sampled.
- The relative origin is the first row written after `start`; header
  rewrites (section 9) do not reset it. The code uses `startTimestamp == 0`
  as "not set" (`file/FileLoggerImpl.java:73,126`).
- Absolute times have no date and wrap at midnight; a log across midnight
  is not monotonic (`file/FileLoggerImpl.java:45,124`; MF-75).

## 6. Data rows

### 6.1 Which parameters, in which order

The CSV writer is registered with the Data, Graph and Dashboard tabs
(`logger/ecu/EcuLogger.java:953-962`). A parameter ticked in **any** of them
is a column; a reference count removes it only when unticked in all
(`file/FileUpdateHandlerImpl.java:77-86,101-110`). Columns are in order of
**first registration** (`LinkedHashMap`, `file/FileUpdateHandlerImpl.java:44`):
the order the user ticked them, or a loaded profile's order, not the
definition's order. Trouble codes are never logged.

### 6.2 Assembling a row

Each poll cycle produces one response holding a value for every polled
parameter, ECU and external alike (`logger/ecu/comms/manager/QueryManagerImpl.java:396-408`).
The writer copies the **text** of each value it has a column for into the
current line; when every column has text, it writes the line with that
response's timestamp and starts an empty one
(`file/FileUpdateHandlerImpl.java:88-99,166-189`). Consequences:

- Normally every cycle gives one row.
- If a column is missing from a response, nothing is written until a later
  response fills it; the row then mixes values from different cycles under
  the later timestamp.
- A column filled twice before the row is complete keeps the later value.

### 6.3 Cell text

A cell is the selected conversion's `format(value)`: `DecimalFormat` with
the conversion's pattern in the **default locale**, rounding `HALF_EVEN`
*(JDK)*, then `<replace>` substitution ([`logger.md`](logger.md) section 4.3).
So:

- Values are **rounded strings**, not raw readings (MF-30): `format="0"` throws away
  every fraction, and engine speed 750.5 is written `750`, 751.5 `752`
  (`logger_STD_EN_v290.xml:90`).
- A negative value that rounds to zero is written `-0` (`-0.00` with two
  decimals) *(JDK)*.
- A `<replace>` turns a number into any text, delimiter included.
- Switches are `1` or `0` (`logger/ecu/definition/EcuSwitchConvertorImpl.java:55-58`).
- The patterns used in the corpus have no grouping, so no thousands
  separators appear.

## 7. Number format, locale and delimiter (MF-30)

Two things depend on the locale, and they are decided separately: the
decimal separator each time a value is formatted
(`file/FileUpdateHandlerImpl.java:92`), the delimiter once at start-up
(`file/FileUpdateHandlerImpl.java:61-63`).

**Decimal separator.** The value text uses the JVM default locale's symbols
*(JDK)*. The setting `<logger locale>` is `system` (leave the JVM default)
or `en_US`; `en_US` calls `Locale.setDefault(en_US)` while settings load,
so `.` decimals (`Settings.java:886-895`; [`settings.md`](settings.md)
section 4.5). The menu item "US number format" and the settings dialog
switch between the two, and need a restart
(`logger/ecu/ui/swing/menubar/action/LogFileNumberFormatAction.java:39-49`;
`legacy/i18n/com/romraider/swing/menubar/action/AbstractAction.properties:30`;
`swing/SettingsForm.java:951-963`).

**Delimiter.** Fixed when the writer is created at logger start-up
(`file/FileUpdateHandlerImpl.java:58-63`):

1. If the system property `user.language.format` is set, the locale name is
   `<user.language.format>_<user.country.format>`; otherwise it is
   `Locale.getDefault().toString()`.
2. If that name is in a hard-coded list of 57 locales, the delimiter is `;`,
   otherwise `,` (`file/FileUpdateHandlerImpl.java:47-57`).

The list (`be_BY` … `vi_VN`) is meant to be the locales with a decimal
comma. Because the two decisions are separate:

- A decimal-comma locale **not** in the list writes `,` decimals with a `,`
  delimiter, and the file cannot be parsed: for example a default locale of
  plain `de` (no country), Norwegian reported as `nb_NO` (the list has only
  `no_NO` and `no_NO_NY`), or any name with a script or variant suffix
  (`file/FileUpdateHandlerImpl.java:49-55,61-63`). **MF-30.**
- A listed locale whose decimal is `.` (`de_CH`, `it_CH` in Java's locale
  data, both in the list at `file/FileUpdateHandlerImpl.java:49,52`) writes
  `;` with `.` decimals: valid, but not what a reader expects.
- With `locale="en_US"` but an OS that sets `user.language.format` to a
  listed locale, the delimiter is `;` and decimals `.`: `setLocale` changes
  the default locale and `user.language`, not `user.language.format`, which
  the delimiter reads first (`Settings.java:886-895`;
  `file/FileUpdateHandlerImpl.java:58-63`).

So a legacy CSV's delimiter is `,` or `;`
(`file/FileUpdateHandlerImpl.java:61-63`), and its decimal mark is `.` or
`,`; the file does not record which.

## 8. Quoting and escaping

None. Names, units and values are concatenated as they are
(`file/FileUpdateHandlerImpl.java:191-207`). The pinned v290 and the v370
STD_EN files have no `,`, `;`, `"` or tab in any parameter name or unit, so
the corpus never breaks a row today, but a `<replace>` text, a German-locale
value with `,` under a `,` delimiter (section 7), or a user plugin name
can. Not RFC 4180.

## 9. Mid-log changes (MF-30, EC-27)

The header is **written again into the same file** whenever the column set
or a unit changes while logging (`file/FileUpdateHandlerImpl.java:77-86,101-110,123-127`):

| Event | Effect |
|-------|--------|
| A parameter is ticked (first registration) | Partial line discarded; new header with the column appended at the end |
| A parameter is unticked in its last tab | Partial line discarded; new header without it (written even if it was not registered) |
| The user changes a parameter's units | Partial line discarded; new header with the new units; the column stays where it was |
| A new ECU ID or protocol reloads the definitions | The tabs re-register their parameters: one or more new headers |

Rows after a repeated header follow the new layout
(`file/FileUpdateHandlerImpl.java:146-148,200-206`); the relative time
origin is kept, since only `start` resets it
(`file/FileLoggerImpl.java:73,96-102,126`). A reader must therefore treat any line whose first cell is
`Time` or `Time (msec)` as a new header (MF-72 for legacy's own reader).

## 10. Values that are not readings

The file cannot tell a real reading from these:

- **Stale frames.** In continuous SSM mode, a bad frame is replaced by the
  previous good one and logged as new, with a new timestamp (**MF-08**;
  [`ssm.md`](ssm.md) section 6).
- **Missing values** become `0.0` before formatting
  (`logger/ecu/comms/query/ResponseImpl.java:44-47`; **MF-09**).
- **NaN or infinite** conversion results become `0.0`
  ([`logger.md`](logger.md) section 4.1).
- **External sensors** have no timestamp of their own; whatever the plugin
  last saw is logged in the current row (**MF-26**;
  `logger/ecu/comms/manager/QueryManagerImpl.java:378-386`;
  `logger/ecu/definition/ExternalDataConvertorImpl.java:47-50`).

## 11. Reading CSVs back

### 11.1 Playback

There is none. `PlaybackManagerImpl` is a stub marked "TODO: Finish me": it
registers parameters 10, 20 and 30 of the list and feeds them a ramp, and
its button is commented out of the UI, opening a hard-coded `foo.csv` that
it never reads (`logger/ecu/ui/playback/PlaybackManagerImpl.java:30-83`;
`logger/ecu/EcuLogger.java:1371-1396`). Nothing in legacy parses the logger's
own CSV for display.

### 11.2 Dyno tab import

The dyno tab's "open log" is the only CSV reader
(`logger/ecu/ui/tab/dyno/DynoControlPanel.java:981-1107`). It accepts
RomRaider, Cobb, AEM and OpenPort 2 logs:

- **Encoding**: `FileReader`, the platform default charset *(JDK)*
  (`DynoControlPanel.java:998`).
- **Delimiter**: split the first line on `;`; if that gives fewer than 3
  fields, try tab, then `,`, each accepted only with more than 2 fields
  (`DynoControlPanel.java:1009-1022`). With 2 columns the delimiter stays `;`.
- **Columns** by substring of the header cell, last match wins: time
  `Time` (RomRaider, milliseconds); `Seconds`, `Time/s`, `time` (seconds,
  × 1000); `Time Stamp` (Cobb `h:m:s` or `m:s`); RPM by `RPM` in upper case
  or `Engine Speed`; throttle `Throttle`; speed `Vehicle Speed`, with units
  from `mph` or `km/h` (`DynoControlPanel.java:120-130,1023-1041`).
- **Values**: every `,` in a value is replaced by `.`, then
  `Double.parseDouble` (`DynoControlPanel.java:1049-1053,1055-1081`).
- **Failures**: only `IOException` is caught (`DynoControlPanel.java:1092-1094`).
  A RomRaider log in absolute-time mode (`14:15:03.512`), a repeated header
  line (section 9), a short row, or a `,`-delimited file with `,` decimals
  throws an uncaught `NumberFormatException` or
  `ArrayIndexOutOfBoundsException` and the import aborts (MF-72). A
  parameter whose name contains `Time` (`Number of Times Injected`,
  `logger_STD_EN_v290.xml:1052`) or `Throttle` after the real column silently
  takes over that column.

## 12. Other CSV files

Not the log format, listed so the port knows they exist. Both go to the same
output directory, use `,` regardless of locale, the platform charset and line
separator, and a `yyyyMMdd-HHmmss` stamp:

- `romraiderDTC_<stamp>.csv`: header `Diagnostic Code Name,Temporary,Memorized`,
  then name and two localised true/false words per code
  (`logger/ecu/ui/swing/tools/ReadCodesResultsPanel.java:64,161-203`;
  `legacy/i18n/com/romraider/logger/ecu/ui/swing/tools/ReadCodesResultsPanel.properties:8`).
- `romraiderLTV_<stamp>.csv`: learning-table values, several titled blocks,
  every cell followed by `,` (`logger/ecu/ui/swing/tools/SSMLearningTableValuesResultsPanel.java:77,322-380`);
  DS2 and NCS have their own variants.

## 13. Worked example

*Hand-composed.* STD_EN v290 definitions, parameters ticked in the order
Engine Speed, Coolant Temperature, Manifold Absolute Pressure, each on its
first conversion; relative time; three cycles 52 ms apart. Raw bytes and
values:

| Cycle | P8 raw (`x/4`, `0`) | P2 raw (`32+9*(x-40)/5`, `0`) | P7 raw (`x*37/255`, `0.00`) |
|-------|---------------------|-------------------------------|-----------------------------|
| 1 | `0BB8` = 3000 → 750 | `7D` = 125 → 185 | `63` = 99 → 14.3647… |
| 2 | `0BBA` = 3002 → 750.5 | `7D` → 185 | `64` = 100 → 14.5098… |
| 3 | `0BBE` = 3006 → 751.5 | `7E` = 126 → 186.8 | `64` → 14.5098… |

(Expressions at `logger_STD_EN_v290.xml:48,90,79`.) With the JVM default
locale `en_US`:

```
Time (msec),Engine Speed (rpm),Coolant Temperature (F),Manifold Absolute Pressure (psi)⏎
0,750,185,14.36⏎
52,750,185,14.51⏎
104,752,187,14.51⏎
```

750.5 → `750` and 751.5 → `752` (`HALF_EVEN`); 186.8 → `187`. With
`de_DE` the same log is:

```
Time (msec);Engine Speed (rpm);Coolant Temperature (F);Manifold Absolute Pressure (psi)⏎
0;750;185;14,36⏎
52;750;185;14,51⏎
104;752;187;14,51⏎
```

If the user then ticks Throttle Opening Angle, the file continues with
`Time (msec),Engine Speed (rpm),Coolant Temperature (F),Manifold Absolute Pressure (psi),Throttle Opening Angle (%)⏎`
and five-column rows.

## 14. Mapping to the port

Pointers, not decisions; the target format is ARCHITECTURE.md "CSV logs"
(schema fixed at file start, raw numbers, `.` decimal, RFC 4180 quoting, a
schema change starts a new file).

- **Writer.** Header cells keep legacy's `<name> (<units>)` text and a first
  column named `Time (msec)` (or a documented successor), so a legacy reader
  sees the same layout. UTF-8, `,`, `.`, RFC 4180 quoting, values at full
  precision rather than the display `format` (MF-30). Gaps are empty cells,
  never `0` (MF-09), and stale frames never reach the file (MF-08).
- **Time.** Monotonic time for the relative column, wall time recorded once
  per file (EC-26).
- **Rotation.** A column-set or unit change closes the file and opens the
  next (EC-27); file names stay unique within a second (MF-74); the
  user's text is sanitised.
- **Durability.** Flushed at least once per row batch, and a final rename or
  sync on close, so a crash leaves a readable file (MF-74).
- **Importer and playback producer** (C12) read legacy files: delimiter `,`
  or `;`, decimal `.` or `,`, windows-1252 or UTF-8, `CR LF` or `LF`,
  repeated header lines starting a new segment, `Time (msec)` or absolute
  `HH:mm:ss.SSS` with no date, and `-0`. A comma-delimited file with comma
  decimals is ambiguous and is refused with a Diagnostic, not guessed.

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-CSV-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID. (Header rewrites, locale-formatted values and the locale list are
**MF-30**.)

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-CSV-01 | MF-72 | Legacy's only CSV reader (dyno import) cannot read legacy's own logs in absolute-time mode or with a mid-file header, catches only `IOException`, and picks columns by substring with last-match-wins, so a parameter named `…Times…` or `…Throttle…` can take over the time or throttle column | `logger/ecu/ui/tab/dyno/DynoControlPanel.java:1009-1094` |
| UL-CSV-02 | MF-73 | The log is written in the platform default charset with no marker, so `°`, `µ` and German names are different bytes on Windows and Linux, and readers (also `FileReader`) guess | `logger/ecu/ui/handler/file/FileLoggerImpl.java:111`; `DynoControlPanel.java:998` |
| UL-CSV-03 | MF-74 | File names have one-second resolution and are opened with truncation, so two logs started in the same second (e.g. a defogger toggle) overwrite each other; the user's "Logfile Text" is put into the path unsanitised | `logger/ecu/ui/handler/file/FileLoggerImpl.java:44,63,132-145`; `logger/ecu/EcuLogger.java:1406-1414` |
| UL-CSV-04 | MF-74 | Rows go through an unflushed `BufferedOutputStream` until the file is closed, so a crash, power loss or unplugged laptop loses the buffered tail of the log | `logger/ecu/ui/handler/file/FileLoggerImpl.java:63,77-88` |
| UL-CSV-05 | MF-75 | The row timestamp is wall-clock time taken after the whole poll cycle, and absolute mode prints time of day without a date: relative times can go backwards on a clock step, and absolute times wrap at midnight. Related to EC-26 | `logger/ecu/comms/query/ResponseImpl.java:32-34`; `logger/ecu/ui/handler/file/FileLoggerImpl.java:45,121-130` |

Ledger entries this page cross-references: **MF-30** (header rewrite,
locale-formatted values, the 57-locale delimiter list), **MF-08** (stale
frame logged as live), **MF-09** (missing value logged as 0), **MF-26**
(untimestamped external values); edge cases **EC-26** (clock jumps) and
**EC-27** (parameter set changes during a log). The table above adds
**MF-72** to **MF-75** (**MF-73**, the platform charset, is shared with
`settings.md` and `definitions.md`).

## Fixtures to write

Stable IDs for US-RR-12-2. Each becomes a CSV golden file under
`fixtures/public/csv/` (hand-written, small, no VIN: leak-guard rule 5) with
a test in rr-live, or a test where noted. Expected text is hand-computed
from this page (D-024), never produced by running Java. Where a fixture pins
a legacy bug, the expected result is the port's fixed behaviour and the
legacy result goes in its notes.

| ID | Kind | What it pins down | Section |
|----|------|-------------------|---------|
| LEG-CSV-01 | Golden `legacy-relative-comma.csv` + importer test | The section 13 `en_US` file, `LF` and `CR LF` variants: header parsed into name and units, `Time (msec)`, three rows | 4, 5, 13 |
| LEG-CSV-02 | Golden `legacy-relative-semicolon.csv` + importer test | The section 13 `de_DE` file: `;` delimiter, `,` decimals | 7, 13 |
| LEG-CSV-03 | Golden `legacy-absolute-time.csv` + importer test | `Time` header with `HH:mm:ss.SSS` cells, including a midnight wrap (`23:59:59.990`, `00:00:00.042`): import gives a monotonic timeline | 5 |
| LEG-CSV-04 | Golden `legacy-header-rewrite.csv` + importer and playback test | A second header mid-file (column added, then a unit changed): import yields three segments; playback equals source (MF-30, EC-27) | 9 |
| LEG-CSV-05 | Golden `legacy-encoding-cp1252.csv` and `legacy-encoding-utf8.csv` | `°F`, `µs`, `m/s²` and a German name in each encoding: both import to the same text (MF-73) | 3 |
| LEG-CSV-06 | Importer test | `-0` and `-0.00` cells read as zero; `750`/`752` HALF_EVEN text is taken as written; a `,`-delimited file with `,` decimals is refused with a Diagnostic | 6.3, 7 |
| LEG-CSV-07 | Writer test (rr-live) | Port output for the section 13 data: UTF-8, `,`, full-precision values (750.5, 751.5, 14.3647…), header text identical to legacy's | 4, 14 |
| LEG-CSV-08 | Writer test (rr-live) | A parameter set change closes the file and opens a new one; a unit change likewise; two files started in one second get distinct names; "Logfile Text" with `/` and `..` is sanitised (MF-74) | 2, 9 |
| LEG-CSV-09 | Writer fault test (T6) | Killing the writer mid-log leaves a file whose last complete row is readable (MF-74) | 1 |
| LEG-CSV-10 | Writer test (rr-live) | A missing value and a stale/invalid frame produce empty cells, never `0` or the previous value (MF-08, MF-09) | 10 |
| LEG-CSV-11 | Writer test with a VirtualClock | Relative time from the monotonic clock is non-decreasing across a wall-clock step back; the wall-clock start is recorded once (EC-26, MF-75) | 5 |
| LEG-CSV-12 | Importer test (dyno collector input) | A legacy log with `Engine Speed (rpm)`, `Throttle Opening Angle (%)` and `Vehicle Speed (mph)` columns imported for the dyno collector by Role, not by header substring (MF-72, MF-31) | 11.2 |
