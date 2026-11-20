//! Little-endian cursor over a borrowed buffer.

use crate::FtmsError;

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub(crate) fn skip(&mut self, n: usize, what: &'static str) -> Result<(), FtmsError> {
        let _ = self.take(n, what)?;
        Ok(())
    }

    pub(crate) fn u8(&mut self, what: &'static str) -> Result<u8, FtmsError> {
        Ok(self.take(1, what)?[0])
    }

    pub(crate) fn u16(&mut self, what: &'static str) -> Result<u16, FtmsError> {
        let bytes = self.take(2, what)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn i16(&mut self, what: &'static str) -> Result<i16, FtmsError> {
        let bytes = self.take(2, what)?;
        Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Unsigned 24-bit integer, little-endian, returned in the low 24 bits.
    pub(crate) fn u24(&mut self, what: &'static str) -> Result<u32, FtmsError> {
        let bytes = self.take(3, what)?;
        Ok(u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16))
    }

    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], FtmsError> {
        let have = self.remaining();
        if have < n {
            return Err(FtmsError::Truncated {
                what,
                have,
                need: n,
            });
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.data[start..self.pos])
    }
}

pub(crate) fn push_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

pub(crate) fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Scale `value` by an exact integer `per_unit` (for example 100 for 0.01) and saturate to `i16`.
pub(crate) fn saturate_i16(value: f32, per_unit: f32) -> Result<i16, FtmsError> {
    if !value.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    let scaled = (value * per_unit).round();
    if scaled >= f32::from(i16::MAX) {
        Ok(i16::MAX)
    } else if scaled <= f32::from(i16::MIN) {
        Ok(i16::MIN)
    } else {
        #[allow(clippy::cast_possible_truncation)]
        Ok(scaled as i16)
    }
}

/// Scale `value` by an exact integer `per_unit` and saturate to `u8`.
///
/// Negative inputs saturate to 0. Non-finite inputs are an error.
pub(crate) fn saturate_u8(value: f32, per_unit: f32) -> Result<u8, FtmsError> {
    if !value.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    let scaled = (value * per_unit).round();
    if scaled <= 0.0 {
        Ok(0)
    } else if scaled >= f32::from(u8::MAX) {
        Ok(u8::MAX)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(scaled as u8)
    }
}
