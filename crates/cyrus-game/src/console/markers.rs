//! The user marker file of `setmarker` and `getmarker` and the host storage seam for it. The layout
//! is preservation's `engine/console.md` marker file section; cyrus keeps its own file outside the
//! install and never writes the original's.
use crate::script::SavedPlacement;
use anyhow::{Result, ensure};
use bevy::prelude::Resource;

const MAGIC: [u8; 4] = *b"UMRK";
const TRAILER: [u8; 4] = *b"END ";
const HEADER_LEN: usize = 12;
const RECORD_LEN: usize = 56;

/// One saved marker: the world and number it is keyed by, and the state it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerRecord {
    pub world: u32,
    pub number: i32,
    pub placement: SavedPlacement,
}

impl MarkerRecord {
    fn words(&self) -> [i32; 14] {
        let placement = &self.placement;
        let mut words = [0; 14];
        words[0] = self.world as i32;
        words[1] = self.number;
        words[2..5].copy_from_slice(&placement.position);
        words[5..8].copy_from_slice(&placement.orientation);
        words[8..11].copy_from_slice(&placement.camera_position);
        words[11..14].copy_from_slice(&placement.camera_angles);
        words
    }

    fn from_words(words: [i32; 14]) -> Self {
        let triple = |start: usize| [words[start], words[start + 1], words[start + 2]];
        Self {
            world: words[0] as u32,
            number: words[1],
            placement: SavedPlacement {
                position: triple(2),
                orientation: triple(5),
                camera_position: triple(8),
                camera_angles: triple(11),
            },
        }
    }
}

/// Parse a marker file. The magic, the zero format tag, the size field (4 plus 56 per record),
/// the total length and the trailer must all agree; anything else is an error naming the check.
pub fn decode(bytes: &[u8]) -> Result<Vec<MarkerRecord>> {
    ensure!(
        bytes.len() >= HEADER_LEN + TRAILER.len(),
        "marker file is truncated: {} bytes",
        bytes.len()
    );
    ensure!(bytes[..4] == MAGIC, "marker file: wrong magic");
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let (size, tag) = (word(4) as usize, word(8));
    ensure!(tag == 0, "marker file: unsupported format tag {tag}");
    ensure!(
        size >= 4 && (size - 4) % RECORD_LEN == 0,
        "marker file: size field {size} is not 4 plus whole records"
    );
    let count = (size - 4) / RECORD_LEN;
    ensure!(
        bytes.len() == HEADER_LEN + count * RECORD_LEN + TRAILER.len(),
        "marker file: {} bytes do not match {count} records",
        bytes.len()
    );
    ensure!(
        bytes[bytes.len() - 4..] == TRAILER,
        "marker file: trailer missing"
    );
    Ok(bytes[HEADER_LEN..bytes.len() - TRAILER.len()]
        .as_chunks::<RECORD_LEN>()
        .0
        .iter()
        .map(|chunk| {
            let mut words = [0; 14];
            for (word, bytes) in words.iter_mut().zip(chunk.as_chunks::<4>().0.iter()) {
                *word = i32::from_le_bytes(*bytes);
            }
            MarkerRecord::from_words(words)
        })
        .collect())
}

/// Read existing records for `setmarker`. A wrong magic or tag starts a new file;
/// other malformed files are refused before the host can overwrite retained records.
pub fn decode_for_update(bytes: &[u8]) -> Result<Vec<MarkerRecord>> {
    ensure!(bytes.len() >= HEADER_LEN, "marker file header is truncated");
    if bytes[..4] != MAGIC || bytes[8..12] != [0; 4] {
        return Ok(Vec::new());
    }
    decode(bytes)
}

/// Write a marker file for `records`, in order.
pub fn encode(records: &[MarkerRecord]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_LEN + records.len() * RECORD_LEN + TRAILER.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&((4 + records.len() * RECORD_LEN) as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for record in records {
        for word in record.words() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&TRAILER);
    bytes
}

/// Store `record`: an existing record of the same world and number is overwritten in place,
/// otherwise it is appended.
pub fn upsert(records: &mut Vec<MarkerRecord>, record: MarkerRecord) {
    match records
        .iter_mut()
        .find(|saved| saved.world == record.world && saved.number == record.number)
    {
        Some(saved) => *saved = record,
        None => records.push(record),
    }
}

/// The first record of `world` and `number`, in file order.
pub fn find(records: &[MarkerRecord], world: u32, number: i32) -> Option<&MarkerRecord> {
    records
        .iter()
        .find(|saved| saved.world == world && saved.number == number)
}

/// Host storage for the marker file bytes. A missing file is `None`; the host keeps the file in its
/// own local state, never in the install.
pub trait MarkerStore: Send + Sync + 'static {
    fn load(&self) -> Result<Option<Vec<u8>>>;
    fn save(&self, bytes: &[u8]) -> Result<()>;
}

