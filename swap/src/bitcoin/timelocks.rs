use serde::{Deserialize, Serialize};
use std::ops::Add;

/// Represent a timelock, expressed in relative block height as defined in
/// [BIP68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki).
/// E.g. The timelock expires 10 blocks after the reference transaction is
/// mined.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct Timelock(u32);

impl Timelock {
    pub const fn new(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl From<Timelock> for u32 {
    fn from(timelock: Timelock) -> Self {
        timelock.0
    }
}

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
}

impl Add<Timelock> for BlockHeight {
    type Output = BlockHeight;

    fn add(self, rhs: Timelock) -> Self::Output {
        BlockHeight(self.0 + rhs.0)
    }
}
