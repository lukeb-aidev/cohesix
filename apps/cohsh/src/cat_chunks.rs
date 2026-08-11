// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Strictly reassemble bounded versioned long-line CAT responses.
// Author: Lukas Bower

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

pub(crate) const CAT_CHUNK_PREFIX: &str = "C1:";
pub(crate) const CAT_CHUNK_MAX_WIRE_BYTES: usize = 256;
pub(crate) const CAT_CHUNK_MAX_COUNT: usize = 64;
pub(crate) const CAT_CHUNK_REASSEMBLED_MAX_BYTES: usize = 2048;

struct Chunk<'a> {
    sequence: usize,
    count: usize,
    digest: &'a str,
    payload: &'a str,
}

pub(crate) fn reassemble_cat_chunks(lines: Vec<String>) -> Result<Vec<String>> {
    let mut output = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        if !lines[index].starts_with(CAT_CHUNK_PREFIX) {
            output.push(lines[index].clone());
            index += 1;
            continue;
        }

        let first = parse_chunk(lines[index].as_str())?;
        if first.sequence != 0 {
            return Err(anyhow!(
                "CAT chunk group starts at sequence {}, expected 0",
                first.sequence
            ));
        }
        let mut reconstructed = String::new();
        for expected_sequence in 0..first.count {
            let line = lines.get(index + expected_sequence).ok_or_else(|| {
                anyhow!("CAT chunk group ended before sequence {expected_sequence}")
            })?;
            let chunk = parse_chunk(line.as_str())?;
            if chunk.sequence != expected_sequence
                || chunk.count != first.count
                || chunk.digest != first.digest
            {
                return Err(anyhow!(
                    "CAT chunk sequence/count/digest mismatch at sequence {expected_sequence}"
                ));
            }
            let next_len = reconstructed
                .len()
                .checked_add(chunk.payload.len())
                .ok_or_else(|| anyhow!("CAT chunk length overflow"))?;
            if next_len > CAT_CHUNK_REASSEMBLED_MAX_BYTES {
                return Err(anyhow!(
                    "CAT chunk group exceeds {} bytes",
                    CAT_CHUNK_REASSEMBLED_MAX_BYTES
                ));
            }
            reconstructed.push_str(chunk.payload);
        }
        let actual = hex::encode(Sha256::digest(reconstructed.as_bytes()));
        if actual != first.digest {
            return Err(anyhow!(
                "CAT chunk digest mismatch: expected {}, got {actual}",
                first.digest
            ));
        }
        output.push(reconstructed);
        index += first.count;
    }
    Ok(output)
}

fn parse_chunk(line: &str) -> Result<Chunk<'_>> {
    if line.len() > CAT_CHUNK_MAX_WIRE_BYTES {
        return Err(anyhow!(
            "CAT chunk exceeds {CAT_CHUNK_MAX_WIRE_BYTES} wire bytes"
        ));
    }
    let mut fields = line.splitn(5, ':');
    let version = fields.next();
    let sequence = fields.next();
    let count = fields.next();
    let digest = fields.next();
    let payload = fields.next();
    let (Some("C1"), Some(sequence), Some(count), Some(digest), Some(payload)) =
        (version, sequence, count, digest, payload)
    else {
        return Err(anyhow!("malformed CAT chunk header"));
    };
    let sequence = parse_fixed_hex(sequence, "sequence")?;
    let count = parse_fixed_hex(count, "count")?;
    if count == 0 || count > CAT_CHUNK_MAX_COUNT || sequence >= count || payload.is_empty() {
        return Err(anyhow!("CAT chunk carries invalid sequence/count/payload"));
    }
    if digest.len() != 64
        || digest.bytes().any(|byte| {
            !byte.is_ascii_hexdigit() || (byte.is_ascii_alphabetic() && byte.is_ascii_uppercase())
        })
    {
        return Err(anyhow!("CAT chunk digest is not lowercase SHA-256 hex"));
    }
    Ok(Chunk {
        sequence,
        count,
        digest,
        payload,
    })
}

