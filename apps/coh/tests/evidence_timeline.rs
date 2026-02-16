// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate evidence timeline generation behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::evidence_timeline::write_timeline;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn evidence_timeline_is_deterministic_for_fixed_pack() -> Result<()> {
    let temp = TempDir::new().expect("tempdir");
    let pack = temp.path();

    std::fs::create_dir_all(pack.join("audit"))?;
    std::fs::create_dir_all(pack.join("proc").join("lease"))?;

    // Out-of-order seq to validate sorting.
    let journal = concat!(
        "{\"seq\":2,\"kind\":\"queen-ctl\",\"path\":\"/queen/ctl\",\"payload\":\"{}\",\"outcome\":\"ok\",\"error\":null,\"role\":\"queen\",\"ticket\":\"sha256:dead\"}\n",
        "{\"seq\":1,\"kind\":\"queen-ctl\",\"path\":\"/queen/ctl\",\"payload\":\"{}\",\"outcome\":\"ok\",\"error\":null,\"role\":\"queen\",\"ticket\":\"sha256:beef\"}\n"
    );
    std::fs::write(pack.join("audit").join("journal"), journal)?;

    let decisions = "{\"seq\":3,\"kind\":\"policy-gate\",\"outcome\":\"approve\",\"id\":\"a1\",\"target\":\"/queen/ctl\",\"path\":\"/queen/ctl\",\"role\":\"queen\",\"ticket\":\"sha256:cafe\"}\n";
    std::fs::write(pack.join("audit").join("decisions"), decisions)?;

    let lease_active = "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=7\n";
    std::fs::write(pack.join("proc").join("lease").join("active"), lease_active)?;

    let summary = write_timeline(pack)?;
    let ndjson = std::fs::read_to_string(&summary.ndjson_path)
        .with_context(|| format!("read {}", summary.ndjson_path.display()))?;
    let lines = ndjson
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);

    let first: Value = serde_json::from_str(lines[0]).context("parse first ndjson")?;
    assert_eq!(first.get("seq").and_then(Value::as_u64), Some(1));

    let second: Value = serde_json::from_str(lines[1]).context("parse second ndjson")?;
    assert_eq!(second.get("seq").and_then(Value::as_u64), Some(2));

    Ok(())
}

