mod constrained_wrapper;

mod hiex;
pub use crate::hiex::*;
pub mod action;

/// Get position in stream using seeks.
/// FIXME: This only exists since the rust version is currently only in nightly
pub(crate) fn stream_position<S>(mut seeker: S) -> std::io::Result<u64>
where
    S: std::io::Seek,
{
    // Seeking to the current position gives our position
    seeker.seek(std::io::SeekFrom::Current(0))
}

/// Get the stream length using seeks
/// If it succeeds, the seek position is not changed.
/// If there was an error then the position is unspecified.
/// FIXME: This only exists since the rust version is currently only in nightly
/// If this errors, then the position in `seeker` is not defined.
pub(crate) fn stream_len<S>(mut seeker: S) -> std::io::Result<u64>
where
    S: std::io::Seek,
{
    // Get the current position, so that we can restore our position.
    let position = stream_position(&mut seeker)?;
    let length = seeker.seek(std::io::SeekFrom::End(0))?;

    // If we're still at the starting position, let's not seek again.
    if position != length {
        seeker.seek(std::io::SeekFrom::Start(position))?;
    }

    Ok(length)
}
