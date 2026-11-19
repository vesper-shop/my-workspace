use crate::api::memos::MemoView;
use serde::Serialize;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::api::moment::Photo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Memos,
    Moment,
    Knowledge,
}

impl TryFrom<&str> for Channel {
    type Error = ConsumerError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "memos" => Ok(Self::Memos),
            "moment" => Ok(Self::Moment),
            "knowledge" => Ok(Self::Knowledge),
            invalid_channel => Err(ConsumerError::UnknownChannel(invalid_channel.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "channel",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChannelView {
    Memos {
        memos: Vec<MemoView>,
        next_cursor: Option<String>,
    },
    Moment {
        photos: Vec<Photo>,
        total: usize,
    },
    Knowledge {
        knowledge: Vec<crate::api::knowledge::Entry>,
        next_cursor: Option<String>,
    },
}

#[derive(Debug)]
pub enum ConsumerError {
    UnknownChannel(String),
}

impl Display for ConsumerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownChannel(channel) => {
                write!(formatter, "unknown consumer channel: {channel}")
            }
        }
    }
}

impl Error for ConsumerError {}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