fn parse_fixed_hex(value: &str, label: &str) -> Result<usize> {
    if value.len() != 4
        || value.bytes().any(|byte| {
            !byte.is_ascii_hexdigit() || (byte.is_ascii_alphabetic() && byte.is_ascii_uppercase())
        })
    {
        return Err(anyhow!(
            "CAT chunk {label} is not four lowercase hex digits"
        ));
    }
    usize::from_str_radix(value, 16).map_err(|_| anyhow!("invalid CAT chunk {label}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(value: &str, chunk_bytes: usize) -> Vec<String> {
        let digest = hex::encode(Sha256::digest(value.as_bytes()));
        let chunks = value.as_bytes().chunks(chunk_bytes).collect::<Vec<_>>();
        chunks
            .iter()
            .enumerate()
            .map(|(sequence, chunk)| {
                format!(
                    "C1:{sequence:04x}:{:04x}:{digest}:{}",
                    chunks.len(),
                    core::str::from_utf8(chunk).expect("ASCII fixture")
                )
            })
            .collect()
    }

    fn utf8_frames(value: &str, chunk_bytes: usize) -> Vec<String> {
        let digest = hex::encode(Sha256::digest(value.as_bytes()));
        let mut payloads = Vec::new();
        let mut start = 0usize;
        while start < value.len() {
            let mut end = start.saturating_add(chunk_bytes).min(value.len());
            while end > start && !value.is_char_boundary(end) {
                end -= 1;
            }
            assert!(end > start, "fixture chunk must fit one UTF-8 scalar");
            payloads.push(&value[start..end]);
            start = end;
        }
        payloads
            .iter()
            .enumerate()
            .map(|(sequence, payload)| {
                format!(
                    "C1:{sequence:04x}:{:04x}:{digest}:{payload}",
                    payloads.len()
                )
            })
            .collect()
    }

    #[test]
    fn reassembles_canonical_json_without_touching_normal_lines() {
        let json = format!(
            "{{\"schema\":\"host-ticket-result/v2\",\"message\":\"{}\"}}",
            "x".repeat(500)
        );
        let mut input = vec!["normal".to_owned()];
        input.extend(frames(json.as_str(), 96));
        input.push("tail".to_owned());
        let output = reassemble_cat_chunks(input).expect("reassemble");
        assert_eq!(output, vec!["normal", json.as_str(), "tail"]);
        let parsed: serde_json::Value = serde_json::from_str(&output[1]).expect("canonical JSON");
        assert_eq!(parsed["schema"], "host-ticket-result/v2");
    }

    #[test]
    fn reassembles_multibyte_json_at_utf8_boundaries() {
        let json = format!(
            "{{\"schema\":\"host-ticket-result/v2\",\"message\":\"{}\"}}",
            "🙂".repeat(300)
        );
        let output = reassemble_cat_chunks(utf8_frames(json.as_str(), 177))
            .expect("reassemble UTF-8 chunks");
        assert_eq!(output, vec![json]);
    }

    #[test]
    fn rejects_partial_reordered_replayed_and_mixed_digest_groups() {
        let source = "x".repeat(500);
        let valid = frames(source.as_str(), 96);

        let mut partial = valid.clone();
        partial.pop();
        assert!(reassemble_cat_chunks(partial).is_err());

        let mut reordered = valid.clone();
        reordered.swap(1, 2);
        assert!(reassemble_cat_chunks(reordered).is_err());

        let mut replayed = valid.clone();
        replayed.insert(2, replayed[1].clone());
        assert!(reassemble_cat_chunks(replayed).is_err());

        let mut mixed = valid;
        mixed[1].replace_range(13..77, &"0".repeat(64));
        assert!(reassemble_cat_chunks(mixed).is_err());
    }

    #[test]
    fn rejects_oversized_or_noncanonical_headers() {
        let oversized = "x".repeat(CAT_CHUNK_REASSEMBLED_MAX_BYTES + 1);
        assert!(reassemble_cat_chunks(frames(oversized.as_str(), 96)).is_err());

        let source = "x".repeat(300);
        let mut uppercase = frames(source.as_str(), 96);
        uppercase[0].replace_range(3..7, "000A");
        assert!(reassemble_cat_chunks(uppercase).is_err());
    }
}
