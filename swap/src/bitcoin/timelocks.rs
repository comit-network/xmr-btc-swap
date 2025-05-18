use anyhow::Context;
use bdk_electrum::electrum_client::HeaderNotification;
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};
use std::ops::Add;
use typeshare::typeshare;

/// Represent a block height, or block number, expressed in absolute block
/// count.
///
/// E.g. The transaction was included in block #655123, 655123 blocks
/// after the genesis block.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockHeight(u32);

impl From<BlockHeight> for u32 {
    fn from(height: BlockHeight) -> Self {
        height.0
    }
}

impl From<u32> for BlockHeight {
    fn from(height: u32) -> Self {
        Self(height)
    }
}

impl TryFrom<HeaderNotification> for BlockHeight {
    type Error = anyhow::Error;

    fn try_from(value: HeaderNotification) -> Result<Self, Self::Error> {
        Ok(Self(
            value
                .height
                .try_into()
                .context("Failed to fit usize into u32")?,
        ))
    }
}

impl Add<u32> for BlockHeight {
    type Output = BlockHeight;
    fn add(self, rhs: u32) -> Self::Output {
        BlockHeight(self.0 + rhs)
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "type", content = "content")]
pub enum ExpiredTimelocks {
    None { blocks_left: u32 },
    Cancel { blocks_left: u32 },
    Punish,
}

impl ExpiredTimelocks {
    /// Check whether the timelock on the cancel transaction has expired.
    ///
    /// Retuns `true` even if the swap has already been canceled or punished.
    pub fn cancel_timelock_expired(&self) -> bool {
        !matches!(self, ExpiredTimelocks::None { .. })
    }
}
