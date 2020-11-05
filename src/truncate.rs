use std::{fs::File, io::Cursor};
use usize_cast::IntoUsize;

// TODO: tests
/// A trait for objects which can be truncated
/// Mainly meant to be used in conjunction with `Write` (and maybe `Seek`)
/// NOTE: This can be also used for increasing the available size.
/// Of which the default should be `0` in cases where it is for bytes.
/// If `Seek` is implemented then it should preserve the position if it is before the end
/// if the position is after the end then it should be set to the last valid position
pub trait Truncate {
    fn truncate(&mut self, new_len: u64) -> std::io::Result<()>;
}

impl Truncate for File {
    fn truncate(&mut self, new_len: u64) -> std::io::Result<()> {
        self.set_len(new_len)
    }
}

impl Truncate for Cursor<&mut Vec<u8>> {
    fn truncate(&mut self, new_len: u64) -> std::io::Result<()> {
        let position = self.position();
        if position >= new_len {
            // TODO: check this. Is 0 a sensible value? also will this be good?
            self.set_position(new_len.saturating_sub(1));
        }

        let new_len = new_len.into_usize();
        // SANITY: Since messing with the underlying vector as we do can mess with the position
        // we manually make sure the position is within bounds above.
        self.get_mut().resize(new_len, 0);
        Ok(())
    }
}

impl Truncate for Cursor<Vec<u8>> {
    fn truncate(&mut self, new_len: u64) -> std::io::Result<()> {
        let position = self.position();
        if position >= new_len {
            // TODO: check this. Is 0 a sensible value? also will this be good?
            self.set_position(new_len.saturating_sub(1));
        }

        let new_len = new_len.into_usize();
        // SANITY: Since messing with the underlying vector as we do can mess with the position
        // we manually make sure the position is within bounds above.
        self.get_mut().resize(new_len, 0);
        Ok(())
    }
}

#[cfg(feature = "tempfile")]
impl Truncate for tempfile::NamedTempFile {
    fn truncate(&mut self, new_len: u64) -> std::io::Result<()> {
        self.as_file_mut().truncate(new_len)
    }
}
#[cfg(feature = "tempfile")]
impl Truncate for tempfile::SpooledTempFile {
    fn truncate(&mut self, new_len: u64) -> std::io::Result<()> {
        self.set_len(new_len)
    }
}
