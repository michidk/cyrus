//! Portable cheat state; preservation owns CHT names and binary encoding.
use serde::Serialize;

/// A named cheat or unnamed persistence slot, retaining its original integer value.
#[derive(Clone, Debug, Serialize)]
pub struct CheatEntry {
    pub name: Option<String>,
    pub value: u32,
}

/// Ordered state decoded from REDGUARD.CHT, including unnamed slots.
#[derive(Clone, Debug, Serialize)]
pub struct CheatFile {
    entries: Vec<CheatEntry>,
}

impl CheatFile {
    pub fn entries(&self) -> &[CheatEntry] {
        &self.entries
    }

    pub fn value(&self, name: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.name.as_deref() == Some(name))
            .map(|e| e.value)
    }

    /// Unknown names leave every slot unchanged.
    pub fn set(&mut self, name: &str, value: u32) -> anyhow::Result<()> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.name.as_deref() == Some(name))
            .ok_or_else(|| anyhow::anyhow!("unknown cheat {name:?}"))?;
        entry.value = value;
        Ok(())
    }
}

/// Decode original bytes without performing filesystem I/O.
#[cfg(feature = "parsers")]
pub fn decode(bytes: &[u8]) -> anyhow::Result<CheatFile> {
    Ok(CheatFile {
        entries: rgpre::import::cht::parse_cht_file(bytes)?
            .entries
            .into_iter()
            .map(|entry| CheatEntry {
                name: entry.name.map(str::to_owned),
                value: entry.value,
            })
            .collect(),
    })
}

/// All-zero state when neither installed nor local CHT state exists.
#[cfg(feature = "parsers")]
pub fn empty() -> CheatFile {
    decode(&[0; rgpre::import::cht::CHT_FILE_SIZE]).expect("valid empty CHT")
}

/// Encode through preservation, retaining the complete original slot array.
#[cfg(feature = "parsers")]
pub fn encode(state: &CheatFile) -> Vec<u8> {
    let values: [u32; rgpre::import::cht::CHT_SLOT_COUNT] = state
        .entries
        .iter()
        .map(|entry| entry.value)
        .collect::<Vec<_>>()
        .try_into()
        .expect("CheatFile can only be constructed by the CHT decoder");
    rgpre::import::cht::encode_cht_file(&values).to_vec()
}
