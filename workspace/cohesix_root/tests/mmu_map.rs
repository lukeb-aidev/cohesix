// CLASSIFICATION: COMMUNITY
// Filename: mmu_map.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2028-01-21

use serde_json::json;

fn init_tables(l1: &mut [u64; 512], l2: &mut [u64; 512], dtb: usize, dtb_end: usize) {
    const BLOCK_FLAGS: u64 = 0b11;
    const DEVICE_FLAGS: u64 = 0b11 | (1 << 2);

    for e in l1.iter_mut() { *e = 0; }
    for e in l2.iter_mut() { *e = 0; }

    l1[0] = (l2.as_ptr() as u64) | 0b11;
    for i in 0..16 {
        l2[i] = ((i as u64) << 21) | BLOCK_FLAGS;
    }
    let dtb_idx = dtb >> 21;
    let dtb_end_idx = (dtb_end + 0x1fffff) >> 21;
    for i in dtb_idx..dtb_end_idx {
        if i < 512 {
            l2[i] = ((i as u64) << 21) | DEVICE_FLAGS;
        }
    }
}

#[test]
fn mmu_map_snapshot() {
    let mut l1 = [0u64; 512];
    let mut l2 = [0u64; 512];
    init_tables(&mut l1, &mut l2, 0x300000, 0x350000);
    let state = json!({"l1": l1, "l2": l2});
    let golden: serde_json::Value = serde_json::from_str(include_str!("golden/mmu_map.json")).unwrap();
    assert_eq!(state, golden);
}
