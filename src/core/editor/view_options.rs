use crate::core::encoding::Encoding;
use crate::core::radix::{ByteGroupSize, DisplayRadix};

/// Display and representation settings for binary viewing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    pub encoding: Encoding,
    pub radix: DisplayRadix,
    pub group_size: ByteGroupSize,
    pub is_big_endian: bool,
}
