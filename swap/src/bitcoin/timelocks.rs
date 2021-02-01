use serde::{Deserialize, Serialize};
use std::ops::Add;

/// Represent a block height, or block number, expressed in absolute block
/// count. E.g. The transaction was included in block #655123, 655123 block
/// after the genesis block.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockHeight(u32);

impl From<BlockHeight> for u32 {
    fn from(height: BlockHeight) -> Self {
        height.0
    }
}

impl BlockHeight {
    pub const fn new(block_height: u32) -> Self {
        Self(block_height)
    }
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(result) => Some(BlockHeight(result)),
            None => None,
        }
    }
}

impl Add<u32> for BlockHeight {
    type Output = BlockHeight;
    fn add(self, rhs: u32) -> Self::Output {
        BlockHeight(self.0 + rhs)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ExpiredTimelocks {
    None,
    Cancel,
    Punish,
}