/// The host's marker storage; without it `setmarker` and `getmarker` report it unavailable.
#[derive(Resource)]
pub struct MarkerStorage(pub Box<dyn MarkerStore>);

#[cfg(test)]
mod tests {
    use super::*;

    fn record(world: u32, number: i32, seed: i32) -> MarkerRecord {
        let triple = |base: i32| [base, base + 1, base + 2];
        MarkerRecord {
            world,
            number,
            placement: SavedPlacement {
                position: triple(seed),
                orientation: triple(seed + 10),
                camera_position: triple(seed + 20),
                camera_angles: triple(seed + 30),
            },
        }
    }

    #[test]
    fn an_empty_file_is_the_magic_a_size_of_four_a_zero_tag_and_the_trailer() {
        let bytes = encode(&[]);
        assert_eq!(bytes, b"UMRK\x04\0\0\0\0\0\0\0END ");
        assert_eq!(decode(&bytes).unwrap(), []);
    }

    #[test]
    fn records_round_trip_in_order_with_a_size_of_four_plus_fifty_six_each() {
        let records = [record(4, 1, 100), record(4, 2, -5), record(9, 1, 7)];
        let bytes = encode(&records);
        assert_eq!(bytes.len(), 12 + 3 * 56 + 4);
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            4 + 3 * 56
        );
        assert_eq!(decode(&bytes).unwrap(), records);
        // The first record's fourteen words are the world, number, position, orientation and the
        // two camera triples, little-endian.
        assert_eq!(&bytes[12..16], &4u32.to_le_bytes());
        assert_eq!(&bytes[16..20], &1i32.to_le_bytes());
        assert_eq!(&bytes[20..24], &100i32.to_le_bytes());
        assert_eq!(&bytes[32..36], &110i32.to_le_bytes());
        assert_eq!(&bytes[44..48], &120i32.to_le_bytes());
        assert_eq!(&bytes[56..60], &130i32.to_le_bytes());
    }

    #[test]
    fn upsert_overwrites_the_same_world_and_number_in_place_and_otherwise_appends() {
        let mut records = vec![record(4, 1, 0), record(4, 2, 0)];
        upsert(&mut records, record(4, 1, 50));
        assert_eq!(records, [record(4, 1, 50), record(4, 2, 0)]);
        upsert(&mut records, record(5, 1, 60));
        assert_eq!(records.len(), 3);
        assert_eq!(find(&records, 5, 1), Some(&record(5, 1, 60)));
        assert_eq!(find(&records, 4, 3), None);
    }

    #[test]
    fn find_returns_the_first_match_in_file_order() {
        let records = [record(4, 1, 1), record(4, 1, 2)];
        assert_eq!(find(&records, 4, 1), Some(&records[0]));
    }

    #[test]
    fn updates_replace_only_wrong_magic_or_tag_and_refuse_other_corruption() {
        let records = [record(1, 1, 20)];
        let good = encode(&records);
        assert_eq!(decode_for_update(&good).unwrap(), records);
        for index in [0, 8] {
            let mut bytes = good.clone();
            bytes[index] ^= 1;
            assert!(decode_for_update(&bytes).unwrap().is_empty());
        }
        let mut bad_size = good.clone();
        bad_size[4] = 5;
        assert!(decode_for_update(&bad_size).is_err());
        let mut bad_trailer = good.clone();
        *bad_trailer.last_mut().unwrap() = 0;
        assert!(decode_for_update(&bad_trailer).is_err());
        assert!(decode_for_update(&good[..10]).is_err());
    }

    #[test]
    fn every_disagreeing_header_field_is_rejected_with_its_check() {
        let good = encode(&[record(1, 1, 0)]);
        let mutated = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = good.clone();
            change(&mut bytes);
            decode(&bytes).unwrap_err().to_string()
        };
        assert!(mutated(&|b| b[0] = b'X').contains("wrong magic"));
        assert!(mutated(&|b| b[8] = 1).contains("format tag"));
        assert!(mutated(&|b| b[4] = 5).contains("whole records"));
        assert!(mutated(&|b| b[4] = 4 + 56 * 2).contains("do not match"));
        assert!(mutated(&|b| *b.last_mut().unwrap() = 0).contains("trailer"));
        assert!(mutated(&|b| b.truncate(10)).contains("truncated"));
    }
}
