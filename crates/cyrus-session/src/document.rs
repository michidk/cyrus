//! Versioned UTF-8 document framing shared by native and browser storage.
use crate::RuntimeSessionSnapshot;
use anyhow::{Context, Result, ensure};

const HEADER: &[u8] = b"CYRUS-SESSION 1\n";
const DIGEST_LEN: usize = 64;

impl RuntimeSessionSnapshot {
    /// Encode all represented owners as one UTF-8 document with an integrity digest.
    /// The host publishes these bytes transactionally. This draft format is not an
    /// original-game save and does not establish complete gameplay admission.
    pub fn encode_document(&self) -> Result<Vec<u8>> {
        ensure!(
            self.version == 1,
            "session document snapshot version unsupported"
        );
        let payload = serde_json::to_vec(self).context("encode session document payload")?;
        let digest = blake3::hash(&payload).to_hex();
        let mut document = Vec::with_capacity(HEADER.len() + DIGEST_LEN + 1 + payload.len());
        document.extend_from_slice(HEADER);
        document.extend_from_slice(digest.as_bytes());
        document.push(b'\n');
        document.extend_from_slice(&payload);
        Ok(document)
    }

    /// Decode and integrity-check a document without loading assets or changing state.
    /// A valid digest is not authentication or installed-source validation. Call
    /// `plan`, `finish`, and `commit` to validate sources, bindings and publication.
    /// Hosts enforce their storage/input size limits before supplying the bytes.
    pub fn decode_document(document: &[u8]) -> Result<Self> {
        let body = document
            .strip_prefix(HEADER)
            .context("session document format/version unsupported")?;
        ensure!(
            body.len() > DIGEST_LEN && body[DIGEST_LEN] == b'\n',
            "session document integrity header truncated or malformed"
        );
        let (digest, payload) = body.split_at(DIGEST_LEN);
        let payload = &payload[1..];
        ensure!(
            blake3::hash(payload).to_hex().as_bytes() == digest,
            "session document payload integrity mismatch"
        );
        let snapshot: Self =
            serde_json::from_slice(payload).context("decode session document payload")?;
        ensure!(
            snapshot.version == 1,
            "session document snapshot version unsupported"
        );
        Ok(snapshot)
    }
}
