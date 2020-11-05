use std::{
    convert::TryInto,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    ops::Range,
};
use usize_cast::IntoUsize;

use crate::{stream_len, stream_position};

pub type ViewRange<T> = Range<T>;

/// 'Sorts' a [`RangeInclusive`]'s values.
/// So if `end` < `start`, then we recreate it as [end, start]
/// Just making sure that the start value is smaller than the end value.
fn sort_range<T>(range: ViewRange<T>) -> ViewRange<T>
where
    T: PartialOrd,
{
    if range.start > range.end {
        range.end..range.start
    } else {
        range
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IntoOffsetError {
    /// `offset < range.start`
    OutOfLowerBounds,
    /// `offset > range.end`
    OutOfUpperBounds,
}
impl From<IntoOffsetError> for std::io::Error {
    fn from(err: IntoOffsetError) -> Self {
        match err {
            IntoOffsetError::OutOfLowerBounds | IntoOffsetError::OutOfUpperBounds => {
                ErrorKind::InvalidInput.into()
            }
        }
    }
}

/// A wrapper around a Reader+Seeker (and potentially Writer!) that stops reading/writing/seeking
/// past/before a certain point
/// Position can
pub struct ConstrainedWrapper<R: Read + Seek> {
    reader: R,
    range: ViewRange<u64>,
}
impl<R> ConstrainedWrapper<R>
where
    R: Read + Seek,
{
    /// Creates a `ConstrainedWrapper` that makes sure that the reader is within range.
    /// If it is _not_ in range, then it seeks to `range.start`, otherwise it does not modify it.
    pub fn new(mut reader: R, range: ViewRange<u64>) -> std::io::Result<Self> {
        let range = sort_range(range);
        let position = stream_position(&mut reader)?;
        if position < range.start || position > range.end {
            reader.seek(SeekFrom::Start(range.start))?;
        }
        Ok(Self::new_unchecked(reader, range))
    }

    /// Creates a `ConstrainedWrapper` without making sure that the reader is wthin `range`
    /// SOUNDNESS: `reader` position `>= range.start` and `<= range.end`.
    /// SOUNDNESS: range is sorted. (So `range.start <= range.end`)
    pub fn new_unchecked(mut reader: R, range: ViewRange<u64>) -> Self {
        debug_assert!({
            let position = stream_position(&mut reader).unwrap();
            position >= range.start && position <= range.end
        });
        Self { reader, range }
    }

    /// Consume self and return inner reader.
    pub fn into_inner(self) -> R {
        self.reader
    }

    pub fn limit(&self) -> u64 {
        self.range.end - self.range.start
    }

    pub fn range(&self) -> &ViewRange<u64> {
        &self.range
    }

    /// Converts an offset into the `reader` into an absolute position into the reader.
    /// If offset is past end, it is clamped to the last position
    pub fn position_from_offset(&self, offset: u64) -> u64 {
        offset + self.range.start
    }

    pub fn position_into_offset(&self, position: u64) -> Result<u64, IntoOffsetError> {
        if position > self.range.end {
            Err(IntoOffsetError::OutOfUpperBounds)
        } else if position < self.range.start {
            Err(IntoOffsetError::OutOfLowerBounds)
        } else {
            Ok(position - self.range.start)
        }
    }

    /// Get the amount of bytes left to consume.
    fn remaining_bytes(&mut self) -> std::io::Result<u64> {
        // The current position in the wrapper. Can't pass `self` to `stream_position`..
        let current_offset: u64 = self.seek(SeekFrom::Current(0))?;
        dbg!(current_offset);
        // The last point
        let offset_end: u64 = self.position_into_offset(self.range.end)?;
        dbg!(offset_end);
        // The maximum amount of bytes that can be used.
        Ok(offset_end.checked_sub(current_offset).unwrap())
    }
}
impl<R> ConstrainedWrapper<R> where R: Read + Seek + Write {}
impl<R> Write for ConstrainedWrapper<R>
where
    R: Read + Seek + Write,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // The starting position
        let absolute_position = stream_position(&mut self.reader)?;
        dbg!(absolute_position);
        if absolute_position >= self.range.end {
            // If we're at the end, we can just early exit with (essentially) EOF
            Ok(0)
        } else {
            // The max length that we can write at our current position.
            let max_length = self.remaining_bytes()?.into_usize().min(buf.len());
            dbg!(max_length);
            self.reader.write(&buf[..max_length])
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.reader.flush()
    }
}
impl<R> Read for ConstrainedWrapper<R>
where
    R: Read + Seek,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Get the max length of the data we can stuff in a buffer.
        let max_length = self.remaining_bytes()?.into_usize().min(buf.len());
        if max_length == 0 {
            // EOF. There are no more bytes to read.
            Ok(0)
        } else {
            let read = self.reader.read(&mut buf[..max_length])?;
            debug_assert!(stream_position(&mut self.reader)? <= self.range.end);
            Ok(read)
        }
    }
}
impl<R> Seek for ConstrainedWrapper<R>
where
    R: Read + Seek,
{
    /// Seek to position.
    /// The position that is returned is relative to `self.range.start`.
    /// If values would overflow/underflow, it returns `ErrorKind::InvalidInput` as its error
    fn seek(&mut self, seek_from: SeekFrom) -> std::io::Result<u64> {
        let (position, offset) = match seek_from {
            SeekFrom::Current(offset) => (stream_position(&mut self.reader)?, offset),
            // We do not allow seeking past the end _at all_. Seeking past the end just puts you at
            // the last value in our range, and doesn't allow you further.
            // TODO: verify that this is correct and not off by one
            SeekFrom::End(offset) => (self.range.end, offset),
            // The start is offset from `self.range.start`
            // so we add it on, but if it overflows, we return that it was invalid input.
            SeekFrom::Start(position) => (
                self.range
                    .start
                    .checked_add(position)
                    .ok_or(ErrorKind::InvalidInput)?,
                0,
            ),
        };

        // Apply the offset to the position, getting the full destination.
        // We turn any errors of resulting negative values into invalid input errors.
        let destination_position = apply_offset(position, offset).map_err(|err| match err {
            // If the result after applying th eoffset was negative, then that is an invalid input.
            OffsetError::Negative => ErrorKind::InvalidInput,
        })?;
        // Clamp the position down to the end position
        let destination_position = destination_position.min(stream_len(&mut self.reader)?);
        // Finally go to the actual position that we desire.
        // We store the resulting position that we are now at, because Read can be crazy :]
        // (also it lets us avoid checking immediately again..)
        let resulting_position = self.reader.seek(SeekFrom::Start(destination_position))?;

        // Get the offset into the reader, which will be the user visible position into this wrapper
        Ok(self.position_into_offset(resulting_position)?)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum OffsetError {
    /// The offset would result in a negative number.
    Negative,
}

/// SOUNDNESS: `offset` should be negative.
fn negative_i64_into_u64_offset(offset: i64) -> Result<u64, OffsetError> {
    debug_assert!(offset.is_negative());

    // Negate the negative value to positive.
    // will be `None` if it is `i64::MIN`, since negated that is 1 higher than `i64::MAX`
    let offset: Option<i64> = offset.checked_neg();
    if let Some(offset) = offset {
        // Since it's a negated negative number we should always be able to transform it into a u64
        // without issue.
        let offset: u64 = offset.try_into().unwrap();
        Ok(offset)
    } else {
        Err(OffsetError::Negative)
    }
}

fn apply_offset(position: u64, offset: i64) -> Result<u64, OffsetError> {
    if offset.is_negative() {
        let offset = negative_i64_into_u64_offset(offset)?;
        position.checked_sub(offset).ok_or(OffsetError::Negative)
    } else {
        // Offset is not negative, so that means it must be within u64
        let offset: u64 = offset.try_into().unwrap();
        // FIXME: check what the std library does on overflows with offsets.
        position.checked_add(offset).ok_or(OffsetError::Negative)
    }
}

#[cfg(test)]
mod tests {
    use super::{sort_range, stream_len, stream_position, ConstrainedWrapper, ViewRange};
    use std::io::{Read, Seek, SeekFrom, Write};

    #[test]
    fn test_sort_range() {
        let range: ViewRange<u32> = 0..5;
        assert_eq!(range, sort_range(range.clone()));

        let range: ViewRange<u32> = 0..u32::MAX;
        assert_eq!(range, sort_range(range.clone()));

        let range: ViewRange<i32> = -100..500;
        assert_eq!(range, sort_range(range.clone()));

        #[allow(clippy::reversed_empty_ranges)]
        let range: ViewRange<i32> = 100..5;
        assert_eq!(5..100, sort_range(range));

        let range: ViewRange<i32> = (i32::MIN)..i32::MAX;
        assert_eq!(range, sort_range(range.clone()));
    }

    #[test]
    fn test_reader() {
        let mut data = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let cursor = std::io::Cursor::new((&mut data) as &mut [u8]);
        // [0, 5), {0, 1, 2, 3, 4}
        let mut cons = ConstrainedWrapper::new(cursor, 0..5).unwrap();

        assert_eq!(stream_position(&mut cons).unwrap(), 0);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);
        assert_eq!(stream_position(&mut cons).unwrap(), 0);

        assert_eq!(cons.seek(SeekFrom::Start(1)).unwrap(), 1);
        assert_eq!(stream_position(&mut cons).unwrap(), 1);

        assert_eq!(cons.seek(SeekFrom::Start(2)).unwrap(), 2);
        assert_eq!(stream_position(&mut cons).unwrap(), 2);

        assert_eq!(cons.seek(SeekFrom::Start(5)).unwrap(), 5);
        assert_eq!(stream_position(&mut cons).unwrap(), 5);

        let mut buf = [99u8; 1];
        assert!(cons.read_exact(&mut buf).is_err());
        assert_eq!(stream_position(&mut cons).unwrap(), 5);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);

        let mut buf = [99u8; 1];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0u8);
        assert_eq!(stream_position(&mut cons).unwrap(), 1);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        let mut buf = [99u8; 1];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 1u8);
        assert_eq!(stream_position(&mut cons).unwrap(), 2);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        let mut buf = [99u8; 1];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 2u8);
        assert_eq!(stream_position(&mut cons).unwrap(), 3);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        let mut buf = [99u8; 1];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 3u8);
        assert_eq!(stream_position(&mut cons).unwrap(), 4);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        let mut buf = [99u8; 1];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 4u8);
        assert_eq!(stream_position(&mut cons).unwrap(), 5);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        let mut buf = [99u8; 1];
        assert!(cons.read_exact(&mut buf).is_err());
        assert_eq!(stream_position(&mut cons).unwrap(), 5);
        assert_eq!(stream_len(&mut cons).unwrap(), 5);

        cons.seek(SeekFrom::Start(0)).unwrap();
        let mut cursor = cons.into_inner();
        let mut cons = ConstrainedWrapper::new(&mut cursor, 3..7).unwrap();
        // Check that since we were outside of bounds that it put us at `range.start`
        assert_eq!(stream_position(&mut cons).unwrap(), 0);
        assert_eq!(cons.position_from_offset(0), 3);

        assert_eq!(stream_len(&mut cons).unwrap(), 4);
        let mut buf = [99u8; 3];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [3u8, 4, 5]);
        assert_eq!(stream_position(&mut cons).unwrap(), 3);

        // == Writing ==

        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);

        let buf = [5u8, 9u8];
        cons.write_all(&buf).unwrap();
        assert_eq!(stream_position(&mut cons).unwrap(), 2);
        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);
        let mut buf = [99u8; 2];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5u8, 9u8]);

        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);
        let buf = [9, 4, 5, 6];
        cons.write_all(&buf).unwrap();
        assert_eq!(stream_position(&mut cons).unwrap(), 4);
        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);
        let mut buf = [99u8; 4];
        cons.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [9, 4, 5, 6]);

        // Writing too much data.
        assert_eq!(cons.seek(SeekFrom::Start(0)).unwrap(), 0);
        let buf = [9, 4, 5, 6, 8];
        assert!(cons.write_all(&buf).is_err());
    }
}
