//! The table codec (US-RR-19-10): a run of consecutive cells to raw values
//! and back, in either [`Layout`].
//!
//! A table of `N` cells occupies `N * cell_bytes` bytes, cell `i` at byte
//! `i * cell_bytes`. Each cell is read and written by the cell codec (the
//! `codec` module: byte order, `MOVI20` nibble, bitfield, sign), so a table
//! never decodes a cell differently from [`Encoding::decode`].
//!
//! # Default layout
//!
//! Each table value is its cell's value: [`Encoding::decode_table`] is
//! [`Encoding::decode`] of every cell, and [`Encoding::encode_table`] is
//! [`Encoding::encode_into`] of every cell, all or nothing.
//!
//! # Bosch subtract
//!
//! Bosch Motronic "subtract" tables (legacy `dataLayout="bosch_subtract"`,
//! `maps/Table.java:82,520-526`) store differences. With `s[j]` the stored
//! value of cell `j` (the ordinary cell decode: signedness and bitfield
//! included, as legacy's per-cell `getValueFromMemory(j)`) and `W` the cell
//! width in bytes, legacy decodes (`maps/DataCell.java:193-207`)
//!
//! ```text
//! value[i] = 2^(8W) - (s[i] + s[i+1] + ... + s[N-1])
//! ```
//!
//! and encodes (`maps/DataCell.java:219-231`, cascading to the earlier cells
//! at `:312-313`)
//!
//! ```text
//! s[N-1] = 2^(8W) - value[N-1]
//! s[i]   = value[i+1] - value[i]        (i < N-1)
//! ```
//!
//! `2^(8W)` is the cell width's, not the bitfield's. Values are not bounded
//! by the storage (an all-zero `u8` cell decodes to 256), so they are
//! [`Raw::Int`] in `i128`; every derived `s[i]` must fit its cell (storage
//! range or bitfield), or the table is refused with nothing written.
//! Legacy truncates an out-of-range `s[i]` silently (an `(int)` cast and a
//! byte split); this codec never does. Float and `U20` storage cannot have
//! this layout ([`Encoding::new`]).
//!
//! Decoding cannot overflow: a slice holds at most `isize::MAX` bytes, so a
//! table of `W`-byte cells has at most `2^63 / W` cells, each `|s[j]| <=
//! 2^(8W)`, and the running sum stays within `2^(8W) + 2^63 / W * 2^(8W) <=
//! 2^125` for `W <= 8`, inside `i128`.

use super::{Encoding, EncodingError, Layout, Raw};

impl Encoding {
    /// Decodes a table of `cells` consecutive cells from the start of `bytes`
    /// (any further bytes are not read), in the encoding's layout.
    ///
    /// For [`Layout::Default`] this is [`decode`](Self::decode) of each cell.
    /// For [`Layout::BoschSubtract`] value `i` is `2^(8W)` minus the sum of
    /// the stored cells from `i` to the end (see the module docs; legacy
    /// `maps/DataCell.java:193-207`).
    ///
    /// # Errors
    ///
    /// [`EncodingError::ShortInput`] if `bytes` is shorter than the table
    /// (`needed` is `cells * cell_bytes`, saturated at `usize::MAX`).
    pub fn decode_table(self, bytes: &[u8], cells: usize) -> Result<Vec<Raw>, EncodingError> {
        let needed = cells.saturating_mul(self.storage.cell_bytes());
        let table = bytes.get(..needed).ok_or(EncodingError::ShortInput {
            needed,
            len: bytes.len(),
        })?;
        let mut values = table
            .chunks_exact(self.storage.cell_bytes())
            .map(|cell| self.decode_cell(cell))
            .collect::<Result<Vec<_>, _>>()?;
        if matches!(self.layout, Layout::BoschSubtract) {
            // value[i] = 2^(8W) - (s[i] + ... + s[N-1]), summed from the end.
            // Every Bosch-subtract storage is an integer (Encoding::new).
            let mut running = self.bosch_total();
            for value in values.iter_mut().rev() {
                if let Raw::Int(stored) = value {
                    running -= *stored;
                    *stored = running;
                }
            }
        }
        Ok(values)
    }

    /// Encodes `values` as a table of consecutive cells from the start of
    /// `bytes`, in the encoding's layout, keeping the bits each cell's
    /// encoding does not own (as [`encode_into`](Self::encode_into)). Bytes
    /// after the table are not touched.
    ///
    /// All or nothing: every cell is checked before any byte is written, so
    /// on error `bytes` is unchanged.
    ///
    /// For [`Layout::BoschSubtract`] the stored cells are derived from the
    /// values: `s[N-1] = 2^(8W) - value[N-1]` and `s[i] = value[i+1] -
    /// value[i]` (legacy `maps/DataCell.java:219-231,312-313`).
    ///
    /// # Errors
    ///
    /// * [`EncodingError::ShortInput`] if `bytes` is shorter than the table;
    /// * [`EncodingError::TableCell`] naming the first cell that cannot be
    ///   stored, with the cell's error ([`EncodingError::KindMismatch`],
    ///   [`EncodingError::OutOfRange`] or
    ///   [`EncodingError::BitFieldOutOfRange`], as
    ///   [`encode_into`](Self::encode_into)). For Bosch subtract a value of
    ///   the wrong kind is reported first; otherwise the value checked is the
    ///   derived stored cell `s[i]` (saturated at the `i128` bounds, which no
    ///   storage reaches), never wrapped as legacy does.
    pub fn encode_table(self, values: &[Raw], bytes: &mut [u8]) -> Result<(), EncodingError> {
        let needed = values.len().saturating_mul(self.storage.cell_bytes());
        let len = bytes.len();
        let table = bytes
            .get_mut(..needed)
            .ok_or(EncodingError::ShortInput { needed, len })?;
        let stored = match self.layout {
            Layout::Default => values.to_vec(),
            Layout::BoschSubtract => self.bosch_stored(values)?,
        };
        let mut scratch = table.to_vec();
        for (index, (raw, cell)) in stored
            .into_iter()
            .zip(scratch.chunks_exact_mut(self.storage.cell_bytes()))
            .enumerate()
        {
            self.encode_cell(raw, cell)
                .map_err(|error| error.in_cell(index))?;
        }
        table.copy_from_slice(&scratch);
        Ok(())
    }

    /// `2^(8W)` for `W`-byte cells. Bosch-subtract storages are the whole-byte
    /// integers, whose value bits are exactly `8W`.
    const fn bosch_total(self) -> i128 {
        1 << self.storage.value_bits()
    }

    /// The stored cells of a Bosch-subtract table of `values`.
    fn bosch_stored(self, values: &[Raw]) -> Result<Vec<Raw>, EncodingError> {
        let ints = values
            .iter()
            .enumerate()
            .map(|(index, raw)| match raw {
                Raw::Int(value) => Ok(*value),
                Raw::F32(_) | Raw::F64(_) => Err(EncodingError::KindMismatch {
                    storage: self.storage,
                    kind: raw.kind(),
                }
                .in_cell(index)),
            })
            .collect::<Result<Vec<i128>, _>>()?;
        let total = [self.bosch_total()];
        let nexts = ints.iter().skip(1).chain(&total);
        Ok(ints
            .iter()
            .zip(nexts)
            .map(|(value, next)| Raw::Int(next.saturating_sub(*value)))
            .collect())
    }
}

impl EncodingError {
    /// This error, reported for table cell `index`.
    fn in_cell(self, index: usize) -> Self {
        Self::TableCell {
            index,
            source: Box::new(self),
        }
    }
}

#[cfg(test)]
mod tests;
