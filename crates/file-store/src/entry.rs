use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Error;

pub const KEY_LEN_LEN: usize = 2;
pub const EXPIRY_LEN: usize = 8;
pub const HEADER_LEN: usize = KEY_LEN_LEN + EXPIRY_LEN;

pub struct FileEntry {
    pub key: String,
    pub expiry_ms: u64,
    pub value: Vec<u8>,
}

impl TryFrom<&[u8]> for FileEntry {
    type Error = Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() < HEADER_LEN {
            return Err(Error::Corrupt("entry too short to contain header".into()));
        }

        let expiry_ms = u64::from_le_bytes(value[..EXPIRY_LEN].try_into().unwrap());

        let key_len =
            u16::from_le_bytes(value[EXPIRY_LEN..HEADER_LEN].try_into().unwrap()) as usize;

        let key_end = HEADER_LEN + key_len;
        if value.len() < key_end {
            return Err(Error::Corrupt("entry truncated in key field".into()));
        }

        let key = String::from_utf8(value[HEADER_LEN..key_end].to_vec())
            .map_err(|e| Error::Corrupt(format!("invalid UTF-8 in key: {e}")))?;

        let value = value[key_end..].to_vec();

        Ok(Self {
            expiry_ms,
            key,
            value,
        })
    }
}

impl FileEntry {
    pub fn new(key: String, value: Vec<u8>) -> Self {
        Self {
            key,
            value,
            expiry_ms: 0,
        }
    }

    pub fn with_ttl(mut self, ttl_ms: u32) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.expiry_ms = now_ms + ttl_ms as u64;
        self
    }

    pub fn encode(&self) -> Vec<u8> {
        let key_bytes = self.key.as_bytes();
        let mut buf = Vec::with_capacity(HEADER_LEN + key_bytes.len() + self.value.len());
        buf.extend_from_slice(&self.expiry_ms.to_le_bytes());
        buf.extend_from_slice(&(key_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(key_bytes);
        buf.extend_from_slice(&self.value);
        buf
    }

    pub fn is_expired(&self) -> bool {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.expiry_ms != 0 && self.expiry_ms <= now_ms
    }
}
