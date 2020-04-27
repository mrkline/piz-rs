use crate::result::*;

/// A checked cast from u64 to usize
///
/// We could use the `cast` crate,
/// (https://docs.rs/cast/0.2.3/cast/)
/// but this is the only one we really need.
pub fn usize<I: Into<u64>>(i: I) -> ZipResult<usize> {
    let i: u64 = i.into();
    if cfg!(target_pointer_width = "64") {
        Ok(i as usize)
    } else {
        if i > usize::MAX as u64 {
            Err(ZipError::InsufficientAddressSpace)
        } else {
            Ok(i as usize)
        }
    }
}
