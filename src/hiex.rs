use crate::{
    action::{Action, ActionError, ActionList, MemoryUsage},
    stream_len,
};
use std::io::{Read, Seek, SeekFrom, Write};
use usize_cast::FromUsize;

// TODO: write a WriteWrapper that stores the data that is being written in an efficient structure
// this would be useful for things like memory, where it doesn't make complete sense
/// F is the type of reader
/// E is the arguments passed to actions when they are being done/undone
pub struct Hiex<F, E>
where
    F: Read + Seek + Write,
{
    reader: F,
    pub actions: ActionList<F, E>,
}
impl<F, E> Hiex<F, E>
where
    F: Read + Seek + Write,
{
    /// NOTE: This will directly write to the reader!
    /// You may want to give it a copy.
    pub fn from_reader(reader: F) -> std::io::Result<Self> {
        Ok(Hiex {
            reader,
            actions: ActionList::new(),
        })
    }

    /// Gets the inner reader
    pub fn into_inner(self) -> F {
        self.reader
    }

    pub fn into_inner_actions(self) -> ActionList<F, E> {
        self.actions
    }

    // FIXME: replace this with an actual call once stream_position is stabilized
    /// Position into the reader.
    /// Uses `std::io::Seek::stream_position` internally.
    pub fn position(&mut self) -> std::io::Result<u64> {
        self.seek(SeekFrom::Current(0))
    }

    // FIXME: replace this with an actual call once `stream_len` is stabilized
    /// Size of the data in reader
    /// Uses `std::io::Seek::stream_len` internally.
    pub fn length(&mut self) -> std::io::Result<u64> {
        // Get the current position, so that we can restore our position.
        let position = self.position()?;
        let length = self.seek(SeekFrom::End(0))?;

        // If we're still at the starting position, let's not seek again.
        if position != length {
            self.seek(SeekFrom::Start(position))?;
        }

        Ok(length)
    }

    pub fn add_action<A>(&mut self, action: A, other: E) -> Result<(), (A, ActionError)>
    where
        A: 'static + Action<F, E>,
    {
        self.actions.add(action, &mut self.reader, other)
    }

    pub fn undo(&mut self, other: E) -> Result<Option<()>, ActionError> {
        self.actions.undo(&mut self.reader, other)
    }

    pub fn redo(&mut self, other: E) -> Result<Option<()>, ActionError> {
        self.actions.redo(&mut self.reader, other)
    }

    /// Seeks to position, then calls `read_exact`
    pub fn read_at(&mut self, position: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.seek(SeekFrom::Start(position))?;
        self.read_exact(buf)
    }

    /// Reads as much as it can at current position
    /// The returned vector has `<= amount` bytes within it.
    /// `amount` is limited to usize, as the vector's size is limited to usize.
    /// Minor note: the buffer returned may have a `capacity == amount` even if it read less data
    /// So may be using somewhat more memory than it needed.
    pub fn read_amount(&mut self, amount: usize) -> std::io::Result<Vec<u8>> {
        // TODO: we could optimize this with seeks. Get the stream length and our position, then
        // get how many bytes are left and create the vector with that amount.
        let mut buffer = Vec::with_capacity(amount);
        {
            // Get a reference, since take consumes the value we give it.
            let reference = Read::by_ref(self);
            // Read at most `amount` bytes
            reference
                .take(u64::from_usize(amount))
                .read_to_end(&mut buffer)?;
        }

        Ok(buffer)
    }

    /// Reads as much as it can
    /// The returned vector has `<= amount` bytes within it.
    /// `amount` is limited to usize, as the vector's size is limited to usize.
    pub fn read_amount_at(&mut self, position: u64, amount: usize) -> std::io::Result<Vec<u8>> {
        self.seek(SeekFrom::Start(position))?;
        self.read_amount(amount)
    }

    // /// Seeks to position, then calls `write_all`
    // pub fn write_at(&mut self, position: u64, buf: &[u8]) -> std::io::Result<()> {
    //     self.seek(SeekFrom::Start(position))?;
    //     self.write_all(buf)
    // }

    /// Seeks to start of self, and starts copying data over to the `writer`.
    /// NOTE: It will start copying to where the `writer` is at when given! It does not seek the
    /// `writer` to the start!
    pub fn save_to<W>(&mut self, mut writer: W) -> std::io::Result<()>
    where
        W: Write,
    {
        self.seek(SeekFrom::Start(0))?;
        std::io::copy(self, &mut writer)?;
        Ok(())
    }
}

// NOTE: Writing should be done via adding an edit action :)
// // Write + Read + Seek implementation for niceness
// impl<F> Write for Hiex<F>
// where
//     F: Read + Seek + Write,
// {
//     fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
//         let position = self.position()?;
//         self.actions.add(EditAction::new(position, buf));
//         self.reader.write(buf)
//     }

//     fn flush(&mut self) -> std::io::Result<()> {
//         self.reader.flush()
//     }
// }
impl<F, E> Read for Hiex<F, E>
where
    F: Read + Seek + Write,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}
impl<F, E> Seek for Hiex<F, E>
where
    F: Read + Seek + Write,
{
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(pos)
    }
}

/// An action where bytes are edited
/// NOTE: if bytes written would increase the size of the file then that is an _error_
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EditAction {
    pub position: u64,
    previous_data: Vec<u8>,
    pub new_data: Vec<u8>,
}
impl EditAction {
    pub fn new(position: u64, new_data: Vec<u8>) -> Self {
        Self {
            position,
            new_data,
            previous_data: Vec::new(),
        }
    }
}
impl<F, E> Action<F, E> for EditAction
where
    F: Read + Seek + Write,
{
    fn apply(&mut self, mut data: &mut F, _other: E) -> Result<(), ActionError> {
        let length = stream_len(&mut data)?;
        let new_data_len = u64::from_usize(self.new_data.len());
        println!(
            "Position: {}, Length: {}, new_data_len: {}",
            self.position, length, new_data_len
        );
        // If we would exceed the file size then the action was invalid to perform.
        if self.position.saturating_add(new_data_len) >= length {
            return Err(ActionError::Invalid);
        }

        // Read in the data to store it for if the action is undone.
        data.seek(SeekFrom::Start(self.position))?;
        self.previous_data.resize(self.new_data.len(), 0);
        data.read_exact(&mut self.previous_data)?;

        // TODO: if this fails, try writing previous data?
        data.seek(SeekFrom::Start(self.position))?;
        data.write_all(&self.new_data)?;

        Ok(())
    }

    fn unapply(&mut self, data: &mut F, _other: E) -> Result<(), ActionError> {
        data.seek(SeekFrom::Start(self.position))?;
        data.write_all(&self.previous_data)?;
        Ok(())
    }
}
impl MemoryUsage for EditAction {
    fn memory_usage(&self) -> usize {
        8 + self.previous_data.len() + self.new_data.len()
    }
}
