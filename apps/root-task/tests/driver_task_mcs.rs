// Author: Lukas Bower
// Purpose: Verify generated active-SC and least-authority MCS driver inventory.
// Copyright 2026 Lukas Bower

use pi4_driver_abi::{
    driver_runtime_command_badge, driver_runtime_completion_badge,
    driver_runtime_standard_fault_badge, DRIVER_RUNTIME_COMMAND_ENDPOINT_SLOT,
    DRIVER_RUNTIME_COMMAND_REPLY_SLOT, DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT,
    DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
};
use root_task::hal::driver_task::{driver_task_temporal_config, DriverTaskHotPath};

#[test]
fn qemu_profile_excludes_pi_only_linked_driver_temporal_records() {
    let pi_only_hot_paths = [
        DriverTaskHotPath::SerialConsole,
        DriverTaskHotPath::UsbKeyboard,
        DriverTaskHotPath::HdmiText,
        DriverTaskHotPath::GenetNic,
        DriverTaskHotPath::Cyw43Wifi,
        DriverTaskHotPath::SdioHost,
        DriverTaskHotPath::PcieRoot,
    ];

    for hot_path in pi_only_hot_paths {
        assert!(
            driver_task_temporal_config(hot_path).is_none(),
            "QEMU must not fabricate an admitted Pi linked-driver TCB for {hot_path:?}"
        );
    }
    assert_eq!(
        root_task::generated::worker_resource_admission_config()
            .handoff
            .driver_fault_records,
        0
    );
}

#[test]
fn driver_badge_domains_and_slots_are_pairwise_disjoint() {
    let task_key = 7;
    let badges = [
        driver_runtime_command_badge(task_key),
        driver_runtime_completion_badge(task_key),
        driver_runtime_standard_fault_badge(task_key),
    ];
    for left in 0..badges.len() {
        for right in left + 1..badges.len() {
            assert_ne!(badges[left], badges[right]);
        }
    }
    let slots = [
        DRIVER_RUNTIME_COMMAND_ENDPOINT_SLOT,
        DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
        DRIVER_RUNTIME_COMMAND_REPLY_SLOT,
        DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT,
    ];
    for left in 0..slots.len() {
        for right in left + 1..slots.len() {
            assert_ne!(slots[left], slots[right]);
        }
    }
}
