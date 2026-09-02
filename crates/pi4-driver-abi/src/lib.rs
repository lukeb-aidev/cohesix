// Author: Lukas Bower
// Purpose: Define pointer-free ABI records shared by Pi 4 root and driver runtimes.
// Copyright 2026 Lukas Bower

#![no_std]
#![deny(unsafe_code)]

/// Magic value for a pointer-free driver runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_MAGIC: u32 = 0x4452_4934;
/// Runtime descriptor layout and shared-protocol version.
pub const DRIVER_RUNTIME_INIT_VERSION: u16 = 13;
/// Magic identifying the only Milestone 26e runtime scheduler contract.
pub const DRIVER_RUNTIME_MCS_MAGIC: u32 = 0x4d43_5331;
/// Version of the scheduler/capability inventory embedded in runtime init.
pub const DRIVER_RUNTIME_MCS_VERSION: u16 = 1;
/// The driver owns one active scheduling context bound only to its TCB.
pub const DRIVER_RUNTIME_MCS_FLAG_ACTIVE_SC: u16 = 1 << 0;
/// A dedicated Reply object receives one command association at a time.
pub const DRIVER_RUNTIME_MCS_FLAG_COMMAND_REPLY: u16 = 1 << 1;
/// Standard and timeout faults use independent supervisor Reply lanes.
pub const DRIVER_RUNTIME_MCS_FLAG_SPLIT_FAULT_REPLIES: u16 = 1 << 2;
/// The root supervisor admits at most one synchronous command per runtime.
pub const DRIVER_RUNTIME_MCS_FLAG_ONE_INFLIGHT: u16 = 1 << 3;
/// All scheduler flags required by the accepted MCS driver ABI.
pub const DRIVER_RUNTIME_MCS_REQUIRED_FLAGS: u16 = DRIVER_RUNTIME_MCS_FLAG_ACTIVE_SC
    | DRIVER_RUNTIME_MCS_FLAG_COMMAND_REPLY
    | DRIVER_RUNTIME_MCS_FLAG_SPLIT_FAULT_REPLIES
    | DRIVER_RUNTIME_MCS_FLAG_ONE_INFLIGHT;
/// Typed synchronous result returned when the driver supervisor contains a faulted Call.
pub const DRIVER_RUNTIME_MCS_FAULTED_CALL_RESULT: u32 = 0x4d43_5346;
/// Magic value for a sealed runtime identity inside an init descriptor.
pub const DRIVER_RUNTIME_IDENTITY_MAGIC: u32 = 0x4452_4944;
const DRIVER_RUNTIME_IDENTITY_HASH_SEED: u32 = 0x811c_9dc5;
const DRIVER_RUNTIME_IDENTITY_HASH_PRIME: u32 = 0x0100_0193;
/// Command `aux0` value used to submit a runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_AUX: u32 = 0x4452_494e;
/// Command `aux0` value used to ask a linked runtime to instantiate its engine state.
pub const DRIVER_RUNTIME_ENGINE_INIT_AUX: u32 = 0x454e_474e;
/// Command `aux0` value transferring an initialized GENET data plane to its
/// exact direct-link generation.
pub const DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_AUX: u32 = 0x4447_484f;
/// Completion detail proving GENET remains on the old path while it quiesces.
pub const DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING: u16 = 0x4700;
/// Completion detail proving the isolated GENET owner accepted the handoff.
pub const DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY: u16 = 0x4701;
/// Completion code used for successful bounded runtime progress.
pub const DRIVER_RUNTIME_COMPLETION_PROGRESS: u16 = 1;
/// Completion code used when a bounded runtime has no consumable work yet.
pub const DRIVER_RUNTIME_COMPLETION_IDLE: u16 = 3;
/// Local-seat USB/HDMI init command used by the root ring client.
pub const DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX: u32 = 0x4c53_494e;
/// Serial-console service command that samples the mini-UART transmitter-idle bit.
pub const DRIVER_RUNTIME_SERIAL_TX_IDLE_AUX: u32 = 0x5345_5244;
/// Serial control command that validates the current four-page SPSC generation.
pub const DRIVER_RUNTIME_SERIAL_SPSC_PROBE_AUX: u32 = 0x5353_5052;

const fn driver_runtime_nonzero_hash(hash: u32) -> u32 {
    if hash == 0 {
        DRIVER_RUNTIME_IDENTITY_MAGIC
    } else {
        hash
    }
}

/// Stable token binding one successful direct-GENET handoff to its generation.
#[must_use]
pub const fn driver_runtime_direct_genet_handoff_token(generation: u64) -> u32 {
    let mut hash = driver_runtime_identity_hash_word(
        DRIVER_RUNTIME_IDENTITY_HASH_SEED,
        DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_AUX,
    );
    hash = driver_runtime_identity_hash_word(hash, generation as u32);
    hash = driver_runtime_identity_hash_word(hash, (generation >> 32) as u32);
    driver_runtime_nonzero_hash(hash)
}

/// Return true only for the exact generation-bound GENET handoff completion.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the primitive-only helper mirrors the fixed completion record without importing an app type"
)]
pub const fn driver_runtime_direct_genet_handoff_completion_exact(
    command_sequence: u32,
    completion_sequence: u32,
    completion_code: u16,
    completion_detail: u16,
    completion_result: u32,
    completion_frame_offset: u32,
    completion_frame_len: u16,
    completion_frame_flags: u16,
    generation: u64,
) -> bool {
    generation != 0
        && command_sequence == completion_sequence
        && completion_code == DRIVER_RUNTIME_COMPLETION_PROGRESS
        && completion_detail == DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY
        && completion_result == driver_runtime_direct_genet_handoff_token(generation)
        && completion_frame_offset == 0
        && completion_frame_len == 0
        && completion_frame_flags == 0
}

/// Return true only for an exact generation-bound, non-switching handoff retry.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the primitive-only helper mirrors the fixed completion record without importing an app type"
)]
pub const fn driver_runtime_direct_genet_handoff_quiescing_completion_exact(
    command_sequence: u32,
    completion_sequence: u32,
    completion_code: u16,
    completion_detail: u16,
    completion_result: u32,
    completion_frame_offset: u32,
    completion_frame_len: u16,
    completion_frame_flags: u16,
    generation: u64,
) -> bool {
    generation != 0
        && command_sequence == completion_sequence
        && completion_code == DRIVER_RUNTIME_COMPLETION_IDLE
        && completion_detail == DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING
        && completion_result == driver_runtime_direct_genet_handoff_token(generation)
        && completion_frame_offset == 0
        && completion_frame_len == 0
        && completion_frame_flags == 0
}

/// Mix one primitive word into the descriptor identity hash.
#[must_use]
pub const fn driver_runtime_identity_hash_word(mut hash: u32, word: u32) -> u32 {
    let mut shift = 0u32;
    while shift < 32 {
        hash ^= (word >> shift) & 0xff;
        hash = hash.wrapping_mul(DRIVER_RUNTIME_IDENTITY_HASH_PRIME);
        shift += 8;
    }
    hash
}

/// Hash a generated runtime artifact name into a pointer-free descriptor word.
#[must_use]
pub const fn driver_runtime_artifact_hash(artifact: &str) -> u32 {
    let bytes = artifact.as_bytes();
    let mut hash = DRIVER_RUNTIME_IDENTITY_HASH_SEED;
    let mut index = 0usize;
    while index < bytes.len() {
        hash ^= bytes[index] as u32;
        hash = hash.wrapping_mul(DRIVER_RUNTIME_IDENTITY_HASH_PRIME);
        index += 1;
    }
    driver_runtime_nonzero_hash(hash)
}

/// Token binding task identity, generated artifact contract, hot path, and role.
#[must_use]
pub const fn driver_runtime_identity_token(
    task_key: u32,
    artifact_hash: u32,
    hot_path: u32,
    role_bit: u32,
) -> u32 {
    let mut hash = driver_runtime_identity_hash_word(
        DRIVER_RUNTIME_IDENTITY_HASH_SEED,
        DRIVER_RUNTIME_INIT_MAGIC,
    );
    hash = driver_runtime_identity_hash_word(hash, DRIVER_RUNTIME_INIT_VERSION as u32);
    hash = driver_runtime_identity_hash_word(hash, DRIVER_RUNTIME_IDENTITY_MAGIC);
    hash = driver_runtime_identity_hash_word(hash, task_key);
    hash = driver_runtime_identity_hash_word(hash, artifact_hash);
    hash = driver_runtime_identity_hash_word(hash, hot_path);
    hash = driver_runtime_identity_hash_word(hash, role_bit);
    driver_runtime_nonzero_hash(hash)
}

/// Epoch for a split-runtime bus link, derived from the client task identity.
#[must_use]
pub const fn driver_runtime_bus_link_epoch(
    task_key: u32,
    client_hot_path: u32,
    owner_hot_path: u32,
    channel_id: u32,
) -> u32 {
    let mut hash = driver_runtime_identity_hash_word(
        DRIVER_RUNTIME_IDENTITY_HASH_SEED,
        DRIVER_RUNTIME_INIT_AUX,
    );
    hash = driver_runtime_identity_hash_word(hash, task_key);
    hash = driver_runtime_identity_hash_word(hash, client_hot_path);
    hash = driver_runtime_identity_hash_word(hash, owner_hot_path);
    hash = driver_runtime_identity_hash_word(hash, channel_id);
    driver_runtime_nonzero_hash(hash)
}
/// USB keyboard enumeration step command used after local-seat engine init.
pub const DRIVER_RUNTIME_USB_ENUMERATE_AUX: u32 = 0x5553_4245;
/// USB runtime init detail: xHCI controller reached run state, no keyboard endpoint yet.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY: u16 = 0x0201;
/// USB runtime init detail: xHCI controller and boot keyboard endpoint are ready.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY: u16 = 0x0202;
/// USB runtime init detail: Enable Slot command is submitted and waiting for completion.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING: u16 = 0x0203;
/// USB service detail: keyboard endpoint is armed, but no interrupt report has arrived.
pub const DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING: u16 = 0x0500;
/// USB service detail: a HID interrupt report arrived, but no console byte was emitted.
pub const DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY: u16 = 0x0501;
/// USB runtime init detail: xHCI command and event rings produced a completion.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY: u16 = 0x0204;
/// USB runtime init detail: at least one root port reported a connected device.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED: u16 = 0x0205;
/// USB runtime init detail: xHCI addressed a root or hub child device.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED: u16 = 0x0206;
/// USB runtime init detail: a device descriptor transfer completed.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR: u16 = 0x0207;
/// USB runtime init detail: configuration descriptor transfer completed.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR: u16 = 0x0208;
/// USB runtime init detail: hub topology was traversed, but no boot keyboard endpoint was ready.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN: u16 = 0x0210;
/// USB runtime init detail: a HID keyboard endpoint was found, but final attach did not complete.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN: u16 = 0x0211;
/// USB runtime init detail: xHCI Enable Slot did not complete for a connected root port.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED: u16 = 0x0212;
/// Magic at the start of a USB hub-port diagnostic completion frame.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_MAGIC: u32 = 0x5553_4850;
/// USB hub-port diagnostic completion frame format version.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_VERSION: u8 = 1;
/// Byte length of the fixed USB hub-port diagnostic completion frame.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_LEN: u16 = 24;
/// Hub-port sample captured after the initial power settle and GET_STATUS.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_INITIAL: u8 = 1;
/// Hub-port sample captured while polling reset completion.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RESET_POLL: u8 = 2;
/// Hub-port sample captured after a disconnected-port power kick.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RECOVERY_POWER: u8 = 3;
/// Hub-port sample captured after a disconnected-port reset recovery.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RECOVERY_RESET: u8 = 4;
/// Hub-port sample captured when the port is ready for child probing.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_READY: u8 = 5;
/// Hub-port sample captured when an absent/disconnected port is skipped.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_SKIP_DISCONNECTED: u8 = 6;
/// Hub-port diagnostic flag: raw wPortStatus has PORT_CONNECTION set.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_CONNECTED: u16 = 1 << 0;
/// Hub-port diagnostic flag: raw wPortStatus has PORT_ENABLE set.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_ENABLED: u16 = 1 << 1;
/// Hub-port diagnostic flag: raw wPortStatus has PORT_RESET set.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_RESET: u16 = 1 << 2;
/// Hub-port diagnostic flag: raw wPortStatus reports low-speed.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_LOW_SPEED: u16 = 1 << 3;
/// Hub-port diagnostic flag: raw wPortStatus reports high-speed.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_HIGH_SPEED: u16 = 1 << 4;
/// Hub-port diagnostic flag: raw wPortChange has C_CONNECTION set.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_C_CONNECTION: u16 = 1 << 5;
/// Hub-port diagnostic flag: raw wPortChange has C_ENABLE set.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_C_ENABLE: u16 = 1 << 6;
/// Hub-port diagnostic flag: raw wPortChange has C_RESET set.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_C_RESET: u16 = 1 << 7;
/// Hub-port diagnostic flag: the runtime will clear C_CONNECTION.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_CLEAR_CONNECTION: u16 = 1 << 8;
/// Hub-port diagnostic flag: the runtime will clear C_ENABLE.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_CLEAR_ENABLE: u16 = 1 << 9;
/// Hub-port diagnostic flag: the runtime will clear C_RESET.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_CLEAR_RESET: u16 = 1 << 10;
/// Hub-port diagnostic flag: sample came from disconnected-port recovery.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_RECOVERY: u16 = 1 << 11;
/// Hub-port diagnostic flag: runtime skipped reset because the port is absent.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_SKIP_RESET: u16 = 1 << 12;
/// Hub-port diagnostic flag: runtime considers the port ready after reset.
pub const DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FLAG_READY: u16 = 1 << 13;
/// USB runtime init detail: xHCI Address Device did not complete after Enable Slot.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED: u16 = 0x0213;
/// USB runtime init detail: device descriptor transfer failed after address.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED: u16 = 0x0214;
/// USB runtime init detail: configuration descriptor transfer failed after device descriptor.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED: u16 = 0x0215;
/// USB runtime init detail: HID keyboard endpoint attach/control setup failed.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED: u16 = 0x0216;
/// USB runtime init detail: hub descriptor or hub context setup failed.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED: u16 = 0x0217;
/// USB runtime init detail: hub SET_CONFIGURATION failed before child probing.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED: u16 = 0x0218;
/// USB runtime init detail: hub descriptor transfer failed before child probing.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED: u16 = 0x0219;
/// USB runtime init detail: xHCI hub context evaluation failed before child probing.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED: u16 = 0x021a;
/// GENET/CYW43 network init command used by the root ring client.
pub const DRIVER_RUNTIME_NET_INIT_AUX: u32 = 0x494e_4954;
/// CYW43 command descriptor submission marker used in `aux0`.
pub const DRIVER_RUNTIME_CYW43_COMMAND_AUX: u32 = 0x4359_5734;
/// An SDIO intake seal already belongs to another command sequence.
pub const DRIVER_RUNTIME_REJECT_SDIO_INTAKE_SEAL_BUSY: u32 = 0x5344_0001;
/// A descriptor-shaped SDIO turn reached dispatch without its intake seal.
pub const DRIVER_RUNTIME_REJECT_SDIO_INTAKE_SEAL_MISSING: u32 = 0x5344_0002;
/// The outer linked-runtime generation predicate rejected the command.
pub const DRIVER_RUNTIME_REJECT_OUTER_GENERATION: u32 = 0x5344_0003;
/// An active CYW43 control cursor was presented with another logical parent.
pub const DRIVER_RUNTIME_REJECT_CYW43_CONTROL_OWNER_MISMATCH: u32 = 0x5344_0004;
/// A caller attempted the retired SDIO generation-reset operation.
pub const DRIVER_RUNTIME_REJECT_SDIO_GENERATION_RESET_ROUTE_MISSING: u32 = 0x5344_0005;
/// A command did not match the active immutable SDIO request identity.
pub const DRIVER_RUNTIME_REJECT_SDIO_RETAINED_OWNER_IDENTITY_MISMATCH: u32 = 0x5344_0006;
/// A fresh SDIO descriptor could not construct a bounded hardware request.
pub const DRIVER_RUNTIME_REJECT_SDIO_REQUEST_IDENTITY_INVALID: u32 = 0x5344_0007;
/// The one-shot CYW43 pull-up action was presented outside its admitted edge.
pub const DRIVER_RUNTIME_REJECT_SDIO_PULLUP_ADMISSION: u32 = 0x5344_0008;
/// An active retained request reached an impossible idle phase.
pub const DRIVER_RUNTIME_REJECT_SDIO_INVALID_RETAINED_PHASE: u32 = 0x5344_0009;
/// DPC activation did not match the admitted live generation and notification state.
pub const DRIVER_RUNTIME_REJECT_SDIO_DPC_ACTIVATION_ADMISSION: u32 = 0x5344_000a;
/// A caller attempted the retired SDIO generation-commit operation.
pub const DRIVER_RUNTIME_REJECT_SDIO_GENERATION_COMMIT_ADMISSION: u32 = 0x5344_000b;
/// The exact DPC source-W1C completed but host `CARD_INT` could not be
/// condition-before-sleep rearmed or its crossing source could not be
/// durably published.
pub const DRIVER_RUNTIME_REJECT_SDIO_DPC_SOURCE_REARM_FAILED: u32 = 0x5344_000c;
/// An exact leased request remained pending beyond its finite physical
/// controller/child lifetime.
pub const DRIVER_RUNTIME_REJECT_SDIO_STEADY_SERVICE_LEASE_EXHAUSTED: u32 = 0x5344_000d;
/// A command or descriptor carried a partial, mutated, or stale steady-service
/// lease marker instead of one exact generation-bound owner lease.
pub const DRIVER_RUNTIME_REJECT_SDIO_STEADY_SERVICE_LEASE_INVALID: u32 = 0x5344_000e;
/// A command or child descriptor carried a partial, mutated, or stale
/// persistent-transaction marker instead of the exact op11-derived authority.
pub const DRIVER_RUNTIME_REJECT_SDIO_PERSISTENT_TRANSACTION_INVALID: u32 = 0x5344_000f;
/// Maximum reciprocal SDIO actions retained by one immutable CYW43 parent command.
///
/// Root uses the same bound to cap child-completion deadline renewals, so a
/// multi-action Linux-shaped operation can outlive each legal child request
/// without turning progress into an unbounded parent lease.
pub const DRIVER_RUNTIME_CYW43_PARENT_MAX_SDIO_ACTIONS: u16 = 1_024;
/// Exact HAL-operation budget for one persistent CYW43 control parent.
pub const DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_OPS: u16 = 192;
/// Exact frame budget for one persistent CYW43 control parent.
pub const DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_FRAMES: u16 = 64;
/// Exact byte budget for one persistent CYW43 control parent.
pub const DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_BYTES: u32 = 65_536;
/// Root-only absolute persistent-parent fault-containment deadline.
///
/// It covers 2.5 seconds of pre-TX work, one 20.56-second worst-case exact
/// SDIO child, and a 2.5-second reply/scheduling margin, rounded up to 30
/// seconds. Unbound-reject grace is error containment, not normal progress.
///
/// This request-lifetime bound is not progress authority and cannot be
/// renewed by notifications, completions, or other scheduling hints.
pub const DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_TIMEOUT_US: u32 = 30_000_000;
/// Exact HAL-operation budget for one urgent steady Ethernet TX parent lease.
pub const DRIVER_RUNTIME_CYW43_STEADY_TX_LEASE_OPS: u16 = 4;
/// Exact frame budget for one urgent steady Ethernet TX parent lease.
pub const DRIVER_RUNTIME_CYW43_STEADY_TX_LEASE_FRAMES: u16 = 1;
/// Exact byte budget for one urgent steady Ethernet TX parent lease.
pub const DRIVER_RUNTIME_CYW43_STEADY_TX_LEASE_BYTES: u32 = 1_536;
/// CYW43 operation: initialize the SDIO transport and firmware upload lane.
pub const DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT: u16 = 1;
/// CYW43 operation: write a firmware chunk into dongle RAM.
pub const DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK: u16 = 2;
/// CYW43 operation: write a normalized NVRAM chunk into dongle RAM.
pub const DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK: u16 = 3;
/// CYW43 operation: write the NVRAM tail marker.
pub const DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL: u16 = 4;
/// CYW43 operation: release the ARMCR4 firmware CPU.
pub const DRIVER_RUNTIME_CYW43_OP_RELEASE: u16 = 5;
/// CYW43 operation: submit one SDPCM/BDC control payload.
pub const DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME: u16 = 6;
/// CYW43 operation: submit one Ethernet payload through SDPCM/BDC.
pub const DRIVER_RUNTIME_CYW43_OP_ETH_TX: u16 = 7;
/// CYW43 operation: poll the Function 2 RX path.
pub const DRIVER_RUNTIME_CYW43_OP_RX_POLL: u16 = 8;
/// CYW43 operation: prepare the firmware upload transport before streaming chunks.
pub const DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP: u16 = 9;
/// CYW43 operation: poll only SDPCM control and event frames.
pub const DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL: u16 = 10;
/// CYW43 operation: submit one control payload and wait for its matching CDC reply.
pub const DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE: u16 = 11;
/// CYW43 transport detail: no transport substage has run.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_START: u16 = 0x5400;
/// CYW43 transport detail: SDIO bus-owner link was validated.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY: u16 = 0x5401;
/// CYW43 transport detail: card-select/adoption stage is ready.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_CARD_READY: u16 = 0x5402;
/// CYW43 transport detail: Function 1 block size is programmed.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_BLOCK_READY: u16 = 0x5403;
/// CYW43 transport detail: Function 2 block size is programmed.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F2_BLOCK_READY: u16 = 0x5404;
/// CYW43 transport detail: Function 1 is enabled and ready.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_ENABLED: u16 = 0x5405;
/// CYW43 transport detail: startup host clock/bus-width state is programmed.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_HOST_READY: u16 = 0x5406;
/// CYW43 transport detail: backplane ALP/window/chipcommon proof completed.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BACKPLANE_READY: u16 = 0x5407;
/// CYW43 transport detail: transport is ready for firmware prep/upload.
pub const DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY: u16 = 0x5408;
/// SDIO fault telemetry proves owner-side command/data-path containment.
pub const DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_CONTAINED: u16 = 1 << 0;
/// SDIO fault telemetry records that owner-side containment failed.
pub const DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_OWNER_PATH_POISONED: u16 = 1 << 1;
/// All valid disposition bits on an SDIO fault-telemetry frame.
pub const DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_MASK: u16 =
    DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_CONTAINED
        | DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_OWNER_PATH_POISONED;
/// Magic identifying the exact SDIO owner fault-telemetry payload.
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_MAGIC: u32 = 0x5344_494f;
/// Current SDIO owner fault-telemetry payload version.
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_VERSION: u32 = 3;
/// Exact byte length of one SDIO owner fault-telemetry payload.
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_BYTES: u16 = 116;
/// Number of aligned words in one SDIO owner fault-telemetry payload.
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_WORDS: usize =
    DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_BYTES as usize / core::mem::size_of::<u32>();
/// Byte offsets of the fixed-width fields in SDIO owner fault telemetry.
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_ARG_OFFSET: usize = 8;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_CMD_FLAGS_OFFSET: usize = 12;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_LEN_BLOCK_OFFSET: usize = 16;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_COUNT_MODE_OFFSET: usize = 20;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_PRESENT_OFFSET: usize = 24;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_INT_STATUS_OFFSET: usize = 28;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_RESPONSE0_OFFSET: usize = 32;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_HOST_CLOCK_OFFSET: usize = 36;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_FAILURE_OFFSET: usize = 40;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_BLOCK_REG_OFFSET: usize = 44;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_PAYLOAD_EDGE_OFFSET: usize = 48;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_PAYLOAD_SUM_OFFSET: usize = 52;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_CS_OFFSET: usize = 56;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_CONBLK_OFFSET: usize = 60;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_NEXTCB_OFFSET: usize = 64;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_ARGUMENT_REG_OFFSET: usize = 68;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_TRANSFER_COMMAND_REG_OFFSET: usize = 72;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_TIMEOUT_GAP_REG_OFFSET: usize = 76;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_INT_ENABLE_REG_OFFSET: usize = 80;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_SIGNAL_ENABLE_REG_OFFSET: usize = 84;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_HOST_CONTROL2_REG_OFFSET: usize = 88;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_TI_OFFSET: usize = 92;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_SOURCE_OFFSET: usize = 96;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_DEST_OFFSET: usize = 100;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_LEN_OFFSET: usize = 104;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_STRIDE_OFFSET: usize = 108;
pub const DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_DEBUG_OFFSET: usize = 112;
/// CYW43 command flag: force Function 1 backplane writes through byte-mode retry.
/// CYW43 command flag: transmit control frames with the Linux SDPCM hw extension.
pub const DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER: u16 = 1 << 1;
/// CYW43 command flag: permit a bounded Function 2 first-read on zero-RFRAME RX polls.
pub const DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD: u16 = 1 << 2;
/// CYW43 command flag: drain pending RX once before transmitting a control frame.
pub const DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN: u16 = 1 << 3;
/// CYW43 command flag: after delivering steady data RX, queue bounded tail frames.
pub const DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN: u16 = 1 << 4;
/// CYW43 command flag: fence an association Join Function-2 transmit against
/// a newly asserted DPC source at the final SDIO pre-issue boundary.
pub const DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE: u16 = 1 << 5;
/// CYW43 command flag: retain one exact Ethernet TX child under a finite,
/// generation-bound SDIO service lease.
///
/// Root may set this only through a typed HAL entry for either one urgent
/// post-Gate-8 data response or one intake-sealed host-EAPOL key response. The
/// CYW43 runtime independently validates the immutable parent and propagates
/// the authority solely to that parent's Function-2 write.
pub const DRIVER_RUNTIME_CYW43_FLAG_STEADY_TX_SERVICE_LEASE: u16 = 1 << 6;
/// CYW43 positive detail: a control Function 2 TX retry recovered a transfer fault.
pub const DRIVER_RUNTIME_CYW43_CONTROL_DETAIL_TX_F2_RETRY_RECOVERED: u16 = 0x5801;
/// CYW43 positive detail: an event/data frame interrupted a retained control exchange.
///
/// The exchange remains active in the isolated runtime. Root must route the
/// frame and resubmit the identical BCDC command identity; the runtime must not
/// transmit the request a second time.
pub const DRIVER_RUNTIME_CYW43_CONTROL_DETAIL_INTERLEAVED_FRAME: u16 = 0x5802;
/// CYW43 positive detail: one strict op8 completion carries a committed RX batch.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_DETAIL: u16 = 0x5803;
/// CYW43 RX idle detail: no detailed RX result was reported.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NONE: u16 = 0;
/// CYW43 RX idle detail: firmware/channel state is not ready for RX.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NOT_READY: u16 = 0x5701;
/// CYW43 RX idle detail: Function 1 RFRAME byte-count read failed.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RFRAME_READ_FAILED: u16 = 0x5702;
/// CYW43 RX idle detail: Function 1 RFRAME reported no pending frame.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NO_RFRAME: u16 = 0x5703;
/// CYW43 RX idle detail: Function 1 RFRAME length was outside the RX buffer.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_INVALID_RFRAME_LEN: u16 = 0x5704;
/// CYW43 RX idle detail: padded Function 2 read request would exceed the RX buffer.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RX_REQUEST_TOO_LARGE: u16 = 0x5705;
/// CYW43 RX idle detail: Function 2 CMD53 read failed.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_F2_READ_FAILED: u16 = 0x5706;
/// CYW43 RX idle detail: SDPCM decoded, but no frame matched the requested channel mask.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS: u16 = 0x5707;
/// CYW43 RX idle detail: zero-RFRAME first-read CMD53 failed.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_FAILED: u16 = 0x5709;
/// CYW43 RX idle detail: zero-RFRAME first-read returned an empty SDPCM prefix.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY: u16 = 0x570a;
/// CYW43 RX idle detail: zero-RFRAME first-read returned a malformed SDPCM prefix.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_INVALID_SDPCM: u16 = 0x570b;
/// CYW43 RX idle detail: zero-RFRAME first-read remainder CMD53 failed.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_FAILED: u16 = 0x570c;
/// CYW43 RX idle detail: zero-RFRAME first-read SDPCM packet exceeded the bounded window.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_TOO_LARGE: u16 = 0x570d;
/// CYW43 RX idle detail: source stayed asserted after bounded empty first-read retry.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY: u16 = 0x570e;
/// CYW43 RX idle detail: next-frame readahead was terminated and NAKed for retry.
pub const DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NEXT_FRAME_READAHEAD_RETRY: u16 = 0x570f;
/// CYW43 RX first-read result: high bit marks a packed RX-source snapshot.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC: u32 = 0x8000_0000;
/// CYW43 RX first-read result: low bits carry the attempted Function 2 probe length.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PROBE_LEN_MASK: u32 = 0x0000_ffff;
/// CYW43 RX first-read result: shift for the sampled CCCR IENx byte.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT: u32 = 16;
/// CYW43 RX first-read result: mask for the sampled CCCR IENx byte.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_MASK: u32 = 0x00ff_0000;
/// CYW43 RX first-read result: firmware SDIO core asserted frame-indication.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FRAME_INDICATED: u32 = 1 << 24;
/// CYW43 RX first-read result: firmware SDIO core asserted a host-interrupt bit.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_HOST_INTERRUPT: u32 = 1 << 25;
/// CYW43 RX first-read result: host SDHCI reported card interrupt status.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CARD_INTERRUPT: u32 = 1 << 26;
/// CYW43 RX first-read result: CCCR IORx reported Function 2 ready.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY: u32 = 1 << 27;
/// CYW43 RX first-read result: source fields are passive under a linked SDIO owner.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PASSIVE: u32 = 1 << 28;
/// CYW43 RX first-read result: source fields came from the last bounded owner sample.
pub const DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CACHED: u32 = 1 << 29;
/// CYW43 frame flag mask carrying the SDPCM channel on frame-ready completions.
pub const DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK: u16 = 0x000f;
/// CYW43 frame flag value for SDPCM control-channel payloads.
pub const DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL: u16 = 0;
/// CYW43 frame flag value for SDPCM event-channel payloads.
pub const DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT: u16 = 1;
/// CYW43 frame flag value for Ethernet data payloads.
pub const DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA: u16 = 2;
/// Maximum decoded CYW43 RX backlog retained by either linked-runtime side.
///
/// Root must be able to preserve one complete child-runtime backlog while the
/// single physical owner finishes an exact control transaction.
pub const DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP: usize = 50;
/// CYW43 frame flag shift carrying the firmware SDPCM credit byte.
pub const DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT: u16 = 8;
/// CYW43 frame flag mask carrying the firmware SDPCM credit byte.
pub const DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK: u16 = 0xff00;

/// Whether CYW43 RX metadata carries only one supported channel and its credit.
#[must_use]
pub const fn driver_runtime_cyw43_rx_frame_flags_valid(flags: u16) -> bool {
    let channel = flags & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK;
    let known =
        DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK | DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK;
    (channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL
        || channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT
        || channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA)
        && flags & !known == 0
}
/// PCIe runtime command operation: read one 32-bit xHCI/VL805 register.
pub const DRIVER_RUNTIME_PCIE_OP_PORT_READ: u16 = 1;
/// PCIe runtime command operation: write one 32-bit xHCI/VL805 register.
pub const DRIVER_RUNTIME_PCIE_OP_PORT_WRITE: u16 = 2;
/// PCIe runtime command operation: flush posted writes.
pub const DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH: u16 = 3;
/// PCIe runtime command operation: enable the direct-GENET root-idle timer.
///
/// This operation is Reply-bearing and idempotent. The isolated owner programs
/// channel 3 only for the first exact enable in one runtime lifetime; later
/// calls return the already-published enable identity without touching MMIO or
/// acknowledging the IRQ handler again.
pub const DRIVER_RUNTIME_PCIE_OP_ROOT_IDLE_TIMER_ENABLE: u16 = 4;
/// SDIO runtime command flag: command has an SDIO data phase.
pub const DRIVER_RUNTIME_SDIO_FLAG_DATA: u16 = 1 << 0;
/// SDIO runtime command flag: data phase writes root-staged bytes to the card.
pub const DRIVER_RUNTIME_SDIO_FLAG_WRITE: u16 = 1 << 1;
/// SDIO runtime command flag: transfer should suppress noisy diagnostics.
pub const DRIVER_RUNTIME_SDIO_FLAG_QUIET: u16 = 1 << 2;
/// SDIO runtime command flag: command expects no response.
pub const DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE: u16 = 1 << 3;
/// SDIO runtime command flag: command expects an OCR/R4-style response.
pub const DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR: u16 = 1 << 4;
/// SDIO runtime command flag: command expects a short response.
pub const DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT: u16 = 1 << 5;
/// SDIO runtime command flag: command expects a short-busy response.
pub const DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY: u16 = 1 << 6;
/// SDIO runtime command flag: command expects a long response.
pub const DRIVER_RUNTIME_SDIO_FLAG_RESP_LONG: u16 = 1 << 7;
/// SDIO bus-owner operation: read one byte with CMD52.
pub const DRIVER_RUNTIME_SDIO_OP_CMD52_READ: u16 = 1;
/// SDIO bus-owner operation: write one byte with CMD52.
pub const DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE: u16 = 2;
/// SDIO bus-owner operation: read bytes or blocks with CMD53.
pub const DRIVER_RUNTIME_SDIO_OP_CMD53_READ: u16 = 3;
/// SDIO bus-owner operation: write bytes or blocks with CMD53.
pub const DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE: u16 = 4;
/// Linux-equivalent issued-request watchdog for every SDIO controller command.
pub const DRIVER_RUNTIME_SDIO_REQUEST_TIMEOUT_US: u32 = 10_000_000;
/// Linux-equivalent pre-issue inhibit fence for each SDIO transfer attempt.
pub const DRIVER_RUNTIME_SDIO_INHIBIT_TIMEOUT_US: u32 = 10_000;
/// Cohesix bcm2835 reset/clock containment fence following a failed attempt.
pub const DRIVER_RUNTIME_SDIO_CONTAINMENT_TIMEOUT_US: u32 = 220_000;
/// Maximum controller attempts after a provably unissued Function-1 request.
pub const DRIVER_RUNTIME_SDIO_TRANSFER_ATTEMPT_LIMIT: u32 = 2;
/// Maximum containment passes owned by one reciprocal SDIO descriptor.
pub const DRIVER_RUNTIME_SDIO_CONTAINMENT_ATTEMPT_LIMIT: u32 = 2;
/// Scheduling/publication margin above the complete reciprocal SDIO lifetime.
pub const DRIVER_RUNTIME_CYW43_SDIO_CHILD_WAIT_MARGIN_US: u32 = 100_000;
/// Maximum CYW43 wait for a controller child, including inhibit, retry, and containment.
pub const DRIVER_RUNTIME_CYW43_SDIO_CHILD_WORST_CASE_US: u32 =
    (DRIVER_RUNTIME_SDIO_REQUEST_TIMEOUT_US + DRIVER_RUNTIME_SDIO_INHIBIT_TIMEOUT_US)
        * DRIVER_RUNTIME_SDIO_TRANSFER_ATTEMPT_LIMIT
        + DRIVER_RUNTIME_SDIO_CONTAINMENT_TIMEOUT_US
            * DRIVER_RUNTIME_SDIO_CONTAINMENT_ATTEMPT_LIMIT
        + DRIVER_RUNTIME_CYW43_SDIO_CHILD_WAIT_MARGIN_US;
/// Maximum HAL operations in one preemptible SDIO host service quantum.
pub const DRIVER_RUNTIME_SDIO_SERVICE_MAX_OPS: u16 = 256;
/// Maximum bytes admitted by the SDIO host contract in one service quantum.
pub const DRIVER_RUNTIME_SDIO_SERVICE_MAX_BYTES: u32 = 65_536;
/// Maximum frames admitted by the SDIO host contract in one service quantum.
pub const DRIVER_RUNTIME_SDIO_SERVICE_MAX_FRAMES: u16 = 64;
/// One reciprocal CYW43-to-SDIO command owns exactly one typed request.
pub const DRIVER_RUNTIME_SDIO_LINK_REQUEST_MAX_FRAMES: u16 = 1;
/// SDIO bus-owner operation: poll interrupt status.
pub const DRIVER_RUNTIME_SDIO_OP_POLL_IRQ: u16 = 5;
/// SDIO bus-owner operation: apply host-controller clock and bus-width state.
pub const DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG: u16 = 6;
/// SDIO bus-owner operation: issue a bounded raw card command with no data phase.
pub const DRIVER_RUNTIME_SDIO_OP_CARD_COMMAND: u16 = 7;
/// Reserved wire value for the retired in-place CYW43 generation-reset operation.
///
/// The descriptor validator rejects this value. Physical recovery belongs only
/// to the canonical root-owned pair restart and cannot enter the SDIO MMIO lane.
pub const DRIVER_RUNTIME_SDIO_OP_GENERATION_RESET: u16 = 8;
/// SDIO bus-owner operation: activate post-release CARD_INT/DPC service.
pub const DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE: u16 = 9;
/// Reserved wire value for the retired in-place CYW43 generation-commit operation.
///
/// The descriptor validator rejects this value. The canonical pair binds the
/// completed physical-lifetime epoch directly; there is no runtime commit lane.
pub const DRIVER_RUNTIME_SDIO_OP_GENERATION_COMMIT: u16 = 10;
/// SDIO engine-init detail: SDIO host reached ready state.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_READY: u16 = 0x5500;
/// SDIO engine-init detail: cold all-path reset failed during power sequencing.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_ALL_FAILED: u16 = 0x5510;
/// SDIO engine-init detail: command/data reset failed after cold power-on.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_CMD_DATA_FAILED: u16 = 0x5511;
/// SDIO engine-init detail: startup clock could not be enabled after cold reset.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_CLOCK_FAILED: u16 = 0x5512;
/// SDIO engine-init detail: command/data inhibit stayed asserted after clock enable.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_INHIBIT_FAILED: u16 = 0x5513;
/// SDIO engine-init detail: generated notification/IRQ topology was invalid.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_INVALID_DESCRIPTOR: u16 = 0x5514;
/// SDIO engine-init detail: the manifest-declared Pi 4 WL_ON power sequence failed.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_WIFI_PWRSEQ_FAILED: u16 = 0x5515;
/// SDIO engine-init detail: firmware GET_GPIO_CONFIG did not complete successfully.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_WIFI_PWRSEQ_GET_CONFIG_FAILED: u16 = 0x5516;
/// SDIO engine-init detail: firmware SET_GPIO_CONFIG did not complete successfully.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_WIFI_PWRSEQ_SET_CONFIG_FAILED: u16 = 0x5517;
/// SDIO engine-init detail: firmware SET_GPIO_STATE low did not complete successfully.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_WIFI_PWRSEQ_ASSERT_LOW_FAILED: u16 = 0x5518;
/// SDIO engine-init detail: firmware SET_GPIO_STATE high did not complete successfully.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_WIFI_PWRSEQ_RELEASE_HIGH_FAILED: u16 = 0x5519;
/// SDIO engine-init detail: request-owned interrupt status did not clear before READY.
pub const DRIVER_RUNTIME_SDIO_INIT_DETAIL_STATUS_CLEAR_FAILED: u16 = 0x551a;
/// Low-byte class for an SDIO WiFi pwrseq firmware-mailbox protocol failure.
pub const DRIVER_RUNTIME_SDIO_PWRSEQ_PROTOCOL_CLASS: u32 = 4;
/// Shift applied to SDIO WiFi pwrseq firmware-mailbox protocol reason bits.
pub const DRIVER_RUNTIME_SDIO_PWRSEQ_PROTOCOL_REASON_SHIFT: u32 = 8;
/// Pwrseq protocol reason: firmware message did not report global success.
pub const DRIVER_RUNTIME_SDIO_PWRSEQ_PROTOCOL_GLOBAL_STATUS: u32 = 1 << 0;
/// Pwrseq protocol reason: firmware did not replace the requested GPIO with zero.
pub const DRIVER_RUNTIME_SDIO_PWRSEQ_PROTOCOL_GPIO_TOKEN: u32 = 1 << 1;
/// Pwrseq protocol reason: a different retained request attempted to resume.
pub const DRIVER_RUNTIME_SDIO_PWRSEQ_PROTOCOL_CURSOR: u32 = 1 << 2;
/// Pwrseq protocol reason: the retained mailbox phase was internally invalid.
pub const DRIVER_RUNTIME_SDIO_PWRSEQ_PROTOCOL_PHASE: u32 = 1 << 3;
/// SDIO response kind: no response.
pub const DRIVER_RUNTIME_SDIO_RESP_NONE: u8 = 0;
/// SDIO response kind: OCR/R4 response.
pub const DRIVER_RUNTIME_SDIO_RESP_OCR: u8 = 1;
/// SDIO response kind: short/R5 response.
pub const DRIVER_RUNTIME_SDIO_RESP_SHORT: u8 = 2;
/// SDIO response kind: short-busy response.
pub const DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY: u8 = 3;
/// SDIO response kind: long response.
pub const DRIVER_RUNTIME_SDIO_RESP_LONG: u8 = 4;
/// Pixel format tag for 32-bit xRGB/BGR framebuffer words.
pub const DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888: u32 = 1;
/// Pixel format tag for 24-bit RGB/BGR framebuffer bytes.
pub const DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_RGB888: u32 = 2;
/// Fixed driver-local virtual base used when root maps the HDMI framebuffer.
pub const DRIVER_RUNTIME_FRAMEBUFFER_VADDR: u64 = 0x7100_0000;
/// Maximum dirty cells admitted in one HDMI compositor turn.
pub const DRIVER_RUNTIME_HDMI_SERVICE_MAX_OPS: u16 = 1_280;
/// Maximum immutable text bytes covered by the HDMI parser/grant envelope.
/// The production command-frame transport remains independently capped at
/// `MAX_DRIVER_TASK_FRAME_BYTES` (1,536 bytes).
pub const DRIVER_RUNTIME_HDMI_SERVICE_MAX_BYTES: u32 = 4_096;
/// Maximum physical framebuffer rows admitted in one HDMI clear turn.
pub const DRIVER_RUNTIME_HDMI_SERVICE_MAX_FRAMES: u16 = 80;
/// Maximum MMIO page descriptors carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES: usize = 16;
/// Maximum DMA page descriptors carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_DMA_PAGES: usize = 80;
/// Maximum root/driver shared pages carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES: usize = 16;
/// Maximum IRQ descriptors carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_IRQS: usize = 4;
/// Maximum bus-link descriptors carried in one init descriptor.
/// Current fixed ABI bound: each isolated runtime may participate in one
/// compiler-declared owner/client bus link. Keeping this bound at one leaves
/// the sealed init descriptor within its dedicated command-ring aperture.
pub const DRIVER_RUNTIME_INIT_MAX_BUS_LINKS: usize = 1;
/// Maximum semantic resource ranges carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_RESOURCE_RANGES: usize = 8;
/// Runtime resource descriptors use 4 KiB pages.
pub const DRIVER_RUNTIME_RESOURCE_PAGE_BYTES: u64 = 4096;
/// Fixed offset of the root/runtime payload area in one ring page.
pub const DRIVER_RUNTIME_RING_FRAME_OFFSET: u16 = 256;
/// Bytes in one command/completion ring page.
pub const DRIVER_RUNTIME_RING_PAGE_BYTES: u16 = 4096;
/// CYW43 runtime scratch offset for outgoing SDPCM Function 2 TX frames.
///
/// This is intentionally separate from the root/runtime frame window and the
/// dedicated CYW43 parent-command descriptor slot.
pub const DRIVER_RUNTIME_CYW43_SDPCM_TX_FRAME_OFFSET: u16 = 2048;
/// Dedicated root-to-CYW43 parent-command descriptor slot.
///
/// The linked CYW43 runtime may publish RX frames, backplane scratch, or SDIO
/// fault telemetry while root prepares a retained command on another core.
/// Keeping the 28-byte descriptor on its own cache line at `1920` makes those
/// writers disjoint; root's command sequence remains the sole publication bit.
pub const DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET: u16 = 1920;
/// Bytes reserved for the contiguous runtime-init descriptor at the canonical
/// ring-frame offset.
///
/// This init-only aperture ends before the dedicated CYW43 parent-command
/// cache line. Ordinary data frames retain their independent 1,536-byte bound.
pub const DRIVER_RUNTIME_INIT_DESCRIPTOR_APERTURE_BYTES: u16 =
    DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET - DRIVER_RUNTIME_RING_FRAME_OFFSET;
/// Fixed offset of the SDIO owner's passive host/card clock snapshot.
///
/// The snapshot occupies the otherwise unused cache line between the
/// CYW43 parent-command descriptor and the SDPCM transmit aperture. Only the
/// isolated SDIO owner writes it; root and CYW43 consume stable read-only
/// samples.
pub const DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET: u16 = 1984;
/// Bytes in the SDIO owner's passive host/card clock snapshot.
pub const DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_BYTES: u16 = 44;
/// Magic value for an initialized SDIO clock snapshot.
pub const DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_MAGIC: u32 = 0x5344_434b;
/// Layout version for [`DriverRuntimeSdioClockSnapshot`].
pub const DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_VERSION: u16 = 1;
/// Fixed offset of the SDIO owner's fault-containment deadline arm.
///
/// The record occupies the complete unused tail between the passive clock
/// snapshot and CYW43's disjoint SDPCM transmit aperture. Only the isolated
/// SDIO owner writes it; root and CYW43 may stable-read it as a condition.
pub const DRIVER_RUNTIME_SDIO_DEADLINE_ARM_OFFSET: u16 = 2028;
/// Bytes in one sequence-last SDIO fault-containment deadline arm.
pub const DRIVER_RUNTIME_SDIO_DEADLINE_ARM_BYTES: u16 = 20;
/// Fixed offset of the runtime progress marker in one ring page.
pub const DRIVER_RUNTIME_RING_PROGRESS_OFFSET: u16 = 128;
/// Fixed offset of the CYW43 private-RX queue's durable root-visible level.
///
/// This record lives in the CYW43 runtime's local command page. The SDIO
/// owner's distinct command page uses the same numeric range for its DPC event
/// ring, so the compiler-declared runtime role and physical ring identity are
/// part of the address. No shared physical page aliases the two records.
pub const DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_OFFSET: u16 = 192;
/// Bytes in one sequence-last CYW43 private-RX queue-state record.
pub const DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES: u16 = 28;
/// Magic value for a committed CYW43 private-RX queue-state record.
pub const DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_MAGIC: u32 = 0x4359_5153;
/// Layout version for [`DriverRuntimeCyw43RxQueueState`].
pub const DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_VERSION: u16 = 2;
/// Queue-state flag: this CYW43 generation is poisoned and cannot serve RX.
pub const DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_FLAG_POISONED: u32 = 1 << 0;
/// Fixed offset of the USB runtime's durable old-good replay receipt.
///
/// USB owns this range only in its role-local command ring. CYW43 and SDIO use
/// overlapping numeric offsets in their distinct HAL-mapped ring pages; no
/// physical command page aliases another runtime's record.
pub const DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_OFFSET: u16 = 192;
/// Bytes in one commit-last USB old-good replay receipt.
pub const DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_BYTES: u16 = 48;
/// Magic value for a USB old-good replay receipt (`USOG`).
pub const DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_MAGIC: u32 = 0x5553_4f47;
/// Layout version for [`DriverRuntimeUsbOldgoodReceipt`].
pub const DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_VERSION: u16 = 1;
/// Fixed offset of the serial owner's passive receive/IRQ state.
///
/// Serial owns this range only in its role-local command ring. USB uses the
/// same numeric range for its old-good receipt in a distinct HAL-mapped ring
/// page, so the compiler-declared runtime role remains part of the address.
pub const DRIVER_RUNTIME_SERIAL_RX_STATE_OFFSET: u16 = 192;
/// Bytes in one sequence-last serial receive/IRQ state record.
pub const DRIVER_RUNTIME_SERIAL_RX_STATE_BYTES: u16 = 48;
/// Magic value for a serial receive/IRQ state record (`SRXS`).
pub const DRIVER_RUNTIME_SERIAL_RX_STATE_MAGIC: u32 = 0x5352_5853;
/// Layout version for [`DriverRuntimeSerialRxState`].
pub const DRIVER_RUNTIME_SERIAL_RX_STATE_VERSION: u16 = 1;
/// Serial receive-state flag: the owner still owes an IRQ-handler ACK.
pub const DRIVER_RUNTIME_SERIAL_RX_STATE_FLAG_ACK_PENDING: u16 = 1 << 0;
/// Magic value for one bounded serial SPSC transport header (`SSPQ`).
pub const DRIVER_RUNTIME_SERIAL_SPSC_MAGIC: u32 = 0x5353_5051;
/// Layout version for [`DriverRuntimeSerialSpscHeader`].
pub const DRIVER_RUNTIME_SERIAL_SPSC_VERSION: u16 = 1;
/// Shared pages assigned to one serial SPSC direction.
pub const DRIVER_RUNTIME_SERIAL_SPSC_PAGES_PER_RING: usize = 2;
/// Exact serial shared-page population: two TX pages followed by two RX pages.
pub const DRIVER_RUNTIME_SERIAL_SPSC_SHARED_PAGES: usize = 4;
/// First shared page in the root-to-runtime serial TX ring.
pub const DRIVER_RUNTIME_SERIAL_TX_SPSC_FIRST_PAGE: usize = 0;
/// First shared page in the runtime-to-root serial RX ring.
pub const DRIVER_RUNTIME_SERIAL_RX_SPSC_FIRST_PAGE: usize = 2;
/// Fixed metadata bytes at the front of each two-page serial SPSC ring.
pub const DRIVER_RUNTIME_SERIAL_SPSC_HEADER_BYTES: usize = 64;
/// Exact byte capacity of either serial SPSC ring.
pub const DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY: usize = DRIVER_RUNTIME_SERIAL_SPSC_PAGES_PER_RING
    * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as usize
    - DRIVER_RUNTIME_SERIAL_SPSC_HEADER_BYTES;
/// SPSC direction flag: root is the sole producer and serial is the consumer.
pub const DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME: u32 = 1 << 0;
/// SPSC direction flag: serial is the sole producer and root is the consumer.
pub const DRIVER_RUNTIME_SERIAL_SPSC_FLAG_RUNTIME_TO_ROOT: u32 = 1 << 1;
/// SPSC state flag: the generation is fenced and cannot carry more bytes.
pub const DRIVER_RUNTIME_SERIAL_SPSC_FLAG_POISONED: u32 = 1 << 2;
/// Complete admitted serial SPSC flag set.
pub const DRIVER_RUNTIME_SERIAL_SPSC_KNOWN_FLAGS: u32 =
    DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME
        | DRIVER_RUNTIME_SERIAL_SPSC_FLAG_RUNTIME_TO_ROOT
        | DRIVER_RUNTIME_SERIAL_SPSC_FLAG_POISONED;

/// Pointer-free metadata shared by one serial SPSC producer and consumer.
///
/// Indices are monotonically wrapping byte cursors. Each owner writes its
/// cursor and then repeats it in the corresponding commit word after a release
/// fence. The peer accepts only an equal cursor/commit pair and performs an
/// acquire fence before touching payload bytes. `doorbell_epoch` advances only
/// when a committed producer transition changes the ring from empty to
/// non-empty; notification history remains a scheduling hint, never byte
/// authority. `consumer_wake_epoch` is the bounded full-to-not-full rearm hint.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSerialSpscHeader {
    /// [`DRIVER_RUNTIME_SERIAL_SPSC_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_SERIAL_SPSC_VERSION`].
    pub version: u16,
    /// Exact metadata bytes.
    pub header_len: u16,
    /// Nonzero root-selected runtime generation.
    pub generation: u32,
    /// [`DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY`].
    pub capacity: u32,
    /// Producer-owned monotonic byte cursor.
    pub producer_index: u32,
    /// Commit-last repetition of `producer_index`.
    pub producer_commit: u32,
    /// Consumer-owned monotonic byte cursor.
    pub consumer_index: u32,
    /// Commit-last repetition of `consumer_index`.
    pub consumer_commit: u32,
    /// Producer-owned empty-to-nonempty scheduling-hint epoch.
    pub doorbell_epoch: u32,
    /// Reserved for a future protocol revision; must remain zero.
    pub reserved0: u32,
    /// Consumer-owned full-to-not-full producer-rearm epoch.
    pub consumer_wake_epoch: u32,
    /// One direction bit plus optional poison state.
    pub flags: u32,
    /// Saturating producer byte telemetry.
    pub produced_bytes: u32,
    /// Saturating consumer byte telemetry.
    pub consumed_bytes: u32,
    /// Maximum committed occupancy observed by the producer.
    pub high_water: u32,
    /// Reserved for a future protocol revision; must remain zero.
    pub reserved1: u32,
}

impl DriverRuntimeSerialSpscHeader {
    /// Construct an empty ring for one exact generation and direction.
    #[must_use]
    pub const fn empty(generation: u32, direction: u32) -> Self {
        Self {
            magic: DRIVER_RUNTIME_SERIAL_SPSC_MAGIC,
            version: DRIVER_RUNTIME_SERIAL_SPSC_VERSION,
            header_len: DRIVER_RUNTIME_SERIAL_SPSC_HEADER_BYTES as u16,
            generation,
            capacity: DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY as u32,
            producer_index: 0,
            producer_commit: 0,
            consumer_index: 0,
            consumer_commit: 0,
            doorbell_epoch: 0,
            reserved0: 0,
            consumer_wake_epoch: 0,
            flags: direction,
            produced_bytes: 0,
            consumed_bytes: 0,
            high_water: 0,
            reserved1: 0,
        }
    }

    /// Return whether the header describes one exact live SPSC generation.
    #[must_use]
    pub const fn valid_for(self, generation: u32, direction: u32) -> bool {
        let direction_bits = self.flags
            & (DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME
                | DRIVER_RUNTIME_SERIAL_SPSC_FLAG_RUNTIME_TO_ROOT);
        self.magic == DRIVER_RUNTIME_SERIAL_SPSC_MAGIC
            && self.version == DRIVER_RUNTIME_SERIAL_SPSC_VERSION
            && self.header_len as usize == DRIVER_RUNTIME_SERIAL_SPSC_HEADER_BYTES
            && generation != 0
            && self.generation == generation
            && self.capacity as usize == DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY
            && self.producer_index == self.producer_commit
            && self.consumer_index == self.consumer_commit
            && self.producer_index.wrapping_sub(self.consumer_index) <= self.capacity
            && direction_bits == direction
            && matches!(
                direction,
                DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME
                    | DRIVER_RUNTIME_SERIAL_SPSC_FLAG_RUNTIME_TO_ROOT
            )
            && self.flags & !DRIVER_RUNTIME_SERIAL_SPSC_KNOWN_FLAGS == 0
            && self.flags & DRIVER_RUNTIME_SERIAL_SPSC_FLAG_POISONED == 0
            && self.high_water <= self.capacity
            && self.reserved0 == 0
            && self.reserved1 == 0
    }

    /// Return the committed byte occupancy, or `None` for corrupt cursors.
    #[must_use]
    pub const fn occupancy(self) -> Option<u32> {
        let occupancy = self.producer_index.wrapping_sub(self.consumer_index);
        if self.producer_index != self.producer_commit
            || self.consumer_index != self.consumer_commit
            || occupancy > self.capacity
        {
            None
        } else {
            Some(occupancy)
        }
    }
}

/// Decide whether a committed producer turn must publish a data doorbell.
///
/// The second predicate closes the enqueue/drain interleaving where the
/// producer originally observed a non-empty ring, the consumer committed a
/// drain through the producer's old cursor, and the producer then exposed new
/// bytes. If the consumer instead observes the new producer commit, its final
/// recheck keeps service active and this extra hint is unnecessary.
#[must_use]
pub const fn driver_runtime_serial_spsc_data_doorbell_due(
    initial_occupancy: u32,
    initial_producer: u32,
    post_commit_consumer: u32,
) -> bool {
    initial_occupancy == 0 || post_commit_consumer == initial_producer
}

/// Validate a consumer's post-commit peer recheck and derive wake state.
///
/// `producer_rearm` covers both a ring that was already full and a producer
/// that committed the final free bytes against the consumer's old cursor
/// while this consumer turn was publishing new space. `work_remaining` is
/// derived from the producer commit sampled after the consumer commit, so a
/// concurrent enqueue cannot be lost before the consumer sleeps.
#[must_use]
pub const fn driver_runtime_serial_spsc_consumer_post_commit(
    initial_available: u32,
    initial_consumer: u32,
    next_consumer: u32,
    post_commit_producer: u32,
) -> Option<(bool, bool)> {
    let capacity = DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY as u32;
    let consumed = next_consumer.wrapping_sub(initial_consumer);
    let remaining = post_commit_producer.wrapping_sub(next_consumer);
    if initial_available > capacity
        || consumed == 0
        || consumed > initial_available
        || remaining > capacity
    {
        return None;
    }
    let producer_rearm = initial_available == capacity
        || post_commit_producer.wrapping_sub(initial_consumer) == capacity;
    Some((producer_rearm, remaining != 0))
}

const _: () = {
    assert!(core::mem::size_of::<DriverRuntimeSerialSpscHeader>() == 64);
    assert!(core::mem::align_of::<DriverRuntimeSerialSpscHeader>() == 64);
    assert!(DRIVER_RUNTIME_SERIAL_TX_SPSC_FIRST_PAGE == 0);
    assert!(DRIVER_RUNTIME_SERIAL_RX_SPSC_FIRST_PAGE == DRIVER_RUNTIME_SERIAL_SPSC_PAGES_PER_RING);
    assert!(
        DRIVER_RUNTIME_SERIAL_RX_SPSC_FIRST_PAGE + DRIVER_RUNTIME_SERIAL_SPSC_PAGES_PER_RING
            == DRIVER_RUNTIME_SERIAL_SPSC_SHARED_PAGES
    );
    assert!(DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY == 8128);
};
/// USB old-good step: xHCI reached its runtime-owned ready terminal.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_XHCI_READY: u32 = 1 << 0;
/// USB old-good step: the linked runtime consumed a successful command event.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_COMMAND_EVENT: u32 = 1 << 1;
/// USB old-good step: one live root port completed reset.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_ROOT_PORT_RESET: u32 = 1 << 2;
/// USB old-good step: the root hub device was addressed.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_ADDRESSED: u32 = 1 << 3;
/// USB old-good step: the hub accepted SET_CONFIGURATION.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_CONFIGURED: u32 = 1 << 4;
/// USB old-good step: the hub descriptor and xHCI hub context completed.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_CONTEXT: u32 = 1 << 5;
/// USB old-good step: the selected downstream hub port completed power settle.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_PORT_POWER: u32 = 1 << 6;
/// USB old-good step: the selected downstream hub port returned GET_STATUS.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_PORT_STATUS: u32 = 1 << 7;
/// USB old-good step: the selected downstream hub port reached reset-ready.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_PORT_READY: u32 = 1 << 8;
/// USB old-good step: the selected hub child entered its device probe.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HUB_CHILD_PROBE: u32 = 1 << 9;
/// USB old-good step: the selected child exposed a boot-keyboard HID endpoint.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_HID_ENDPOINT: u32 = 1 << 10;
/// USB old-good step: one interrupt-IN transfer was armed for that endpoint.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_INTERRUPT_IN: u32 = 1 << 11;
/// USB old-good step: the runtime accepted its first provenance-safe HID report.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_FIRST_REPORT: u32 = 1 << 12;
/// USB old-good step: the runtime decoded its first linked-runtime HID byte.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_FIRST_BYTE: u32 = 1 << 13;
/// Exact complete ordered USB old-good step mask.
pub const DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK: u32 = (1 << 14) - 1;
/// Sticky receipt bit set when a lifecycle attempts a skipped/reordered step.
pub const DRIVER_RUNTIME_USB_OLDGOOD_INVALID_ORDER: u32 = 1 << 31;
/// Fixed offset of the SDIO owner's physical WiFi lifetime record.
///
/// The record occupies the reserved gap immediately after the generic runtime
/// progress marker and before the CYW43/SDIO DPC event ring. Only the isolated
/// SDIO owner writes this record.
pub const DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_OFFSET: u16 = 144;
/// Bytes in the SDIO owner's physical WiFi lifetime record.
pub const DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_BYTES: u16 = 16;
/// Magic value for an initialized SDIO physical WiFi lifetime record.
pub const DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_MAGIC: u32 = 0x5344_4c46;
/// Fixed offset of the retained-command continuation grant.
///
/// The command record occupies bytes `0..40` and the completion record begins
/// at byte 64. This pointer-free ticket uses the otherwise reserved gap so a
/// linked producer can durably grant one later runtime quantum without
/// republishing the immutable command or relying on a coalescing badge as
/// authority.
pub const DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET: u16 = 40;
/// Bytes in one retained-command continuation grant.
pub const DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES: u16 = 24;
/// Magic value for a retained-command continuation grant.
pub const DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC: u32 = 0x4452_4347;
/// Consumer-state bit proving a grant was admitted before physical I/O.
///
/// Root-owned and delegated CYW43-to-SDIO grant IDs are restricted to the low
/// 31 bits. The isolated consumer first publishes
/// `grant_id | ADMITTED_BIT` after its exact ACK-before-I/O check, then
/// replaces that value with the unmodified `grant_id` only after the admitted
/// bounded action finishes. A producer may use the low-domain completion to
/// publish a successor, while the high-bit state can only be waited on.
pub const DRIVER_RUNTIME_CONTINUATION_GRANT_ACTION_ADMITTED_BIT: u32 = 1 << 31;

/// Return the distinct in-flight consumer value for one low-domain grant.
#[must_use]
pub const fn driver_runtime_continuation_grant_action_admitted_id(grant_id: u32) -> Option<u32> {
    if grant_id != 0 && grant_id & DRIVER_RUNTIME_CONTINUATION_GRANT_ACTION_ADMITTED_BIT == 0 {
        Some(grant_id | DRIVER_RUNTIME_CONTINUATION_GRANT_ACTION_ADMITTED_BIT)
    } else {
        None
    }
}

/// Return whether a consumer value proves admission but not action completion.
#[must_use]
pub const fn driver_runtime_continuation_grant_action_admitted(
    consumed_grant_id: u32,
    grant_id: u32,
) -> bool {
    match driver_runtime_continuation_grant_action_admitted_id(grant_id) {
        Some(admitted) => consumed_grant_id == admitted,
        None => false,
    }
}
/// Fixed offset of the exact heartbeat for a grant-free owner command.
///
/// A finite steady lease or persistent physical SDIO child and a continuation
/// grant are mutually exclusive authority modes for one immutable command, so
/// their records deliberately share the same fixed 24-byte slot. Distinct
/// magic values make cross-mode samples fail closed.
pub const DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_OFFSET: u16 =
    DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET;
/// Bytes in one exact grant-free owner heartbeat record.
pub const DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_BYTES: u16 =
    DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES;
/// Magic value for a grant-free owner heartbeat record.
pub const DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC: u32 = 0x4452_5350;
/// Fixed offset of an exact generic MCS one-way continuation wait receipt.
///
/// Generic one-way commands cannot reuse the command endpoint after their
/// initial notification-prompted intake. Their final-prewait receipt is
/// mutually exclusive with continuation grants, steady-service progress, and
/// persistent op11 waits, so it shares the same fixed auxiliary slot. The
/// distinct magic prevents a diagnostic heartbeat or another authority mode
/// from being interpreted as permission to issue a scheduling prompt.
pub const DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_OFFSET: u16 =
    DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET;
/// Bytes in one exact generic MCS one-way continuation wait receipt.
pub const DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_BYTES: u16 = DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES;
/// Magic value for a committed generic one-way wait receipt (`DROW`).
pub const DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC: u32 = 0x4452_4f57;
/// Magic value after root acknowledges that exact wait slice (`DROA`).
pub const DRIVER_RUNTIME_ONE_WAY_WAIT_ACK_MAGIC: u32 = 0x4452_4f41;
/// Fixed offset of the exact persistent-transaction wait receipt.
///
/// Continuation grants, finite steady-service progress, and persistent op11
/// waits are mutually exclusive command modes, so all three records share the
/// same fixed 24-byte auxiliary slot. Their distinct magic values make a
/// cross-mode sample fail closed.
pub const DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_OFFSET: u16 =
    DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET;
/// Bytes in one exact persistent-transaction wait receipt.
pub const DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_BYTES: u16 =
    DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES;
/// Magic value for a committed persistent-transaction wait receipt (`DRPW`).
pub const DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC: u32 = 0x4452_5057;
/// Bytes in the runtime progress marker.
pub const DRIVER_RUNTIME_RING_PROGRESS_BYTES: u16 = 16;
/// Runtime progress-marker magic.
pub const DRIVER_RUNTIME_RING_PROGRESS_MAGIC: u32 = 0x4452_5047;
/// Fixed offset of the non-network runtime cadence record.
///
/// This role-local range is used only by serial, USB, HDMI, GENET, and PCIe
/// runtime images. The SDIO owner deliberately uses the same numeric range for
/// its physical-lifetime and DPC records, so CYW43/SDIO must continue to use
/// their existing episode evidence instead of publishing this record.
pub const DRIVER_RUNTIME_CADENCE_OFFSET: u16 = 144;
/// Bytes in one sequence-last runtime cadence record.
pub const DRIVER_RUNTIME_CADENCE_BYTES: u16 = 48;
/// Magic value for a runtime cadence record (`DRCD`).
pub const DRIVER_RUNTIME_CADENCE_MAGIC: u32 = 0x4452_4344;
/// Runtime cadence layout version.
pub const DRIVER_RUNTIME_CADENCE_VERSION: u16 = 2;
/// Cadence exit: one command has entered its runtime service episode.
pub const DRIVER_RUNTIME_CADENCE_EXIT_ENTER: u16 = 1;
/// Cadence exit: bounded useful work completed and the same episode continues.
pub const DRIVER_RUNTIME_CADENCE_EXIT_PROGRESS: u16 = 2;
/// Cadence exit: the runtime is yielding with a retained command.
pub const DRIVER_RUNTIME_CADENCE_EXIT_YIELD: u16 = 3;
/// Cadence exit: the exact command reached a terminal completion.
pub const DRIVER_RUNTIME_CADENCE_EXIT_TERMINAL: u16 = 4;
/// Cadence flag: useful work remains in the same command episode.
pub const DRIVER_RUNTIME_CADENCE_FLAG_WORK_REMAINS: u16 = 1 << 0;
/// Cadence flag: `work_completed` and `work_total` count bytes.
pub const DRIVER_RUNTIME_CADENCE_FLAG_WORK_BYTES: u16 = 1 << 1;
/// Cadence flag: `previous_entry_cntvct_lo` identifies the preceding episode.
pub const DRIVER_RUNTIME_CADENCE_FLAG_PREVIOUS_ENTRY_VALID: u16 = 1 << 2;

/// Passive sequence-last evidence for one non-network runtime service episode.
///
/// The record is diagnostic only: it carries no capability, notification,
/// continuation, retry, or scheduling authority. A producer clears
/// `committed_sequence`, writes the complete body, then repeats `sequence` in
/// the final word. Readers accept two identical valid samples.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCadenceRecord {
    /// [`DRIVER_RUNTIME_CADENCE_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CADENCE_VERSION`].
    pub version: u16,
    /// Exact record size.
    pub len: u16,
    /// Exact root command sequence being measured.
    pub sequence: u32,
    /// Most recent durable runtime progress phase.
    pub phase: u32,
    /// Virtual-counter sample taken when this command entered service.
    pub entry_cntvct: u64,
    /// Low counter word from the preceding command entry, when flagged valid.
    pub previous_entry_cntvct_lo: u32,
    /// Low counter word represented by this publication.
    pub last_cntvct_lo: u32,
    /// Bounded useful work completed at this phase.
    pub work_completed: u32,
    /// Complete bounded work extent, or zero when not applicable.
    pub work_total: u32,
    /// One `DRIVER_RUNTIME_CADENCE_EXIT_*` value.
    pub exit_reason: u16,
    /// `DRIVER_RUNTIME_CADENCE_FLAG_*` evidence bits.
    pub flags: u16,
    /// Sequence-last commit; exactly repeats `sequence`.
    pub committed_sequence: u32,
}

impl DriverRuntimeCadenceRecord {
    /// Construct one uncommitted cadence body.
    ///
    /// Arguments intentionally mirror the fixed wire fields so call sites
    /// cannot obscure which bounded measurement is published.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn staged(
        sequence: u32,
        phase: u32,
        entry_cntvct: u64,
        last_cntvct: u64,
        work_completed: u32,
        work_total: u32,
        exit_reason: u16,
        flags: u16,
    ) -> Self {
        Self::staged_with_previous_entry(
            sequence,
            phase,
            entry_cntvct,
            0,
            last_cntvct as u32,
            work_completed,
            work_total,
            exit_reason,
            flags,
        )
    }

    /// Construct one uncommitted cadence body with an inter-entry sample.
    ///
    /// Arguments intentionally mirror the fixed wire fields so the two-entry
    /// timing identity remains explicit at every publication site.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn staged_with_previous_entry(
        sequence: u32,
        phase: u32,
        entry_cntvct: u64,
        previous_entry_cntvct_lo: u32,
        last_cntvct_lo: u32,
        work_completed: u32,
        work_total: u32,
        exit_reason: u16,
        flags: u16,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_CADENCE_MAGIC,
            version: DRIVER_RUNTIME_CADENCE_VERSION,
            len: core::mem::size_of::<Self>() as u16,
            sequence,
            phase,
            entry_cntvct,
            previous_entry_cntvct_lo,
            last_cntvct_lo,
            work_completed,
            work_total,
            exit_reason,
            flags,
            committed_sequence: 0,
        }
    }

    /// Return whether this is one complete authority-free publication.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_CADENCE_MAGIC
            && self.version == DRIVER_RUNTIME_CADENCE_VERSION
            && self.len as usize == core::mem::size_of::<Self>()
            && self.sequence != 0
            && self.committed_sequence == self.sequence
            && matches!(
                self.exit_reason,
                DRIVER_RUNTIME_CADENCE_EXIT_ENTER
                    | DRIVER_RUNTIME_CADENCE_EXIT_PROGRESS
                    | DRIVER_RUNTIME_CADENCE_EXIT_YIELD
                    | DRIVER_RUNTIME_CADENCE_EXIT_TERMINAL
            )
            && self.flags
                & !(DRIVER_RUNTIME_CADENCE_FLAG_WORK_REMAINS
                    | DRIVER_RUNTIME_CADENCE_FLAG_WORK_BYTES
                    | DRIVER_RUNTIME_CADENCE_FLAG_PREVIOUS_ENTRY_VALID)
                == 0
            && (self.work_total == 0 || self.work_completed <= self.work_total)
    }
}

/// Passive sequence-last evidence for the isolated serial owner's RX path.
///
/// The record is diagnostic only. It carries no command, completion, IRQ,
/// notification, scheduling, or retry authority. The runtime publishes it
/// after initialization, explicit root RX turns, and exceptional IRQ state;
/// root accepts only two identical complete samples.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSerialRxState {
    /// [`DRIVER_RUNTIME_SERIAL_RX_STATE_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_SERIAL_RX_STATE_VERSION`].
    pub version: u16,
    /// Exact record size.
    pub len: u16,
    /// Monotonic nonzero diagnostic publication identity.
    pub publication: u32,
    /// Bound-notification wakes accepted by the serial owner.
    pub irq_wakes: u32,
    /// Successful serial IRQ-handler acknowledgements.
    pub irq_acks: u32,
    /// Failed serial IRQ-handler acknowledgement attempts.
    pub irq_ack_failures: u32,
    /// Mini-UART hardware-overrun observations.
    pub hardware_overrun_events: u32,
    /// RX software-queue-full observations.
    pub queue_full_events: u32,
    /// Bytes accepted into the owner's bounded RX queue.
    pub received_bytes: u32,
    /// Bytes currently retained in that queue.
    pub queued_bytes: u16,
    /// [`DRIVER_RUNTIME_SERIAL_RX_STATE_FLAG_ACK_PENDING`] and future flags.
    pub flags: u16,
    /// Low virtual-counter word sampled at publication.
    pub last_cntvct_lo: u32,
    /// Sequence-last commit; exactly repeats `publication`.
    pub committed_publication: u32,
}

impl DriverRuntimeSerialRxState {
    /// Return whether this is one complete authority-free publication.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_SERIAL_RX_STATE_MAGIC
            && self.version == DRIVER_RUNTIME_SERIAL_RX_STATE_VERSION
            && self.len as usize == core::mem::size_of::<Self>()
            && self.publication != 0
            && self.committed_publication == self.publication
            && self.flags & !DRIVER_RUNTIME_SERIAL_RX_STATE_FLAG_ACK_PENDING == 0
    }
}

/// Root-to-runtime entry marker requesting a cold local-state reset before receive admission.
pub const DRIVER_RUNTIME_TASK_KEY_RESTART_FLAG: u32 = 1 << 31;
/// Runtime has observed the staged command.
pub const DRIVER_RUNTIME_RING_PROGRESS_COMMAND_OBSERVED: u32 = 1;
/// Runtime has validated the command role and is entering dispatch.
pub const DRIVER_RUNTIME_RING_PROGRESS_COMMAND_VALIDATED: u32 = 2;
/// Runtime is entering the role-specific engine-init path.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN: u32 = 3;
/// Runtime completed the role-specific engine-init path.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DONE: u32 = 4;
/// Runtime rejected the role-specific engine-init path.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FAILED: u32 = 5;
/// Runtime descriptor state matches the command hot path.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_READY: u32 = 6;
/// Runtime descriptor state does not match the command hot path.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_MISMATCH: u32 = 7;
/// Runtime loaded the retained init descriptor for engine init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_LOADED: u32 = 8;
/// Runtime validated descriptor identity for engine init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY: u32 = 9;
/// Runtime started validating mapped resources for engine init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN: u32 = 14;
/// Runtime rejected mapped resources for engine init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED: u32 = 15;
/// Runtime validated the init-descriptor header during resource checks.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID: u32 = 50;
/// Runtime found an invalid init descriptor during resource checks.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID: u32 = 51;
/// Runtime verified the command hot path matches the retained descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY: u32 = 52;
/// Runtime found a hot-path mismatch during resource checks.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH: u32 = 53;
/// Runtime computed bounded MMIO/DMA/shared resource totals.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY: u32 = 54;
/// Runtime found a missing or undersized MMIO resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING: u32 = 55;
/// Runtime verified required MMIO resources.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY: u32 = 56;
/// Runtime found a missing or undersized DMA resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING: u32 = 57;
/// Runtime verified required DMA resources.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY: u32 = 58;
/// Runtime found a missing or undersized shared resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING: u32 = 59;
/// Runtime verified required shared resources.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY: u32 = 60;
/// Runtime found a missing framebuffer resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FRAMEBUFFER_MISSING: u32 = 61;
/// Runtime verified the framebuffer resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FRAMEBUFFER_READY: u32 = 62;
/// Runtime found a missing pointer-free bus-owner link.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING: u32 = 63;
/// Runtime verified required pointer-free bus-owner links.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY: u32 = 64;
/// Runtime found an authority window that must not be present for the role.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT: u32 = 65;
/// Runtime completed role-specific resource validation.
pub const DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY: u32 = 66;
/// Runtime validated mapped resources for engine init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY: u32 = 10;
/// Runtime entered role-specific hardware init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN: u32 = 11;
/// Runtime completed role-specific hardware init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_DONE: u32 = 12;
/// Runtime rejected role-specific hardware init.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_FAILED: u32 = 13;
/// USB runtime read xHCI capability registers.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ: u32 = 20;
/// USB runtime halted the xHCI controller before reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HALTED: u32 = 21;
/// USB runtime completed xHCI controller reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_DONE: u32 = 22;
/// USB runtime prepared DMA structures.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_READY: u32 = 23;
/// USB runtime is programming the xHCI Device Context Base Address Array.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_BEGIN: u32 = 70;
/// USB runtime wrote the low 32-bit half of DCBAAP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_LOW_WRITTEN: u32 = 90;
/// USB runtime wrote the high 32-bit half of DCBAAP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_WRITTEN: u32 = 91;
/// USB runtime flushed the high 32-bit half of DCBAAP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_FLUSHED: u32 = 92;
/// USB runtime is programming the xHCI command ring control register.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_BEGIN: u32 = 71;
/// USB runtime wrote the low 32-bit half of CRCR.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_LOW_WRITTEN: u32 = 93;
/// USB runtime wrote the high 32-bit half of CRCR.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_WRITTEN: u32 = 94;
/// USB runtime flushed the high 32-bit half of CRCR.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_FLUSHED: u32 = 95;
/// USB runtime is quiescing device notification delivery.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DNCTRL_BEGIN: u32 = 72;
/// USB runtime is programming the enabled-slot count.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_BEGIN: u32 = 73;
/// USB runtime wrote the enabled-slot count.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_WRITTEN: u32 = 103;
/// USB runtime flushed the enabled-slot count write.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_FLUSHED: u32 = 104;
/// USB runtime is programming the primary interrupter control register.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_IMAN_BEGIN: u32 = 74;
/// USB runtime is programming the interrupter moderation register.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_IMOD_BEGIN: u32 = 75;
/// USB runtime is programming the event-ring segment-table size.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTSZ_BEGIN: u32 = 76;
/// USB runtime is programming the event-ring segment-table address.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_BEGIN: u32 = 77;
/// USB runtime wrote the low 32-bit half of ERSTBA.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_LOW_WRITTEN: u32 = 96;
/// USB runtime wrote the high 32-bit half of ERSTBA.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_WRITTEN: u32 = 97;
/// USB runtime flushed the high 32-bit half of ERSTBA.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_FLUSHED: u32 = 98;
/// USB runtime is programming the event-ring dequeue pointer.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_BEGIN: u32 = 78;
/// USB runtime wrote the low 32-bit half of ERDP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_LOW_WRITTEN: u32 = 99;
/// USB runtime wrote the high 32-bit half of ERDP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_WRITTEN: u32 = 100;
/// USB runtime flushed the high 32-bit half of ERDP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_FLUSHED: u32 = 101;
/// USB runtime is publishing the xHCI scratchpad array through DCBAA slot 0.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_BEGIN: u32 = 102;
/// USB runtime wrote DCBAA slot 0 with the scratchpad pointer-array address.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_WRITTEN: u32 = 105;
/// USB runtime cleaned DCBAA slot 0 after writing the scratchpad array pointer.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_CLEANED: u32 = 106;
/// USB runtime filled the xHCI scratchpad pointer array.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_FILLED: u32 = 107;
/// USB runtime cleaned the xHCI scratchpad pointer array.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_CLEANED: u32 = 108;
/// USB runtime is submitting the gate-4 xHCI command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_SUBMIT_BEGIN: u32 = 131;
/// USB runtime wrote and cleaned the gate-4 command TRB.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_TRB_WRITTEN: u32 = 132;
/// USB runtime is ringing xHCI doorbell 0 for the gate-4 proof command.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_BEGIN: u32 = 133;
/// USB runtime completed the xHCI doorbell 0 publish edge.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_DONE: u32 = 134;
/// USB runtime is polling the event ring for the gate-4 command completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_BEGIN: u32 = 135;
/// USB runtime found no matching command completion in this bounded poll slice.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_PENDING: u32 = 136;
/// USB runtime saw the gate-4 command-completion event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_READY: u32 = 137;
/// USB runtime rejected the gate-4 command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_FAILED: u32 = 138;
/// USB runtime sees an empty event TRB while polling the gate-4 command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_SLOT_EMPTY: u32 = 184;
/// USB runtime sees a nonempty event TRB whose cycle bit is not yet consumable.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_CYCLE_MISMATCH: u32 = 185;
/// USB runtime is about to read the next event TRB for the gate-4 command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN: u32 = 186;
/// USB runtime resolved the event TRB address and is entering the DMA read edge.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN: u32 = 188;
/// USB runtime completed the event-ring DMA load barrier before reading the TRB.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE: u32 = 189;
/// USB runtime completed event-ring cache invalidation before reading TRB words.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE: u32 = 199;
/// USB runtime completed the next event TRB read for the gate-4 command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE: u32 = 187;
/// USB runtime is resetting a connected root port before Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN: u32 = 190;
/// USB runtime completed root-port reset and is entering Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_DONE: u32 = 191;
/// USB runtime asserted root-port power and flushed the PORTSC write.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POWER_WRITE_DONE: u32 = 325;
/// USB runtime is waiting for root-port connect status after power assertion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_WAIT_BEGIN: u32 = 326;
/// USB runtime exhausted the root-port connect wait after power assertion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_TIMEOUT: u32 = 327;
/// USB runtime set the root-port reset bit and flushed the PORTSC write.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PR_SET: u32 = 328;
/// USB runtime is polling for root-port reset completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN: u32 = 329;
/// USB runtime observed root-port reset-change completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PRC_SEEN: u32 = 330;
/// USB runtime observed reset completion without a port-enable bit.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_ENABLE_TIMEOUT: u32 = 331;
/// USB runtime exhausted the root-port reset completion wait.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_TIMEOUT: u32 = 332;
/// USB runtime is retrying the U-Boot-shaped root-port reset envelope.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_RETRY: u32 = 333;
/// USB runtime exhausted the U-Boot-shaped root-port reset envelope.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_FAILED: u32 = 334;
/// USB runtime is running the stale U-Boot root-port cleanup reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_BEGIN: u32 = 335;
/// USB runtime completed the stale U-Boot root-port cleanup reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_DONE: u32 = 336;
/// USB runtime could not complete the stale cleanup reset and kept first reset proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_FAILED: u32 = 337;
/// USB runtime is submitting Enable Slot for the Address Device path.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_BEGIN: u32 = 192;
/// USB runtime completed Enable Slot and has a candidate slot id.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_DONE: u32 = 193;
/// USB runtime published input/device contexts through DCBAA for Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_CONTEXTS_PUBLISHED: u32 = 194;
/// USB runtime is waiting for the Address Device command completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN: u32 = 195;
/// USB runtime completed the Address Device command.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE: u32 = 196;
/// USB runtime did not complete the Address Device command.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED: u32 = 197;
/// USB runtime published the addressed-device state and is entering descriptors.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED: u32 = 198;
/// USB runtime is polling the Address Device command event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_BEGIN: u32 = 341;
/// USB runtime is peeking the event ring for Address Device command completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PEEK_BEGIN: u32 = 342;
/// USB runtime found no event TRB while polling Address Device completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY: u32 = 343;
/// USB runtime found an event-ring cycle mismatch while polling Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_CYCLE_MISMATCH: u32 = 344;
/// USB runtime saw a command-completion event while polling Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_COMMAND: u32 = 345;
/// USB runtime saw a port-status event while polling Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PORT_STATUS: u32 = 346;
/// USB runtime saw a non-command event while polling Address Device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_OTHER: u32 = 347;
/// USB runtime is still inside the bounded Address Device command poll.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_PENDING: u32 = 348;
/// USB runtime is submitting the addressed device descriptor request.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_BEGIN: u32 = 218;
/// USB runtime published EP0 TRBs and rang the addressed device doorbell.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DOORBELL_DONE: u32 = 219;
/// USB runtime is polling EP0 for the device descriptor transfer.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN: u32 = 220;
/// USB runtime observed a device descriptor data-stage transfer event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT: u32 = 221;
/// USB runtime observed the device descriptor status-stage transfer event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT: u32 = 222;
/// USB runtime observed a failed device descriptor transfer event or ack edge.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_FAILED: u32 = 223;
/// USB runtime timed out before any device descriptor data-stage event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_TIMEOUT: u32 = 224;
/// USB runtime timed out after the data-stage event but before status completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_TIMEOUT: u32 = 225;
/// USB runtime is submitting the full-speed device descriptor prime request.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_BEGIN: u32 = 226;
/// USB runtime rang EP0 doorbell for the full-speed descriptor prime.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DOORBELL_DONE: u32 = 227;
/// USB runtime is polling EP0 for the full-speed descriptor prime.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN: u32 = 228;
/// USB runtime observed a full-speed descriptor-prime data event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DATA_EVENT: u32 = 229;
/// USB runtime observed the full-speed descriptor-prime status event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT: u32 = 230;
/// USB runtime observed a failed full-speed descriptor-prime transfer edge.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_FAILED: u32 = 231;
/// USB runtime timed out before a full-speed descriptor-prime data event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_TIMEOUT: u32 = 232;
/// USB runtime timed out after full-speed descriptor-prime data but before status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_TIMEOUT: u32 = 233;
/// USB runtime is submitting the configuration descriptor header request.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_BEGIN: u32 = 234;
/// USB runtime rang EP0 doorbell for the configuration descriptor header.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DOORBELL_DONE: u32 = 235;
/// USB runtime is polling EP0 for the configuration descriptor header.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN: u32 = 236;
/// USB runtime observed a configuration descriptor header data event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DATA_EVENT: u32 = 237;
/// USB runtime observed the configuration descriptor header status event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT: u32 = 238;
/// USB runtime observed a failed configuration descriptor header transfer edge.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_FAILED: u32 = 239;
/// USB runtime timed out before a configuration descriptor header data event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_TIMEOUT: u32 = 240;
/// USB runtime timed out after configuration descriptor header data but before status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_TIMEOUT: u32 = 241;
/// USB runtime is submitting the full configuration descriptor request.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_BEGIN: u32 = 242;
/// USB runtime rang EP0 doorbell for the full configuration descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DOORBELL_DONE: u32 = 243;
/// USB runtime is polling EP0 for the full configuration descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_WAIT_BEGIN: u32 = 244;
/// USB runtime observed a full configuration descriptor data event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DATA_EVENT: u32 = 245;
/// USB runtime observed the full configuration descriptor status event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT: u32 = 246;
/// USB runtime observed a failed full configuration descriptor transfer edge.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_FAILED: u32 = 247;
/// USB runtime timed out before a full configuration descriptor data event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_TIMEOUT: u32 = 248;
/// USB runtime timed out after full configuration descriptor data but before status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_TIMEOUT: u32 = 249;
/// USB runtime saw a port-status event while polling the command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PORT_STATUS: u32 = 145;
/// USB runtime saw a command-completion event while polling the command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_COMMAND: u32 = 146;
/// USB runtime saw a non-command event while polling the command proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_OTHER: u32 = 147;
/// USB runtime is acknowledging a consumed command-proof event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_BEGIN: u32 = 148;
/// USB runtime completed prompt-safe ERDP acknowledgement for a command-proof event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_DONE: u32 = 149;
/// USB runtime is returning a pending command-proof completion to root.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_RETURN_PENDING: u32 = 150;
/// Runtime is about to enter the bounded engine-init marker handoff.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER: u32 = 151;
/// USB runtime is about to reset its local controller state.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_BEGIN: u32 = 152;
/// USB runtime finished resetting local controller state.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_DONE: u32 = 153;
/// USB runtime is about to touch xHCI registers for hardware init.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HW_ENTRY: u32 = 154;
/// SDIO runtime is about to reset its local host state.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_BEGIN: u32 = 155;
/// SDIO runtime finished resetting local host state.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_DONE: u32 = 156;
/// SDIO runtime is about to touch SDHCI registers for hardware init.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_HW_ENTRY: u32 = 157;
/// USB runtime entered its local init routine before touching state storage.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_INIT_ENTRY: u32 = 158;
/// USB runtime is about to borrow/reset its local controller state.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_ACCESS_BEGIN: u32 = 159;
/// USB runtime found the descriptor-published DMA arena.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_RANGE_READY: u32 = 160;
/// USB runtime is about to read xHCI capability registers.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ_BEGIN: u32 = 161;
/// USB runtime rejected the xHCI capability register snapshot.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_INVALID: u32 = 162;
/// Historical USB runtime request for a PCIe-owner xHCI posted-write flush.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_BEGIN: u32 = 163;
/// Historical USB runtime PCIe-owner posted-write flush completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_DONE: u32 = 164;
/// Historical USB runtime PCIe-owner posted-write flush failure.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_FAILED: u32 = 165;
/// USB runtime is about to clear the xHCI RUN bit.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_BEGIN: u32 = 166;
/// USB runtime is polling for xHCI halted status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_WAIT_BEGIN: u32 = 167;
/// USB runtime is about to assert xHCI host-controller reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_BEGIN: u32 = 168;
/// USB runtime is polling for xHCI reset completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_WAIT_BEGIN: u32 = 169;
/// USB runtime is polling for xHCI controller-not-ready clear.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CNR_WAIT_BEGIN: u32 = 170;
/// USB runtime is polling for xHCI run-state transition after RUN.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_WAIT_BEGIN: u32 = 171;
/// Runtime entered the role-specific engine-init dispatcher.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY: u32 = 172;
/// Runtime selected the SDIO host engine-init branch.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_ENGINE_INIT_BRANCH: u32 = 173;
/// SDIO runtime is about to reset cached SDHCI register shadows.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_BEGIN: u32 = 174;
/// SDIO runtime reset cached SDHCI register shadows.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_DONE: u32 = 175;
/// Runtime selected the CYW43 Wi-Fi engine-init branch.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_ENGINE_INIT_BRANCH: u32 = 176;
/// CYW43 runtime is about to reset its local Wi-Fi state.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_BEGIN: u32 = 177;
/// CYW43 runtime reset its local Wi-Fi state.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_DONE: u32 = 178;
/// CYW43 runtime rejected a forbidden direct SDIO MMIO window.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_FORBIDDEN_SDIO_MMIO: u32 = 179;
/// CYW43 runtime is checking the pointer-free SDIO owner bus link.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_CHECK_BEGIN: u32 = 180;
/// CYW43 runtime is checking the shared firmware/control resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_CHECK_BEGIN: u32 = 181;
/// CYW43 runtime could not find the shared firmware/control resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_MISSING: u32 = 182;
/// CYW43 runtime verified the shared firmware/control resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_READY: u32 = 183;
/// CYW43 runtime entered the post-NVRAM firmware release sequence.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_BEGIN: u32 = 206;
/// CYW43 runtime is writing the firmware reset vector.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_RESET_VECTOR_BEGIN: u32 = 207;
/// CYW43 runtime is releasing the ARMCR4 core from reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_ARMCR4_RESET_BEGIN: u32 = 208;
/// CYW43 runtime is restoring the upload clock/bus-width lane before HT.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_UPLOAD_CLOCK_BEGIN: u32 = 209;
/// CYW43 runtime is programming post-release Function 2 sideband state.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_POST_CONFIG_BEGIN: u32 = 210;
/// CYW43 runtime is requesting/proving the post-release HT clock.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_HT_CLOCK_BEGIN: u32 = 211;
/// CYW43 runtime is enabling SDIO Function 2 after firmware release.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_F2_ENABLE_BEGIN: u32 = 212;
/// CYW43 runtime is programming SDPCM interrupt masks.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_INT_MASK_BEGIN: u32 = 213;
/// CYW43 runtime is proving SDIO corecontrol Function 2 readiness.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_CORECONTROL_BEGIN: u32 = 214;
/// CYW43 runtime is publishing the SDPCM firmware protocol version mailbox.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_MAILBOX_VERSION_BEGIN: u32 = 215;
/// CYW43 runtime is waiting for the firmware ready/devready mailbox.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_FIRMWARE_READY_BEGIN: u32 = 216;
/// CYW43 runtime observed firmware ready/devready mailbox proof.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_RELEASE_FIRMWARE_READY_DONE: u32 = 217;
/// ARMCR4 reset diagnostic edge: pre-reset IOCTRL write failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_PRERESET_WRITE: u8 = 1;
/// ARMCR4 reset diagnostic edge: pre-reset IOCTRL flush read failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_PRERESET_FLUSH: u8 = 2;
/// ARMCR4 reset diagnostic edge: RESETCTRL assert write failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_ASSERT_WRITE: u8 = 3;
/// ARMCR4 reset diagnostic edge: in-reset IOCTRL write failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_IN_RESET_WRITE: u8 = 4;
/// ARMCR4 reset diagnostic edge: in-reset IOCTRL flush read failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_IN_RESET_FLUSH: u8 = 5;
/// ARMCR4 reset diagnostic edge: RESETCTRL clear write failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_CLEAR_WRITE: u8 = 6;
/// ARMCR4 reset diagnostic edge: post-reset IOCTRL write failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_POSTRESET_WRITE: u8 = 7;
/// ARMCR4 reset diagnostic edge: post-reset IOCTRL flush read failed.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_POSTRESET_FLUSH: u8 = 8;
/// ARMCR4 reset result bit marking a valid RESETCTRL readback byte.
pub const DRIVER_RUNTIME_CYW43_ARMCR4_RESET_RESULT_READBACK_VALID: u32 = 1 << 16;

/// Pack an exact ARMCR4 reset edge, bounded attempt, and optional readback.
pub const fn driver_runtime_cyw43_armcr4_reset_result(
    edge: u8,
    attempt: u8,
    readback: Option<u8>,
) -> u32 {
    let mut result = edge as u32 | ((attempt as u32) << 8);
    if let Some(readback) = readback {
        result |= DRIVER_RUNTIME_CYW43_ARMCR4_RESET_RESULT_READBACK_VALID;
        result |= (readback as u32) << 24;
    }
    result
}

/// Return the exact ARMCR4 reset edge from a packed completion result.
pub const fn driver_runtime_cyw43_armcr4_reset_result_edge(result: u32) -> u8 {
    (result & 0xff) as u8
}

/// Return the bounded ARMCR4 reset attempt from a packed completion result.
pub const fn driver_runtime_cyw43_armcr4_reset_result_attempt(result: u32) -> u8 {
    ((result >> 8) & 0xff) as u8
}

/// Return the ARMCR4 RESETCTRL readback when the packed result carries one.
pub const fn driver_runtime_cyw43_armcr4_reset_result_readback(result: u32) -> Option<u8> {
    if result & DRIVER_RUNTIME_CYW43_ARMCR4_RESET_RESULT_READBACK_VALID != 0 {
        Some((result >> 24) as u8)
    } else {
        None
    }
}
/// CYW43 runtime is issuing the SDPCM Function 2 control write.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_TX_BEGIN: u32 = 338;
/// CYW43 runtime completed the SDPCM Function 2 control write.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_TX_DONE: u32 = 339;
/// CYW43 runtime is polling for the matching SDPCM/CDC control reply.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_POLL_BEGIN: u32 = 340;
/// CYW43 runtime observed zero RFRAME and is issuing a bounded Function 2 first-read.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_FIRSTREAD_BEGIN: u32 = 250;
/// CYW43 runtime completed a bounded Function 2 first-read.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_FIRSTREAD_DONE: u32 = 251;
/// CYW43 runtime's bounded Function 2 first-read returned an empty prefix.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_FIRSTREAD_EMPTY: u32 = 252;
/// CYW43 runtime's bounded Function 2 first-read returned a malformed SDPCM prefix.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_FIRSTREAD_INVALID: u32 = 253;
/// CYW43 runtime's bounded Function 2 first-read reached the expected control reply.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_FIRSTREAD_FRAME: u32 = 254;
/// CYW43 runtime's bounded Function 2 remainder read failed after a valid first-read.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_REMAINDER_FAILED: u32 = 255;
/// USB runtime is parsing the full configuration descriptor for a HID keyboard endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN: u32 = 256;
/// USB runtime found a candidate HID interrupt-IN endpoint in the configuration descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_FOUND: u32 = 257;
/// USB runtime did not find a usable HID interrupt-IN endpoint in the configuration descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MISSING: u32 = 258;
/// USB runtime is submitting Configure Endpoint for the HID interrupt-IN endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_BEGIN: u32 = 259;
/// USB runtime completed Configure Endpoint for the HID interrupt-IN endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_DONE: u32 = 260;
/// USB runtime failed Configure Endpoint for the HID interrupt-IN endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_FAILED: u32 = 261;
/// USB runtime is issuing SET_CONFIGURATION for the HID keyboard configuration.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_BEGIN: u32 = 262;
/// USB runtime completed SET_CONFIGURATION for the HID keyboard configuration.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_DONE: u32 = 263;
/// USB runtime failed SET_CONFIGURATION for the HID keyboard configuration.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_FAILED: u32 = 264;
/// USB runtime is issuing HID class control setup after endpoint configuration.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_BEGIN: u32 = 265;
/// USB runtime completed required HID class control setup.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_DONE: u32 = 266;
/// USB runtime failed required HID class control setup.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_FAILED: u32 = 267;
/// USB runtime is arming the first HID interrupt-IN transfer queue.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_BEGIN: u32 = 268;
/// USB runtime armed the HID interrupt-IN transfer queue.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY: u32 = 269;
/// USB runtime could not arm the HID interrupt-IN transfer queue.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_FAILED: u32 = 270;
/// USB runtime found keyboard data outside an empty boot-looking HID payload window.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_REPORT_FLEXIBLE_KEY_FALLBACK: u32 = 0x0416;
/// USB runtime is about to zero the complete xHCI DMA arena.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_ZERO_BEGIN: u32 = 0x0417;
/// USB runtime completed another bounded DMA-zero diagnostic chunk.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_ZERO_PROGRESS: u32 = 0x0418;
/// USB runtime completed the complete xHCI DMA-arena zero.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_ZERO_DONE: u32 = 0x0419;
/// USB runtime is constructing the xHCI ring/context graph.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RING_GRAPH_BEGIN: u32 = 0x041a;
/// USB runtime constructed the xHCI ring/context graph.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RING_GRAPH_DONE: u32 = 0x041b;
/// USB runtime is about to publish the complete xHCI DMA arena to PoC.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_CLEAN_BEGIN: u32 = 0x041c;
/// USB runtime published the complete xHCI DMA arena to PoC.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_CLEAN_DONE: u32 = 0x041d;
/// USB keyboard service result bit offset for the last HID report classification.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT: u32 = 9;
/// USB keyboard service result mask for the last HID report classification.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK: u32 = 0x7f;
/// USB keyboard frame flag: payload contains decoded console input bytes only.
pub const DRIVER_RUNTIME_USB_KEYBOARD_FRAME_FLAG_INPUT: u16 = 0x0001;
/// USB keyboard report status: no interrupt report has been classified yet.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE: u8 = 0;
/// USB keyboard report status: interrupt completion carried too little payload.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_SHORT: u8 = 1;
/// USB keyboard report status: interrupt payload decoded as an idle all-zero report.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE: u8 = 2;
/// USB keyboard report status: interrupt payload decoded but contained no key byte.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY: u8 = 3;
/// USB keyboard report status: interrupt payload was nonzero but no supported layout decoded.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODE_FAILED: u8 = 4;
/// USB keyboard report status: flexible-key fallback recovered a key-bearing report.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_FLEXIBLE_FALLBACK: u8 = 5;
/// USB keyboard report status: report decoded and produced at least one console byte.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_PRODUCED_BYTE: u8 = 6;
/// USB keyboard report status: report had a key but no new console byte was emitted.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_FILTERED_KEY: u8 = 7;
/// USB keyboard report status: endpoint-matched transfer event did not match a queued report slot.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER: u8 = 8;
/// USB keyboard report status: post-first-byte interrupt queue collapsed below the refill floor.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE: u8 = 9;
/// USB keyboard report status: endpoint recovery reset the interrupt-IN dequeue pointer.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS: u8 = 10;
/// USB keyboard report status: endpoint recovery failed before the interrupt-IN queue was rearmed.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED: u8 = 11;
/// USB keyboard report status: endpoint is armed and this keyboard does not emit idle reports.
pub const DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NO_IDLE_REPORT: u8 = 12;
/// USB keyboard service aux: request post-first-byte interrupt-IN endpoint recovery.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX: u32 = 0x5553_4252;
/// USB keyboard diagnostic frame magic packed into `DriverFrameDescriptor.offset[31:16]`.
pub const DRIVER_RUNTIME_USB_KEYBOARD_DIAG_MAGIC: u16 = 0x554b;
/// USB keyboard recovery stage: no recovery state has been recorded.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_NONE: u8 = 0;
/// USB keyboard recovery stage: endpoint recovery was entered.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_BEGIN: u8 = 1;
/// USB keyboard recovery stage: endpoint was not in a ready polling state.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_NOT_READY: u8 = 2;
/// USB keyboard recovery stage: bounded hard-recovery limit was reached.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_LIMIT: u8 = 3;
/// USB keyboard recovery stage: stop-endpoint command was submitted.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_STOP_ENDPOINT: u8 = 4;
/// USB keyboard recovery stage: reset-endpoint command was submitted.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_RESET_ENDPOINT: u8 = 5;
/// USB keyboard recovery stage: interrupt-IN transfer ring was rebuilt.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_RESET_RING: u8 = 6;
/// USB keyboard recovery stage: Set TR Dequeue Pointer command was submitted.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_SET_DEQUEUE: u8 = 7;
/// USB keyboard recovery stage: interrupt-IN queue was rearmed.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_REARM: u8 = 8;
/// USB keyboard recovery stage: recovery finished and the endpoint queue is ready.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_READY: u8 = 9;
/// USB keyboard recovery reason: no recovery trigger has been recorded.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_NONE: u8 = 0;
/// USB keyboard recovery reason: root requested recovery after the hard limit.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_REENUMERATION_LIMIT: u8 = 1;
/// USB keyboard recovery reason: post-first-byte interrupt queue collapsed.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_QUEUE_COLLAPSE: u8 = 2;
/// USB keyboard recovery reason: full queue produced no fresh event on a recovery poll.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_FULL_QUEUE_NO_EVENT: u8 = 3;
/// USB keyboard recovery reason: steady queue only produced idle reports.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_STEADY_IDLE: u8 = 4;
/// USB keyboard recovery reason: queue depth exceeded the steady target.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_OVERQUEUE: u8 = 5;
/// USB keyboard recovery reason: first-report queue fell below target.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_PRE_FIRST_UNDERFILLED: u8 = 6;
/// USB keyboard recovery reason: root-requested recovery saw an underfilled queue.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_AUX_UNDERFILLED: u8 = 7;
/// USB keyboard recovery reason: steady transfer event did not match an armed report.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_STEADY_UNMATCHED: u8 = 8;
/// USB keyboard recovery reason: root-requested recovery saw an unmatched transfer.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_AUX_UNMATCHED: u8 = 9;
/// USB keyboard recovery reason: unmatched transfer streak crossed the hard threshold.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_HARD_UNMATCHED: u8 = 10;
/// USB keyboard recovery reason: endpoint rearm failed after a matched completion.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_REARM_COLLAPSE: u8 = 11;
/// USB keyboard recovery reason: matched transfer completed with a fault status.
pub const DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_MATCHED_TRANSFER_FAULT: u8 = 12;
/// USB runtime is traversing a hub after the root device had no direct keyboard endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_BEGIN: u32 = 271;
/// USB runtime is probing one hub child port for a keyboard-capable device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_PROBE_BEGIN: u32 = 272;
/// USB runtime completed hub traversal without finding a keyboard endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_NO_KEYBOARD: u32 = 273;
/// USB runtime parsed a configuration with no HID keyboard-capable interface.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERFACE: u32 = 274;
/// USB runtime found a keyboard-capable interface without a usable interrupt-IN endpoint.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERRUPT_IN: u32 = 275;
/// USB runtime stopped HID endpoint parsing at a malformed descriptor boundary.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MALFORMED: u32 = 276;
/// USB runtime sees an empty event TRB while polling full-speed descriptor-prime data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_SLOT_EMPTY: u32 =
    277;
/// USB runtime sees a cycle-mismatched event TRB while polling full-speed descriptor-prime data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_CYCLE_MISMATCH:
    u32 = 278;
/// USB runtime consumed an event that did not match full-speed descriptor-prime data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_IGNORED: u32 =
    279;
/// USB runtime sees an empty event TRB while polling full-speed descriptor-prime status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_SLOT_EMPTY: u32 =
    280;
/// USB runtime sees a cycle-mismatched event TRB while polling full-speed descriptor-prime status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_CYCLE_MISMATCH:
    u32 = 281;
/// USB runtime consumed an event that did not match full-speed descriptor-prime status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_IGNORED: u32 = 282;
/// USB runtime sees an empty event TRB while polling the final device descriptor data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY: u32 = 283;
/// USB runtime sees a cycle-mismatched event TRB while polling final device descriptor data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH: u32 =
    284;
/// USB runtime consumed an event that did not match final device descriptor data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_IGNORED: u32 = 285;
/// USB runtime sees an empty event TRB while polling the final device descriptor status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY: u32 = 286;
/// USB runtime sees a cycle-mismatched event TRB while polling final device descriptor status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH: u32 = 287;
/// USB runtime consumed an event that did not match final device descriptor status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_IGNORED: u32 = 288;
/// USB runtime sees an empty event TRB while polling configuration-header data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY: u32 =
    289;
/// USB runtime sees a cycle-mismatched event TRB while polling configuration-header data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_CYCLE_MISMATCH:
    u32 = 290;
/// USB runtime consumed an event that did not match configuration-header data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_IGNORED: u32 =
    291;
/// USB runtime sees an empty event TRB while polling configuration-header status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_SLOT_EMPTY: u32 =
    292;
/// USB runtime sees a cycle-mismatched event TRB while polling configuration-header status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_CYCLE_MISMATCH:
    u32 = 293;
/// USB runtime consumed an event that did not match configuration-header status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_IGNORED: u32 = 294;
/// USB runtime sees an empty event TRB while polling full-configuration data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_SLOT_EMPTY: u32 =
    295;
/// USB runtime sees a cycle-mismatched event TRB while polling full-configuration data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_CYCLE_MISMATCH:
    u32 = 296;
/// USB runtime consumed an event that did not match full-configuration data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_IGNORED: u32 = 297;
/// USB runtime sees an empty event TRB while polling full-configuration status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_SLOT_EMPTY: u32 =
    298;
/// USB runtime sees a cycle-mismatched event TRB while polling full-configuration status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_CYCLE_MISMATCH: u32 =
    299;
/// USB runtime consumed an event that did not match full-configuration status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_IGNORED: u32 = 300;
/// USB runtime is setting the hub configuration before child traversal.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_BEGIN: u32 = 301;
/// USB runtime set the hub configuration before child traversal.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DONE: u32 = 302;
/// USB runtime is reading the USB hub descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_BEGIN: u32 = 303;
/// USB runtime read the USB hub descriptor.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DONE: u32 = 304;
/// USB runtime is evaluating the xHCI hub context.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_BEGIN: u32 = 305;
/// USB runtime evaluated the xHCI hub context.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_DONE: u32 = 306;
/// USB runtime is powering a downstream hub port.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_BEGIN: u32 = 307;
/// USB runtime completed the downstream hub port power settle.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_DONE: u32 = 308;
/// USB runtime is resetting a downstream hub port.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_BEGIN: u32 = 309;
/// USB runtime found a ready downstream hub port.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_READY: u32 = 310;
/// USB runtime is re-probing a hub child with a fallback speed.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_SPEED_FALLBACK_BEGIN: u32 = 311;
/// USB runtime rang EP0 for the hub descriptor control transfer.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DOORBELL_DONE: u32 = 312;
/// USB runtime is polling the hub descriptor data-stage transfer event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN: u32 = 313;
/// USB runtime observed the hub descriptor data-stage transfer event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DATA_EVENT: u32 = 314;
/// USB runtime observed the hub descriptor status-stage transfer event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT: u32 = 315;
/// USB runtime failed the hub descriptor control transfer.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_FAILED: u32 = 316;
/// USB runtime timed out waiting for the hub descriptor data-stage event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_TIMEOUT: u32 = 317;
/// USB runtime timed out waiting for the hub descriptor status-stage event.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_TIMEOUT: u32 = 318;
/// USB runtime sees an empty event TRB while polling hub descriptor data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY: u32 = 319;
/// USB runtime sees a cycle-mismatched event TRB while polling hub descriptor data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH: u32 = 320;
/// USB runtime consumed an event that did not match hub descriptor data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_IGNORED: u32 = 321;
/// USB runtime sees an empty event TRB while polling hub descriptor status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY: u32 = 322;
/// USB runtime sees a cycle-mismatched event TRB while polling hub descriptor status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH: u32 = 323;
/// USB runtime consumed an event that did not match hub descriptor status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_IGNORED: u32 = 324;
/// USB runtime rang EP0 for hub SET_CONFIGURATION.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DOORBELL_DONE: u32 = 400;
/// USB runtime is polling hub SET_CONFIGURATION status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_WAIT_BEGIN: u32 = 401;
/// USB runtime observed hub SET_CONFIGURATION status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT: u32 = 402;
/// USB runtime failed hub SET_CONFIGURATION.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_FAILED: u32 = 403;
/// USB runtime timed out waiting for hub SET_CONFIGURATION status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_TIMEOUT: u32 = 404;
/// USB runtime sees an empty event TRB while polling hub SET_CONFIGURATION status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_SLOT_EMPTY: u32 = 405;
/// USB runtime sees a cycle-mismatched event TRB while polling hub SET_CONFIGURATION status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_CYCLE_MISMATCH: u32 =
    406;
/// USB runtime consumed an event that did not match hub SET_CONFIGURATION status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED: u32 = 407;
/// USB runtime is reading downstream hub-port status after power/reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_BEGIN: u32 = 408;
/// USB runtime read downstream hub-port status after power/reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DONE: u32 = 409;
/// USB runtime could not read downstream hub-port status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_FAILED: u32 = 410;
/// USB runtime is submitting downstream hub-port reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_BEGIN: u32 = 411;
/// USB runtime submitted downstream hub-port reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_DONE: u32 = 412;
/// USB runtime could not submit downstream hub-port reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_FAILED: u32 = 413;
/// USB runtime rang EP0 for downstream hub-port GET_STATUS.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DOORBELL_DONE: u32 = 414;
/// USB runtime is polling downstream hub-port GET_STATUS data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_WAIT_BEGIN: u32 = 415;
/// USB runtime observed downstream hub-port GET_STATUS data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DATA_EVENT: u32 = 416;
/// USB runtime observed downstream hub-port GET_STATUS status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT: u32 = 417;
/// USB runtime timed out waiting for downstream hub-port GET_STATUS data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_TIMEOUT: u32 = 418;
/// USB runtime timed out waiting for downstream hub-port GET_STATUS status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_TIMEOUT: u32 = 419;
/// USB runtime sees an empty event TRB while polling hub-port GET_STATUS data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_SLOT_EMPTY: u32 = 420;
/// USB runtime sees a cycle-mismatched event TRB while polling hub-port GET_STATUS data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_CYCLE_MISMATCH: u32 = 421;
/// USB runtime consumed an event that did not match hub-port GET_STATUS data.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_IGNORED: u32 = 422;
/// USB runtime sees an empty event TRB while polling hub-port GET_STATUS status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_SLOT_EMPTY: u32 = 423;
/// USB runtime sees a cycle-mismatched event TRB while polling hub-port GET_STATUS status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_CYCLE_MISMATCH: u32 = 424;
/// USB runtime consumed an event that did not match hub-port GET_STATUS status.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_IGNORED: u32 = 425;
/// USB runtime acknowledged the hub-port GET_STATUS status event to ERDP.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ACK_DONE: u32 = 426;
/// USB runtime invalidated and read the hub-port GET_STATUS payload bytes.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_PAYLOAD_READ: u32 = 427;
/// USB runtime read a downstream hub-port status with no connected device.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DISCONNECTED: u32 = 428;
/// USB runtime read downstream hub-port status while reset is still asserted.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_RESET_ACTIVE: u32 = 429;
/// USB runtime read downstream hub-port status without the enable bit.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ENABLE_MISSING: u32 = 430;
/// USB runtime started clearing downstream hub-port status-change bits.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_BEGIN: u32 = 431;
/// USB runtime finished clearing downstream hub-port status-change bits.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_DONE: u32 = 432;
/// USB runtime failed while clearing downstream hub-port status-change bits.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_FAILED: u32 = 433;
/// Linked runtime entered its no_std entry path and installed its IPC buffer.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_ENTRY_READY: u32 = 200;
/// Linked runtime reached the root-published command endpoint/shared-ring intake loop.
/// Sequence zero identifies initial admission; otherwise sequence identifies
/// the last command retired before this receive-ready publication.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RECV_READY: u32 = 201;
/// Linked runtime completed an intake poll without consuming a new command.
/// Its sequence preserves the same last-retired-command identity as receive-ready.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_READY: u32 = 202;
/// Linked runtime saw a non-one-way command before receiving a reply cap.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_REPLY_PENDING: u32 = 203;
/// Linked runtime is about to poll the command endpoint for a reply-cap command.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_BEGIN: u32 = 204;
/// Linked runtime is about to read the uncached shared command ring.
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RING_READ_BEGIN: u32 = 205;
/// USB runtime is requesting xHCI controller run state.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_BEGIN: u32 = 79;
/// USB runtime published xHCI command/event rings.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RINGS_READY: u32 = 24;
/// USB runtime requested xHCI run state.
pub const DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_REQUESTED: u32 = 25;
/// SDIO runtime started host reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_BEGIN: u32 = 30;
/// SDIO runtime powered the SDIO card slot.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_POWER_READY: u32 = 31;
/// SDIO runtime enabled the startup clock.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_CLOCK_READY: u32 = 32;
/// SDIO runtime cleared command/data inhibit after clock enable.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_READY: u32 = 33;
/// SDIO runtime is about to disable the SDHCI clock before reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_CLOCK_DISABLE_BEGIN: u32 = 86;
/// SDIO runtime is about to drop SDHCI slot power before reset.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_POWER_DISABLE_BEGIN: u32 = 87;
/// SDIO owner is asserting Pi 4 WL_ON low through its admitted power resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_LOW_BEGIN: u32 = 434;
/// SDIO owner completed the bounded Pi 4 WL_ON low/off interval.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_LOW_DONE: u32 = 435;
/// SDIO owner is deasserting Pi 4 WL_ON through its admitted power resource.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_HIGH_BEGIN: u32 = 436;
/// SDIO owner completed the bounded Pi 4 WL_ON high/startup interval.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_HIGH_DONE: u32 = 437;
/// SDIO owner started a retained firmware GET_GPIO_CONFIG transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_GET_CONFIG_BEGIN: u32 = 438;
/// SDIO owner completed the retained firmware GET_GPIO_CONFIG transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_GET_CONFIG_DONE: u32 = 439;
/// SDIO owner started a retained firmware SET_GPIO_CONFIG transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_SET_CONFIG_BEGIN: u32 = 440;
/// SDIO owner completed the retained firmware SET_GPIO_CONFIG transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_SET_CONFIG_DONE: u32 = 441;
/// SDIO owner started a retained firmware SET_GPIO_STATE-low transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_ASSERT_LOW_BEGIN: u32 = 442;
/// SDIO owner completed the retained firmware SET_GPIO_STATE-low transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_ASSERT_LOW_DONE: u32 = 443;
/// SDIO owner started a retained firmware SET_GPIO_STATE-high transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_RELEASE_HIGH_BEGIN: u32 = 444;
/// SDIO owner completed the retained firmware SET_GPIO_STATE-high transaction.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_WIFI_PWRSEQ_RELEASE_HIGH_DONE: u32 = 445;
/// Runtime recognized an engine-init aux word before entering the handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DISPATCH: u32 = 34;
/// Runtime entered the engine-init handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_ENTER: u32 = 35;
/// Runtime accepted the engine-init aux word.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_AUX_MATCH: u32 = 36;
/// Runtime accepted the zero-frame engine-init shape.
pub const DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FRAME_READY: u32 = 37;
/// HDMI runtime started rendering a text frame.
pub const DRIVER_RUNTIME_RING_PROGRESS_HDMI_FRAME_BEGIN: u32 = 40;
/// HDMI runtime completed rendering a non-empty text frame.
pub const DRIVER_RUNTIME_RING_PROGRESS_HDMI_FRAME_DONE: u32 = 41;
/// HDMI runtime rejected or produced no visible text frame.
pub const DRIVER_RUNTIME_RING_PROGRESS_HDMI_FRAME_FAILED: u32 = 42;
/// Runtime selected the role-specific service handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_SERVICE_DISPATCH: u32 = 43;
/// Runtime selected the HDMI text service handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_SERVICE_DISPATCH_HDMI: u32 = 44;
/// Runtime selected the USB keyboard service handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_SERVICE_DISPATCH_USB: u32 = 45;
/// Runtime selected the SDIO host service handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_SERVICE_DISPATCH_SDIO: u32 = 46;
/// Runtime selected the CYW43 Wi-Fi service handler.
pub const DRIVER_RUNTIME_RING_PROGRESS_SERVICE_DISPATCH_CYW43: u32 = 47;
/// Runtime is publishing the command completion record.
pub const DRIVER_RUNTIME_RING_PROGRESS_COMPLETION_PUBLISH: u32 = 48;
/// CYW43 runtime started a restartable transport substage.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_BEGIN: u32 = 110;
/// CYW43 runtime validated the SDIO bus-owner link.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_READY: u32 = 111;
/// CYW43 runtime is adopting the SDIO card-selected owner state.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_ADOPT_BEGIN: u32 = 119;
/// CYW43 runtime is asking the SDIO owner to replay startup host config.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_HOST_CONFIG_BEGIN: u32 = 125;
/// CYW43 runtime is asking the SDIO owner to issue CMD0.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD0_BEGIN: u32 = 126;
/// CYW43 runtime is asking the SDIO owner to issue CMD5 for OCR discovery.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_OCR_BEGIN: u32 = 127;
/// CYW43 runtime is asking the SDIO owner to issue CMD5 for card readiness.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_READY_BEGIN: u32 = 128;
/// CYW43 runtime is asking the SDIO owner to issue CMD3 for RCA assignment.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD3_RCA_BEGIN: u32 = 129;
/// CYW43 runtime is asking the SDIO owner to issue CMD7 for card selection.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD7_SELECT_BEGIN: u32 = 130;
/// CYW43 runtime is publishing a nested command to the linked SDIO owner.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_BEGIN: u32 = 140;
/// CYW43 runtime sent the nested SDIO-owner notification.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_DONE: u32 = 141;
/// CYW43 runtime is waiting for the linked SDIO owner completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_BEGIN: u32 = 142;
/// CYW43 runtime timed out waiting for the linked SDIO owner.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_TIMEOUT: u32 = 143;
/// CYW43 runtime received the linked SDIO owner completion.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_REPLY: u32 = 144;
/// CYW43 exhausted the issued-unknown reap deadline and requires a fenced pair restart.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_PAIR_RESTART_REQUIRED: u32 = 446;
/// SDIO retained a CYW43 owner-notification edge before the delegated command needed it.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_WAKE_RETAINED: u32 = 458;
/// SDIO is blocking because no stable delegated continuation grant is currently published.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_WAIT_BEGIN: u32 = 459;
/// SDIO observed the exact immutable delegated continuation grant before sleeping.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_READY: u32 = 460;
/// SDIO rejected a stale, consumed, mutated, or wrong-generation continuation grant.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_REJECTED: u32 = 461;
/// SDIO acknowledged the exact delegated continuation grant before the owner quantum.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_ACCEPTED: u32 = 462;
/// SDIO could not commit the exact continuation-grant acknowledgement and ran no owner quantum.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_ACK_FAILED: u32 = 463;
/// SDIO admitted a high-domain delegated command in its current owner generation.
pub const DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_COMMAND_ADMITTED: u32 = 464;
/// A root-owned retained command is waiting for an exact continuation grant.
pub const DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_WAIT_BEGIN: u32 = 465;
/// A root-owned retained command observed its exact continuation grant.
pub const DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_READY: u32 = 466;
/// A root-owned retained command rejected a stale or mutated grant.
pub const DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_REJECTED: u32 = 467;
/// A root-owned retained command acknowledged its exact continuation grant.
pub const DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_ACCEPTED: u32 = 468;
/// A root-owned retained command could not acknowledge its exact grant.
pub const DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_ACK_FAILED: u32 = 469;
/// CYW43 retained backplane attach is issuing its one-shot ALP request.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_ALP_REQUEST: u32 = 447;
/// CYW43 retained backplane attach is consuming one ALP readback turn.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_ALP_POLL: u32 = 448;
/// CYW43 retained backplane attach is issuing FORCE_ALP.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_FORCE_ALP: u32 = 449;
/// CYW43 retained backplane attach is consuming one 65-us settle turn.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_FORCE_ALP_SETTLE: u32 = 450;
/// Legacy capture: CYW43 issued the one-shot extra-pull-up clear.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_PULLUP_CLEAR: u32 = 451;
/// Reserved legacy value for captures that accepted a contained pull-up fault.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_PULLUP_FAULT_CONTAINED: u32 = 452;
/// CYW43 retained backplane attach is reading the first ChipCommon word.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_CHIPCOMMON_READ: u32 = 453;
/// CYW43 retained attach is programming the ChipCommon window low byte.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_WINDOW_LOW: u32 = 454;
/// CYW43 retained attach is programming the ChipCommon window middle byte.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_WINDOW_MID: u32 = 455;
/// CYW43 retained attach is programming the ChipCommon window high byte.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_WINDOW_HIGH: u32 = 456;
/// CYW43 retained backplane attach skipped the optional Pi extra-pull-up clear.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_PULLUP_SKIPPED: u32 = 457;
/// CYW43 runtime proved card selection/adoption.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_READY: u32 = 112;
/// CYW43 runtime is programming Function 1 block size.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_BEGIN: u32 = 120;
/// CYW43 runtime programmed Function 1 block size.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_READY: u32 = 113;
/// CYW43 runtime is programming Function 2 block size.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_BEGIN: u32 = 121;
/// CYW43 runtime programmed Function 2 block size.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_READY: u32 = 114;
/// CYW43 runtime is enabling SDIO Function 1.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLE_BEGIN: u32 = 122;
/// CYW43 runtime enabled Function 1.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLED: u32 = 115;
/// CYW43 runtime is programming startup host clock and bus width.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_CONFIG_BEGIN: u32 = 123;
/// CYW43 runtime programmed startup host clock/bus width.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_READY: u32 = 116;
/// CYW43 runtime is proving ALP/backplane transport.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_BEGIN: u32 = 124;
/// CYW43 runtime proved backplane transport.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_READY: u32 = 117;
/// CYW43 runtime completed transport init.
pub const DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_READY: u32 = 118;
/// Offset namespace base for runtime shared-buffer payloads referenced by an
/// owner-ring descriptor.
pub const DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE: u16 = DRIVER_RUNTIME_RING_PAGE_BYTES;
/// Bytes addressable through one CYW43 Function-1 backplane aperture.
pub const DRIVER_RUNTIME_CYW43_BACKPLANE_APERTURE_BYTES: u16 = 32 * 1024;
/// Maximum SDIO descriptor payload carried outside the owner command ring.
///
/// One exact backplane aperture lets the CYW43 runtime retain a Linux-shaped
/// firmware stream window while the SDIO owner splits that immutable parent
/// payload into its bounded CMD53 child requests.
pub const DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES: u16 =
    DRIVER_RUNTIME_CYW43_BACKPLANE_APERTURE_BYTES;
/// Shared pages required to map the complete CYW43/SDIO payload aperture.
pub const DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES: usize =
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as usize / DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as usize;
/// Exclusive ABI offset at the end of the CYW43/SDIO payload aperture.
pub const DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET: u16 =
    DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE + DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES;
/// Root-to-CYW43 post-release TX slice in the shared payload arena.
pub const DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES: u16 =
    DRIVER_RUNTIME_RING_PAGE_BYTES;
/// CYW43-private post-release Function 2 RX slice in the shared payload arena.
pub const DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_OFFSET: u16 =
    DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
        + DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES;
/// Bytes reserved for CYW43-private post-release Function 2 RX.
pub const DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_BYTES: u16 =
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES - DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES;
/// Magic value for one committed CYW43 root-visible RX batch.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_MAGIC: u32 = 0x4359_5242;
/// Layout version for [`DriverRuntimeCyw43RxBatchRecord`].
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_VERSION: u16 = 3;
/// Right shift applied to passive CYW43 RX stage deltas before publication.
pub const DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SHIFT: u32 = 11;
/// Saturated or unavailable passive CYW43 RX stage delta.
pub const DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED: u16 = u16::MAX;
/// Maximum frames committed in one CYW43-to-root RX batch.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP: usize = 8;
/// Bytes reserved for each exact RX frame slot in a batch.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_FRAME_BYTES: u16 = 1_536;
/// Magic value for one root-owned CYW43 RX batch acknowledgement.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_MAGIC: u32 = 0x4359_414b;
/// Layout version for [`DriverRuntimeCyw43RxBatchAck`].
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_VERSION: u16 = 1;
/// Exact bytes in one cache-line-isolated CYW43 RX batch acknowledgement.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES: u16 = 64;
/// Fixed offset of the cache-isolated CYW43 bus-episode diagnostic.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET: u16 = 49_344;
/// Exact bytes in one cache-isolated CYW43 bus-episode diagnostic.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES: u16 = 128;
/// Exact 32-bit words in one CYW43 bus-episode diagnostic.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_WORDS: usize =
    DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES as usize / core::mem::size_of::<u32>();
/// Magic value for one CYW43 bus-episode diagnostic.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_MAGIC: u32 = 0x4359_4245;
/// Layout version for [`DriverRuntimeCyw43BusEpisodeRecord`].
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_VERSION: u16 = 1;
/// Fixed offset of the passive SDIO-child timing mailbox in the linked owner ring.
///
/// The mailbox occupies one otherwise-unused cache line between the bounded
/// SDIO fault telemetry and the passive clock snapshot. CYW43 stages one exact
/// published child; the sole SDIO owner preserves that identity, adds physical
/// timing, and commits it before CYW43 consumes it as diagnostic evidence.
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_OFFSET: u16 = 1_920;
/// Exact bytes in one passive SDIO-child timing mailbox.
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_BYTES: u16 = 64;
/// Magic value for [`DriverRuntimeSdioChildTimingMailbox`].
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_MAGIC: u32 = 0x5344_544d;
/// Layout version for [`DriverRuntimeSdioChildTimingMailbox`].
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_VERSION: u16 = 1;
/// The linked CYW43 producer committed the exact child publication timestamp.
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_PUBLISHED: u32 = 1 << 0;
/// The sole SDIO owner admitted the exact linked child.
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_INTAKE: u32 = 1 << 1;
/// The sole SDIO owner wrote `SDHCI_COMMAND` for the exact child.
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_ISSUED: u32 = 1 << 2;
/// The sole SDIO owner joined the exact physical terminal conditions.
pub const DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_TERMINAL: u32 = 1 << 3;
/// Fixed offset of the selected CYW43 DPC-child timing trace.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET: u16 =
    DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET + DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES;
/// Exact bytes in one selected CYW43 DPC-child timing trace.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES: u16 = 512;
/// Maximum exact SDIO children retained for one selected DATA event.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_ENTRY_CAP: usize = 16;
/// Magic value for [`DriverRuntimeCyw43DpcChildTimingRecord`].
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_MAGIC: u32 = 0x4359_4454;
/// Layout version for [`DriverRuntimeCyw43DpcChildTimingRecord`].
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_VERSION: u16 = 1;
/// Fixed offset of the current CYW43 DPC-client diagnostic.
///
/// This cache-isolated record follows the selected DPC-child timing trace in
/// the otherwise-unused tail of the CYW43 RX shared region.
pub const DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET: u16 =
    DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET + DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES;
/// Exact bytes in one current CYW43 DPC-client diagnostic.
pub const DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES: u16 = 128;
/// Exact 32-bit words in one current CYW43 DPC-client diagnostic.
pub const DRIVER_RUNTIME_CYW43_DPC_CLIENT_WORDS: usize =
    DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES as usize / core::mem::size_of::<u32>();
/// Magic value for [`DriverRuntimeCyw43DpcClientRecord`].
pub const DRIVER_RUNTIME_CYW43_DPC_CLIENT_MAGIC: u32 = 0x4359_4443;
/// Layout version for [`DriverRuntimeCyw43DpcClientRecord`].
pub const DRIVER_RUNTIME_CYW43_DPC_CLIENT_VERSION: u16 = 1;
/// The selected DATA event reached a complete, exact queue commit.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_COMPLETE: u32 = 1 << 0;
/// More exact children were observed than the bounded trace can retain.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_OVERFLOW: u32 = 1 << 1;
/// At least one exact timing boundary was unavailable or mismatched.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_UNKNOWN: u32 = 1 << 2;
/// An SDIO mailbox did not match the exact CYW43 event/child identity.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_MAILBOX_MISMATCH: u32 = 1 << 3;
/// Entry metadata: the CYW43 child publication timestamp is valid.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_PUBLISHED: u8 = 1 << 0;
/// Entry metadata: the SDIO-owner intake timestamp is valid.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_INTAKE: u8 = 1 << 1;
/// Entry metadata: the physical SDHCI issue timestamp is valid.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_ISSUED: u8 = 1 << 2;
/// Entry metadata: the joined physical terminal timestamp is valid.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_TERMINAL: u8 = 1 << 3;
/// Entry metadata: the exact CYW43 completion-acceptance timestamp is valid.
pub const DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_ACCEPTED: u8 = 1 << 4;
/// A root foreground command opened the bus-service episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_FOREGROUND: u16 = 1;
/// Durable CYW43 DPC work opened the bus-service episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_DPC: u16 = 2;
/// Foreground and durable DPC work were both visible at episode admission.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_FOREGROUND_AND_DPC: u16 = 3;
/// The episode is still active and has no terminal exit classification.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_ACTIVE: u16 = 0;
/// The admitted parent reached its exact terminal.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_TERMINAL: u16 = 1;
/// The owner classified an external wait and is about to perform its final
/// durable-condition and exact-terminal rechecks before sleeping.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_PREWAIT_CHECKPOINT: u16 = 2;
/// The bounded episode yielded at its deterministic fairness boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_FAIRNESS: u16 = 3;
/// The episode ended at a typed fault.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_FAULT: u16 = 4;
/// A child operation reached a typed terminal during the episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_CHILD_TERMINAL: u32 = 1 << 0;
/// At least one durable DPC sequence was observed during the episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_DPC_OBSERVED: u32 = 1 << 1;
/// At least one Function-2 RX poll progressed during the episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_OP8_PROGRESS: u32 = 1 << 2;
/// At least one root-visible RX frame progressed during the episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_RX_PROGRESS: u32 = 1 << 3;
/// At least one foreground TX frame progressed during the episode.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_TX_PROGRESS: u32 = 1 << 4;
/// The episode recorded a typed fault exit.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_FAULT: u32 = 1 << 5;
/// No physical child engine has reached a terminal in this publication.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_NONE: u16 = 0;
/// The exact descriptor selects the SDHCI command-only engine.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_COMMAND: u16 = 1;
/// The exact descriptor selects the SDHCI programmed-I/O engine.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_PIO: u16 = 2;
/// The exact descriptor selects the joined SDHCI plus BCM2835 DMA engine.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_DMA: u16 = 3;
/// The selected child contract requires SDHCI/CARD_INT IRQ158.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_IRQ158: u16 = 1 << 0;
/// The selected child contract also requires BCM2835 DMA4 IRQ116.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_IRQ116: u16 = 1 << 1;
/// A foreground parent remains durably pending at the episode boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_FOREGROUND: u32 = 1 << 0;
/// CYW43 DPC work remains durably pending at the episode boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_DPC: u32 = 1 << 1;
/// A physical SDIO or DMA terminal remains pending at the episode boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_EXTERNAL_WAIT: u32 = 1 << 2;
/// CYW43-to-root RX work remains durably pending at the episode boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RX: u32 = 1 << 3;
/// Root-to-CYW43 TX work remains durably pending at the episode boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_TX: u32 = 1 << 4;
/// One exact SDIO child remains active at the episode boundary.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CHILD_ACTIVE: u32 = 1 << 5;
/// The active child was issued but has no observed physical terminal yet.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CHILD_ISSUED_UNKNOWN: u32 = 1 << 6;
/// A committed DPC event remains in the ACK-pending state.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_ACK_PENDING: u32 = 1 << 7;
/// CARD_INT remains masked pending exact DPC acknowledgement and rearm.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CARD_INT_MASKED: u32 = 1 << 8;
/// A linked command, completion, DPC, or RX ring is durably poisoned.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RING_POISON: u32 = 1 << 9;
/// CYW43's private RX queue remains nonempty.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_PRIVATE_RX_QUEUE: u32 = 1 << 10;
/// A committed root-visible RX batch remains unacknowledged.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_UNACKED_RX_BATCH: u32 = 1 << 11;
/// A Function-2 RX-poll parent remains active.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_OP8_ACTIVE: u32 = 1 << 12;
/// A persistent control-exchange parent remains active.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_OP11_ACTIVE: u32 = 1 << 13;
/// Foreground Function-2 TX is durably waiting for SDPCM credit.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_TX_CREDIT_WAIT: u32 = 1 << 14;
/// A bounded CYW43-local continuation remains durably admitted.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_LOCAL_CONTINUATION: u32 = 1 << 15;
/// Typed fault-containment recovery remains active.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RECOVERY: u32 = 1 << 16;
/// A typed CYW43/SDIO pair restart remains active.
pub const DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_PAIR_RESTART: u32 = 1 << 17;
/// First shared-buffer page reserved exclusively for CYW43 RX batching.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_FIRST_SHARED_PAGE: usize =
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES;
/// Shared-buffer pages reserved exclusively for CYW43 RX batching.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_SHARED_PAGES: usize = 4;
/// Total shared-buffer pages needed for the SDIO aperture and RX batch region.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_REQUIRED_SHARED_PAGES: usize =
    DRIVER_RUNTIME_CYW43_RX_BATCH_FIRST_SHARED_PAGE + DRIVER_RUNTIME_CYW43_RX_BATCH_SHARED_PAGES;
/// Fixed offset of the sequence-last CYW43 RX batch header.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET: u16 = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET;
/// Exact bytes in one CYW43 RX batch header.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES: u16 = 128;
/// Fixed offset of the first exact CYW43 RX batch payload slot.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_OFFSET: u16 =
    DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET + DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES;
/// Distance in bytes between exact CYW43 RX batch payload slots.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_STRIDE: u16 =
    DRIVER_RUNTIME_CYW43_RX_BATCH_FRAME_BYTES;
/// Fixed offset of the root-owned CYW43 RX batch acknowledgement.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_OFFSET: u16 =
    DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_OFFSET
        + DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP as u16
            * DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_STRIDE;
/// Bytes reserved for the complete CYW43 RX batch header and payload region.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_REGION_BYTES: u16 =
    DRIVER_RUNTIME_CYW43_RX_BATCH_SHARED_PAGES as u16 * DRIVER_RUNTIME_RING_PAGE_BYTES;
/// Exclusive offset at the end of the CYW43 RX batch region.
pub const DRIVER_RUNTIME_CYW43_RX_BATCH_END_OFFSET: u16 =
    DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET + DRIVER_RUNTIME_CYW43_RX_BATCH_REGION_BYTES;

/// Exact shared-buffer offset for one CYW43 RX batch payload slot.
#[must_use]
pub const fn driver_runtime_cyw43_rx_batch_payload_offset(index: usize) -> Option<u32> {
    if index < DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP {
        Some(
            DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_OFFSET as u32
                + index as u32 * DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_STRIDE as u32,
        )
    } else {
        None
    }
}

/// Quantize one modulo-32 CNTVCT interval for passive CYW43 RX evidence.
///
/// Values through `0xfffe` are exact floors in units of 2^11 ticks. Larger
/// intervals use [`DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED`].
#[must_use]
pub const fn driver_runtime_cyw43_rx_stage_delta_q11(
    start_cntvct_lo: u32,
    end_cntvct_lo: u32,
) -> u16 {
    let quantized = end_cntvct_lo.wrapping_sub(start_cntvct_lo)
        >> DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SHIFT;
    if quantized >= u16::MAX as u32 {
        DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED
    } else {
        quantized as u16
    }
}

/// Pack the first RX CHANNEL_DATA entry's stage intervals into one ABI word.
#[must_use]
pub const fn driver_runtime_cyw43_rx_stage_deltas_q11_pack(
    source_to_queue: u16,
    queue_to_precommit: u16,
) -> u32 {
    source_to_queue as u32 | ((queue_to_precommit as u32) << 16)
}

/// Extract the source-to-private-queue interval from one packed ABI word.
#[must_use]
pub const fn driver_runtime_cyw43_rx_stage_deltas_q11_source_to_queue(packed: u32) -> u16 {
    packed as u16
}

/// Extract the private-queue-to-precommit interval from one packed ABI word.
#[must_use]
pub const fn driver_runtime_cyw43_rx_stage_deltas_q11_queue_to_precommit(packed: u32) -> u16 {
    (packed >> 16) as u16
}

const _: () = {
    assert!(DRIVER_RUNTIME_CYW43_BACKPLANE_APERTURE_BYTES == 0x8000);
    assert!(DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES == 32 * 1024);
    assert!(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES == DRIVER_RUNTIME_RING_PAGE_BYTES as u64);
    assert!((DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as usize)
        .is_multiple_of(DRIVER_RUNTIME_RING_PAGE_BYTES as usize));
    assert!(DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES == 8);
    assert!(DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES <= DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES);
    assert!(
        DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
            + DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32
            <= u16::MAX as u32
    );
    assert!(
        DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET as u32
            == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
                + DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32
    );
    assert!((DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET as usize)
        .is_multiple_of(DRIVER_RUNTIME_RING_PAGE_BYTES as usize));
    assert!(
        DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_OFFSET
            == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
                + DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES
    );
    assert!(
        DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_OFFSET as u32
            + DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_BYTES as u32
            == DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET as u32
    );
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_FIRST_SHARED_PAGE == 8);
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_SHARED_PAGES == 4);
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_REQUIRED_SHARED_PAGES == 12);
    assert!(
        DRIVER_RUNTIME_CYW43_RX_BATCH_REQUIRED_SHARED_PAGES <= DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES
    );
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET == DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET);
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET.is_multiple_of(64));
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES == 128);
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_OFFSET.is_multiple_of(64));
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES == 64);
    assert!(DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_OFFSET.is_multiple_of(64));
    assert!(
        DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET
            == DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_OFFSET + DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES
    );
    assert!(DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET.is_multiple_of(64));
    assert!(DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES == 128);
    assert!(DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_OFFSET.is_multiple_of(64));
    assert!(DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_BYTES == 64);
    assert!(
        DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_OFFSET as u32
            + DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_BYTES as u32
            <= DRIVER_RUNTIME_CYW43_SDPCM_TX_FRAME_OFFSET as u32
    );
    assert!(DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET.is_multiple_of(64));
    assert!(DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES == 512);
    assert!(DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET.is_multiple_of(64));
    assert!(DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES == 128);
    assert!(core::mem::size_of::<DriverRuntimeSdioChildTimingMailbox>() == 64);
    assert!(core::mem::align_of::<DriverRuntimeSdioChildTimingMailbox>() == 64);
    assert!(core::mem::size_of::<DriverRuntimeCyw43DpcChildTimingEntry>() == 28);
    assert!(core::mem::size_of::<DriverRuntimeCyw43DpcChildTimingRecord>() == 512);
    assert!(core::mem::align_of::<DriverRuntimeCyw43DpcChildTimingRecord>() == 64);
    assert!(core::mem::size_of::<DriverRuntimeCyw43DpcClientRecord>() == 128);
    assert!(core::mem::align_of::<DriverRuntimeCyw43DpcClientRecord>() == 64);
    assert!(
        DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET as u32
            + DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES as u32
            == DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET as u32
    );
    assert!(
        DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET as u32
            + DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES as u32
            == DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET as u32
    );
    assert!(
        DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET as u32
            + DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES as u32
            <= DRIVER_RUNTIME_CYW43_RX_BATCH_END_OFFSET as u32
    );
    assert!(
        DRIVER_RUNTIME_CYW43_RX_BATCH_END_OFFSET as usize
            == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as usize
                + DRIVER_RUNTIME_CYW43_RX_BATCH_REQUIRED_SHARED_PAGES
                    * DRIVER_RUNTIME_RING_PAGE_BYTES as usize
    );
};
/// First child CSpace slot reserved for driver-owned IRQ handler caps.
pub const DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT: u32 = 4;
/// Child CSpace slot containing the SDIO owner's BCM2835 DMA IRQ handler cap.
///
/// The SDHCI host IRQ retains the base slot. The linked DMA engine has an
/// independent IRQHandler capability so each physical source can be
/// acknowledged exactly after their badges coalesce on the local notification.
pub const DRIVER_TASK_CHILD_SDIO_DMA_IRQ_HANDLER_SLOT: u32 =
    DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT + 1;
/// Child CSpace slot containing the PCIe runtime's Pi system-timer IRQ handler.
///
/// PCIe owns no other IRQ in the selected Pi profile, so its independently
/// isolated CSpace reuses the base handler slot without aliasing another
/// physical source.
pub const DRIVER_TASK_CHILD_PCIE_TIMER_IRQ_HANDLER_SLOT: u32 =
    DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT;
/// Child CSpace slot containing the explicit MCS command Reply object.
pub const DRIVER_RUNTIME_COMMAND_REPLY_SLOT: u32 = 6;
/// Child CSpace slot containing a send-only completion-wake notification cap.
pub const DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT: u32 = 7;
/// Child CSpace slot containing each runtime's local notification receive cap.
pub const DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT: u32 = 3;
/// Fixed child CSpace slot containing the read-only command endpoint cap.
pub const DRIVER_RUNTIME_COMMAND_ENDPOINT_SLOT: u32 = 2;
/// Maximum synchronous commands associated with one runtime Reply object.
pub const DRIVER_RUNTIME_MAX_INFLIGHT_COMMANDS: u16 = 1;
/// Logical supervisor lane retaining standard-fault Reply authority.
pub const DRIVER_RUNTIME_STANDARD_FAULT_REPLY_LANE: u16 = 1;
/// Logical supervisor lane retaining timeout-fault Reply authority.
pub const DRIVER_RUNTIME_TIMEOUT_FAULT_REPLY_LANE: u16 = 2;
/// High badge-domain discriminator for root-to-driver command Calls.
pub const DRIVER_RUNTIME_COMMAND_BADGE_DOMAIN: u64 = 0x1000_0000_0000_0000;
/// High badge-domain discriminator for driver-to-root completion wakes.
pub const DRIVER_RUNTIME_COMPLETION_BADGE_DOMAIN: u64 = 0x2000_0000_0000_0000;
/// Legacy task-key badge-domain discriminator for nonselected model fixtures.
///
/// Active MCS descriptors use the compiler-owned temporal fault range instead.
pub const DRIVER_RUNTIME_STANDARD_FAULT_BADGE_DOMAIN: u64 = 0x4000_0000_0000_0000;
/// Mask selecting the scheduler-owned high badge domain.
pub const DRIVER_RUNTIME_BADGE_DOMAIN_MASK: u64 = 0xf000_0000_0000_0000;

/// Exact command badge for one stable driver task key.
#[must_use]
pub const fn driver_runtime_command_badge(task_key: u32) -> u64 {
    DRIVER_RUNTIME_COMMAND_BADGE_DOMAIN | task_key as u64
}

/// Returns true only for a root-to-driver command endpoint badge.
///
/// Driver-local IRQ/DPC notification badges occupy the low 32 bits. Keeping
/// this discriminator in the ABI lets a bound-notification `Recv` distinguish
/// a synchronous command from a scheduling wake without inspecting ring data.
#[must_use]
pub const fn driver_runtime_badge_is_command(badge: u64) -> bool {
    badge & DRIVER_RUNTIME_BADGE_DOMAIN_MASK == DRIVER_RUNTIME_COMMAND_BADGE_DOMAIN
}

/// Exact one-way completion badge for one stable driver task key.
#[must_use]
pub const fn driver_runtime_completion_badge(task_key: u32) -> u64 {
    DRIVER_RUNTIME_COMPLETION_BADGE_DOMAIN | task_key as u64
}

/// Legacy standard-fault badge for a task-key-only compatibility fixture.
///
/// Root must not advertise this value in an active MCS descriptor. The
/// selected temporal manifest supplies the actual standard-fault badge.
#[must_use]
pub const fn driver_runtime_standard_fault_badge(task_key: u32) -> u64 {
    DRIVER_RUNTIME_STANDARD_FAULT_BADGE_DOMAIN | task_key as u64
}
/// BCM2711 auxiliary mini-UART interrupt used by the isolated serial runtime.
pub const DRIVER_RUNTIME_SERIAL_IRQ: u32 = 125;
/// Nonzero notification badge bound to [`DRIVER_RUNTIME_SERIAL_IRQ`].
pub const DRIVER_RUNTIME_SERIAL_IRQ_BADGE: u32 = DRIVER_RUNTIME_SERIAL_IRQ + 1;
/// BCM2711 system-timer channel 3 interrupt used by the isolated PCIe runtime.
///
/// Channel 3 is GIC SPI 67, translated by the selected seL4 Pi profile to IRQ
/// 99. The architecture timer remains kernel-owned; this board timer exists
/// only to produce a bounded root-control scheduling hint.
pub const DRIVER_RUNTIME_PCIE_TIMER_IRQ: u32 = 99;
/// One-hot notification badge bound to [`DRIVER_RUNTIME_PCIE_TIMER_IRQ`].
pub const DRIVER_RUNTIME_PCIE_TIMER_IRQ_BADGE: u32 = 1 << 11;
/// Exact BCM2711 peripheral address of the BCM system-timer register page.
pub const DRIVER_RUNTIME_PI4_SYSTEM_TIMER_PADDR: u64 = 0xFE00_3000;
/// Fixed free-running BCM system-timer counter frequency.
pub const DRIVER_RUNTIME_PI4_SYSTEM_TIMER_CLOCK_HZ: u32 = 1_000_000;
/// Generated PCIe-owner MCS period admitted by the selected Pi profile.
pub const DRIVER_RUNTIME_PCIE_TIMER_OWNER_PERIOD_US: u32 = 10_000;
/// C3 wake interval derived as half of the declared PCIe-owner period.
pub const DRIVER_RUNTIME_PCIE_TIMER_INTERVAL_US: u32 =
    DRIVER_RUNTIME_PCIE_TIMER_OWNER_PERIOD_US / 2;
/// PCIe-role-local offset of the durable root-idle timer state.
///
/// Other isolated roles use the same numeric range in distinct physical ring
/// pages. The record is disjoint from the command, continuation grant,
/// completion, progress, cadence, and frame apertures in the PCIe ring.
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_OFFSET: u16 = 192;
/// Exact bytes in one [`DriverRuntimePcieTimerState`] publication.
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_BYTES: u16 = 40;
/// Magic value for a PCIe-owned root-idle timer state (`PTMR`).
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_MAGIC: u32 = 0x5054_4d52;
/// Layout version for [`DriverRuntimePcieTimerState`].
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_VERSION: u16 = 1;
/// Timer state before the exact direct-GENET idle path enables channel 3.
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_DISARMED: u32 = 1;
/// Timer state after one exact enable; IRQ service self-rearms at 5 ms.
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_ENABLED: u32 = 2;
/// Timer state after a source, publication, signal, or ACK invariant failed.
pub const DRIVER_RUNTIME_PCIE_TIMER_STATE_FAULTED: u32 = 3;

/// Durable sequence-last state for the PCIe-owned root-idle timer.
///
/// Root uses this record only to prove whether the exact owner lifetime is
/// already enabled. The absolute root `KernelTimer` deadline remains the time
/// authority; the IRQ notification remains only a scheduling hint.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimePcieTimerState {
    /// [`DRIVER_RUNTIME_PCIE_TIMER_STATE_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_PCIE_TIMER_STATE_VERSION`].
    pub version: u16,
    /// Exact record bytes.
    pub len: u16,
    /// Generated PCIe driver-task key.
    pub task_key: u32,
    /// Sealed PCIe runtime descriptor identity token.
    pub identity_token: u32,
    /// Monotonic nonzero publication identity.
    pub publication: u32,
    /// One `DRIVER_RUNTIME_PCIE_TIMER_STATE_*` value.
    pub state: u32,
    /// Root command sequence that first enabled this lifetime, or zero.
    pub enable_sequence: u32,
    /// Channel-3 compare value at this exceptional state publication.
    pub deadline_clo: u32,
    /// Owner-local IRQ count sampled only for an exceptional state publication.
    /// Healthy 5 ms IRQs deliberately do not republish or clean this record.
    pub irq_count: u32,
    /// Sequence-last commit; exactly repeats `publication`.
    pub committed_publication: u32,
}

impl DriverRuntimePcieTimerState {
    /// Build one uncommitted state publication for a sealed runtime lifetime.
    #[must_use]
    pub const fn staged(
        task_key: u32,
        identity_token: u32,
        publication: u32,
        state: u32,
        enable_sequence: u32,
        deadline_clo: u32,
        irq_count: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_PCIE_TIMER_STATE_MAGIC,
            version: DRIVER_RUNTIME_PCIE_TIMER_STATE_VERSION,
            len: DRIVER_RUNTIME_PCIE_TIMER_STATE_BYTES,
            task_key,
            identity_token,
            publication,
            state,
            enable_sequence,
            deadline_clo,
            irq_count,
            committed_publication: 0,
        }
    }

    /// Commit this complete body by repeating its publication identity.
    #[must_use]
    pub const fn commit(mut self) -> Self {
        if self.body_valid() {
            self.committed_publication = self.publication;
        }
        self
    }

    const fn body_valid(self) -> bool {
        let state_valid = match self.state {
            DRIVER_RUNTIME_PCIE_TIMER_STATE_DISARMED => self.enable_sequence == 0,
            DRIVER_RUNTIME_PCIE_TIMER_STATE_ENABLED | DRIVER_RUNTIME_PCIE_TIMER_STATE_FAULTED => {
                self.enable_sequence != 0
            }
            _ => false,
        };
        self.magic == DRIVER_RUNTIME_PCIE_TIMER_STATE_MAGIC
            && self.version == DRIVER_RUNTIME_PCIE_TIMER_STATE_VERSION
            && self.len == DRIVER_RUNTIME_PCIE_TIMER_STATE_BYTES
            && self.task_key != 0
            && self.identity_token != 0
            && self.publication != 0
            && state_valid
    }

    /// Whether this is one complete identity-bound publication.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid() && self.committed_publication == self.publication
    }

    /// Whether this exact descriptor lifetime is already timer-enabled.
    #[must_use]
    pub const fn enabled_for(self, task_key: u32, identity_token: u32) -> bool {
        self.valid()
            && self.task_key == task_key
            && self.identity_token == identity_token
            && self.state == DRIVER_RUNTIME_PCIE_TIMER_STATE_ENABLED
    }

    /// Accept only two identical complete samples.
    #[must_use]
    pub const fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first.magic == second.magic
            && first.version == second.version
            && first.len == second.len
            && first.task_key == second.task_key
            && first.identity_token == second.identity_token
            && first.publication == second.publication
            && first.state == second.state
            && first.enable_sequence == second.enable_sequence
            && first.deadline_clo == second.deadline_clo
            && first.irq_count == second.irq_count
            && first.committed_publication == second.committed_publication
            && first.valid()
        {
            Some(first)
        } else {
            None
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<DriverRuntimePcieTimerState>() == 40);
    assert!(core::mem::align_of::<DriverRuntimePcieTimerState>() == 4);
    assert!(DRIVER_RUNTIME_PCIE_TIMER_STATE_OFFSET >= 160);
    assert!(
        DRIVER_RUNTIME_PCIE_TIMER_STATE_OFFSET + DRIVER_RUNTIME_PCIE_TIMER_STATE_BYTES
            <= DRIVER_RUNTIME_RING_FRAME_OFFSET
    );
    assert!(core::mem::offset_of!(DriverRuntimePcieTimerState, committed_publication) == 36);
};
/// Exact seL4 IRQ identity for the BCM2711 GENET general/descriptor-ring line.
///
/// The selected Pi profile's repository-managed `kernel.dts` records this as
/// GIC SPI 157. The resolved manifest carries the already-translated seL4 IRQ
/// number so neither root nor the isolated runtime infers a platform offset.
pub const DRIVER_RUNTIME_GENET_IRQ: u32 = 189;
/// One-hot notification badge bound to [`DRIVER_RUNTIME_GENET_IRQ`].
pub const DRIVER_RUNTIME_GENET_IRQ_BADGE: u32 = 1 << 10;
/// Exact badge delivered when console-network signals direct GENET TX work.
pub const DRIVER_RUNTIME_GENET_DIRECT_LINK_NOTIFICATION_BADGE: u32 = 1 << 8;
/// Concise alias used by the fixed direct-GENET descriptor.
pub const DRIVER_RUNTIME_DIRECT_GENET_NOTIFICATION_BADGE: u32 =
    DRIVER_RUNTIME_GENET_DIRECT_LINK_NOTIFICATION_BADGE;
/// CYW43 child CSpace slot containing its send-only root Network-wake notification cap.
pub const DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT: u32 = 11;
/// Exact badge delivered to root after CYW43 commits Network service progress.
pub const DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE: u32 = 1;
/// Child CSpace slot containing the send-only root-control fan-in notification cap.
///
/// Every admitted MCS physical runtime receives this fixed scheduling-hint
/// authority. Durable command, input, and fault records remain the authority
/// for selecting work after root wakes.
pub const DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT: u32 = 12;
/// Exact coalescing badge on every root-control fan-in notification cap.
pub const DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_BADGE: u32 = 1;
/// BCM2711 SDIO host interrupt used by the CYW43 card function.
pub const DRIVER_RUNTIME_SDIO_IRQ: u32 = 158;
/// Nonzero notification badge bound to [`DRIVER_RUNTIME_SDIO_IRQ`].
pub const DRIVER_RUNTIME_SDIO_IRQ_BADGE: u32 = DRIVER_RUNTIME_SDIO_IRQ + 1;
// BCM2711 device-tree SPI values become seL4 IRQ IDs after the GIC SPI offset.
const BCM2711_GIC_SPI_IRQ_BASE: u32 = 32;
const BCM2835_DMA0_SPI: u32 = 0x50;
const BCM2835_SDIO_DMA_CHANNEL: u32 = 4;
/// BCM2711 BCM2835 DMA channel 4 interrupt used by the SDIO data engine.
pub const DRIVER_RUNTIME_SDIO_DMA_IRQ: u32 =
    BCM2711_GIC_SPI_IRQ_BASE + BCM2835_DMA0_SPI + BCM2835_SDIO_DMA_CHANNEL;
/// Disjoint notification bit bound to [`DRIVER_RUNTIME_SDIO_DMA_IRQ`].
///
/// Unlike the legacy IRQ-plus-one badge, this is deliberately one bit so a
/// single local notification word can report SDHCI, DMA, peer, and retained
/// root wake sources without losing their physical identity.
pub const DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE: u32 = 1 << 9;
/// Reserved high notification bit for root-owned retained-command scheduling.
///
/// Peer and IRQ badges coalesce with bitwise OR. Keeping this bit outside their
/// mask makes stale authority fail closed. The badge is only a durable wake:
/// the immutable request identity and exact generation grant remain the
/// authority for one root-owned foreground quantum.
pub const DRIVER_RUNTIME_RESERVED_ROOT_BADGE: u32 = 1 << 31;
/// Badge delivered to CYW43 when the SDIO owner signals its reciprocal peer cap.
pub const DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE: u32 = 2;
/// Badge delivered to the SDIO owner when CYW43 signals retained work.
///
/// This durable notification edge is deliberately disjoint from the SDIO IRQ,
/// the reciprocal SDIO-to-CYW43 DPC badge, and reserved root authority.
pub const DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE: u32 = 1 << 8;

const _: () = {
    assert!(DRIVER_RUNTIME_GENET_IRQ_BADGE & DRIVER_RUNTIME_RESERVED_ROOT_BADGE == 0);
    assert!(DRIVER_RUNTIME_PCIE_TIMER_INTERVAL_US == 5_000);
    assert!(DRIVER_RUNTIME_PCIE_TIMER_IRQ_BADGE & DRIVER_RUNTIME_RESERVED_ROOT_BADGE == 0);
    assert!(
        DRIVER_RUNTIME_GENET_IRQ_BADGE & DRIVER_RUNTIME_GENET_DIRECT_LINK_NOTIFICATION_BADGE == 0
    );
    assert!(
        DRIVER_RUNTIME_GENET_DIRECT_LINK_NOTIFICATION_BADGE & DRIVER_RUNTIME_RESERVED_ROOT_BADGE
            == 0
    );
    assert!(DRIVER_TASK_CHILD_SDIO_DMA_IRQ_HANDLER_SLOT != DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT);
    assert!(DRIVER_RUNTIME_SDIO_DMA_IRQ == 116);
    assert!(DRIVER_RUNTIME_SDIO_DMA_IRQ != DRIVER_RUNTIME_SDIO_IRQ);
    assert!(DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE & DRIVER_RUNTIME_SDIO_IRQ_BADGE == 0);
    assert!(
        DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE & DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE == 0
    );
    assert!(DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE & DRIVER_RUNTIME_RESERVED_ROOT_BADGE == 0);
};

/// Durable level of the CYW43 runtime's private decoded-RX queue.
///
/// `commit_sequence` is the sequence-last publication word. Before changing
/// any body field, the sole CYW43 producer clears that word and makes the clear
/// visible. It then writes the body, cleans and barriers the record, and writes
/// a new nonzero sequence last. Readers accept only two identical, valid
/// samples. A root-wake notification merely prompts a new sample; this record
/// is the durable condition that determines whether work remains visible.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43RxQueueState {
    /// Fixed [`DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Nonzero CYW43 runtime generation, or zero before runtime admission.
    pub generation: u32,
    /// Frames durably retained in the private decoded-RX queue.
    pub queue_depth: u16,
    /// Exact [`DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP`] ABI capacity.
    pub queue_capacity: u16,
    /// Queue condition flags, including poison containment.
    pub flags: u32,
    /// Exact source line in `apps/pi4-driver-runtime/src/lib.rs` that first
    /// changed `recovery_required` from false to true for this generation.
    ///
    /// This is passive exact-image evidence only. It cannot authorize work or
    /// recovery, and must remain zero for a healthy queue state.
    pub recovery_source_line: u32,
    /// Nonzero monotonically increasing commit sequence, written last.
    pub commit_sequence: u32,
}

impl DriverRuntimeCyw43RxQueueState {
    /// Canonical initialized state before the first CYW43 runtime generation.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_MAGIC,
            version: DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_VERSION,
            len: DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES,
            generation: 0,
            queue_depth: 0,
            queue_capacity: DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP as u16,
            flags: 0,
            recovery_source_line: 0,
            commit_sequence: 0,
        }
    }

    /// Byte-zero form produced by initial command-ring construction.
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            magic: 0,
            version: 0,
            len: 0,
            generation: 0,
            queue_depth: 0,
            queue_capacity: 0,
            flags: 0,
            recovery_source_line: 0,
            commit_sequence: 0,
        }
    }

    /// Whether this is the byte-zero initial ring form.
    #[must_use]
    pub const fn is_zeroed(self) -> bool {
        self.magic == 0
            && self.version == 0
            && self.len == 0
            && self.generation == 0
            && self.queue_depth == 0
            && self.queue_capacity == 0
            && self.flags == 0
            && self.recovery_source_line == 0
            && self.commit_sequence == 0
    }

    /// Whether every body field is internally consistent before final commit.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_MAGIC
            && self.version == DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_VERSION
            && self.len == DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES
            && self.queue_capacity == DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP as u16
            && self.queue_depth <= self.queue_capacity
            && self.flags & !DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_FLAG_POISONED == 0
            && (self.flags & DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_FLAG_POISONED != 0)
                == (self.recovery_source_line != 0)
            && (self.generation != 0
                || (self.queue_depth == 0
                    && self.flags == 0
                    && self.recovery_source_line == 0
                    && self.commit_sequence == 0))
    }

    /// Whether this is either the canonical empty state or one committed level.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid()
            && ((self.generation == 0 && self.commit_sequence == 0)
                || (self.generation != 0 && self.commit_sequence != 0))
    }

    /// Whether a runtime generation has committed this durable queue level.
    #[must_use]
    pub const fn committed(self) -> bool {
        self.valid() && self.generation != 0
    }

    /// Whether this committed generation is fenced from further RX service.
    #[must_use]
    pub const fn poisoned(self) -> bool {
        self.committed() && self.flags & DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_FLAG_POISONED != 0
    }

    /// Return the immutable first runtime recovery source for a poisoned level.
    #[must_use]
    pub const fn recovery_source_line(self) -> Option<u32> {
        if self.poisoned() && self.recovery_source_line != 0 {
            Some(self.recovery_source_line)
        } else {
            None
        }
    }

    /// Whether root must service the durable RX condition without another wake.
    #[must_use]
    pub const fn work_visible(self) -> bool {
        self.committed() && self.queue_depth != 0 && !self.poisoned()
    }

    /// Strictly monotonic next commit sequence, or `None` at exhaustion.
    #[must_use]
    pub const fn next_commit_sequence(self) -> Option<u32> {
        if !self.valid() || self.commit_sequence == u32::MAX {
            None
        } else {
            Some(self.commit_sequence + 1)
        }
    }

    /// Accept two identical, valid volatile samples as one stable queue level.
    ///
    /// A byte-zero initial command ring is normalized to [`Self::empty`].
    /// Callers must place their platform load/cache barriers between samples.
    #[must_use]
    pub const fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        let first = if first.is_zeroed() {
            Self::empty()
        } else {
            first
        };
        let second = if second.is_zeroed() {
            Self::empty()
        } else {
            second
        };
        if first.magic == second.magic
            && first.version == second.version
            && first.len == second.len
            && first.generation == second.generation
            && first.queue_depth == second.queue_depth
            && first.queue_capacity == second.queue_capacity
            && first.flags == second.flags
            && first.commit_sequence == second.commit_sequence
            && first.valid()
        {
            Some(first)
        } else {
            None
        }
    }
}

/// Exact metadata for one fixed CYW43 RX batch payload slot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43RxBatchEntry {
    /// Exact shared-buffer offset for this entry index.
    pub offset: u32,
    /// Decoded payload bytes in this fixed slot.
    pub len: u16,
    /// Supported SDPCM channel plus the observed firmware credit.
    pub flags: u16,
}

impl DriverRuntimeCyw43RxBatchEntry {
    /// Empty metadata for an unused batch slot.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            offset: 0,
            len: 0,
            flags: 0,
        }
    }

    /// Whether every field is zero for an unused batch slot.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.offset == 0 && self.len == 0 && self.flags == 0
    }

    /// Whether this entry names the exact payload slot assigned to `index`.
    #[must_use]
    pub const fn valid_for_index(self, index: usize) -> bool {
        match driver_runtime_cyw43_rx_batch_payload_offset(index) {
            Some(expected_offset) => {
                self.offset == expected_offset
                    && self.len != 0
                    && self.len <= DRIVER_RUNTIME_CYW43_RX_BATCH_FRAME_BYTES
                    && driver_runtime_cyw43_rx_frame_flags_valid(self.flags)
            }
            None => false,
        }
    }
}

/// Root-visible batch of frames drained under one persistent CYW43 transaction.
///
/// The sole CYW43 producer clears `committed_parent_sequence` before changing
/// the body, writes every payload and header body field, cleans and barriers the
/// full region, then repeats `parent_sequence` in the final field. Root accepts
/// only two identical records whose final commit matches the immutable parent.
/// A terminal op8 completion reports the entry count in `result`, points
/// `frame` at this header, and uses
/// [`DRIVER_RUNTIME_CYW43_RX_BATCH_DETAIL`]. A retained op11 transaction may
/// instead expose the same immutable batch as sideband state and continue only
/// after root publishes an exact [`DriverRuntimeCyw43RxBatchAck`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43RxBatchRecord {
    /// Fixed [`DRIVER_RUNTIME_CYW43_RX_BATCH_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CYW43_RX_BATCH_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Immutable nonzero CYW43 parent-command sequence.
    pub parent_sequence: u32,
    /// Nonzero CYW43 runtime generation owning this batch.
    pub generation: u32,
    /// Exact committed private-queue state observed after the batch drain.
    pub queue_commit_sequence: u32,
    /// Number of populated entries in `entries`.
    pub count: u16,
    /// Frames still durable in the CYW43 private queue after this batch.
    pub remaining: u16,
    /// Fixed metadata for each exact payload slot.
    pub entries: [DriverRuntimeCyw43RxBatchEntry; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
    /// Low CNTVCT word for each entry's exact DPC source episode.
    ///
    /// Every populated v3 slot has a valid raw modulo-32 value, including
    /// zero. These timestamps are passive evidence and never scheduling or
    /// recovery authority.
    pub source_cntvct_lo: [u32; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
    /// Packed Q11 stage deltas for the first populated CHANNEL_DATA entry.
    ///
    /// The low half is source-to-successful private-queue commit and the high
    /// half is that queue commit to the final precommit evidence-word sample.
    /// Values through `0xfffe` are exact quantized floors; `0xffff` is
    /// saturated or unknown. A batch with no CHANNEL_DATA entry publishes zero;
    /// raw zero is also a valid measured value when a CHANNEL_DATA entry does
    /// exist. This word is passive evidence and never wake, admission,
    /// scheduling, or recovery authority.
    pub first_data_stage_deltas_q11: u32,
    /// Sequence-last commit; exactly repeats `parent_sequence` when complete.
    pub committed_parent_sequence: u32,
}

impl DriverRuntimeCyw43RxBatchRecord {
    /// Canonical uncommitted empty header.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_RX_BATCH_MAGIC,
            version: DRIVER_RUNTIME_CYW43_RX_BATCH_VERSION,
            len: DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES,
            parent_sequence: 0,
            generation: 0,
            queue_commit_sequence: 0,
            count: 0,
            remaining: 0,
            entries: [DriverRuntimeCyw43RxBatchEntry::empty();
                DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
            source_cntvct_lo: [0; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
            first_data_stage_deltas_q11: 0,
            committed_parent_sequence: 0,
        }
    }

    /// Construct an uncommitted body; publish it only after calling [`Self::commit`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn staged(
        parent_sequence: u32,
        generation: u32,
        queue_commit_sequence: u32,
        count: u16,
        remaining: u16,
        entries: [DriverRuntimeCyw43RxBatchEntry; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
        source_cntvct_lo: [u32; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
        first_data_stage_deltas_q11: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_RX_BATCH_MAGIC,
            version: DRIVER_RUNTIME_CYW43_RX_BATCH_VERSION,
            len: DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES,
            parent_sequence,
            generation,
            queue_commit_sequence,
            count,
            remaining,
            entries,
            source_cntvct_lo,
            first_data_stage_deltas_q11,
            committed_parent_sequence: 0,
        }
    }

    /// Whether every header body field and exact payload slot is consistent.
    ///
    /// This deliberately ignores the sequence-last field so a writer can
    /// validate a staged body before committing it.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_CYW43_RX_BATCH_MAGIC
            || self.version != DRIVER_RUNTIME_CYW43_RX_BATCH_VERSION
            || self.len != DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES
            || self.parent_sequence == 0
            || self.generation == 0
            || self.queue_commit_sequence == 0
            || self.count == 0
            || self.count as usize > DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP
            || self.remaining as usize > DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP
            || self.count as usize + self.remaining as usize > DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP
        {
            return false;
        }

        let mut index = 0;
        while index < DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP {
            if index < self.count as usize {
                if !self.entries[index].valid_for_index(index) {
                    return false;
                }
            } else if !self.entries[index].is_empty() || self.source_cntvct_lo[index] != 0 {
                return false;
            }
            index += 1;
        }

        true
    }

    /// Whether the final sequence-last field commits this complete header.
    #[must_use]
    pub const fn committed(self) -> bool {
        self.parent_sequence != 0 && self.committed_parent_sequence == self.parent_sequence
    }

    /// Whether this is one complete, sequence-last committed RX batch.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid() && self.committed()
    }

    /// Return this valid staged body with its parent sequence committed last.
    #[must_use]
    pub const fn commit(mut self) -> Self {
        if self.body_valid() {
            self.committed_parent_sequence = self.parent_sequence;
        }
        self
    }

    /// Whether this batch is no newer than the current durable queue condition.
    ///
    /// `remaining` is the historical post-drain level committed with this
    /// immutable batch. A later same-generation enqueue may advance both the
    /// queue commit and current depth before root copies the batch, so current
    /// depth is deliberately not required to equal `remaining`. Commit
    /// exhaustion is fault-contained instead of wrapping.
    #[must_use]
    pub const fn valid_for_queue_state(self, queue_state: DriverRuntimeCyw43RxQueueState) -> bool {
        self.valid()
            && queue_state.committed()
            && !queue_state.poisoned()
            && self.generation == queue_state.generation
            && queue_state.commit_sequence >= self.queue_commit_sequence
    }

    /// Whether this batch matches one immutable parent and durable queue level.
    #[must_use]
    pub const fn valid_for_parent_and_queue_state(
        self,
        expected_parent_sequence: u32,
        queue_state: DriverRuntimeCyw43RxQueueState,
    ) -> bool {
        expected_parent_sequence != 0
            && self.parent_sequence == expected_parent_sequence
            && self.valid_for_queue_state(queue_state)
    }

    /// Whether two records name the same behavior-bearing batch identity.
    ///
    /// The first-data timing word is passive evidence only and is deliberately
    /// excluded. A difference in that word may degrade diagnostics, but must
    /// never reject payload, acknowledge progress, or authorize recovery.
    #[must_use]
    pub fn authority_identity_matches(self, other: Self) -> bool {
        let mut left = self;
        left.first_data_stage_deltas_q11 = 0;
        let mut right = other;
        right.first_data_stage_deltas_q11 = 0;
        left == right
    }

    /// Accept two authority-identical, valid volatile samples as one stable RX batch.
    ///
    /// Callers must place their platform load/cache barriers between samples
    /// and copy payloads only after this header has stabilized. The passive
    /// first-data timing word is deliberately excluded from behavioral
    /// identity: a mismatch degrades only that evidence to saturated/unknown
    /// and cannot reject an otherwise exact batch or authorize recovery.
    #[must_use]
    pub fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first.authority_identity_matches(second) && first.valid() && second.valid() {
            let mut snapshot = first;
            if first.first_data_stage_deltas_q11 != second.first_data_stage_deltas_q11 {
                snapshot.first_data_stage_deltas_q11 =
                    driver_runtime_cyw43_rx_stage_deltas_q11_pack(
                        DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
                        DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
                    );
            }
            Some(snapshot)
        } else {
            None
        }
    }
}

/// Root-owned acknowledgement for one exact CYW43 RX sideband batch.
///
/// Root clears `committed_queue_commit_sequence` before changing the body,
/// writes the complete immutable batch identity, cleans and barriers this
/// dedicated cache line, then repeats `queue_commit_sequence` in the final
/// word. The CYW43 producer accepts only two identical samples which exactly
/// match its still-published batch. This durable acknowledgement is authority;
/// a notification can only prompt the producer to sample it again.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43RxBatchAck {
    /// Fixed [`DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Nonzero CYW43 runtime generation owning the acknowledged batch.
    pub generation: u32,
    /// Immutable nonzero CYW43 parent-command sequence.
    pub parent_sequence: u32,
    /// Exact private-queue commit sequence named by the acknowledged batch.
    pub queue_commit_sequence: u32,
    /// Exact number of batch entries consumed by root.
    pub count: u16,
    /// Must remain zero so future layouts fail closed.
    pub reserved: [u8; 38],
    /// Sequence-last commit; exactly repeats `queue_commit_sequence`.
    pub committed_queue_commit_sequence: u32,
}

impl DriverRuntimeCyw43RxBatchAck {
    /// Canonical uncommitted empty acknowledgement.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_MAGIC,
            version: DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_VERSION,
            len: DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES,
            generation: 0,
            parent_sequence: 0,
            queue_commit_sequence: 0,
            count: 0,
            reserved: [0; 38],
            committed_queue_commit_sequence: 0,
        }
    }

    /// Construct an uncommitted body for one immutable RX batch identity.
    #[must_use]
    pub const fn staged(
        generation: u32,
        parent_sequence: u32,
        queue_commit_sequence: u32,
        count: u16,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_MAGIC,
            version: DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_VERSION,
            len: DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES,
            generation,
            parent_sequence,
            queue_commit_sequence,
            count,
            reserved: [0; 38],
            committed_queue_commit_sequence: 0,
        }
    }

    /// Whether every acknowledgement body field is internally consistent.
    ///
    /// This deliberately ignores the sequence-last field so root can validate
    /// the staged body before committing it.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_MAGIC
            || self.version != DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_VERSION
            || self.len != DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES
            || self.generation == 0
            || self.parent_sequence == 0
            || self.queue_commit_sequence == 0
            || self.count == 0
            || self.count as usize > DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP
        {
            return false;
        }

        let mut index = 0;
        while index < self.reserved.len() {
            if self.reserved[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Whether this is one complete, sequence-last committed acknowledgement.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid() && self.committed_queue_commit_sequence == self.queue_commit_sequence
    }

    /// Return this valid staged body with its queue sequence committed last.
    #[must_use]
    pub const fn commit(mut self) -> Self {
        if self.body_valid() {
            self.committed_queue_commit_sequence = self.queue_commit_sequence;
        }
        self
    }

    /// Whether this committed acknowledgement names exactly `batch`.
    #[must_use]
    pub const fn matches_batch(self, batch: DriverRuntimeCyw43RxBatchRecord) -> bool {
        self.valid()
            && batch.valid()
            && self.generation == batch.generation
            && self.parent_sequence == batch.parent_sequence
            && self.queue_commit_sequence == batch.queue_commit_sequence
            && self.count == batch.count
    }

    /// Accept two identical valid samples as one stable acknowledgement.
    ///
    /// Callers must place their platform load/cache barriers between samples.
    #[must_use]
    pub fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first == second && first.valid() {
            Some(first)
        } else {
            None
        }
    }
}

/// Passive sequence-last timing for one exact linked CYW43-to-SDIO child.
///
/// CYW43 publishes the exact child identity and publication tick before it
/// signals the linked owner. SDIO preserves that body, adds the physical issue
/// and joined-terminal ticks, and commits `child_sequence` last before normal
/// completion publication. No field participates in admission, retry,
/// recovery, wake, or scheduler policy.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSdioChildTimingMailbox {
    /// Fixed [`DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Exact nonzero linked child sequence.
    pub child_sequence: u32,
    /// Immutable descriptor/action fingerprint.
    pub descriptor_fingerprint: u32,
    /// Exact nonzero SDIO physical-owner epoch.
    pub physical_epoch: u32,
    /// Exact nonzero DPC event sequence.
    pub event_sequence: u32,
    /// Typed CYW43 DPC action.
    pub action: u8,
    /// Typed CYW43 DPC I/O kind.
    pub io_kind: u8,
    /// Typed CYW43 DPC I/O phase.
    pub io_phase: u8,
    /// [`DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_*`] value.
    pub engine: u8,
    /// `DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_*` evidence bits.
    pub flags: u32,
    /// Low CNTVCT word sampled for the passive mailbox stage before command commit.
    pub published_cntvct_lo: u32,
    /// Low CNTVCT word sampled when SDIO admits the exact linked child.
    pub intake_cntvct_lo: u32,
    /// Low CNTVCT word sampled immediately after physical command issue.
    pub issued_cntvct_lo: u32,
    /// Low CNTVCT word sampled after the joined physical terminal.
    pub terminal_cntvct_lo: u32,
    /// Must remain zero so future layouts fail closed.
    pub reserved: [u8; 12],
    /// Sequence-last commit; exactly repeats `child_sequence`.
    pub committed_child_sequence: u32,
}

impl DriverRuntimeSdioChildTimingMailbox {
    const FLAG_MASK: u32 = DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_PUBLISHED
        | DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_INTAKE
        | DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_ISSUED
        | DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_TERMINAL;

    /// Canonical empty mailbox.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_MAGIC,
            version: DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_VERSION,
            len: DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_BYTES,
            child_sequence: 0,
            descriptor_fingerprint: 0,
            physical_epoch: 0,
            event_sequence: 0,
            action: 0,
            io_kind: 0,
            io_phase: 0,
            engine: 0,
            flags: 0,
            published_cntvct_lo: 0,
            intake_cntvct_lo: 0,
            issued_cntvct_lo: 0,
            terminal_cntvct_lo: 0,
            reserved: [0; 12],
            committed_child_sequence: 0,
        }
    }

    /// Whether the diagnostic body is internally consistent.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_MAGIC
            || self.version != DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_VERSION
            || self.len != DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_BYTES
            || self.child_sequence == 0
            || self.descriptor_fingerprint == 0
            || self.physical_epoch == 0
            || self.event_sequence == 0
            || self.action == 0
            || self.action > 19
            || self.io_kind == 0
            || self.io_kind > 6
            || self.io_phase == 0
            || self.io_phase > 4
            || !(self.engine as u16 == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_COMMAND
                || self.engine as u16 == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_PIO
                || self.engine as u16 == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_DMA)
            || self.flags & !Self::FLAG_MASK != 0
            || self.flags & DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_PUBLISHED == 0
            || (self.flags & DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_ISSUED != 0
                && self.flags & DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_INTAKE == 0)
            || (self.flags & DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_TERMINAL != 0
                && self.flags & DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_ISSUED == 0)
        {
            return false;
        }
        let mut index = 0;
        while index < self.reserved.len() {
            if self.reserved[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Whether the sequence-last mailbox publication is complete.
    #[must_use]
    pub const fn committed(self) -> bool {
        self.body_valid() && self.committed_child_sequence == self.child_sequence
    }
}

/// One exact child entry in a selected CYW43 DPC DATA-event timing trace.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43DpcChildTimingEntry {
    /// Exact nonzero linked child sequence.
    pub child_sequence: u32,
    /// Packed action, I/O kind, I/O phase, engine, and validity flags.
    pub meta: u32,
    /// Low CNTVCT word sampled for the passive mailbox stage before command commit.
    pub published_cntvct_lo: u32,
    /// Low CNTVCT word sampled when SDIO admits the exact linked child.
    pub intake_cntvct_lo: u32,
    /// Low CNTVCT word sampled immediately after `SDHCI_COMMAND` issue.
    pub issued_cntvct_lo: u32,
    /// Low CNTVCT word sampled after the joined physical terminal.
    pub terminal_cntvct_lo: u32,
    /// Low CNTVCT word sampled when CYW43 accepts the exact completion.
    pub accepted_cntvct_lo: u32,
}

impl DriverRuntimeCyw43DpcChildTimingEntry {
    /// Canonical empty entry.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            child_sequence: 0,
            meta: 0,
            published_cntvct_lo: 0,
            intake_cntvct_lo: 0,
            issued_cntvct_lo: 0,
            terminal_cntvct_lo: 0,
            accepted_cntvct_lo: 0,
        }
    }

    /// Pack one typed entry metadata word.
    #[must_use]
    pub const fn pack_meta(action: u8, io_kind: u8, io_phase: u8, engine: u8, flags: u8) -> u32 {
        action as u32
            | ((io_kind as u32) << 8)
            | ((io_phase as u32) << 16)
            | (((engine & 0x03) as u32) << 24)
            | (((flags & 0x1f) as u32) << 26)
    }

    /// Typed action stored in [`Self::meta`].
    #[must_use]
    pub const fn action(self) -> u8 {
        self.meta as u8
    }

    /// Typed I/O kind stored in [`Self::meta`].
    #[must_use]
    pub const fn io_kind(self) -> u8 {
        (self.meta >> 8) as u8
    }

    /// Typed I/O phase stored in [`Self::meta`].
    #[must_use]
    pub const fn io_phase(self) -> u8 {
        (self.meta >> 16) as u8
    }

    /// Typed child engine stored in [`Self::meta`].
    #[must_use]
    pub const fn engine(self) -> u8 {
        ((self.meta >> 24) & 0x03) as u8
    }

    /// Validity flags stored in [`Self::meta`].
    #[must_use]
    pub const fn flags(self) -> u8 {
        ((self.meta >> 26) & 0x1f) as u8
    }

    /// Whether this populated entry contains a complete exact timing tuple.
    #[must_use]
    pub const fn complete(self) -> bool {
        let required = DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_PUBLISHED
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_INTAKE
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_ISSUED
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_TERMINAL
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_ACCEPTED;
        self.child_sequence != 0
            && self.action() != 0
            && self.action() <= 19
            && self.io_kind() >= 1
            && self.io_kind() <= 6
            && self.io_phase() >= 1
            && self.io_phase() <= 4
            && self.flags() == required
            && self.meta & (1 << 31) == 0
            && (self.engine() as u16 == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_COMMAND
                || self.engine() as u16 == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_PIO
                || self.engine() as u16 == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_DMA)
    }

    /// Whether this entry is the all-zero unused representation.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.child_sequence == 0
            && self.meta == 0
            && self.published_cntvct_lo == 0
            && self.intake_cntvct_lo == 0
            && self.issued_cntvct_lo == 0
            && self.terminal_cntvct_lo == 0
            && self.accepted_cntvct_lo == 0
    }
}

/// Passive selected DATA-event trace for the unresolved CYW43 DPC interval.
///
/// CYW43 publishes only a complete queue-committed event, keeps the slowest
/// source-to-queue event seen in the current physical epoch, and commits
/// `publication_sequence` last. Root may format the record, but no consumer
/// may use it to admit service or recovery.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43DpcChildTimingRecord {
    /// Fixed [`DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Nonzero publication identity committed in the final word.
    pub publication_sequence: u32,
    /// Exact nonzero SDIO physical-owner epoch.
    pub physical_epoch: u32,
    /// Exact nonzero DPC event sequence.
    pub event_sequence: u32,
    /// Low CNTVCT word at CYW43 DPC event admission.
    pub source_cntvct_lo: u32,
    /// Low CNTVCT word after the successful durable DATA queue commit.
    pub queue_commit_cntvct_lo: u32,
    /// Exact nonzero private queue-state commit sequence.
    pub queue_commit_sequence: u32,
    /// Exact selected DATA frame length.
    pub data_len: u16,
    /// Number of populated child entries.
    pub child_count: u16,
    /// `DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_*` evidence bits.
    pub flags: u32,
    /// Selected event's source-to-queue interval in Q11 ticks.
    pub selected_source_to_queue_q11: u16,
    /// Largest complete source-to-queue interval observed in this epoch.
    pub overall_max_source_to_queue_q11: u16,
    /// Count of otherwise eligible events that exceeded the bounded entry cap.
    pub overflow_samples: u32,
    /// Count of otherwise eligible events with missing/mismatched child evidence.
    pub unknown_samples: u32,
    /// Must remain zero so future layouts fail closed.
    pub reserved: [u8; 8],
    /// Bounded exact child sequence in publication order.
    pub entries:
        [DriverRuntimeCyw43DpcChildTimingEntry; DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_ENTRY_CAP],
    /// Sequence-last commit; exactly repeats `publication_sequence`.
    pub committed_publication_sequence: u32,
}

impl DriverRuntimeCyw43DpcChildTimingRecord {
    const FLAG_MASK: u32 = DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_COMPLETE
        | DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_OVERFLOW
        | DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_UNKNOWN
        | DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_MAILBOX_MISMATCH;

    /// Canonical empty trace.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_MAGIC,
            version: DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_VERSION,
            len: DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES,
            publication_sequence: 0,
            physical_epoch: 0,
            event_sequence: 0,
            source_cntvct_lo: 0,
            queue_commit_cntvct_lo: 0,
            queue_commit_sequence: 0,
            data_len: 0,
            child_count: 0,
            flags: 0,
            selected_source_to_queue_q11: 0,
            overall_max_source_to_queue_q11: 0,
            overflow_samples: 0,
            unknown_samples: 0,
            reserved: [0; 8],
            entries: [DriverRuntimeCyw43DpcChildTimingEntry::empty();
                DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_ENTRY_CAP],
            committed_publication_sequence: 0,
        }
    }

    /// Whether every passive body field is internally consistent.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_MAGIC
            || self.version != DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_VERSION
            || self.len != DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES
            || self.publication_sequence == 0
            || self.physical_epoch == 0
            || self.event_sequence == 0
            || self.queue_commit_sequence == 0
            || self.data_len == 0
            || self.child_count == 0
            || self.child_count as usize > DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_ENTRY_CAP
            || self.flags & !Self::FLAG_MASK != 0
            || self.flags & DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_COMPLETE == 0
        {
            return false;
        }
        let exact = self.flags
            & (DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_OVERFLOW
                | DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_UNKNOWN
                | DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_MAILBOX_MISMATCH)
            == 0;
        if exact {
            if self.selected_source_to_queue_q11
                == DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED
                || self.selected_source_to_queue_q11 != self.overall_max_source_to_queue_q11
                || self.selected_source_to_queue_q11
                    != driver_runtime_cyw43_rx_stage_delta_q11(
                        self.source_cntvct_lo,
                        self.queue_commit_cntvct_lo,
                    )
            {
                return false;
            }
            let total_ticks = self
                .queue_commit_cntvct_lo
                .wrapping_sub(self.source_cntvct_lo);
            let mut previous = self.source_cntvct_lo;
            let mut stage_sum = 0u64;
            let mut exact_index = 0usize;
            while exact_index < self.child_count as usize {
                let entry = self.entries[exact_index];
                let stages = [
                    entry.published_cntvct_lo.wrapping_sub(previous),
                    entry
                        .intake_cntvct_lo
                        .wrapping_sub(entry.published_cntvct_lo),
                    entry.issued_cntvct_lo.wrapping_sub(entry.intake_cntvct_lo),
                    entry
                        .terminal_cntvct_lo
                        .wrapping_sub(entry.issued_cntvct_lo),
                    entry
                        .accepted_cntvct_lo
                        .wrapping_sub(entry.terminal_cntvct_lo),
                ];
                let mut stage_index = 0usize;
                while stage_index < stages.len() {
                    if stages[stage_index] > total_ticks {
                        return false;
                    }
                    stage_sum = stage_sum.saturating_add(stages[stage_index] as u64);
                    stage_index += 1;
                }
                previous = entry.accepted_cntvct_lo;
                exact_index += 1;
            }
            let tail = self.queue_commit_cntvct_lo.wrapping_sub(previous);
            if tail > total_ticks || stage_sum.saturating_add(tail as u64) != total_ticks as u64 {
                return false;
            }
        }
        let mut index = 0usize;
        while index < DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_ENTRY_CAP {
            if index < self.child_count as usize {
                if exact && !self.entries[index].complete() {
                    return false;
                }
                if self.entries[index].child_sequence == 0 {
                    return false;
                }
            } else if !self.entries[index].is_empty() {
                return false;
            }
            index += 1;
        }
        index = 0;
        while index < self.reserved.len() {
            if self.reserved[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Whether the sequence-last trace publication is complete.
    #[must_use]
    pub const fn committed(self) -> bool {
        self.body_valid() && self.committed_publication_sequence == self.publication_sequence
    }
}

/// Passive, current CYW43 DPC-client accounting for one physical epoch.
///
/// The isolated CYW43 runtime is the sole writer. It clears
/// `committed_publication_sequence`, writes the body, cleans the two dedicated
/// cache lines, and repeats the publication identity in the final
/// `committed_publication_sequence` word. Root may stable-read the record for
/// diagnostics only; it never grants admission, wake, scheduling, retry,
/// recovery, or physical-owner authority.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43DpcClientRecord {
    /// Fixed [`DRIVER_RUNTIME_CYW43_DPC_CLIENT_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CYW43_DPC_CLIENT_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Nonzero publication identity committed in the final word.
    pub publication_sequence: u32,
    /// Exact nonzero SDIO physical-owner epoch.
    pub physical_epoch: u32,
    /// Exact wrapping live-ring consumer sequence durably accepted by CYW43.
    pub consumer_sequence: u32,
    /// Generation-bound SDIO-owner rearm publication attempts.
    pub rearms: u32,
    /// DPC event epoch mismatches observed by CYW43.
    pub epoch_errors: u32,
    /// DPC event sequence faults observed by CYW43.
    pub sequence_errors: u32,
    /// Initial DPC source samples classified in this epoch.
    pub source_samples: u32,
    /// Samples carrying a frame indication.
    pub source_frame: u32,
    /// Samples carrying a host-mailbox indication.
    pub source_hostmail: u32,
    /// Samples carrying a flow-control change.
    pub source_fc_change: u32,
    /// Samples carrying flow-control state.
    pub source_fc_state: u32,
    /// Samples carrying CHIPACTIVE.
    pub source_chipactive: u32,
    /// Samples carrying another nonzero source.
    pub source_other: u32,
    /// Samples with no source bits set.
    pub source_spurious: u32,
    /// Event-associated CYW43 DPC service turns.
    pub turns: u32,
    /// Exact SDIO-owner children published for DPC service.
    pub owner_children: u32,
    /// Exact SDIO-owner turns attributed to DPC service.
    pub owner_turns: u32,
    /// Root-visible frames completed during DPC service.
    pub frames_completed: u32,
    /// CYW43 DPC turns attributed to completed frames.
    pub frame_turns: u32,
    /// SDIO-owner turns attributed to completed frames.
    pub frame_owner_turns: u32,
    /// Must remain zero so future layouts fail closed.
    pub reserved: [u8; 36],
    /// Sequence-last commit; exactly repeats `publication_sequence`.
    pub committed_publication_sequence: u32,
}

impl DriverRuntimeCyw43DpcClientRecord {
    /// Canonical uncommitted empty diagnostic.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_DPC_CLIENT_MAGIC,
            version: DRIVER_RUNTIME_CYW43_DPC_CLIENT_VERSION,
            len: DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES,
            publication_sequence: 0,
            physical_epoch: 0,
            consumer_sequence: 0,
            rearms: 0,
            epoch_errors: 0,
            sequence_errors: 0,
            source_samples: 0,
            source_frame: 0,
            source_hostmail: 0,
            source_fc_change: 0,
            source_fc_state: 0,
            source_chipactive: 0,
            source_other: 0,
            source_spurious: 0,
            turns: 0,
            owner_children: 0,
            owner_turns: 0,
            frames_completed: 0,
            frame_turns: 0,
            frame_owner_turns: 0,
            reserved: [0; 36],
            committed_publication_sequence: 0,
        }
    }

    /// Encode the fixed shared-memory representation as little-endian words.
    #[must_use]
    pub const fn to_le_words(self) -> [u32; DRIVER_RUNTIME_CYW43_DPC_CLIENT_WORDS] {
        let mut words = [0; DRIVER_RUNTIME_CYW43_DPC_CLIENT_WORDS];
        words[0] = self.magic;
        words[1] = self.version as u32 | ((self.len as u32) << 16);
        words[2] = self.publication_sequence;
        words[3] = self.physical_epoch;
        words[4] = self.consumer_sequence;
        words[5] = self.rearms;
        words[6] = self.epoch_errors;
        words[7] = self.sequence_errors;
        words[8] = self.source_samples;
        words[9] = self.source_frame;
        words[10] = self.source_hostmail;
        words[11] = self.source_fc_change;
        words[12] = self.source_fc_state;
        words[13] = self.source_chipactive;
        words[14] = self.source_other;
        words[15] = self.source_spurious;
        words[16] = self.turns;
        words[17] = self.owner_children;
        words[18] = self.owner_turns;
        words[19] = self.frames_completed;
        words[20] = self.frame_turns;
        words[21] = self.frame_owner_turns;
        let mut index = 0usize;
        while index < self.reserved.len() / core::mem::size_of::<u32>() {
            let byte = index * core::mem::size_of::<u32>();
            words[22 + index] = u32::from_le_bytes([
                self.reserved[byte],
                self.reserved[byte + 1],
                self.reserved[byte + 2],
                self.reserved[byte + 3],
            ]);
            index += 1;
        }
        words[31] = self.committed_publication_sequence;
        words
    }

    /// Decode the fixed little-endian shared-memory word representation.
    #[must_use]
    pub const fn from_le_words(words: [u32; DRIVER_RUNTIME_CYW43_DPC_CLIENT_WORDS]) -> Self {
        let mut reserved = [0; 36];
        let mut index = 0usize;
        while index < reserved.len() / core::mem::size_of::<u32>() {
            let bytes = words[22 + index].to_le_bytes();
            let byte = index * core::mem::size_of::<u32>();
            reserved[byte] = bytes[0];
            reserved[byte + 1] = bytes[1];
            reserved[byte + 2] = bytes[2];
            reserved[byte + 3] = bytes[3];
            index += 1;
        }
        Self {
            magic: words[0],
            version: words[1] as u16,
            len: (words[1] >> 16) as u16,
            publication_sequence: words[2],
            physical_epoch: words[3],
            consumer_sequence: words[4],
            rearms: words[5],
            epoch_errors: words[6],
            sequence_errors: words[7],
            source_samples: words[8],
            source_frame: words[9],
            source_hostmail: words[10],
            source_fc_change: words[11],
            source_fc_state: words[12],
            source_chipactive: words[13],
            source_other: words[14],
            source_spurious: words[15],
            turns: words[16],
            owner_children: words[17],
            owner_turns: words[18],
            frames_completed: words[19],
            frame_turns: words[20],
            frame_owner_turns: words[21],
            reserved,
            committed_publication_sequence: words[31],
        }
    }

    /// Whether every passive body field is internally consistent.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_CYW43_DPC_CLIENT_MAGIC
            || self.version != DRIVER_RUNTIME_CYW43_DPC_CLIENT_VERSION
            || self.len != DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES
            || self.publication_sequence == 0
            || self.physical_epoch == 0
        {
            return false;
        }
        let mut index = 0usize;
        while index < self.reserved.len() {
            if self.reserved[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Return this valid staged body with its publication sequence committed.
    #[must_use]
    pub const fn commit(mut self) -> Self {
        if self.body_valid() {
            self.committed_publication_sequence = self.publication_sequence;
        }
        self
    }

    /// Whether this is one complete, sequence-last committed publication.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid() && self.committed_publication_sequence == self.publication_sequence
    }

    /// Accept two identical, valid samples as one stable client diagnostic.
    ///
    /// Callers must place their platform load/cache barriers between samples.
    #[must_use]
    pub fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first == second && first.valid() {
            Some(first)
        } else {
            None
        }
    }
}

/// Immutable identity which opens one bounded CYW43 bus-service episode.
///
/// This is a construction helper rather than a shared-memory record. The
/// diagnostic producer passes it to
/// [`DriverRuntimeCyw43BusEpisodeRecord::staged`] before adding progress and
/// the typed episode exit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43BusEpisodeStart {
    /// Nonzero publication sequence committed by this record version.
    pub publication_sequence: u32,
    /// Monotonic nonzero bus-service episode identity.
    pub episode_sequence: u32,
    /// CYW43 logical-runtime generation; zero is valid for persistent work.
    pub logical_generation: u32,
    /// Nonzero SDIO physical-owner lifetime epoch.
    pub physical_epoch: u32,
    /// Immutable parent sequence, or zero for a DPC-only episode.
    pub parent_sequence: u32,
    /// Immutable CYW43 parent op, or zero for a DPC-only episode.
    pub parent_op: u16,
    /// One typed `DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_*` value.
    pub cause: u16,
    /// Virtual-counter sample at initial admission.
    pub first_cntvct: u64,
}

/// Passive, bounded summary of one CYW43 bus-service episode.
///
/// The CYW43 runtime clears `committed_publication_sequence`, writes this
/// complete body, cleans both dedicated cache lines, executes the required
/// publication barrier, then repeats `publication_sequence` in the final word.
/// Readers accept only two identical, valid samples. The record reports
/// durable state but never authorizes work, signals another runtime, retries a
/// transfer, or creates a second physical lane.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43BusEpisodeRecord {
    /// Fixed [`DRIVER_RUNTIME_CYW43_BUS_EPISODE_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_CYW43_BUS_EPISODE_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Nonzero publication identity committed in the final word.
    pub publication_sequence: u32,
    /// Nonzero monotonic bus-service episode identity.
    pub episode_sequence: u32,
    /// CYW43 logical-runtime generation; zero is valid for persistent work.
    pub logical_generation: u32,
    /// Nonzero SDIO physical-owner lifetime epoch.
    pub physical_epoch: u32,
    /// Immutable parent sequence, or zero for a DPC-only episode.
    pub parent_sequence: u32,
    /// Immutable CYW43 parent op, or zero for a DPC-only episode.
    pub parent_op: u16,
    /// One typed `DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_*` value.
    pub cause: u16,
    /// Virtual-counter sample at initial admission.
    pub first_cntvct: u64,
    /// Most recent virtual-counter sample represented by this publication.
    pub last_cntvct: u64,
    /// Exact active or terminal SDIO child sequence, when present.
    pub child_sequence: u32,
    /// Typed child terminal completion code, when terminal.
    pub child_code: u16,
    /// Typed child terminal detail, when terminal.
    pub child_detail: u16,
    /// Child terminal result word, when terminal.
    pub child_result: u32,
    /// Engine selected by the exact active or terminal child descriptor.
    pub child_engine: u16,
    /// IRQ contract required by the selected child engine.
    pub child_irq_contract: u16,
    /// Most recent durable DPC sequence observed by the episode.
    pub dpc_sequence: u32,
    /// Count of Function-2 RX-poll progress transitions.
    pub op8_progress: u32,
    /// Count of root-visible RX progress transitions.
    pub rx_progress: u32,
    /// Count of foreground TX progress transitions.
    pub tx_progress: u32,
    /// Durable `DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_*` levels at exit.
    pub final_pending_mask: u32,
    /// One typed `DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_*` value.
    pub exit_reason: u16,
    /// Exit-specific typed detail.
    pub exit_detail: u16,
    /// Exit-specific result word.
    pub exit_result: u32,
    /// Bounded `DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_*` evidence bits.
    pub flags: u32,
    /// Must remain zero so future layouts fail closed.
    pub reserved: [u8; 28],
    /// Sequence-last commit; exactly repeats `publication_sequence`.
    pub committed_publication_sequence: u32,
}

impl DriverRuntimeCyw43BusEpisodeRecord {
    const FLAG_MASK: u32 = DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_CHILD_TERMINAL
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_DPC_OBSERVED
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_OP8_PROGRESS
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_RX_PROGRESS
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_TX_PROGRESS
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_FAULT;
    const PENDING_MASK: u32 = DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_FOREGROUND
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_DPC
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_EXTERNAL_WAIT
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RX
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_TX
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CHILD_ACTIVE
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CHILD_ISSUED_UNKNOWN
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_ACK_PENDING
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CARD_INT_MASKED
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RING_POISON
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_PRIVATE_RX_QUEUE
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_UNACKED_RX_BATCH
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_OP8_ACTIVE
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_OP11_ACTIVE
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_TX_CREDIT_WAIT
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_LOCAL_CONTINUATION
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RECOVERY
        | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_PAIR_RESTART;

    /// Canonical uncommitted empty episode diagnostic.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_CYW43_BUS_EPISODE_MAGIC,
            version: DRIVER_RUNTIME_CYW43_BUS_EPISODE_VERSION,
            len: DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES,
            publication_sequence: 0,
            episode_sequence: 0,
            logical_generation: 0,
            physical_epoch: 0,
            parent_sequence: 0,
            parent_op: 0,
            cause: 0,
            first_cntvct: 0,
            last_cntvct: 0,
            child_sequence: 0,
            child_code: 0,
            child_detail: 0,
            child_result: 0,
            child_engine: DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_NONE,
            child_irq_contract: 0,
            dpc_sequence: 0,
            op8_progress: 0,
            rx_progress: 0,
            tx_progress: 0,
            final_pending_mask: 0,
            exit_reason: DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_ACTIVE,
            exit_detail: 0,
            exit_result: 0,
            flags: 0,
            reserved: [0; 28],
            committed_publication_sequence: 0,
        }
    }

    /// Construct the uncommitted first publication for one immutable episode.
    #[must_use]
    pub const fn staged(start: DriverRuntimeCyw43BusEpisodeStart) -> Self {
        Self {
            publication_sequence: start.publication_sequence,
            episode_sequence: start.episode_sequence,
            logical_generation: start.logical_generation,
            physical_epoch: start.physical_epoch,
            parent_sequence: start.parent_sequence,
            parent_op: start.parent_op,
            cause: start.cause,
            first_cntvct: start.first_cntvct,
            last_cntvct: start.first_cntvct,
            ..Self::empty()
        }
    }

    /// Encode the fixed shared-memory representation as little-endian words.
    ///
    /// This keeps passive readers and host tests on bounded primitive accesses
    /// rather than introducing another typed raw-pointer boundary.
    #[must_use]
    pub const fn to_le_words(self) -> [u32; DRIVER_RUNTIME_CYW43_BUS_EPISODE_WORDS] {
        let mut words = [0; DRIVER_RUNTIME_CYW43_BUS_EPISODE_WORDS];
        words[0] = self.magic;
        words[1] = self.version as u32 | ((self.len as u32) << 16);
        words[2] = self.publication_sequence;
        words[3] = self.episode_sequence;
        words[4] = self.logical_generation;
        words[5] = self.physical_epoch;
        words[6] = self.parent_sequence;
        words[7] = self.parent_op as u32 | ((self.cause as u32) << 16);
        words[8] = self.first_cntvct as u32;
        words[9] = (self.first_cntvct >> 32) as u32;
        words[10] = self.last_cntvct as u32;
        words[11] = (self.last_cntvct >> 32) as u32;
        words[12] = self.child_sequence;
        words[13] = self.child_code as u32 | ((self.child_detail as u32) << 16);
        words[14] = self.child_result;
        words[15] = self.child_engine as u32 | ((self.child_irq_contract as u32) << 16);
        words[16] = self.dpc_sequence;
        words[17] = self.op8_progress;
        words[18] = self.rx_progress;
        words[19] = self.tx_progress;
        words[20] = self.final_pending_mask;
        words[21] = self.exit_reason as u32 | ((self.exit_detail as u32) << 16);
        words[22] = self.exit_result;
        words[23] = self.flags;
        let mut index = 0usize;
        while index < self.reserved.len() / core::mem::size_of::<u32>() {
            let byte = index * core::mem::size_of::<u32>();
            words[24 + index] = u32::from_le_bytes([
                self.reserved[byte],
                self.reserved[byte + 1],
                self.reserved[byte + 2],
                self.reserved[byte + 3],
            ]);
            index += 1;
        }
        words[31] = self.committed_publication_sequence;
        words
    }

    /// Decode the fixed little-endian shared-memory word representation.
    #[must_use]
    pub const fn from_le_words(words: [u32; DRIVER_RUNTIME_CYW43_BUS_EPISODE_WORDS]) -> Self {
        let mut reserved = [0; 28];
        let mut index = 0usize;
        while index < reserved.len() / core::mem::size_of::<u32>() {
            let bytes = words[24 + index].to_le_bytes();
            let byte = index * core::mem::size_of::<u32>();
            reserved[byte] = bytes[0];
            reserved[byte + 1] = bytes[1];
            reserved[byte + 2] = bytes[2];
            reserved[byte + 3] = bytes[3];
            index += 1;
        }
        Self {
            magic: words[0],
            version: words[1] as u16,
            len: (words[1] >> 16) as u16,
            publication_sequence: words[2],
            episode_sequence: words[3],
            logical_generation: words[4],
            physical_epoch: words[5],
            parent_sequence: words[6],
            parent_op: words[7] as u16,
            cause: (words[7] >> 16) as u16,
            first_cntvct: words[8] as u64 | ((words[9] as u64) << 32),
            last_cntvct: words[10] as u64 | ((words[11] as u64) << 32),
            child_sequence: words[12],
            child_code: words[13] as u16,
            child_detail: (words[13] >> 16) as u16,
            child_result: words[14],
            child_engine: words[15] as u16,
            child_irq_contract: (words[15] >> 16) as u16,
            dpc_sequence: words[16],
            op8_progress: words[17],
            rx_progress: words[18],
            tx_progress: words[19],
            final_pending_mask: words[20],
            exit_reason: words[21] as u16,
            exit_detail: (words[21] >> 16) as u16,
            exit_result: words[22],
            flags: words[23],
            reserved,
            committed_publication_sequence: words[31],
        }
    }

    /// Whether every diagnostic body field is internally consistent.
    ///
    /// This deliberately ignores the sequence-last commit field so a producer
    /// can validate a staged body before publishing it.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_CYW43_BUS_EPISODE_MAGIC
            || self.version != DRIVER_RUNTIME_CYW43_BUS_EPISODE_VERSION
            || self.len != DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES
            || self.publication_sequence == 0
            || self.episode_sequence == 0
            || self.physical_epoch == 0
            || self.last_cntvct < self.first_cntvct
            || !Self::cause_valid(self.cause)
            || !Self::parent_identity_valid(self)
            || !Self::exit_valid(self.exit_reason)
            || (self.flags & !Self::FLAG_MASK) != 0
            || (self.final_pending_mask & !Self::PENDING_MASK) != 0
            || !Self::progress_flags_valid(self)
            || !Self::child_identity_valid(self)
            || !Self::exit_fields_valid(self)
        {
            return false;
        }

        let mut index = 0;
        while index < self.reserved.len() {
            if self.reserved[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }

    const fn cause_valid(cause: u16) -> bool {
        cause == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_FOREGROUND
            || cause == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_DPC
            || cause == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_FOREGROUND_AND_DPC
    }

    const fn parent_identity_valid(self) -> bool {
        if self.cause == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_DPC {
            self.parent_sequence == 0 && self.parent_op == 0
        } else {
            self.parent_sequence != 0 && self.parent_op != 0
        }
    }

    const fn exit_valid(exit_reason: u16) -> bool {
        exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_ACTIVE
            || exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_TERMINAL
            || exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_PREWAIT_CHECKPOINT
            || exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_FAIRNESS
            || exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_FAULT
    }

    const fn progress_flags_valid(self) -> bool {
        ((self.dpc_sequence != 0)
            == ((self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_DPC_OBSERVED) != 0))
            && ((self.op8_progress != 0)
                == ((self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_OP8_PROGRESS) != 0))
            && ((self.rx_progress != 0)
                == ((self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_RX_PROGRESS) != 0))
            && ((self.tx_progress != 0)
                == ((self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_TX_PROGRESS) != 0))
    }

    const fn child_identity_valid(self) -> bool {
        if self.child_sequence == 0 {
            self.child_code == 0
                && self.child_detail == 0
                && self.child_result == 0
                && self.child_engine == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_NONE
                && self.child_irq_contract == 0
                && (self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_CHILD_TERMINAL) == 0
        } else if (self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_CHILD_TERMINAL) == 0 {
            self.child_code == 0
                && self.child_detail == 0
                && self.child_result == 0
                && Self::child_transport_valid(self)
        } else {
            self.child_code != 0 && Self::child_transport_valid(self)
        }
    }

    const fn child_transport_valid(self) -> bool {
        if self.child_engine == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_DMA {
            self.child_irq_contract
                == (DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_IRQ158
                    | DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_IRQ116)
        } else if self.child_engine == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_PIO
            || self.child_engine == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_COMMAND
        {
            self.child_irq_contract == DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_IRQ158
        } else {
            false
        }
    }

    const fn exit_fields_valid(self) -> bool {
        let fault_flag = (self.flags & DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_FAULT) != 0;
        if self.exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_ACTIVE {
            self.exit_detail == 0 && self.exit_result == 0 && !fault_flag
        } else if self.exit_reason == DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_FAULT {
            fault_flag
        } else {
            !fault_flag
        }
    }

    /// Return this valid staged body with its publication sequence committed.
    #[must_use]
    pub const fn commit(mut self) -> Self {
        if self.body_valid() {
            self.committed_publication_sequence = self.publication_sequence;
        }
        self
    }

    /// Whether this is one complete, sequence-last committed publication.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid() && self.committed_publication_sequence == self.publication_sequence
    }

    /// Accept two identical, valid samples as one stable episode diagnostic.
    ///
    /// Callers must place their platform load/cache barriers between samples.
    #[must_use]
    pub fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first == second && first.valid() {
            Some(first)
        } else {
            None
        }
    }
}

const _: () = {
    assert!(
        DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_OFFSET + DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES
            <= DRIVER_RUNTIME_RING_FRAME_OFFSET
    );
    assert!(
        core::mem::size_of::<DriverRuntimeCyw43RxQueueState>()
            == DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES as usize
    );
    assert!(core::mem::align_of::<DriverRuntimeCyw43RxQueueState>() == 4);
    assert!(core::mem::size_of::<DriverRuntimeCyw43RxBatchEntry>() == 8);
    assert!(core::mem::align_of::<DriverRuntimeCyw43RxBatchEntry>() == 4);
    assert!(core::mem::size_of::<DriverRuntimeCyw43RxBatchRecord>() == 128);
    assert!(core::mem::align_of::<DriverRuntimeCyw43RxBatchRecord>() == 4);
    assert!(core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, entries) == 24);
    assert!(core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, source_cntvct_lo) == 88);
    assert!(
        core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, first_data_stage_deltas_q11) == 120
    );
    assert!(
        core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, committed_parent_sequence) == 124
    );
    assert!(core::mem::size_of::<DriverRuntimeCyw43RxBatchAck>() == 64);
    assert!(core::mem::align_of::<DriverRuntimeCyw43RxBatchAck>() == 64);
    assert!(
        core::mem::offset_of!(
            DriverRuntimeCyw43RxBatchAck,
            committed_queue_commit_sequence
        ) == 60
    );
    assert!(core::mem::size_of::<DriverRuntimeCyw43BusEpisodeRecord>() == 128);
    assert!(core::mem::align_of::<DriverRuntimeCyw43BusEpisodeRecord>() == 64);
    assert!(
        core::mem::offset_of!(
            DriverRuntimeCyw43BusEpisodeRecord,
            committed_publication_sequence
        ) == 124
    );
    assert!(core::mem::size_of::<DriverRuntimeCyw43DpcClientRecord>() == 128);
    assert!(core::mem::align_of::<DriverRuntimeCyw43DpcClientRecord>() == 64);
    assert!(
        core::mem::offset_of!(
            DriverRuntimeCyw43DpcClientRecord,
            committed_publication_sequence
        ) == 124
    );
};

/// Passive, identity-bound receipt for one USB hub-keyboard old-good replay.
///
/// The isolated USB runtime clears `committed_publication_sequence`, writes the
/// complete body, executes the shared-ring publication barrier, then repeats
/// `publication_sequence` in the final aligned word. Readers accept only two
/// identical valid samples and compare the descriptor and USB-to-PCIe link
/// identities with the current generated contract. This record never grants a
/// turn, signals a task, retries hardware, or replaces end-to-end input proof.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeUsbOldgoodReceipt {
    /// Fixed [`DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_VERSION`].
    pub version: u16,
    /// Exact record bytes.
    pub len: u16,
    /// Generated USB driver-task key.
    pub task_key: u32,
    /// Sealed USB runtime descriptor identity token.
    pub identity_token: u32,
    /// Generated USB-to-PCIe link epoch.
    pub link_epoch: u32,
    /// Sealed USB-to-PCIe link token.
    pub link_token: u32,
    /// Nonzero controller/attach lifecycle represented by this receipt.
    pub lifetime_epoch: u32,
    /// Nonzero monotonic publication identity within this lifecycle.
    pub publication_sequence: u32,
    /// Ordered [`DRIVER_RUNTIME_USB_OLDGOOD_STEP_*`] prefix plus poison bit.
    pub step_mask: u32,
    /// Packed root-port, parent-hub, child-slot, and endpoint topology.
    pub topology: u32,
    /// Exact interrupt-IN transfer generation that produced the first byte.
    pub input_generation: u32,
    /// Sequence-last commit; exactly repeats `publication_sequence`.
    pub committed_publication_sequence: u32,
}

impl DriverRuntimeUsbOldgoodReceipt {
    /// Construct an identity-bound, uncommitted receipt for one new lifecycle.
    #[must_use]
    pub const fn new(
        task_key: u32,
        identity_token: u32,
        link_epoch: u32,
        link_token: u32,
        lifetime_epoch: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_MAGIC,
            version: DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_VERSION,
            len: DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_BYTES,
            task_key,
            identity_token,
            link_epoch,
            link_token,
            lifetime_epoch,
            publication_sequence: 1,
            step_mask: 0,
            topology: 0,
            input_generation: 0,
            committed_publication_sequence: 0,
        }
    }

    /// Canonical byte-zero record before the USB runtime publishes identity.
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            magic: 0,
            version: 0,
            len: 0,
            task_key: 0,
            identity_token: 0,
            link_epoch: 0,
            link_token: 0,
            lifetime_epoch: 0,
            publication_sequence: 0,
            step_mask: 0,
            topology: 0,
            input_generation: 0,
            committed_publication_sequence: 0,
        }
    }

    const fn ordered_prefix_valid(self) -> bool {
        let steps = self.step_mask & DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK;
        steps & steps.wrapping_add(1) == 0
    }

    const fn body_valid(self) -> bool {
        let known_mask =
            DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK | DRIVER_RUNTIME_USB_OLDGOOD_INVALID_ORDER;
        let endpoint_seen = self.step_mask & DRIVER_RUNTIME_USB_OLDGOOD_STEP_HID_ENDPOINT != 0;
        let first_byte_seen = self.step_mask & DRIVER_RUNTIME_USB_OLDGOOD_STEP_FIRST_BYTE != 0;
        self.magic == DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_MAGIC
            && self.version == DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_VERSION
            && self.len == DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_BYTES
            && self.task_key != 0
            && self.identity_token != 0
            && self.link_epoch != 0
            && self.link_token != 0
            && self.lifetime_epoch != 0
            && self.publication_sequence != 0
            && self.step_mask & !known_mask == 0
            && self.ordered_prefix_valid()
            && endpoint_seen == (self.topology != 0)
            && first_byte_seen == (self.input_generation != 0)
    }

    /// Return this valid staged body with its publication sequence committed.
    #[must_use]
    pub const fn commit(mut self) -> Self {
        if self.body_valid() {
            self.committed_publication_sequence = self.publication_sequence;
        }
        self
    }

    /// Whether this is one internally valid, sequence-last publication.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.body_valid() && self.committed_publication_sequence == self.publication_sequence
    }

    /// Whether this publication retains the complete unpoisoned 14-step replay.
    #[must_use]
    pub const fn complete(self) -> bool {
        self.valid()
            && self.step_mask == DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK
            && self.topology != 0
            && self.input_generation != 0
    }

    /// Whether this lifecycle observed a skipped or reordered old-good step.
    #[must_use]
    pub const fn poisoned(self) -> bool {
        self.valid() && self.step_mask & DRIVER_RUNTIME_USB_OLDGOOD_INVALID_ORDER != 0
    }

    /// Accept two identical, valid samples as one stable passive receipt.
    ///
    /// Callers must place their platform load/cache barriers between samples.
    #[must_use]
    pub fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first == second && first.valid() {
            Some(first)
        } else {
            None
        }
    }
}

const _: () = {
    assert!(
        DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_OFFSET + DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_BYTES
            <= DRIVER_RUNTIME_RING_FRAME_OFFSET
    );
    assert!(core::mem::size_of::<DriverRuntimeUsbOldgoodReceipt>() == 48);
    assert!(core::mem::align_of::<DriverRuntimeUsbOldgoodReceipt>() == 4);
    assert!(
        core::mem::offset_of!(
            DriverRuntimeUsbOldgoodReceipt,
            committed_publication_sequence
        ) == 44
    );
};

/// SDIO-owner identity for complete physical WiFi power lifetimes.
///
/// `begun_epoch` is the sequence-last commit word for a new lifetime. The SDIO
/// owner advances it exactly once before the first WL_ON/power-sequence
/// operation. Exactly one of `completed_epoch` or `failed_epoch` may then equal
/// the current begun epoch. Older terminal epochs remain intact so a later
/// successful lifetime does not erase the most recent failure. Higher layers
/// may use a stable completed epoch only to reject stale lifecycle state. This
/// passive record never authorizes or advances a command or continuation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSdioPhysicalLifetimeRecord {
    /// Fixed [`DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_MAGIC`] discriminator.
    pub magic: u32,
    /// Most recent strictly monotonic physical lifetime begun by SDIO.
    pub begun_epoch: u32,
    /// Most recent physical lifetime that reached its ready terminal.
    pub completed_epoch: u32,
    /// Most recent physical lifetime that reached or was fenced as failed.
    pub failed_epoch: u32,
}

impl DriverRuntimeSdioPhysicalLifetimeRecord {
    /// Initialized record before the first physical lifetime.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_MAGIC,
            begun_epoch: 0,
            completed_epoch: 0,
            failed_epoch: 0,
        }
    }

    /// Byte-zero form produced by initial command-ring construction.
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            magic: 0,
            begun_epoch: 0,
            completed_epoch: 0,
            failed_epoch: 0,
        }
    }

    /// Whether this is the byte-zero initial ring form.
    #[must_use]
    pub const fn is_zeroed(self) -> bool {
        self.magic == 0
            && self.begun_epoch == 0
            && self.completed_epoch == 0
            && self.failed_epoch == 0
    }

    /// Whether fields describe one internally consistent owner history.
    #[must_use]
    pub const fn valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_MAGIC {
            return false;
        }
        if self.begun_epoch == 0 {
            return self.completed_epoch == 0 && self.failed_epoch == 0;
        }
        if self.completed_epoch > self.begun_epoch
            || self.failed_epoch > self.begun_epoch
            || (self.completed_epoch != 0 && self.completed_epoch == self.failed_epoch)
        {
            return false;
        }
        true
    }

    /// Whether the most recent begun lifetime has no terminal yet.
    #[must_use]
    pub const fn active(self) -> bool {
        self.valid()
            && self.begun_epoch != 0
            && self.completed_epoch != self.begun_epoch
            && self.failed_epoch != self.begun_epoch
    }

    /// Strictly monotonic next epoch, or `None` after exhausting `u32`.
    #[must_use]
    pub const fn next_epoch(self) -> Option<u32> {
        if !self.valid() || self.begun_epoch == u32::MAX {
            None
        } else {
            Some(self.begun_epoch + 1)
        }
    }

    /// Accept two identical, valid volatile samples as a stable snapshot.
    ///
    /// A byte-zero initial command ring is normalized to [`Self::empty`].
    /// Callers must place their platform load/cache barriers between samples.
    #[must_use]
    pub const fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        let first = if first.is_zeroed() {
            Self::empty()
        } else {
            first
        };
        let second = if second.is_zeroed() {
            Self::empty()
        } else {
            second
        };
        if first.magic == second.magic
            && first.begun_epoch == second.begun_epoch
            && first.completed_epoch == second.completed_epoch
            && first.failed_epoch == second.failed_epoch
            && first.valid()
        {
            Some(first)
        } else {
            None
        }
    }
}

/// Passive SDIO-owner proof of the programmed host clock and negotiated card mode.
///
/// `sequence` is the sequence-last commit word. The SDIO owner publishes zero
/// there, writes the immutable snapshot, and commits the nonzero retained
/// command sequence last. Readers accept only two identical, valid samples.
/// The record is evidence only: it never authorizes or advances bus work.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSdioClockSnapshot {
    /// Fixed [`DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_MAGIC`] discriminator.
    pub magic: u32,
    /// [`DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_VERSION`].
    pub version: u16,
    /// Exact record size in bytes.
    pub len: u16,
    /// Nonzero retained SDIO command sequence committed last by the owner.
    pub sequence: u32,
    /// Completed physical WiFi lifetime that owns this configuration.
    pub physical_lifetime_epoch: u32,
    /// Clock rate requested by the CYW43 client.
    pub requested_clock_hz: u32,
    /// Generated BCM2711 SDIO base-clock truth used for divider selection.
    pub base_clock_hz: u32,
    /// Effective card clock after applying `divider`.
    pub effective_clock_hz: u32,
    /// Generated virtual-counter frequency used for elapsed-time deadlines.
    pub timer_clock_hz: u32,
    /// Decoded SDHCI clock divisor.
    pub divider: u16,
    /// Final SDHCI `CLOCK_CONTROL` register readback.
    pub clock_control: u16,
    /// Final SDHCI `HOST_CONTROL` register readback.
    pub host_control: u8,
    /// Final CCCR `SPEED` register readback when flagged valid.
    pub cccr_speed: u8,
    /// Final CCCR `BUS_INTERFACE_CONTROL` readback when flagged valid.
    pub cccr_interface: u8,
    /// [`Self::FLAG_*`] proof bits.
    pub flags: u8,
    /// Must remain zero.
    pub reserved: u32,
}

impl DriverRuntimeSdioClockSnapshot {
    /// Requested clock is an observed, nonzero client value.
    pub const FLAG_REQUEST_VALID: u8 = 1 << 0;
    /// SDHCI clock/control registers were read back after programming.
    pub const FLAG_CLOCK_READBACK_VALID: u8 = 1 << 1;
    /// SDHCI reported its internal clock stable.
    pub const FLAG_INTERNAL_CLOCK_STABLE: u8 = 1 << 2;
    /// SDHCI reported the card clock enabled.
    pub const FLAG_CARD_CLOCK_ENABLED: u8 = 1 << 3;
    /// The CYW43 client supplied a read-back CCCR high-speed negotiation.
    pub const FLAG_CARD_HIGH_SPEED: u8 = 1 << 4;
    /// SDHCI reported the 4-bit host-width selection.
    pub const FLAG_HOST_WIDTH_4BIT: u8 = 1 << 5;
    /// `cccr_speed` is a read-back CCCR value.
    pub const FLAG_CCCR_SPEED_VALID: u8 = 1 << 6;
    /// `cccr_interface` is a read-back CCCR value.
    pub const FLAG_CCCR_INTERFACE_VALID: u8 = 1 << 7;

    /// SDHCI `CLOCK_CONTROL` internal-clock enable bit.
    pub const CLOCK_CONTROL_INTERNAL_ENABLE: u16 = 1 << 0;
    /// SDHCI `CLOCK_CONTROL` internal-clock stable bit.
    pub const CLOCK_CONTROL_INTERNAL_STABLE: u16 = 1 << 1;
    /// SDHCI `CLOCK_CONTROL` card-clock enable bit.
    pub const CLOCK_CONTROL_CARD_ENABLE: u16 = 1 << 2;
    /// CCCR `SPEED` high-speed enable bit.
    pub const CCCR_SPEED_EHS: u8 = 1 << 1;
    /// CCCR bus-width mask.
    pub const CCCR_INTERFACE_WIDTH_MASK: u8 = 0x03;
    /// CCCR 4-bit bus-width encoding.
    pub const CCCR_INTERFACE_WIDTH_4BIT: u8 = 0x02;

    /// Byte-zero form produced by initial command-ring construction.
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            magic: 0,
            version: 0,
            len: 0,
            sequence: 0,
            physical_lifetime_epoch: 0,
            requested_clock_hz: 0,
            base_clock_hz: 0,
            effective_clock_hz: 0,
            timer_clock_hz: 0,
            divider: 0,
            clock_control: 0,
            host_control: 0,
            cccr_speed: 0,
            cccr_interface: 0,
            flags: 0,
            reserved: 0,
        }
    }

    /// Returns true when this snapshot is complete and internally consistent.
    #[must_use]
    pub const fn valid(self) -> bool {
        let request_valid = self.flags & Self::FLAG_REQUEST_VALID != 0;
        let readback_valid = self.flags & Self::FLAG_CLOCK_READBACK_VALID != 0;
        let stable = self.flags & Self::FLAG_INTERNAL_CLOCK_STABLE != 0;
        let card_enabled = self.flags & Self::FLAG_CARD_CLOCK_ENABLED != 0;
        let card_high_speed = self.flags & Self::FLAG_CARD_HIGH_SPEED != 0;
        let host_width_4bit = self.flags & Self::FLAG_HOST_WIDTH_4BIT != 0;
        let cccr_speed_valid = self.flags & Self::FLAG_CCCR_SPEED_VALID != 0;
        let cccr_interface_valid = self.flags & Self::FLAG_CCCR_INTERFACE_VALID != 0;
        self.magic == DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_MAGIC
            && self.version == DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_VERSION
            && self.len == DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_BYTES
            && self.sequence != 0
            && self.physical_lifetime_epoch != 0
            && request_valid
            && readback_valid
            && self.requested_clock_hz != 0
            && self.base_clock_hz != 0
            && self.effective_clock_hz != 0
            && self.timer_clock_hz != 0
            && self.divider != 0
            && self.effective_clock_hz == self.base_clock_hz / (self.divider as u32)
            && stable == (self.clock_control & Self::CLOCK_CONTROL_INTERNAL_STABLE != 0)
            && card_enabled == (self.clock_control & Self::CLOCK_CONTROL_CARD_ENABLE != 0)
            && (!readback_valid || self.clock_control & Self::CLOCK_CONTROL_INTERNAL_ENABLE != 0)
            && card_high_speed == (cccr_speed_valid && self.cccr_speed & Self::CCCR_SPEED_EHS != 0)
            && host_width_4bit == (self.host_control & 0x02 != 0)
            && (!cccr_speed_valid || self.cccr_speed != 0)
            && (!cccr_interface_valid
                || self.cccr_interface & Self::CCCR_INTERFACE_WIDTH_MASK
                    == Self::CCCR_INTERFACE_WIDTH_4BIT)
            && (!cccr_interface_valid || host_width_4bit)
            && self.reserved == 0
    }

    /// Whether this snapshot proves the final high-speed, 4-bit Gate 4 state.
    #[must_use]
    pub const fn gate4_ready(self) -> bool {
        self.valid()
            && self.flags & Self::FLAG_INTERNAL_CLOCK_STABLE != 0
            && self.flags & Self::FLAG_CARD_CLOCK_ENABLED != 0
            && self.flags & Self::FLAG_CARD_HIGH_SPEED != 0
            && self.flags & Self::FLAG_HOST_WIDTH_4BIT != 0
            && self.flags & Self::FLAG_CCCR_SPEED_VALID != 0
            && self.flags & Self::FLAG_CCCR_INTERFACE_VALID != 0
    }

    /// Accept two identical, valid volatile samples as one stable snapshot.
    #[must_use]
    pub const fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first.magic == second.magic
            && first.version == second.version
            && first.len == second.len
            && first.sequence == second.sequence
            && first.physical_lifetime_epoch == second.physical_lifetime_epoch
            && first.requested_clock_hz == second.requested_clock_hz
            && first.base_clock_hz == second.base_clock_hz
            && first.effective_clock_hz == second.effective_clock_hz
            && first.timer_clock_hz == second.timer_clock_hz
            && first.divider == second.divider
            && first.clock_control == second.clock_control
            && first.host_control == second.host_control
            && first.cccr_speed == second.cccr_speed
            && first.cccr_interface == second.cccr_interface
            && first.flags == second.flags
            && first.reserved == second.reserved
            && first.valid()
        {
            Some(first)
        } else {
            None
        }
    }
}

/// Durable fault-containment deadline for one exact issued SDIO request.
///
/// The isolated SDIO owner writes the body while
/// `committed_request_sequence` is zero, cleans and orders those fields, then
/// commits the immutable request sequence last. Root may use an expired stable
/// snapshot only to prompt the existing CYW43-to-SDIO condition recheck; the
/// notification carries no authority and the SDIO owner remains the sole
/// terminal decision-maker. The owner clears the commit word before exposing
/// a terminal or resetting the request cursor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSdioDeadlineArm {
    /// Completed physical WiFi lifetime that owns this request.
    pub physical_lifetime_epoch: u32,
    /// Immutable retained SDIO request sequence.
    pub request_sequence: u32,
    /// Low 32 bits of the absolute `CNTVCT_EL0` expiry tick.
    pub expiry_ticks_lo: u32,
    /// High 32 bits of the absolute `CNTVCT_EL0` expiry tick.
    pub expiry_ticks_hi: u32,
    /// Sequence-last commit; must equal `request_sequence` when visible.
    pub committed_request_sequence: u32,
}

impl DriverRuntimeSdioDeadlineArm {
    /// Empty or explicitly cleared form.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            physical_lifetime_epoch: 0,
            request_sequence: 0,
            expiry_ticks_lo: 0,
            expiry_ticks_hi: 0,
            committed_request_sequence: 0,
        }
    }

    /// Staged body before the sequence-last commit.
    #[must_use]
    pub const fn staged(
        physical_lifetime_epoch: u32,
        request_sequence: u32,
        expiry_ticks: u64,
    ) -> Self {
        Self {
            physical_lifetime_epoch,
            request_sequence,
            expiry_ticks_lo: expiry_ticks as u32,
            expiry_ticks_hi: (expiry_ticks >> 32) as u32,
            committed_request_sequence: 0,
        }
    }

    /// Return the immutable absolute expiry tick.
    #[must_use]
    pub const fn expiry_ticks(self) -> u64 {
        (self.expiry_ticks_hi as u64) << 32 | self.expiry_ticks_lo as u64
    }

    /// Whether the staged body is complete before commit.
    #[must_use]
    pub const fn body_valid(self) -> bool {
        self.physical_lifetime_epoch != 0
            && self.request_sequence != 0
            && self.committed_request_sequence == 0
    }

    /// Return the sequence-last committed form of a valid staged body.
    #[must_use]
    pub const fn commit(mut self) -> Option<Self> {
        if !self.body_valid() {
            return None;
        }
        self.committed_request_sequence = self.request_sequence;
        Some(self)
    }

    /// Whether this is one complete committed deadline identity.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.physical_lifetime_epoch != 0
            && self.request_sequence != 0
            && self.committed_request_sequence == self.request_sequence
    }

    /// Accept only two identical, complete volatile samples.
    #[must_use]
    pub const fn stable_snapshot(first: Self, second: Self) -> Option<Self> {
        if first.physical_lifetime_epoch == second.physical_lifetime_epoch
            && first.request_sequence == second.request_sequence
            && first.expiry_ticks_lo == second.expiry_ticks_lo
            && first.expiry_ticks_hi == second.expiry_ticks_hi
            && first.committed_request_sequence == second.committed_request_sequence
            && first.valid()
        {
            Some(first)
        } else {
            None
        }
    }
}

/// Durable authority for exactly one retained-command continuation quantum.
///
/// `grant_id` is the sequence-last commit word. Producers publish zero there,
/// then the immutable request identity, and finally a nonzero monotonically
/// increasing ID. For every exact grant the consumer publishes
/// `grant_id | ACTION_ADMITTED_BIT` after exact ACK-before-I/O admission, then
/// the unmodified `grant_id` only after its one bounded outer action completes.
/// Producers re-signal only an unadmitted ID and never overwrite an admitted
/// action.
/// Consumers accept the record only when both reads of the ID match and its
/// request/fingerprint/generation match the retained command. A notification
/// is therefore only a wake hint; its coalesced badge cannot create, duplicate,
/// or mutate foreground authority.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeContinuationGrant {
    /// Fixed [`DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC`] discriminator.
    pub magic: u32,
    /// Immutable retained command sequence.
    pub request_sequence: u32,
    /// Fingerprint of every command action field except the sequence.
    pub action_fingerprint: u32,
    /// Producer-owned runtime generation for stale-grant rejection.
    pub generation: u32,
    /// Nonzero sequence-last grant commit ID.
    pub grant_id: u32,
    /// Consumer-published admission/completion frontier for this grant.
    pub consumed_grant_id: u32,
}

impl DriverRuntimeContinuationGrant {
    /// Return a byte-zero, uncommitted grant record.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: 0,
            request_sequence: 0,
            action_fingerprint: 0,
            generation: 0,
            grant_id: 0,
            consumed_grant_id: 0,
        }
    }

    /// Build one immutable grant whose ID is published last by the producer.
    #[must_use]
    pub const fn new(
        request_sequence: u32,
        action_fingerprint: u32,
        generation: u32,
        grant_id: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC,
            request_sequence,
            action_fingerprint,
            generation,
            grant_id,
            consumed_grant_id: 0,
        }
    }
}

/// Exact monotonic owner-service heartbeat for one grant-free command.
///
/// `committed_slice` is published last. Readers accept the record only when
/// two reads of that word agree, it equals `service_slice`, and the complete
/// request sequence/fingerprint/generation identity matches their immutable
/// retained parent or child ticket. A newer slice proves one exact changed
/// wait, interrupt, controller/data, or pending-parent child-terminal frontier.
/// It is diagnostic and does not renew physical authority: the command's
/// independent deadline bounds total authority, and rewriting the slot cannot
/// extend another command's inactivity fence.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSteadyServiceProgress {
    /// Fixed [`DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC`] discriminator.
    pub magic: u32,
    /// Immutable retained command sequence.
    pub request_sequence: u32,
    /// Fingerprint of every command action field except the sequence.
    pub action_fingerprint: u32,
    /// Producer-owned runtime generation for stale-progress rejection.
    pub generation: u32,
    /// Nonzero monotonically increasing completed service-slice count.
    pub service_slice: u32,
    /// Sequence-last commit copy of [`Self::service_slice`].
    pub committed_slice: u32,
}

impl DriverRuntimeSteadyServiceProgress {
    /// Return a byte-zero, uncommitted progress record.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: 0,
            request_sequence: 0,
            action_fingerprint: 0,
            generation: 0,
            service_slice: 0,
            committed_slice: 0,
        }
    }

    /// Build one exact record whose commit copy is published last.
    #[must_use]
    pub const fn new(
        request_sequence: u32,
        action_fingerprint: u32,
        generation: u32,
        service_slice: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC,
            request_sequence,
            action_fingerprint,
            generation,
            service_slice,
            committed_slice: service_slice,
        }
    }

    /// Return whether the record is structurally committed.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC
            && self.request_sequence != 0
            && self.generation != 0
            && self.service_slice != 0
            && self.committed_slice == self.service_slice
    }
}

/// Exact receipt that one generic MCS one-way runtime armed its next wait.
///
/// The child publishes this record only after one bounded command quantum is
/// still pending and after its final durable-condition recheck. Root accepts
/// it only while the same request, action fingerprint, runtime identity, ring,
/// and MCS capability generation remain live. `committed_wait_slice` is
/// published last and cleared by the child before interpreting the matching
/// prompt. The receipt grants no physical action and renews no deadline; it
/// proves only that one coalescing reserved-root scheduling hint is due.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeOneWayWaitReceipt {
    /// Fixed [`DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC`] discriminator.
    pub magic: u32,
    /// Immutable retained command sequence.
    pub request_sequence: u32,
    /// Fingerprint of every command action field except the sequence.
    pub action_fingerprint: u32,
    /// Sealed runtime descriptor identity for cross-runtime rejection.
    pub runtime_identity_token: u32,
    /// Nonzero monotonically increasing wait slice for this exact command.
    pub wait_slice: u32,
    /// Sequence-last commit copy of [`Self::wait_slice`].
    pub committed_wait_slice: u32,
}

impl DriverRuntimeOneWayWaitReceipt {
    /// Return a byte-zero, uncommitted wait receipt.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: 0,
            request_sequence: 0,
            action_fingerprint: 0,
            runtime_identity_token: 0,
            wait_slice: 0,
            committed_wait_slice: 0,
        }
    }

    /// Build one exact receipt whose commit copy is published last.
    #[must_use]
    pub const fn new(
        request_sequence: u32,
        action_fingerprint: u32,
        runtime_identity_token: u32,
        wait_slice: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC,
            request_sequence,
            action_fingerprint,
            runtime_identity_token,
            wait_slice,
            committed_wait_slice: wait_slice,
        }
    }

    /// Return whether the sequence-last record is structurally committed.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC
            && self.request_sequence != 0
            && self.action_fingerprint != 0
            && self.runtime_identity_token != 0
            && self.wait_slice != 0
            && self.committed_wait_slice == self.wait_slice
    }

    /// Return whether root durably acknowledged this exact wait slice.
    #[must_use]
    pub const fn acknowledged(self) -> bool {
        self.magic == DRIVER_RUNTIME_ONE_WAY_WAIT_ACK_MAGIC
            && self.request_sequence != 0
            && self.action_fingerprint != 0
            && self.runtime_identity_token != 0
            && self.wait_slice != 0
            && self.committed_wait_slice == self.wait_slice
    }
}

/// Exact receipt that one persistent op11 owner armed its external wait.
///
/// The CYW43 runtime publishes this record only after exhausting deterministic
/// local work and rechecking every durable parent, child, DPC, queue, credit,
/// and terminal condition. `committed_wait_epoch` is published last. Root may
/// use a stable exact-identity receipt only to release prompt pre-wait
/// scheduling adjacency; it grants no operation, wake, retry, or deadline
/// renewal. The runtime clears the commit before re-entering runnable owner
/// work or publishing the parent terminal.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimePersistentWaitReceipt {
    /// Fixed [`DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC`] discriminator.
    pub magic: u32,
    /// Immutable persistent parent command sequence.
    pub request_sequence: u32,
    /// Fingerprint of every command action field except the sequence.
    pub action_fingerprint: u32,
    /// Root-owned logical connection generation; zero is a valid bootstrap value.
    pub logical_generation: u32,
    /// Nonzero monotonic wait epoch for this exact parent.
    pub wait_epoch: u32,
    /// Sequence-last commit copy of [`Self::wait_epoch`].
    pub committed_wait_epoch: u32,
}

impl DriverRuntimePersistentWaitReceipt {
    /// Return a byte-zero, uncommitted wait receipt.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: 0,
            request_sequence: 0,
            action_fingerprint: 0,
            logical_generation: 0,
            wait_epoch: 0,
            committed_wait_epoch: 0,
        }
    }

    /// Build one exact receipt whose commit copy is published last.
    #[must_use]
    pub const fn new(
        request_sequence: u32,
        action_fingerprint: u32,
        logical_generation: u32,
        wait_epoch: u32,
    ) -> Self {
        Self {
            magic: DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC,
            request_sequence,
            action_fingerprint,
            logical_generation,
            wait_epoch,
            committed_wait_epoch: wait_epoch,
        }
    }

    /// Return whether the sequence-last record is structurally committed.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC
            && self.request_sequence != 0
            && self.action_fingerprint != 0
            && self.wait_epoch != 0
            && self.committed_wait_epoch == self.wait_epoch
    }
}

const fn driver_runtime_continuation_fingerprint_mix(mut hash: u32, value: u32) -> u32 {
    hash ^= value;
    hash = hash.wrapping_mul(16_777_619);
    hash
}

/// Fixed network-role bit used by linked runtime command routing.
pub const DRIVER_RUNTIME_ROLE_NET: u32 = 1 << 3;

/// Return whether primitive command routing selects the root-owned CYW43 lane.
///
/// This shared predicate prevents HAL and runtime transport selection from
/// drifting into different fallback behavior. Sequence numbers and logical
/// generation values are intentionally excluded: the local CYW43 ring has its
/// own request namespace, and generation zero is the valid bootstrap epoch.
/// The immutable continuation grant binds both fields separately.
#[must_use]
pub const fn driver_runtime_is_cyw43_root_continuation(
    hot_path: u32,
    role: u32,
    aux0: u32,
) -> bool {
    hot_path == HOT_PATH_CYW43_WIFI
        && role == DRIVER_RUNTIME_ROLE_NET
        && aux0 == DRIVER_RUNTIME_CYW43_COMMAND_AUX
}

/// Fingerprint every immutable action field in a fixed runtime command.
///
/// The request sequence is carried independently in
/// [`DriverRuntimeContinuationGrant`]. The linked generation is also repeated
/// explicitly there while remaining part of the complete action fingerprint
/// through `aux1`. Payload bytes remain protected by the already-retained
/// command/staging ticket; a continuation can only advance that intake and can
/// never republish a payload.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub const fn driver_runtime_continuation_action_fingerprint(
    opcode: u16,
    flags: u16,
    arg0: u32,
    arg1: u32,
    aux0: u32,
    aux1: u32,
    max_ops: u16,
    max_frames: u16,
    max_bytes: u32,
    frame_offset: u32,
    frame_len: u16,
    frame_flags: u16,
) -> u32 {
    let mut hash = 2_166_136_261u32;
    hash = driver_runtime_continuation_fingerprint_mix(hash, opcode as u32);
    hash = driver_runtime_continuation_fingerprint_mix(hash, flags as u32);
    hash = driver_runtime_continuation_fingerprint_mix(hash, arg0);
    hash = driver_runtime_continuation_fingerprint_mix(hash, arg1);
    hash = driver_runtime_continuation_fingerprint_mix(hash, aux0);
    hash = driver_runtime_continuation_fingerprint_mix(hash, aux1);
    hash = driver_runtime_continuation_fingerprint_mix(hash, max_ops as u32);
    hash = driver_runtime_continuation_fingerprint_mix(hash, max_frames as u32);
    hash = driver_runtime_continuation_fingerprint_mix(hash, max_bytes);
    hash = driver_runtime_continuation_fingerprint_mix(hash, frame_offset);
    hash = driver_runtime_continuation_fingerprint_mix(hash, frame_len as u32);
    hash = driver_runtime_continuation_fingerprint_mix(hash, frame_flags as u32);
    hash | 1
}
/// Level-sensitive runtime IRQ trigger tag.
pub const DRIVER_RUNTIME_IRQ_TRIGGER_LEVEL: u16 = 0;
/// Edge-sensitive runtime IRQ trigger tag.
pub const DRIVER_RUNTIME_IRQ_TRIGGER_EDGE: u16 = 1;
/// Child CSpace slot where USB receives the PCIe/VL805 bus-owner endpoint cap.
pub const DRIVER_RUNTIME_BUS_LINK_PCIE_ENDPOINT_SLOT: u32 = 9;
/// CYW43 CSpace slot containing the send-only SDIO-owner notification cap.
pub const DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT: u32 = 8;
/// Child CSpace slot containing GENET's send-only console-network peer wake.
///
/// Slot 8 is role-local: GENET cannot also participate in the CYW43/SDIO bus
/// link, so this authority never aliases a live cap in the same child.
pub const DRIVER_RUNTIME_CHILD_DIRECT_GENET_PEER_NOTIFICATION_SLOT: u32 = 8;
/// Concise alias carried in [`DriverRuntimeDirectGenetDescriptor`].
pub const DRIVER_RUNTIME_DIRECT_GENET_PEER_NOTIFICATION_SLOT: u32 =
    DRIVER_RUNTIME_CHILD_DIRECT_GENET_PEER_NOTIFICATION_SLOT;
/// SDIO-owner CSpace slot containing the send-only CYW43 notification cap.
pub const DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT: u32 = 10;
/// USB-local virtual address where root maps the PCIe owner command ring.
pub const DRIVER_RUNTIME_BUS_LINK_PCIE_RING_VADDR: u64 = 0x70e0_1000;
/// CYW43-local virtual address where root maps the SDIO owner command ring.
pub const DRIVER_RUNTIME_BUS_LINK_SDIO_RING_VADDR: u64 = 0x70e0_0000;
/// Command flag: root delivered this turn with send-only IPC and expects no reply cap.
pub const DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY: u16 = 1 << 13;
/// Command flag: this immutable, fingerprinted CYW43 command authorizes one
/// complete persistent transaction.
///
/// Root may mint this flag only for a valid staged
/// [`DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE`] descriptor. The command
/// sequence and generation remain the sole parent identity; notifications are
/// scheduling hints and no recurrent continuation grant is part of this
/// authority.
pub const DRIVER_RUNTIME_COMMAND_FLAG_PERSISTENT_TRANSACTION: u16 = 1 << 10;
/// Command flag: this immutable, fingerprinted command carries typed authority
/// for one finite steady-data-plane service lease.
///
/// The producer must pair this with the role-specific descriptor marker. The
/// linked runtimes propagate it from an admitted urgent CYW43 parent to its
/// exact SDIO child; a descriptor marker alone grants no continuation power.
pub const DRIVER_RUNTIME_COMMAND_FLAG_STEADY_SERVICE_LEASE: u16 = 1 << 11;

/// Resource range kind: memory-mapped device registers.
pub const DRIVER_RUNTIME_RESOURCE_KIND_MMIO: u16 = 1;
/// Resource range kind: runtime-owned DMA pages.
pub const DRIVER_RUNTIME_RESOURCE_KIND_DMA: u16 = 2;
/// Resource range kind: root/runtime shared pages outside the command ring.
pub const DRIVER_RUNTIME_RESOURCE_KIND_SHARED: u16 = 3;
/// Resource range kind: HDMI framebuffer aperture.
pub const DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER: u16 = 4;

/// Resource range flag: virtual addresses are contiguous in the runtime.
pub const DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS: u16 = 1 << 0;
/// Resource range flag: physical addresses are contiguous.
pub const DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS: u16 = 1 << 1;
/// Resource range flag: physical addresses are device-visible bus addresses.
pub const DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE: u16 = 1 << 2;
/// Resource range flag: pages are also intentionally visible to root.
pub const DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED: u16 = 1 << 3;
/// Resource range flag: pages are CPU-only and cannot back device DMA.
pub const DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY: u16 = 1 << 4;
/// Complete allowed resource-range flag set for ABI v13.
pub const DRIVER_RUNTIME_RESOURCE_ALLOWED_FLAGS: u16 = DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
    | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
    | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE
    | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED
    | DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY;

/// Generic runtime buffer tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_GENERIC: u32 = 0;
/// Mini-UART MMIO tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_SERIAL_MINI_UART: u32 = 1;
/// VL805/xHCI MMIO tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI: u32 = 2;
/// HDMI control-register MMIO tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_HDMI_REGS: u32 = 3;
/// HDMI framebuffer tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER: u32 = 4;
/// BCM GENET register MMIO tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS: u32 = 5;
/// CYW43 firmware/control buffer tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL: u32 = 6;
/// SDHCI/SDIO host MMIO tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST: u32 = 7;
/// BCM2711 PCIe host bridge MMIO tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST: u32 = 8;
/// Generic driver-local DMA arena tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA: u32 = 9;
/// Generic root/runtime shared control buffer tag.
pub const DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL: u32 = 10;
/// Pi 4 firmware-mailbox aperture used only by the SDIO owner for the
/// manifest-declared CYW43 WL_ON power sequence.
pub const DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ: u32 = 11;
/// Low, uncached, runtime-private request page for the Pi firmware mailbox.
pub const DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST: u32 = 12;
/// BCM2711 BCM2835 DMA-controller MMIO owned by the linked SDIO runtime.
pub const DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA: u32 = 13;
/// CPU-only packet pages shared exclusively by GENET and console-network.
pub const DRIVER_RUNTIME_RESOURCE_TAG_GENET_DIRECT_LINK: u32 = 14;
/// Isolated PCIe runtime's sole BCM system-timer channel-3 MMIO page.
pub const DRIVER_RUNTIME_RESOURCE_TAG_PI4_SYSTEM_TIMER: u32 = 15;

/// Direct GENET link contract: pages are never device-visible DMA buffers.
pub const DRIVER_RUNTIME_DIRECT_GENET_FLAG_CPU_ONLY: u32 = 1 << 0;
/// Direct GENET link contract: the shared pages are reused only after bootstrap.
pub const DRIVER_RUNTIME_DIRECT_GENET_FLAG_POST_BOOTSTRAP_REUSE: u32 = 1 << 1;
/// Direct GENET link contract: both peers receive bounded notification hints.
pub const DRIVER_RUNTIME_DIRECT_GENET_FLAG_PEER_NOTIFICATIONS: u32 = 1 << 2;
/// Exact fixed flags admitted for the direct GENET link.
pub const DRIVER_RUNTIME_DIRECT_GENET_REQUIRED_FLAGS: u32 =
    DRIVER_RUNTIME_DIRECT_GENET_FLAG_CPU_ONLY
        | DRIVER_RUNTIME_DIRECT_GENET_FLAG_POST_BOOTSTRAP_REUSE
        | DRIVER_RUNTIME_DIRECT_GENET_FLAG_PEER_NOTIFICATIONS;
/// Exact CPU-only page population: control + 15 RX + 16 TX pages.
pub const DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT: u16 = 32;
/// Exact base-page bytes used by every direct GENET page.
pub const DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES: u16 = 4096;

/// Fixed pointer-free direct GENET capability and layout descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeDirectGenetDescriptor {
    /// Exact [`DRIVER_RUNTIME_DIRECT_GENET_REQUIRED_FLAGS`] set.
    pub flags: u32,
    /// Exact [`DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT`].
    pub shared_page_count: u16,
    /// Exact [`DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES`].
    pub page_bytes: u16,
    /// GENET child slot containing its send-only console-network wake cap.
    pub peer_notification_slot: u32,
    /// Badge delivered to GENET when console-network signals durable TX work.
    pub peer_notification_badge: u32,
}

impl DriverRuntimeDirectGenetDescriptor {
    /// Absent direct-link authority.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            flags: 0,
            shared_page_count: 0,
            page_bytes: 0,
            peer_notification_slot: 0,
            peer_notification_badge: 0,
        }
    }

    /// Exact direct-link authority admitted to the isolated GENET runtime.
    #[must_use]
    pub const fn exact() -> Self {
        Self {
            flags: DRIVER_RUNTIME_DIRECT_GENET_REQUIRED_FLAGS,
            shared_page_count: DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT,
            page_bytes: DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES,
            peer_notification_slot: DRIVER_RUNTIME_DIRECT_GENET_PEER_NOTIFICATION_SLOT,
            peer_notification_badge: DRIVER_RUNTIME_DIRECT_GENET_NOTIFICATION_BADGE,
        }
    }

    /// Whether every fixed authority and layout field is exact.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.flags == DRIVER_RUNTIME_DIRECT_GENET_REQUIRED_FLAGS
            && self.shared_page_count == DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT
            && self.page_bytes == DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES
            && self.peer_notification_slot == DRIVER_RUNTIME_DIRECT_GENET_PEER_NOTIFICATION_SLOT
            && self.peer_notification_badge == DRIVER_RUNTIME_DIRECT_GENET_NOTIFICATION_BADGE
    }

    /// Whether no direct-link authority is present.
    #[must_use]
    pub const fn empty_valid(self) -> bool {
        self.flags == 0
            && self.shared_page_count == 0
            && self.page_bytes == 0
            && self.peer_notification_slot == 0
            && self.peer_notification_badge == 0
    }
}

/// Bus link flag: child runtime issues requests to the linked bus owner.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT: u32 = 1 << 0;
/// Bus link flag: channel carries only pointer-free ring offsets/lengths.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE: u32 = 1 << 1;
/// Bus link flag: this descriptor is the bus-owner side of a reciprocal link.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER: u32 = 1 << 2;
/// Bus link flag: the linked DPC path includes bounded notification delivery.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS: u32 = 1 << 3;
/// Bus link flag: the owner publishes events through the bounded DPC event ring.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING: u32 = 1 << 4;
/// Bus link channel id for USB using the PCIe/VL805 owner.
pub const DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE: u32 = 1;
/// Bus link channel id for CYW43 using the SDIO owner.
pub const DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO: u32 = 2;
/// Magic value for the CYW43/SDIO bounded DPC event ring.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_MAGIC: u32 = 0x4450_4352;
/// CYW43/SDIO bounded DPC event-ring layout version.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_VERSION: u16 = 3;
/// Fixed metadata offset for the DPC event ring in the owner command page.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET: u16 = 160;
/// Fixed bytes reserved for the DPC event ring before the payload window.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_BYTES: u16 = 96;
const _: () = {
    assert!(
        DRIVER_RUNTIME_RING_PROGRESS_OFFSET + DRIVER_RUNTIME_RING_PROGRESS_BYTES
            == DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_OFFSET
    );
    assert!(
        DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_OFFSET + DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_BYTES
            == DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET
    );
    assert!(core::mem::size_of::<DriverRuntimeSdioPhysicalLifetimeRecord>() == 16);
    assert!(core::mem::align_of::<DriverRuntimeSdioPhysicalLifetimeRecord>() == 4);
    assert!(core::mem::size_of::<DriverRuntimeSdioClockSnapshot>() == 44);
    assert!(core::mem::align_of::<DriverRuntimeSdioClockSnapshot>() == 4);
    assert!(DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET.is_multiple_of(64));
    assert!(
        DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET + 64
            <= DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET
    );
    assert!(
        DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET + DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_BYTES
            == DRIVER_RUNTIME_SDIO_DEADLINE_ARM_OFFSET
    );
    assert!(
        DRIVER_RUNTIME_SDIO_DEADLINE_ARM_OFFSET + DRIVER_RUNTIME_SDIO_DEADLINE_ARM_BYTES
            == DRIVER_RUNTIME_CYW43_SDPCM_TX_FRAME_OFFSET
    );
    assert!(core::mem::size_of::<DriverRuntimeSdioDeadlineArm>() == 20);
    assert!(core::mem::align_of::<DriverRuntimeSdioDeadlineArm>() == 4);
    assert!(core::mem::offset_of!(DriverRuntimeSdioDeadlineArm, committed_request_sequence) == 16);
};
/// Fixed number of producer entries in the bounded DPC event ring.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH: usize = 4;
/// DPC event-ring flag: the producer observed bounded overflow pressure.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OVERRUN: u32 = 1 << 0;
/// DPC event-ring flag: the SDIO owner must retry the seL4 IRQ acknowledgement.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_ACK_PENDING: u32 = 1 << 1;
/// DPC event-ring flag: the current runtime generation is poisoned and must recover.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_POISONED: u32 = 1 << 2;
/// DPC event-ring flag: SDHCI `CARD_INT` signalling is currently masked.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_CARD_IRQ_MASKED: u32 = 1 << 3;
/// DPC event-ring flag: the SDIO owner has admitted the current physical epoch.
///
/// This is durable owner state, not a consumable scheduling edge. The sole SDIO
/// owner sets it during exact `DPC_ACTIVATE` after admitting the generation-long
/// physical service state and clears it whenever that lifetime is poisoned or
/// reset; the child terminal remains separate completion proof.
pub const DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OWNER_ACTIVE: u32 = 1 << 4;
/// Complete set of flags admitted by [`DriverRuntimeDpcEventRing::valid`].
pub const DRIVER_RUNTIME_DPC_EVENT_RING_KNOWN_FLAGS: u32 =
    DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OVERRUN
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_ACK_PENDING
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_POISONED
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_CARD_IRQ_MASKED
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OWNER_ACTIVE;
/// DPC event flag: the SDHCI host reported a card interrupt.
pub const DRIVER_RUNTIME_DPC_EVENT_FLAG_CARD_INTERRUPT: u16 = 1 << 0;
/// DPC event flag: the producer retained a level source for another service turn.
pub const DRIVER_RUNTIME_DPC_EVENT_FLAG_SOURCE_PENDING: u16 = 1 << 1;

/// Runtime hot-path ids. These mirror the root-task command ABI.
pub const HOT_PATH_SERIAL_CONSOLE: u32 = 1;
/// USB keyboard hot-path id.
pub const HOT_PATH_USB_KEYBOARD: u32 = 2;
/// HDMI text/framebuffer hot-path id.
pub const HOT_PATH_HDMI_TEXT: u32 = 3;
/// GENET NIC hot-path id.
pub const HOT_PATH_GENET_NIC: u32 = 4;
/// CYW43 Wi-Fi hot-path id.
pub const HOT_PATH_CYW43_WIFI: u32 = 5;
/// SDIO host hot-path id.
pub const HOT_PATH_SDIO_HOST: u32 = 6;
/// PCIe root hot-path id.
pub const HOT_PATH_PCIE_ROOT: u32 = 7;

/// Magic value for fixed-layout runtime counter snapshots.
pub const DRIVER_RUNTIME_COUNTER_MAGIC: u32 = 0x4452_4354;
/// Runtime counter snapshot layout version.
pub const DRIVER_RUNTIME_COUNTER_VERSION: u16 = 1;
/// Counter snapshot was produced by root-side ring bookkeeping.
pub const DRIVER_RUNTIME_COUNTER_FLAG_ROOT_SNAPSHOT: u32 = 1 << 0;
/// Counter snapshot was produced by linked-runtime-local bookkeeping.
pub const DRIVER_RUNTIME_COUNTER_FLAG_RUNTIME_SNAPSHOT: u32 = 1 << 1;

/// GENET completion result carries role-specific diagnostic metadata.
///
/// Bits 0..11 preserve the TX descriptor window previously reported by the
/// runtime. The remaining fields expose bounded RX backlog state so benchmark
/// evidence can split runtime backlog from root/smoltcp backlog without adding
/// a new command surface.
pub const DRIVER_RUNTIME_GENET_RESULT_PACKED: u32 = 1 << 31;
/// GENET completion-result TX in-flight bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_TX_IN_FLIGHT_SHIFT: u32 = 0;
/// GENET completion-result TX free bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_TX_FREE_SHIFT: u32 = 6;
/// GENET completion-result RX runtime queue-count bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_RX_QUEUE_COUNT_SHIFT: u32 = 12;
/// GENET completion-result RX runtime queue high-water bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_RX_QUEUE_HIGH_WATER_SHIFT: u32 = 17;
/// GENET completion-result RX max-drained-per-turn bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_RX_MAX_DRAIN_SHIFT: u32 = 22;
/// GENET completion-result RX drain-budget-hit flag bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_RX_DRAIN_HIT_SHIFT: u32 = 27;
/// GENET completion-result RX byte-budget-hit flag bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_RX_BYTE_HIT_SHIFT: u32 = 28;
/// GENET completion-result RX runtime overflow-seen flag bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_RX_OVERFLOW_SHIFT: u32 = 29;
/// GENET completion-result same-owner command RX drain-seen flag bit shift.
pub const DRIVER_RUNTIME_GENET_RESULT_COMMAND_RX_DRAIN_SEEN_SHIFT: u32 = 30;
/// GENET completion-result six-bit field mask.
pub const DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK: u32 = 0x3f;
/// GENET completion-result five-bit field mask.
pub const DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK: u32 = 0x1f;

const fn driver_runtime_genet_result_clamp(value: u32, mask: u32) -> u32 {
    if value > mask {
        mask
    } else {
        value
    }
}

/// Role-specific GENET completion diagnostics before bit-packing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeGenetCompletionResultParts {
    /// Free TX descriptors reported by the runtime.
    pub tx_free: u16,
    /// TX descriptors currently in flight.
    pub tx_in_flight: u16,
    /// Current runtime RX queue depth.
    pub rx_queue_count: u8,
    /// Runtime RX queue high-water depth.
    pub rx_queue_high_water: u8,
    /// Maximum RX frames drained by one runtime service turn.
    pub rx_max_drained_per_turn: u8,
    /// Whether any runtime service turn hit the RX drain-count budget.
    pub rx_drain_budget_hit: bool,
    /// Whether any runtime service turn hit the RX byte budget.
    pub rx_byte_budget_hit: bool,
    /// Whether the runtime RX queue overflowed.
    pub rx_overflow_seen: bool,
    /// Whether an admitted RX command drained at least one durable frame.
    pub command_rx_drain_seen: bool,
}

/// Pack role-specific GENET completion diagnostics into a primitive result.
#[must_use]
pub const fn driver_runtime_genet_completion_result(
    parts: DriverRuntimeGenetCompletionResultParts,
) -> u32 {
    DRIVER_RUNTIME_GENET_RESULT_PACKED
        | (driver_runtime_genet_result_clamp(
            parts.tx_in_flight as u32,
            DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK,
        ) << DRIVER_RUNTIME_GENET_RESULT_TX_IN_FLIGHT_SHIFT)
        | (driver_runtime_genet_result_clamp(
            parts.tx_free as u32,
            DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK,
        ) << DRIVER_RUNTIME_GENET_RESULT_TX_FREE_SHIFT)
        | (driver_runtime_genet_result_clamp(
            parts.rx_queue_count as u32,
            DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK,
        ) << DRIVER_RUNTIME_GENET_RESULT_RX_QUEUE_COUNT_SHIFT)
        | (driver_runtime_genet_result_clamp(
            parts.rx_queue_high_water as u32,
            DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK,
        ) << DRIVER_RUNTIME_GENET_RESULT_RX_QUEUE_HIGH_WATER_SHIFT)
        | (driver_runtime_genet_result_clamp(
            parts.rx_max_drained_per_turn as u32,
            DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK,
        ) << DRIVER_RUNTIME_GENET_RESULT_RX_MAX_DRAIN_SHIFT)
        | ((parts.rx_drain_budget_hit as u32) << DRIVER_RUNTIME_GENET_RESULT_RX_DRAIN_HIT_SHIFT)
        | ((parts.rx_byte_budget_hit as u32) << DRIVER_RUNTIME_GENET_RESULT_RX_BYTE_HIT_SHIFT)
        | ((parts.rx_overflow_seen as u32) << DRIVER_RUNTIME_GENET_RESULT_RX_OVERFLOW_SHIFT)
        | ((parts.command_rx_drain_seen as u32)
            << DRIVER_RUNTIME_GENET_RESULT_COMMAND_RX_DRAIN_SEEN_SHIFT)
}

/// Returns true when a GENET completion result uses the packed diagnostic form.
#[must_use]
pub const fn driver_runtime_genet_result_is_packed(result: u32) -> bool {
    result & DRIVER_RUNTIME_GENET_RESULT_PACKED != 0
}

/// Decode GENET TX free descriptors from a packed completion result.
#[must_use]
pub const fn driver_runtime_genet_result_tx_free(result: u32) -> u16 {
    ((result >> DRIVER_RUNTIME_GENET_RESULT_TX_FREE_SHIFT)
        & DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK) as u16
}

/// Decode GENET TX in-flight descriptors from a packed completion result.
#[must_use]
pub const fn driver_runtime_genet_result_tx_in_flight(result: u32) -> u16 {
    ((result >> DRIVER_RUNTIME_GENET_RESULT_TX_IN_FLIGHT_SHIFT)
        & DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK) as u16
}

/// Decode GENET runtime RX queue depth from a packed completion result.
#[must_use]
pub const fn driver_runtime_genet_result_rx_queue_count(result: u32) -> u16 {
    ((result >> DRIVER_RUNTIME_GENET_RESULT_RX_QUEUE_COUNT_SHIFT)
        & DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK) as u16
}

/// Decode GENET runtime RX queue high-water from a packed completion result.
#[must_use]
pub const fn driver_runtime_genet_result_rx_queue_high_water(result: u32) -> u16 {
    ((result >> DRIVER_RUNTIME_GENET_RESULT_RX_QUEUE_HIGH_WATER_SHIFT)
        & DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK) as u16
}

/// Decode GENET runtime maximum RX frames drained in one turn.
#[must_use]
pub const fn driver_runtime_genet_result_rx_max_drained_per_turn(result: u32) -> u16 {
    ((result >> DRIVER_RUNTIME_GENET_RESULT_RX_MAX_DRAIN_SHIFT)
        & DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK) as u16
}

/// Decode whether GENET runtime RX ever hit its drain budget.
#[must_use]
pub const fn driver_runtime_genet_result_rx_drain_budget_hit(result: u32) -> bool {
    result & (1 << DRIVER_RUNTIME_GENET_RESULT_RX_DRAIN_HIT_SHIFT) != 0
}

/// Decode whether GENET runtime RX ever hit its byte budget.
#[must_use]
pub const fn driver_runtime_genet_result_rx_byte_budget_hit(result: u32) -> bool {
    result & (1 << DRIVER_RUNTIME_GENET_RESULT_RX_BYTE_HIT_SHIFT) != 0
}

/// Decode whether GENET runtime RX queue overflow was observed.
#[must_use]
pub const fn driver_runtime_genet_result_rx_overflow_seen(result: u32) -> bool {
    result & (1 << DRIVER_RUNTIME_GENET_RESULT_RX_OVERFLOW_SHIFT) != 0
}

/// Decode whether an admitted GENET RX command drained durable hardware work.
#[must_use]
pub const fn driver_runtime_genet_result_command_rx_drain_seen(result: u32) -> bool {
    result & (1 << DRIVER_RUNTIME_GENET_RESULT_COMMAND_RX_DRAIN_SEEN_SHIFT) != 0
}

/// Primitive-only linked-runtime counter snapshot.
///
/// This record is intentionally separate from command and completion records so
/// benchmark evidence cannot change command authority or completion semantics.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCounterSnapshot {
    /// [`DRIVER_RUNTIME_COUNTER_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_COUNTER_VERSION`].
    pub version: u16,
    /// Total record bytes.
    pub len: u16,
    /// Runtime hot-path id covered by this snapshot.
    pub hot_path: u32,
    /// Primitive snapshot flags.
    pub flags: u32,
    /// Last root-assigned sequence observed by the producer.
    pub sequence: u32,
    /// Reserved for alignment and future fields.
    pub reserved: u32,
    /// Root-submitted turns accepted into the active slot.
    pub submitted_turns: u64,
    /// Turns that published a matching completion sequence.
    pub completed_turns: u64,
    /// Completed turns whose result was idle.
    pub idle_turns: u64,
    /// Completed turns whose result was fault.
    pub fault_turns: u64,
    /// Completed turns whose result exhausted the service budget.
    pub budget_exhausted_turns: u64,
    /// Completed turns that published one frame descriptor.
    pub frame_ready_turns: u64,
    /// Descriptors consumed or returned by bounded service turns.
    pub descriptors_drained: u64,
    /// Bytes staged into root/runtime shared payload areas.
    pub staged_bytes: u64,
    /// Root/runtime cache-clean operations.
    pub cache_clean_ops: u64,
    /// Bytes covered by cache-clean operations.
    pub cache_clean_bytes: u64,
    /// Root/runtime cache-invalidate operations.
    pub cache_invalidate_ops: u64,
    /// Bytes covered by cache-invalidate operations.
    pub cache_invalidate_bytes: u64,
    /// IPC send attempts for bounded nonblocking turns.
    pub send_attempts: u64,
    /// Cooperative yields issued while waiting for completions.
    pub yield_count: u64,
    /// Active-slot conflicts that returned busy/backpressure.
    pub busy_conflicts: u64,
    /// Same-request keep-active resumes admitted by fingerprint.
    pub same_request_resumes: u64,
    /// Bounded turns that timed out before completion.
    pub timeouts: u64,
    /// Timeouts deliberately kept active for later prompt slices.
    pub keep_active_timeouts: u64,
    /// Active turns aborted after exhausting the keep-active limit.
    pub aborts: u64,
    /// Budget or service overruns reported by the producer.
    pub overruns: u64,
    /// Drops reported by bounded queues or producer pressure.
    pub drops: u64,
    /// Frame-ready RX frames observed by the producer.
    pub rx_frames: u64,
    /// TX frames submitted or completed by the producer.
    pub tx_frames: u64,
    /// Frame-ready RX bytes observed by the producer.
    pub rx_bytes: u64,
    /// TX bytes submitted or completed by the producer.
    pub tx_bytes: u64,
    /// Role-specific counter slot 0.
    pub role_aux0: u64,
    /// Role-specific counter slot 1.
    pub role_aux1: u64,
    /// Role-specific counter slot 2.
    pub role_aux2: u64,
    /// Role-specific counter slot 3.
    pub role_aux3: u64,
}

impl DriverRuntimeCounterSnapshot {
    /// Empty counter snapshot with the fixed header populated.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_COUNTER_MAGIC,
            version: DRIVER_RUNTIME_COUNTER_VERSION,
            len: core::mem::size_of::<Self>() as u16,
            hot_path: 0,
            flags: 0,
            sequence: 0,
            reserved: 0,
            submitted_turns: 0,
            completed_turns: 0,
            idle_turns: 0,
            fault_turns: 0,
            budget_exhausted_turns: 0,
            frame_ready_turns: 0,
            descriptors_drained: 0,
            staged_bytes: 0,
            cache_clean_ops: 0,
            cache_clean_bytes: 0,
            cache_invalidate_ops: 0,
            cache_invalidate_bytes: 0,
            send_attempts: 0,
            yield_count: 0,
            busy_conflicts: 0,
            same_request_resumes: 0,
            timeouts: 0,
            keep_active_timeouts: 0,
            aborts: 0,
            overruns: 0,
            drops: 0,
            rx_frames: 0,
            tx_frames: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            role_aux0: 0,
            role_aux1: 0,
            role_aux2: 0,
            role_aux3: 0,
        }
    }

    /// Empty snapshot for one known hot path.
    #[must_use]
    pub const fn for_hot_path(hot_path: u32, flags: u32, sequence: u32) -> Self {
        let mut snapshot = Self::empty();
        snapshot.hot_path = hot_path;
        snapshot.flags = flags;
        snapshot.sequence = sequence;
        snapshot
    }

    /// Returns true when the snapshot is bounded and non-authority-bearing.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_COUNTER_MAGIC
            && self.version == DRIVER_RUNTIME_COUNTER_VERSION
            && self.len as usize == core::mem::size_of::<Self>()
            && self.hot_path >= HOT_PATH_SERIAL_CONSOLE
            && self.hot_path <= HOT_PATH_PCIE_ROOT
            && self.reserved == 0
            && (self.flags
                & !(DRIVER_RUNTIME_COUNTER_FLAG_ROOT_SNAPSHOT
                    | DRIVER_RUNTIME_COUNTER_FLAG_RUNTIME_SNAPSHOT))
                == 0
    }
}

/// Descriptor flag: MMIO pages are mapped at the fixed runtime MMIO base.
pub const DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED: u32 = 1 << 0;
/// Descriptor flag: DMA pages include device-visible physical addresses.
pub const DRIVER_RUNTIME_INIT_FLAG_DMA_PADDRS: u32 = 1 << 1;
/// Descriptor flag: shared pages are root-visible ring/client buffers.
pub const DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS: u32 = 1 << 2;
/// Descriptor flag: descriptor does not carry any root pointer or callback context.
pub const DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE: u32 = 1 << 3;
/// Descriptor flag: framebuffer metadata is present for HDMI.
pub const DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER: u32 = 1 << 4;
/// Descriptor flag: firmware/control shared buffers are present for CYW43/SDIO.
pub const DRIVER_RUNTIME_INIT_FLAG_FIRMWARE_BUFFERS: u32 = 1 << 5;
/// Descriptor flag: bus address translation values are present.
pub const DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING: u32 = 1 << 6;
/// Descriptor flag: IRQ descriptors and child slots are present.
pub const DRIVER_RUNTIME_INIT_FLAG_IRQS_BOUND: u32 = 1 << 7;
/// Descriptor flag: the runtime is deliberately poll-only.
pub const DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY: u32 = 1 << 8;
/// Descriptor flag: bus-owner links are present for split drivers.
pub const DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS: u32 = 1 << 9;
/// Descriptor flag: the runtime must reject root contexts for hardware work.
pub const DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN: u32 = 1 << 10;
/// Descriptor flag: GENET owns one generation-bound CPU-only packet link to
/// the isolated console-network runtime.
pub const DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET: u32 = 1 << 11;

/// Required descriptor flags for any acceptance-eligible hardware runtime.
pub const DRIVER_RUNTIME_INIT_REQUIRED_FLAGS: u32 = DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE
    | DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS
    | DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING
    | DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN;
/// Complete allowed runtime-init flag set for ABI v13.
pub const DRIVER_RUNTIME_INIT_ALLOWED_FLAGS: u32 = DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED
    | DRIVER_RUNTIME_INIT_FLAG_DMA_PADDRS
    | DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS
    | DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE
    | DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER
    | DRIVER_RUNTIME_INIT_FLAG_FIRMWARE_BUFFERS
    | DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING
    | DRIVER_RUNTIME_INIT_FLAG_IRQS_BOUND
    | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
    | DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS
    | DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN
    | DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET;

/// One mapped runtime page physical address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimePageDescriptor {
    /// Physical address backing this page, or zero when not device-visible.
    pub paddr: u64,
}

impl DriverRuntimePageDescriptor {
    /// Empty page descriptor.
    #[must_use]
    pub const fn empty() -> Self {
        Self { paddr: 0 }
    }

    /// Construct a non-empty page descriptor.
    #[must_use]
    pub const fn new(paddr: usize) -> Self {
        Self {
            paddr: paddr as u64,
        }
    }
}

/// Role-specific framebuffer geometry for HDMI runtime ownership.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeFramebufferDescriptor {
    /// Driver-local virtual base for the mapped framebuffer.
    pub vaddr: u64,
    /// Physical base of the framebuffer when known.
    pub paddr: u64,
    /// Framebuffer width in pixels.
    pub width: u32,
    /// Framebuffer height in pixels.
    pub height: u32,
    /// Bytes per scanline.
    pub pitch: u32,
    /// Pixel format tag owned by the runtime.
    pub format: u32,
}

/// Fixed SDIO command record carried in the shared driver ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeSdioCommandDescriptor {
    /// [`DRIVER_RUNTIME_SDIO_OP_*`] value.
    pub op: u16,
    /// Function number for CMD52/CMD53.
    pub function: u8,
    /// [`DRIVER_RUNTIME_SDIO_RESP_*`] value.
    pub response_kind: u8,
    /// SDIO register/window address, or raw card-command argument for
    /// [`DRIVER_RUNTIME_SDIO_OP_CARD_COMMAND`].
    pub addr: u32,
    /// Data payload offset inside the fixed command ring page or the bounded
    /// runtime shared-buffer payload window.
    pub data_offset: u16,
    /// Data bytes for byte-mode transfers, or raw command index for
    /// [`DRIVER_RUNTIME_SDIO_OP_CARD_COMMAND`].
    pub len: u16,
    /// Block size for CMD53 block-mode transfers.
    pub block_size: u16,
    /// Block count for CMD53 block-mode transfers.
    pub block_count: u16,
    /// Role-specific primitive flags.
    pub flags: u16,
    /// HOST_CONFIG-only CCCR readbacks, or zero for every other operation.
    ///
    /// The low byte carries `SPEED` when
    /// [`Self::FLAG_HOST_CCCR_SPEED_VALID`] is set. The high byte carries
    /// `BUS_INTERFACE_CONTROL` when
    /// [`Self::FLAG_HOST_CCCR_INTERFACE_VALID`] is set.
    pub reserved: u16,
    /// Bounded command timeout in microseconds.
    pub timeout_us: u32,
}

impl DriverRuntimeSdioCommandDescriptor {
    /// CMD53 address increments after each byte/block.
    pub const FLAG_INCREMENT: u16 = 1 << 0;
    /// Host-config command requests 4-bit SDIO bus width.
    pub const FLAG_HOST_BUS_WIDTH_4BIT: u16 = 1 << 1;
    /// Host-config command requests SDHCI high-speed mode.
    pub const FLAG_HOST_HIGH_SPEED: u16 = 1 << 2;
    /// DPC activation must publish one generation-bound source-probe event
    /// even when the host `CARD_INT` latch is not currently asserted.
    pub const FLAG_DPC_FORCE_SOURCE_PROBE: u16 = 1 << 3;
    /// A Function-2 CMD53 write must establish a healthy, same-generation DPC
    /// ring with host `CARD_INT` physically armed, then recheck that durable
    /// condition at the final pre-issue boundary. Visible DPC work defers the
    /// child without issuing.
    pub const FLAG_PRE_TX_DPC_FENCE: u16 = 1 << 4;
    /// HOST_CONFIG carries a read-back CCCR `SPEED` byte in `reserved[7:0]`.
    pub const FLAG_HOST_CCCR_SPEED_VALID: u16 = 1 << 5;
    /// HOST_CONFIG carries a read-back CCCR `BUS_INTERFACE_CONTROL` byte in
    /// `reserved[15:8]`.
    pub const FLAG_HOST_CCCR_INTERFACE_VALID: u16 = 1 << 6;
    /// A post-activation CYW43 steady-data-plane child may retain this exact
    /// SDIO request
    /// across bounded owner slices without publishing a fresh continuation
    /// grant for every internal controller phase.
    ///
    /// The command still names one operation, one generation, and one typed
    /// binding: either a DPC event or an explicitly marked urgent Ethernet TX
    /// parent. This flag is invalid for boot, host-config, card-command, and
    /// DPC-activation descriptors.
    pub const FLAG_STEADY_SERVICE_LEASE: u16 = 1 << 7;
    /// This exact DPC child clears the initial CYW43 SDIO-core interrupt
    /// status and requires the sole SDIO owner to rearm host `CARD_INT` after
    /// the successful W1C transfer and before publishing child completion.
    ///
    /// The marker is valid only on the event-bound, four-byte Function-1
    /// backplane write that carries [`Self::FLAG_STEADY_SERVICE_LEASE`].
    pub const FLAG_DPC_SOURCE_W1C_REARM: u16 = 1 << 8;
    /// This exact Function-2 write is the child of a HAL-marked finite
    /// Ethernet TX parent and may use the same finite SDIO service lease as an
    /// event-bound DPC child.
    ///
    /// The child remains identity-bound to its immutable parent sequence and
    /// shared generation. Ordinary data parents require the post-Gate-8
    /// priority lease; host-EAPOL parents require an intake-sealed M2, M4, or
    /// group-key response. Control, boot, bulk, and untyped op7 paths cannot
    /// infer the authority from Function 2 or operation number alone.
    pub const FLAG_STEADY_TX_SERVICE_LEASE: u16 = 1 << 9;
    /// This exact CYW43-linked primitive belongs to one immutable persistent
    /// parent transaction.
    ///
    /// The marker is valid only for a bounded Function-1 or Function-2 CMD52 or
    /// CMD53 primitive, or the exact DPC activation derived by that parent. It
    /// carries no standalone authority: the linked CYW43 runtime must derive it
    /// from the exact parent command's
    /// [`DRIVER_RUNTIME_COMMAND_FLAG_PERSISTENT_TRANSACTION`] identity.
    pub const FLAG_PERSISTENT_TRANSACTION: u16 = 1 << 10;

    /// Empty descriptor.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            op: 0,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
            addr: 0,
            data_offset: 0,
            len: 0,
            block_size: 0,
            block_count: 0,
            flags: 0,
            reserved: 0,
            timeout_us: 0,
        }
    }

    /// Returns true when the command is bounded and internally consistent.
    #[must_use]
    pub const fn valid(self) -> bool {
        let known_op = self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE
            || self.op == DRIVER_RUNTIME_SDIO_OP_POLL_IRQ
            || self.op == DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG
            || self.op == DRIVER_RUNTIME_SDIO_OP_CARD_COMMAND
            || self.op == DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE;
        let host_config = self.op == DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG;
        let card_command = self.op == DRIVER_RUNTIME_SDIO_OP_CARD_COMMAND;
        let dpc_activate = self.op == DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE;
        let known_response = self.response_kind == DRIVER_RUNTIME_SDIO_RESP_NONE
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_OCR
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_SHORT
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_LONG;
        let cmd52 = self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE;
        let cmd53 = self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE;
        let pre_tx_dpc_fence = self.flags & Self::FLAG_PRE_TX_DPC_FENCE != 0;
        let steady_service_lease = self.flags & Self::FLAG_STEADY_SERVICE_LEASE != 0;
        let dpc_source_w1c_rearm = self.flags & Self::FLAG_DPC_SOURCE_W1C_REARM != 0;
        let steady_tx_service_lease = self.flags & Self::FLAG_STEADY_TX_SERVICE_LEASE != 0;
        let persistent_transaction = self.flags & Self::FLAG_PERSISTENT_TRANSACTION != 0;
        let persistent_transaction_flags_valid = if cmd52 {
            self.flags & !Self::FLAG_PERSISTENT_TRANSACTION == 0
        } else if cmd53 {
            self.flags
                & !(Self::FLAG_INCREMENT
                    | Self::FLAG_PRE_TX_DPC_FENCE
                    | Self::FLAG_PERSISTENT_TRANSACTION)
                == 0
        } else if dpc_activate {
            self.flags & !(Self::FLAG_DPC_FORCE_SOURCE_PROBE | Self::FLAG_PERSISTENT_TRANSACTION)
                == 0
        } else {
            false
        };
        let host_cccr_speed_valid = self.flags & Self::FLAG_HOST_CCCR_SPEED_VALID != 0;
        let host_cccr_interface_valid = self.flags & Self::FLAG_HOST_CCCR_INTERFACE_VALID != 0;
        let host_cccr_speed = (self.reserved & 0xff) as u8;
        let host_cccr_interface = (self.reserved >> 8) as u8;
        let host_known_flags = Self::FLAG_HOST_BUS_WIDTH_4BIT
            | Self::FLAG_HOST_HIGH_SPEED
            | Self::FLAG_HOST_CCCR_SPEED_VALID
            | Self::FLAG_HOST_CCCR_INTERFACE_VALID;
        let read_result = self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_POLL_IRQ;
        let effective_len = if read_result {
            1
        } else if host_config || card_command || dpc_activate {
            0
        } else if self.block_count != 0 {
            (self.block_count as u32).saturating_mul(self.block_size as u32)
        } else {
            self.len as u32
        };
        let payload_end = self.data_offset as u32 + effective_len;
        let ring_payload = self.data_offset >= DRIVER_RUNTIME_RING_FRAME_OFFSET
            && payload_end <= DRIVER_RUNTIME_RING_PAGE_BYTES as u32;
        let shared_payload = self.data_offset == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
            && payload_end
                <= (DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
                    + DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32);
        known_op
            && known_response
            && self.function <= 7
            && (host_config || card_command || dpc_activate || self.addr < (1 << 17))
            && (!host_config
                || (self.function == 0
                    && self.response_kind == DRIVER_RUNTIME_SDIO_RESP_NONE
                    && self.data_offset == 0
                    && self.len == 0
                    && self.block_size == 0
                    && self.block_count == 0
                    && self.flags & !host_known_flags == 0
                    && (!host_cccr_speed_valid
                        || (self.flags & Self::FLAG_HOST_HIGH_SPEED != 0
                            && host_cccr_speed & DriverRuntimeSdioClockSnapshot::CCCR_SPEED_EHS
                                != 0))
                    && (host_cccr_speed_valid || host_cccr_speed == 0)
                    && (!host_cccr_interface_valid
                        || (self.flags & Self::FLAG_HOST_BUS_WIDTH_4BIT != 0
                            && host_cccr_interface
                                & DriverRuntimeSdioClockSnapshot::CCCR_INTERFACE_WIDTH_MASK
                                == DriverRuntimeSdioClockSnapshot::CCCR_INTERFACE_WIDTH_4BIT))
                    && (host_cccr_interface_valid || host_cccr_interface == 0)
                    && self.addr <= 100_000_000))
            && (!card_command
                || (self.function == 0
                    && self.data_offset == 0
                    && self.len <= 63
                    && self.block_size == 0
                    && self.block_count == 0
                    && self.flags == 0
                    && self.reserved == 0))
            && (!dpc_activate
                || (self.function == 0
                    && self.response_kind == DRIVER_RUNTIME_SDIO_RESP_NONE
                    && self.addr != 0
                    && self.data_offset == 0
                    && self.len == 0
                    && self.block_size == 0
                    && self.block_count == 0
                    && self.flags
                        & !(Self::FLAG_DPC_FORCE_SOURCE_PROBE | Self::FLAG_PERSISTENT_TRANSACTION)
                        == 0
                    && self.reserved == 0))
            && (!pre_tx_dpc_fence
                || (self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE && self.function == 2))
            && (!steady_service_lease || ((cmd52 || cmd53) && self.reserved == 0))
            && (!persistent_transaction
                || (((cmd52 || cmd53)
                    && (self.function == 1 || self.function == 2)
                    && self.response_kind == DRIVER_RUNTIME_SDIO_RESP_SHORT)
                    || dpc_activate)
                    && self.reserved == 0
                    && persistent_transaction_flags_valid
                    && !steady_service_lease
                    && !steady_tx_service_lease
                    && !dpc_source_w1c_rearm)
            && (!steady_tx_service_lease
                || (steady_service_lease
                    && self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE
                    && self.function == 2
                    && !dpc_source_w1c_rearm
                    && self.reserved == 0))
            && (!dpc_source_w1c_rearm
                || (steady_service_lease
                    && self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE
                    && self.function == 1
                    && self.len == 4
                    && self.block_size == 0
                    && self.block_count == 0
                    && self.flags & Self::FLAG_INCREMENT != 0
                    && self.reserved == 0))
            && (!cmd52 || (self.len == 1 && self.block_count == 0 && self.block_size == 0))
            && (!cmd53
                || ((self.len != 0 || self.block_count != 0)
                    && (self.block_count == 0
                        || (self.block_size != 0
                            && self.block_size <= 512
                            && self.block_count <= 511))))
            && (host_config
                || card_command
                || dpc_activate
                || (effective_len != 0 && (ring_payload || shared_payload)))
    }
}

/// Fixed CYW43 command record carried in the shared driver ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeCyw43CommandDescriptor {
    /// [`DRIVER_RUNTIME_CYW43_OP_*`] value.
    pub op: u16,
    /// Role-specific primitive flags.
    pub flags: u16,
    /// Backplane target address for firmware/NVRAM/control writes.
    pub target_addr: u32,
    /// Payload offset in the fixed shared CYW43/SDIO payload aperture.
    pub payload_offset: u16,
    /// Payload bytes carried in this command.
    pub payload_len: u16,
    /// Total stream length for chunked transfers.
    pub total_len: u32,
    /// Operation-specific argument. Firmware chunk commands use this as the
    /// logical firmware byte count when the physical payload carries final-tail
    /// transfer padding.
    pub arg0: u32,
    /// Operation-specific argument.
    pub arg1: u32,
    /// Reserved for alignment and future fields.
    pub reserved: u32,
}

impl DriverRuntimeCyw43CommandDescriptor {
    /// Empty descriptor.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            op: 0,
            flags: 0,
            target_addr: 0,
            payload_offset: 0,
            payload_len: 0,
            total_len: 0,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        }
    }

    /// Returns true when the command is pointer-free and bounded to the ring.
    #[must_use]
    pub const fn valid(self) -> bool {
        let known_op = self.op == DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
            || self.op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP
            || self.op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
            || self.op == DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
            || self.op == DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL
            || self.op == DRIVER_RUNTIME_CYW43_OP_RELEASE
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
            || self.op == DRIVER_RUNTIME_CYW43_OP_ETH_TX
            || self.op == DRIVER_RUNTIME_CYW43_OP_RX_POLL
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE;
        let bulk_stream_payload = self.op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
            || self.op == DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK;
        let carries_payload = bulk_stream_payload
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
            || self.op == DRIVER_RUNTIME_CYW43_OP_ETH_TX;
        let zero_payload = self.op == DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
            || self.op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP
            || self.op == DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL
            || self.op == DRIVER_RUNTIME_CYW43_OP_RELEASE
            || self.op == DRIVER_RUNTIME_CYW43_OP_RX_POLL
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL;
        let known_flags = match self.op {
            DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK => self.flags == 0,
            DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME => {
                self.flags
                    & !(DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
                        | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN)
                    == 0
            }
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE => {
                self.flags
                    & !(DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
                        | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
                        | DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE)
                    == 0
            }
            DRIVER_RUNTIME_CYW43_OP_ETH_TX => {
                self.flags & !DRIVER_RUNTIME_CYW43_FLAG_STEADY_TX_SERVICE_LEASE == 0
            }
            DRIVER_RUNTIME_CYW43_OP_RX_POLL => {
                self.flags
                    & !(DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
                        | DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN)
                    == 0
            }
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL => {
                self.flags & !DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD == 0
            }
            _ => self.flags == 0,
        };
        let payload_end = self.payload_offset as u32 + self.payload_len as u32;
        let bulk_shared_payload = self.payload_offset == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
            && payload_end
                <= DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
                    + DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32;
        let post_release_shared_payload = self.payload_offset
            == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
            && payload_end
                <= DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
                    + DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES as u32;
        let shared_payload = (bulk_stream_payload && bulk_shared_payload)
            || ((self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
                || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
                || self.op == DRIVER_RUNTIME_CYW43_OP_ETH_TX)
                && post_release_shared_payload);
        known_op
            && known_flags
            && (self.flags & DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE == 0
                || self.flags & DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN != 0)
            && ((carries_payload && self.payload_len != 0 && shared_payload)
                || (zero_payload && self.payload_len == 0))
            && (self.total_len == 0 || self.total_len >= self.payload_len as u32)
    }
}

impl DriverRuntimeFramebufferDescriptor {
    /// Empty framebuffer descriptor.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            vaddr: 0,
            paddr: 0,
            width: 0,
            height: 0,
            pitch: 0,
            format: 0,
        }
    }

    /// Returns true when the geometry is bounded and usable.
    #[must_use]
    pub const fn valid(self) -> bool {
        let bytes_per_pixel = match self.format {
            DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888 => 4,
            DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_RGB888 => 3,
            _ => 0,
        };
        let min_pitch = self.width.saturating_mul(bytes_per_pixel);
        self.vaddr != 0
            && self.paddr != 0
            && self.vaddr >= DRIVER_RUNTIME_FRAMEBUFFER_VADDR
            && self.width != 0
            && self.height != 0
            && self.pitch != 0
            && bytes_per_pixel != 0
            && self.pitch >= min_pitch
            && self.pitch <= 16 * 1024
            && self.height <= 4096
    }
}

/// One IRQ source handed to an isolated runtime without root pointers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeIrqDescriptor {
    /// Platform IRQ number.
    pub irq: u32,
    /// Notification badge value expected for this IRQ.
    pub badge: u32,
    /// Child CSpace slot containing the IRQ handler cap.
    pub handler_slot: u32,
    /// Child CSpace slot containing the notification cap.
    pub notification_slot: u32,
    /// Trigger mode tag.
    pub trigger: u16,
    /// Role-specific primitive flags.
    pub flags: u16,
    /// Reserved for alignment and future fields.
    pub reserved: u32,
}

impl DriverRuntimeIrqDescriptor {
    /// Empty IRQ descriptor.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            irq: 0,
            badge: 0,
            handler_slot: 0,
            notification_slot: 0,
            trigger: 0,
            flags: 0,
            reserved: 0,
        }
    }

    /// Returns true when this pointer-free IRQ handoff is structurally valid.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.irq != 0
            && self.badge != 0
            && self.handler_slot >= DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT
            && self.notification_slot == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
            && (self.trigger == DRIVER_RUNTIME_IRQ_TRIGGER_LEVEL
                || self.trigger == DRIVER_RUNTIME_IRQ_TRIGGER_EDGE)
            && self.flags == 0
            && self.reserved == 0
    }
}

/// One sequence-stamped event published by the SDIO owner for CYW43 DPC work.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeDpcEventEntry {
    /// Monotonic producer sequence committed after the remaining fields.
    pub sequence: u32,
    /// Captured SDHCI interrupt status bits.
    pub host_int_status: u32,
    /// Bounded owner signal/reason bits for the CYW43 consumer.
    pub signal_status: u32,
    /// Producer-defined primitive reason code.
    pub reason: u16,
    /// [`DRIVER_RUNTIME_DPC_EVENT_FLAG_*`] bitset.
    pub flags: u16,
}

impl DriverRuntimeDpcEventEntry {
    /// Empty event entry.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sequence: 0,
            host_int_status: 0,
            signal_status: 0,
            reason: 0,
            flags: 0,
        }
    }

    /// Returns true when this entry uses only known primitive flags.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.flags
            & !(DRIVER_RUNTIME_DPC_EVENT_FLAG_CARD_INTERRUPT
                | DRIVER_RUNTIME_DPC_EVENT_FLAG_SOURCE_PENDING)
            == 0
    }
}

/// Fixed single-producer/single-consumer SDIO-to-CYW43 DPC event ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeDpcEventRing {
    /// [`DRIVER_RUNTIME_DPC_EVENT_RING_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_DPC_EVENT_RING_VERSION`].
    pub version: u16,
    /// Total record bytes.
    pub len: u16,
    /// Generated bus-link epoch shared by both peers.
    pub epoch: u32,
    /// Sequence after the newest committed producer entry.
    pub producer: u32,
    /// Sequence after the newest consumed entry.
    pub consumer: u32,
    /// [`DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_*`] bitset.
    pub flags: u32,
    /// Bounded producer-overrun counter.
    pub overruns: u32,
    /// Saturating count of failed seL4 IRQ acknowledgement attempts.
    pub ack_failures: u32,
    /// Bounded sequence-indexed events.
    pub entries: [DriverRuntimeDpcEventEntry; DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH],
}

impl DriverRuntimeDpcEventRing {
    /// Empty ring bound to one generated bus-link epoch.
    #[must_use]
    pub const fn empty(epoch: u32) -> Self {
        Self {
            magic: DRIVER_RUNTIME_DPC_EVENT_RING_MAGIC,
            version: DRIVER_RUNTIME_DPC_EVENT_RING_VERSION,
            len: core::mem::size_of::<Self>() as u16,
            epoch,
            producer: 0,
            consumer: 0,
            flags: 0,
            overruns: 0,
            ack_failures: 0,
            entries: [DriverRuntimeDpcEventEntry::empty(); DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH],
        }
    }

    /// Returns true when the ring is bounded and uses the current fixed layout.
    #[must_use]
    pub const fn valid(self) -> bool {
        if self.magic != DRIVER_RUNTIME_DPC_EVENT_RING_MAGIC
            || self.version != DRIVER_RUNTIME_DPC_EVENT_RING_VERSION
            || self.len as usize != core::mem::size_of::<Self>()
            || self.epoch == 0
            || self.flags & !DRIVER_RUNTIME_DPC_EVENT_RING_KNOWN_FLAGS != 0
            || self.producer.wrapping_sub(self.consumer)
                > DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH as u32
        {
            return false;
        }
        let mut index = 0usize;
        while index < DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH {
            if !self.entries[index].valid() {
                return false;
            }
            index += 1;
        }
        true
    }
}

/// One pointer-free link between split bus-owner driver runtimes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeBusLinkDescriptor {
    /// Runtime hot path that owns the linked bus.
    pub owner_hot_path: u32,
    /// Primitive channel id inside the shared page region.
    pub channel_id: u32,
    /// Offset of the shared channel metadata.
    pub shared_offset: u32,
    /// Bytes reserved for the channel.
    pub shared_len: u32,
    /// Role-specific primitive flags.
    pub flags: u32,
    /// Per-client link epoch bound to the runtime task key.
    pub epoch: u32,
    /// Sealed token over epoch, owner, channel, window, and flags.
    pub token: u32,
    /// Generated epoch shared by both peers for the DPC event ring.
    pub shared_epoch: u32,
    /// Peer runtime hot path for reciprocal doorbell delivery.
    pub peer_hot_path: u32,
    /// Local notification receive slot in this runtime's CSpace.
    pub local_notification_slot: u32,
    /// Send-only peer doorbell slot in this runtime's CSpace.
    ///
    /// The CYW43 client holds a send-only cap to the SDIO owner's notification
    /// here; the SDIO owner holds a send-only cap to the CYW43 client's
    /// completion/DPC notification.
    pub peer_notification_slot: u32,
    /// Fixed DPC event-ring offset inside the owner command page.
    pub event_offset: u16,
    /// Fixed DPC event-ring bytes.
    pub event_len: u16,
    /// Fixed DPC event-ring entry depth.
    pub event_depth: u16,
    /// Reserved for fixed-layout evolution.
    pub reserved: u16,
}

impl DriverRuntimeBusLinkDescriptor {
    /// Empty bus-link descriptor.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            owner_hot_path: 0,
            channel_id: 0,
            shared_offset: 0,
            shared_len: 0,
            flags: 0,
            epoch: 0,
            token: 0,
            shared_epoch: 0,
            peer_hot_path: 0,
            local_notification_slot: 0,
            peer_notification_slot: 0,
            event_offset: 0,
            event_len: 0,
            event_depth: 0,
            reserved: 0,
        }
    }

    /// Construct a non-empty bus-link descriptor.
    #[must_use]
    pub const fn new(
        owner_hot_path: u32,
        channel_id: u32,
        shared_offset: u32,
        shared_len: u32,
        flags: u32,
    ) -> Self {
        Self {
            owner_hot_path,
            channel_id,
            shared_offset,
            shared_len,
            flags,
            epoch: 0,
            token: 0,
            shared_epoch: 0,
            peer_hot_path: 0,
            local_notification_slot: 0,
            peer_notification_slot: 0,
            event_offset: 0,
            event_len: 0,
            event_depth: 0,
            reserved: 0,
        }
    }

    /// Attach the role-specific peer doorbell and bounded DPC event-ring metadata.
    #[must_use]
    pub const fn with_notification_dpc(
        mut self,
        peer_hot_path: u32,
        local_notification_slot: u32,
        peer_notification_slot: u32,
        shared_epoch: u32,
    ) -> Self {
        self.peer_hot_path = peer_hot_path;
        self.local_notification_slot = local_notification_slot;
        self.peer_notification_slot = peer_notification_slot;
        self.shared_epoch = shared_epoch;
        self.event_offset = DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET;
        self.event_len = DRIVER_RUNTIME_DPC_EVENT_RING_BYTES;
        self.event_depth = DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH as u16;
        self.flags |= DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS
            | DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING;
        self
    }

    /// Return this link with the per-client epoch and token populated.
    #[must_use]
    pub const fn with_sealed_identity(mut self, task_key: u32, client_hot_path: u32) -> Self {
        self.epoch = driver_runtime_bus_link_epoch(
            task_key,
            client_hot_path,
            self.owner_hot_path,
            self.channel_id,
        );
        self.token = self.identity_token_for_client(client_hot_path);
        self
    }

    /// Returns true when the link is structurally valid and sealed for this client.
    #[must_use]
    pub const fn sealed_for_client(self, task_key: u32, client_hot_path: u32) -> bool {
        self.valid()
            && self.epoch
                == driver_runtime_bus_link_epoch(
                    task_key,
                    client_hot_path,
                    self.owner_hot_path,
                    self.channel_id,
                )
            && self.epoch != 0
            && self.token == self.identity_token_for_client(client_hot_path)
            && self.token != 0
    }

    const fn identity_token_for_client(self, client_hot_path: u32) -> u32 {
        let mut hash = driver_runtime_identity_hash_word(
            DRIVER_RUNTIME_IDENTITY_HASH_SEED,
            DRIVER_RUNTIME_INIT_AUX,
        );
        hash = driver_runtime_identity_hash_word(hash, client_hot_path);
        hash = driver_runtime_identity_hash_word(hash, self.owner_hot_path);
        hash = driver_runtime_identity_hash_word(hash, self.channel_id);
        hash = driver_runtime_identity_hash_word(hash, self.shared_offset);
        hash = driver_runtime_identity_hash_word(hash, self.shared_len);
        hash = driver_runtime_identity_hash_word(hash, self.flags);
        hash = driver_runtime_identity_hash_word(hash, self.epoch);
        hash = driver_runtime_identity_hash_word(hash, self.shared_epoch);
        hash = driver_runtime_identity_hash_word(hash, self.peer_hot_path);
        hash = driver_runtime_identity_hash_word(hash, self.local_notification_slot);
        hash = driver_runtime_identity_hash_word(hash, self.peer_notification_slot);
        hash = driver_runtime_identity_hash_word(hash, self.event_offset as u32);
        hash = driver_runtime_identity_hash_word(hash, self.event_len as u32);
        hash = driver_runtime_identity_hash_word(hash, self.event_depth as u32);
        driver_runtime_nonzero_hash(hash)
    }

    const fn notification_dpc_fields_absent(self) -> bool {
        self.peer_hot_path == 0
            && self.shared_epoch == 0
            && self.local_notification_slot == 0
            && self.peer_notification_slot == 0
            && self.event_offset == 0
            && self.event_len == 0
            && self.event_depth == 0
            && self.flags
                & (DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER
                    | DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS
                    | DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING)
                == 0
    }

    /// Returns true when this link carries the complete reciprocal DPC topology.
    #[must_use]
    pub const fn notification_dpc_valid(self) -> bool {
        let role =
            self.flags & (DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER);
        let client = role == DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT;
        let owner = role == DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER;
        let peer_matches = if client {
            self.owner_hot_path == HOT_PATH_SDIO_HOST
                && self.peer_hot_path == HOT_PATH_SDIO_HOST
                && self.peer_notification_slot == DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT
        } else if owner {
            self.owner_hot_path == HOT_PATH_SDIO_HOST
                && self.peer_hot_path == HOT_PATH_CYW43_WIFI
                && self.peer_notification_slot == DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT
        } else {
            false
        };
        self.channel_id == DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
            && peer_matches
            && self.local_notification_slot == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
            && self.shared_epoch != 0
            && self.event_offset == DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET
            && self.event_len == DRIVER_RUNTIME_DPC_EVENT_RING_BYTES
            && self.event_depth == DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH as u16
            && self.reserved == 0
            && (self.flags & DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE) != 0
            && (self.flags & DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS) != 0
            && (self.flags & DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING) != 0
    }

    /// Returns true when the link contains a bounded pointer-free channel.
    #[must_use]
    pub const fn valid(self) -> bool {
        let shared_end = self.shared_offset.saturating_add(self.shared_len);
        let known_flags = DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT
            | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE
            | DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER
            | DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS
            | DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING;
        let known_channel = self.channel_id == DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE
            || self.channel_id == DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO;
        let owner_matches_channel = if self.channel_id == DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        {
            self.owner_hot_path == HOT_PATH_SDIO_HOST
        } else {
            self.owner_hot_path == HOT_PATH_PCIE_ROOT
        };
        let valid_window = if self.channel_id == DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO {
            self.shared_offset == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
                && self.shared_len == DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32
        } else {
            self.shared_len != 0 && shared_end <= DRIVER_RUNTIME_RING_PAGE_BYTES as u32
        };
        self.owner_hot_path >= HOT_PATH_SERIAL_CONSOLE
            && self.owner_hot_path <= HOT_PATH_PCIE_ROOT
            && known_channel
            && owner_matches_channel
            && valid_window
            && self.flags & !known_flags == 0
            && (self.flags & DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE) != 0
            && self.reserved == 0
            && (self.notification_dpc_fields_absent() || self.notification_dpc_valid())
    }
}

/// One semantic resource range handed to an isolated runtime.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeResourceRangeDescriptor {
    /// [`DRIVER_RUNTIME_RESOURCE_KIND_*`] value.
    pub kind: u16,
    /// [`DRIVER_RUNTIME_RESOURCE_FLAG_*`] bitset.
    pub flags: u16,
    /// Role-specific resource tag.
    pub tag: u32,
    /// First driver-local virtual address for this resource.
    pub vaddr: u64,
    /// First physical address when known.
    pub paddr: u64,
    /// Bounded byte length represented by this range.
    pub bytes: u64,
    /// Pages represented by this range.
    pub page_count: u16,
    /// First index in the legacy page array, when descriptors were emitted.
    pub first_page_index: u16,
    /// Reserved for alignment and future fields.
    pub reserved: u32,
}

impl DriverRuntimeResourceRangeDescriptor {
    /// Empty resource range.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            kind: 0,
            flags: 0,
            tag: DRIVER_RUNTIME_RESOURCE_TAG_GENERIC,
            vaddr: 0,
            paddr: 0,
            bytes: 0,
            page_count: 0,
            first_page_index: 0,
            reserved: 0,
        }
    }

    /// Construct a non-empty resource range descriptor.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "fixed-layout ABI constructor mirrors the descriptor field order"
    )]
    pub const fn new(
        kind: u16,
        flags: u16,
        tag: u32,
        vaddr: u64,
        paddr: u64,
        bytes: u64,
        page_count: u16,
        first_page_index: u16,
    ) -> Self {
        Self {
            kind,
            flags,
            tag,
            vaddr,
            paddr,
            bytes,
            page_count,
            first_page_index,
            reserved: 0,
        }
    }

    /// Returns true when the range is bounded and non-empty.
    #[must_use]
    pub const fn valid(self) -> bool {
        let known_kind = self.kind == DRIVER_RUNTIME_RESOURCE_KIND_MMIO
            || self.kind == DRIVER_RUNTIME_RESOURCE_KIND_DMA
            || self.kind == DRIVER_RUNTIME_RESOURCE_KIND_SHARED
            || self.kind == DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER;
        let max_bytes = (self.page_count as u64).saturating_mul(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES);
        let cpu_only = self.flags & DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY != 0;
        let physical_authority_valid = if cpu_only {
            self.paddr == 0
                && self.flags
                    & (DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                        | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE)
                    == 0
        } else {
            self.paddr != 0
        };
        known_kind
            && self.flags & !DRIVER_RUNTIME_RESOURCE_ALLOWED_FLAGS == 0
            && self.vaddr != 0
            && physical_authority_valid
            && self.bytes != 0
            && self.page_count != 0
            && self.bytes <= max_bytes
            && self.reserved == 0
    }
}

/// Pointer-free descriptor submitted by root before a driver runtime owns work.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimeInitDescriptor {
    /// [`DRIVER_RUNTIME_INIT_MAGIC`].
    pub magic: u32,
    /// [`DRIVER_RUNTIME_INIT_VERSION`].
    pub version: u16,
    /// Total descriptor bytes.
    pub len: u16,
    /// Hot path id owned by the runtime.
    pub hot_path: u32,
    /// Driver role bit expected by root-task proof.
    pub role_bit: u32,
    /// Primitive descriptor flags.
    pub flags: u32,
    /// MMIO page descriptors populated in `mmio_pages`.
    pub mmio_page_count: u16,
    /// DMA page descriptors populated in `dma_pages`.
    pub dma_page_count: u16,
    /// Shared page descriptors populated in `shared_pages`.
    pub shared_page_count: u16,
    /// IRQ descriptors populated in `irqs`.
    pub irq_count: u16,
    /// Bus-link descriptors populated in `bus_links`.
    pub bus_link_count: u16,
    /// Semantic resource ranges populated in `resource_ranges`.
    pub resource_range_count: u16,
    /// Fixed child slot containing the send-only root-control fan-in wake cap.
    pub root_control_wake_notification_slot: u32,
    /// Child CSpace slot containing a send-only root wake cap, or zero when absent.
    pub root_wake_notification_slot: u32,
    /// Exact badge on the root wake cap, or zero when absent.
    pub root_wake_notification_badge: u32,
    /// Optional CPU-only direct link to the isolated console-network runtime.
    pub direct_genet: DriverRuntimeDirectGenetDescriptor,
    /// [`DRIVER_RUNTIME_IDENTITY_MAGIC`] when root sealed this descriptor.
    pub identity_magic: u32,
    /// Stable driver-task key supplied in the runtime entry register.
    pub task_key: u32,
    /// Hash of the generated runtime artifact contract selected by root.
    pub artifact_hash: u32,
    /// Sealed token over task key, artifact hash, hot path, and role bit.
    pub identity_token: u32,
    /// [`DRIVER_RUNTIME_MCS_MAGIC`].
    pub scheduler_magic: u32,
    /// [`DRIVER_RUNTIME_MCS_VERSION`].
    pub scheduler_version: u16,
    /// Required `DRIVER_RUNTIME_MCS_FLAG_*` set.
    pub scheduler_flags: u16,
    /// Generated logical scheduling-context slot for this active runtime.
    pub scheduling_context_slot: u32,
    /// Size bits of the admitted MCS scheduling context.
    pub scheduling_context_bits: u8,
    /// Core whose SchedControl configured the active scheduling context.
    pub sched_control_core: u8,
    /// Generated maximum refill count.
    pub max_refills: u8,
    /// Generated core affinity for the driver TCB.
    pub affinity_core: u8,
    /// Generated active scheduling-context budget in microseconds.
    pub budget_us: u32,
    /// Generated active scheduling-context period in microseconds.
    pub period_us: u32,
    /// Fixed child slot holding the read-only command endpoint.
    pub command_endpoint_slot: u32,
    /// Fixed child slot holding the explicit command Reply object.
    pub command_reply_slot: u32,
    /// Fixed child slot holding the read-only local notification.
    pub irq_notification_slot: u32,
    /// Fixed child slot holding the send-only completion wake cap.
    pub completion_notification_slot: u32,
    /// Explicit zero replacing the historical padding before 64-bit badges.
    pub command_cap_reserved: u32,
    /// Exact badge on the root's Write + GrantReply command cap.
    pub command_badge: u64,
    /// Exact badge on the child's one-way completion notification cap.
    pub completion_badge: u64,
    /// Exact badge delivered for a standard fault from this runtime.
    pub standard_fault_badge: u64,
    /// Exact generated MCS timeout-fault badge for this runtime.
    pub timeout_fault_badge: u64,
    /// Supervisor-owned standard-fault Reply lane identifier.
    pub standard_fault_reply_lane: u16,
    /// Supervisor-owned timeout-fault Reply lane identifier.
    pub timeout_fault_reply_lane: u16,
    /// Exact synchronous command cardinality admitted for this runtime.
    pub max_inflight_commands: u16,
    /// Reserved; must be zero.
    pub scheduler_reserved: u16,
    /// Device bus alias OR mask, or zero when physical addresses are direct.
    pub bus_alias_or: u64,
    /// Device bus alias AND mask, or all ones when physical addresses are direct.
    pub bus_alias_and: u64,
    /// Fixed driver-local virtual base for MMIO pages.
    pub mmio_vaddr_base: u64,
    /// Fixed driver-local virtual base for runtime-owned DMA pages.
    pub dma_vaddr_base: u64,
    /// Fixed driver-local virtual base for shared pages.
    pub shared_vaddr_base: u64,
    /// Role-specific framebuffer descriptor for HDMI.
    pub framebuffer: DriverRuntimeFramebufferDescriptor,
    /// Mapped MMIO pages.
    pub mmio_pages: [DriverRuntimePageDescriptor; DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES],
    /// Runtime-owned DMA pages.
    pub dma_pages: [DriverRuntimePageDescriptor; DRIVER_RUNTIME_INIT_MAX_DMA_PAGES],
    /// Root/driver shared pages outside the command ring.
    pub shared_pages: [DriverRuntimePageDescriptor; DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES],
    /// Driver-owned IRQ sources.
    pub irqs: [DriverRuntimeIrqDescriptor; DRIVER_RUNTIME_INIT_MAX_IRQS],
    /// Bus-owner links for split runtimes such as USB/PCIe and CYW43/SDIO.
    pub bus_links: [DriverRuntimeBusLinkDescriptor; DRIVER_RUNTIME_INIT_MAX_BUS_LINKS],
    /// Explicit zero replacing the historical padding before resource ranges.
    pub reserved_tail: u32,
    /// Semantic resource ranges for large or role-specific apertures.
    pub resource_ranges:
        [DriverRuntimeResourceRangeDescriptor; DRIVER_RUNTIME_INIT_MAX_RESOURCE_RANGES],
}

impl DriverRuntimeInitDescriptor {
    /// Empty descriptor with the correct fixed header.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_INIT_MAGIC,
            version: DRIVER_RUNTIME_INIT_VERSION,
            len: core::mem::size_of::<Self>() as u16,
            hot_path: 0,
            role_bit: 0,
            flags: DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE,
            mmio_page_count: 0,
            dma_page_count: 0,
            shared_page_count: 0,
            irq_count: 0,
            bus_link_count: 0,
            resource_range_count: 0,
            root_control_wake_notification_slot: 0,
            root_wake_notification_slot: 0,
            root_wake_notification_badge: 0,
            direct_genet: DriverRuntimeDirectGenetDescriptor::empty(),
            identity_magic: 0,
            task_key: 0,
            artifact_hash: 0,
            identity_token: 0,
            scheduler_magic: 0,
            scheduler_version: 0,
            scheduler_flags: 0,
            scheduling_context_slot: 0,
            scheduling_context_bits: 0,
            sched_control_core: 0,
            max_refills: 0,
            affinity_core: 0,
            budget_us: 0,
            period_us: 0,
            command_endpoint_slot: 0,
            command_reply_slot: 0,
            irq_notification_slot: 0,
            completion_notification_slot: 0,
            command_cap_reserved: 0,
            command_badge: 0,
            completion_badge: 0,
            standard_fault_badge: 0,
            timeout_fault_badge: 0,
            standard_fault_reply_lane: 0,
            timeout_fault_reply_lane: 0,
            max_inflight_commands: 0,
            scheduler_reserved: 0,
            bus_alias_or: 0,
            bus_alias_and: u64::MAX,
            mmio_vaddr_base: 0,
            dma_vaddr_base: 0,
            shared_vaddr_base: 0,
            framebuffer: DriverRuntimeFramebufferDescriptor::empty(),
            mmio_pages: [DriverRuntimePageDescriptor::empty(); DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES],
            dma_pages: [DriverRuntimePageDescriptor::empty(); DRIVER_RUNTIME_INIT_MAX_DMA_PAGES],
            shared_pages: [DriverRuntimePageDescriptor::empty();
                DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES],
            irqs: [DriverRuntimeIrqDescriptor::empty(); DRIVER_RUNTIME_INIT_MAX_IRQS],
            bus_links: [DriverRuntimeBusLinkDescriptor::empty(); DRIVER_RUNTIME_INIT_MAX_BUS_LINKS],
            reserved_tail: 0,
            resource_ranges: [DriverRuntimeResourceRangeDescriptor::empty();
                DRIVER_RUNTIME_INIT_MAX_RESOURCE_RANGES],
        }
    }

    /// Return this descriptor sealed for one runtime task and generated artifact.
    #[must_use]
    pub const fn with_sealed_identity(mut self, task_key: u32, artifact_hash: u32) -> Self {
        self.identity_magic = DRIVER_RUNTIME_IDENTITY_MAGIC;
        self.task_key = task_key;
        self.artifact_hash = artifact_hash;
        self.identity_token =
            driver_runtime_identity_token(task_key, artifact_hash, self.hot_path, self.role_bit);
        if self.scheduler_magic == DRIVER_RUNTIME_MCS_MAGIC {
            self.command_badge = driver_runtime_command_badge(task_key);
            self.completion_badge = driver_runtime_completion_badge(task_key);
        }
        let mut index = 0usize;
        while index < self.bus_link_count as usize {
            self.bus_links[index] =
                self.bus_links[index].with_sealed_identity(task_key, self.hot_path);
            index += 1;
        }
        self
    }

    /// Bind this descriptor to one compiler-admitted active MCS runtime.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "the fixed MCS inventory is copied directly from generated temporal truth"
    )]
    pub const fn with_mcs_scheduler(
        mut self,
        task_key: u32,
        scheduling_context_slot: u32,
        scheduling_context_bits: u8,
        sched_control_core: u8,
        max_refills: u8,
        affinity_core: u8,
        budget_us: u32,
        period_us: u32,
        standard_fault_badge: u64,
        timeout_fault_badge: u64,
    ) -> Self {
        self.scheduler_magic = DRIVER_RUNTIME_MCS_MAGIC;
        self.scheduler_version = DRIVER_RUNTIME_MCS_VERSION;
        self.scheduler_flags = DRIVER_RUNTIME_MCS_REQUIRED_FLAGS;
        self.scheduling_context_slot = scheduling_context_slot;
        self.scheduling_context_bits = scheduling_context_bits;
        self.sched_control_core = sched_control_core;
        self.max_refills = max_refills;
        self.affinity_core = affinity_core;
        self.budget_us = budget_us;
        self.period_us = period_us;
        self.command_endpoint_slot = DRIVER_RUNTIME_COMMAND_ENDPOINT_SLOT;
        self.command_reply_slot = DRIVER_RUNTIME_COMMAND_REPLY_SLOT;
        self.irq_notification_slot = DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT;
        self.completion_notification_slot = DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT;
        self.root_control_wake_notification_slot =
            DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT;
        self.command_badge = driver_runtime_command_badge(task_key);
        self.completion_badge = driver_runtime_completion_badge(task_key);
        self.standard_fault_badge = standard_fault_badge;
        self.timeout_fault_badge = timeout_fault_badge;
        self.standard_fault_reply_lane = DRIVER_RUNTIME_STANDARD_FAULT_REPLY_LANE;
        self.timeout_fault_reply_lane = DRIVER_RUNTIME_TIMEOUT_FAULT_REPLY_LANE;
        self.max_inflight_commands = DRIVER_RUNTIME_MAX_INFLIGHT_COMMANDS;
        self.scheduler_reserved = 0;
        self
    }

    /// Admit the exact CPU-only direct GENET link.
    #[must_use]
    pub const fn with_direct_genet(mut self) -> Self {
        self.flags |= DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET;
        self.direct_genet = DriverRuntimeDirectGenetDescriptor::exact();
        self
    }

    /// Returns true when the descriptor's identity fields are self-consistent.
    #[must_use]
    pub const fn sealed_identity_self_consistent(self) -> bool {
        self.sealed_identity_valid_for_task(self.task_key)
    }

    /// Returns true when root sealed the descriptor for this runtime task key.
    #[must_use]
    pub const fn sealed_identity_valid_for_task(self, task_key: u32) -> bool {
        self.valid()
            && self.identity_magic == DRIVER_RUNTIME_IDENTITY_MAGIC
            && self.task_key == task_key
            && self.artifact_hash != 0
            && self.identity_token
                == driver_runtime_identity_token(
                    task_key,
                    self.artifact_hash,
                    self.hot_path,
                    self.role_bit,
                )
            && self.identity_token != 0
            && self.sealed_bus_links_valid_for_task(task_key)
    }

    /// Returns true when every populated bus link is sealed for this descriptor.
    #[must_use]
    pub const fn sealed_bus_links_valid_for_task(self, task_key: u32) -> bool {
        let mut index = 0usize;
        while index < self.bus_link_count as usize {
            if !self.bus_links[index].sealed_for_client(task_key, self.hot_path) {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Returns true when the descriptor header and bounds are valid.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_INIT_MAGIC
            && self.version == DRIVER_RUNTIME_INIT_VERSION
            && self.len as usize == core::mem::size_of::<Self>()
            && self.hot_path >= HOT_PATH_SERIAL_CONSOLE
            && self.hot_path <= HOT_PATH_PCIE_ROOT
            && self.role_bit != 0
            && (self.flags & DRIVER_RUNTIME_INIT_REQUIRED_FLAGS)
                == DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            && self.flags & !DRIVER_RUNTIME_INIT_ALLOWED_FLAGS == 0
            && self.shared_page_count != 0
            && (self.mmio_page_count as usize) <= DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES
            && (self.dma_page_count as usize) <= DRIVER_RUNTIME_INIT_MAX_DMA_PAGES
            && (self.shared_page_count as usize) <= DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES
            && (self.irq_count as usize) <= DRIVER_RUNTIME_INIT_MAX_IRQS
            && (self.bus_link_count as usize) <= DRIVER_RUNTIME_INIT_MAX_BUS_LINKS
            && (self.resource_range_count as usize) <= DRIVER_RUNTIME_INIT_MAX_RESOURCE_RANGES
            && self.root_wake_notification_valid()
            && self.direct_genet_link_valid()
            && self.command_cap_reserved == 0
            && self.reserved_tail == 0
            && self.mcs_scheduler_valid()
            && if self.irq_count == 0 {
                (self.flags & DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY) != 0
            } else {
                (self.flags & DRIVER_RUNTIME_INIT_FLAG_IRQS_BOUND) != 0
            }
            && if self.bus_link_count == 0 {
                true
            } else {
                (self.flags & DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS) != 0
            }
            && self.valid_resource_ranges()
            && self.valid_irqs()
            && self.valid_bus_links()
    }

    /// Returns true when the scheduler inventory is exact and least-authority.
    #[must_use]
    pub const fn mcs_scheduler_valid(self) -> bool {
        self.scheduler_magic == DRIVER_RUNTIME_MCS_MAGIC
            && self.scheduler_version == DRIVER_RUNTIME_MCS_VERSION
            && self.scheduler_flags == DRIVER_RUNTIME_MCS_REQUIRED_FLAGS
            && self.scheduling_context_slot != 0
            && self.scheduling_context_bits != 0
            && self.sched_control_core < 4
            && self.affinity_core == self.sched_control_core
            && self.max_refills >= 2
            && self.budget_us != 0
            && self.period_us != 0
            && self.budget_us <= self.period_us
            && self.command_endpoint_slot == DRIVER_RUNTIME_COMMAND_ENDPOINT_SLOT
            && self.command_reply_slot == DRIVER_RUNTIME_COMMAND_REPLY_SLOT
            && self.irq_notification_slot == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
            && self.completion_notification_slot == DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT
            && self.root_control_wake_notification_slot
                == DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT
            && self.command_badge == driver_runtime_command_badge(self.task_key)
            && self.completion_badge == driver_runtime_completion_badge(self.task_key)
            && self.standard_fault_badge != 0
            && self.standard_fault_badge != driver_runtime_standard_fault_badge(self.task_key)
            && self.timeout_fault_badge != 0
            && self.command_badge != self.completion_badge
            && self.command_badge != self.standard_fault_badge
            && self.command_badge != self.timeout_fault_badge
            && self.completion_badge != self.standard_fault_badge
            && self.completion_badge != self.timeout_fault_badge
            && self.standard_fault_badge != self.timeout_fault_badge
            && self.standard_fault_reply_lane == DRIVER_RUNTIME_STANDARD_FAULT_REPLY_LANE
            && self.timeout_fault_reply_lane == DRIVER_RUNTIME_TIMEOUT_FAULT_REPLY_LANE
            && self.standard_fault_reply_lane != self.timeout_fault_reply_lane
            && self.max_inflight_commands == DRIVER_RUNTIME_MAX_INFLIGHT_COMMANDS
            && self.scheduler_reserved == 0
    }

    /// Returns true when the optional child-to-root wake authority is absent or exact.
    ///
    /// Only CYW43 may receive this capability. The fixed slot and badge prevent
    /// descriptor-controlled authority from aliasing another child capability.
    #[must_use]
    pub const fn root_wake_notification_valid(self) -> bool {
        (self.root_wake_notification_slot == 0 && self.root_wake_notification_badge == 0)
            || (self.hot_path == HOT_PATH_CYW43_WIFI
                && self.root_wake_notification_slot
                    == DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT
                && self.root_wake_notification_badge
                    == DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE)
    }

    /// Returns true only for an absent link or the exact GENET-only CPU link.
    #[must_use]
    pub const fn direct_genet_link_valid(self) -> bool {
        let enabled = self.flags & DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET != 0;
        let mut direct_ranges = 0usize;
        let mut ranges_valid = true;
        let mut legacy_shared_pages_cpu_only = true;
        let mut shared_index = 0usize;
        while shared_index < self.shared_page_count as usize {
            if self.shared_pages[shared_index].paddr != 0 {
                legacy_shared_pages_cpu_only = false;
            }
            shared_index += 1;
        }
        let mut index = 0usize;
        while index < self.resource_range_count as usize {
            let range = self.resource_ranges[index];
            let direct_tag = range.tag == DRIVER_RUNTIME_RESOURCE_TAG_GENET_DIRECT_LINK;
            let cpu_only = range.flags & DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY != 0;
            if enabled && range.kind == DRIVER_RUNTIME_RESOURCE_KIND_SHARED && !direct_tag {
                ranges_valid = false;
            }
            if direct_tag || cpu_only {
                if !enabled || !direct_tag || !cpu_only {
                    ranges_valid = false;
                } else {
                    direct_ranges += 1;
                    let exact_flags = DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                        | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED
                        | DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY;
                    if range.kind != DRIVER_RUNTIME_RESOURCE_KIND_SHARED
                        || range.flags != exact_flags
                        || range.vaddr != self.shared_vaddr_base
                        || range.paddr != 0
                        || range.bytes
                            != DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT as u64
                                * DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES as u64
                        || range.page_count != DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT
                        || range.first_page_index != 0
                        || range.reserved != 0
                    {
                        ranges_valid = false;
                    }
                }
            }
            index += 1;
        }
        if enabled {
            self.hot_path == HOT_PATH_GENET_NIC
                && self.bus_link_count == 0
                && self.shared_page_count == DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES as u16
                && legacy_shared_pages_cpu_only
                && self.direct_genet.valid()
                && direct_ranges == 1
                && ranges_valid
        } else {
            self.direct_genet.empty_valid() && direct_ranges == 0 && ranges_valid
        }
    }

    /// Returns true when populated resource ranges are valid.
    #[must_use]
    pub const fn valid_resource_ranges(self) -> bool {
        let mut index = 0;
        while index < self.resource_range_count as usize {
            if !self.resource_ranges[index].valid() {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Returns true when populated bus-link descriptors are valid.
    #[must_use]
    pub const fn valid_bus_links(self) -> bool {
        let mut index = 0;
        while index < self.bus_link_count as usize {
            if !self.bus_links[index].valid() {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Returns true when every populated IRQ handoff is structurally valid.
    #[must_use]
    pub const fn valid_irqs(self) -> bool {
        let mut index = 0usize;
        while index < self.irq_count as usize {
            if !self.irqs[index].valid() {
                return false;
            }
            let mut prior = 0usize;
            while prior < index {
                if self.irqs[prior].handler_slot == self.irqs[index].handler_slot
                    || self.irqs[prior].badge & self.irqs[index].badge != 0
                {
                    return false;
                }
                prior += 1;
            }
            index += 1;
        }
        true
    }

    /// Returns true when this descriptor matches one generated runtime spec.
    #[must_use]
    pub const fn valid_for_resources(
        self,
        hot_path: u32,
        role_bit: u32,
        mmio_pages: u16,
        dma_pages: u16,
        shared_pages: u16,
    ) -> bool {
        let mmio_total =
            self.resource_pages_or_count(DRIVER_RUNTIME_RESOURCE_KIND_MMIO, self.mmio_page_count);
        let dma_total =
            self.resource_pages_or_count(DRIVER_RUNTIME_RESOURCE_KIND_DMA, self.dma_page_count);
        let shared_total = self
            .resource_pages_or_count(DRIVER_RUNTIME_RESOURCE_KIND_SHARED, self.shared_page_count);
        self.valid()
            && self.hot_path == hot_path
            && self.role_bit == role_bit
            && mmio_total == mmio_pages
            && dma_total == dma_pages
            && shared_total == shared_pages
    }

    /// Returns true when this descriptor is eligible to back HDMI ownership.
    #[must_use]
    pub const fn hdmi_ready(self) -> bool {
        self.valid()
            && self.hot_path == HOT_PATH_HDMI_TEXT
            && (self.flags & DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER) != 0
            && self.framebuffer.valid()
    }

    /// Returns total pages for one resource kind, or the legacy count when no
    /// semantic ranges were supplied.
    #[must_use]
    pub const fn resource_pages_or_count(self, kind: u16, fallback: u16) -> u16 {
        let pages = self.resource_pages_by_kind(kind);
        if pages == 0 {
            fallback
        } else {
            pages
        }
    }

    /// Returns total pages for one resource kind.
    #[must_use]
    pub const fn resource_pages_by_kind(self, kind: u16) -> u16 {
        let mut total = 0u16;
        let mut index = 0;
        while index < self.resource_range_count as usize {
            let range = self.resource_ranges[index];
            if range.kind == kind {
                total = total.saturating_add(range.page_count);
            }
            index += 1;
        }
        total
    }

    /// Returns total pages for one resource kind and tag.
    #[must_use]
    pub const fn resource_pages_by_kind_and_tag(self, kind: u16, tag: u32) -> u16 {
        let mut total = 0u16;
        let mut index = 0;
        while index < self.resource_range_count as usize {
            let range = self.resource_ranges[index];
            if range.kind == kind && range.tag == tag {
                total = total.saturating_add(range.page_count);
            }
            index += 1;
        }
        total
    }

    /// Returns true when the descriptor includes one matching resource range.
    #[must_use]
    pub const fn has_resource_range(self, kind: u16, tag: u32) -> bool {
        self.resource_pages_by_kind_and_tag(kind, tag) != 0
    }

    /// Returns true when a matching range starts at the expected driver-local
    /// virtual address and carries at least `min_pages` pages.
    #[must_use]
    pub const fn has_resource_range_at(
        self,
        kind: u16,
        tag: u32,
        expected_vaddr: u64,
        min_pages: u16,
    ) -> bool {
        self.has_resource_range_at_with_flags(kind, tag, expected_vaddr, min_pages, 0)
    }

    /// Returns true when a matching range starts at the expected driver-local
    /// virtual address, carries at least `min_pages`, and includes all
    /// `required_flags`.
    #[must_use]
    pub const fn has_resource_range_at_with_flags(
        self,
        kind: u16,
        tag: u32,
        expected_vaddr: u64,
        min_pages: u16,
        required_flags: u16,
    ) -> bool {
        let mut index = 0;
        while index < self.resource_range_count as usize {
            let range = self.resource_ranges[index];
            if range.kind == kind
                && range.tag == tag
                && range.vaddr == expected_vaddr
                && range.page_count >= min_pages
                && (range.flags & required_flags) == required_flags
            {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Returns true when the descriptor includes a bus link to `owner_hot_path`.
    #[must_use]
    pub const fn has_bus_link_to(self, owner_hot_path: u32) -> bool {
        let mut index = 0;
        while index < self.bus_link_count as usize {
            if self.bus_links[index].owner_hot_path == owner_hot_path {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Returns true when the descriptor includes the exact pointer-free bus
    /// channel required by a split runtime.
    #[must_use]
    pub const fn has_pointer_free_bus_link(self, owner_hot_path: u32, channel_id: u32) -> bool {
        let mut index = 0;
        while index < self.bus_link_count as usize {
            let link = self.bus_links[index];
            if link.owner_hot_path == owner_hot_path
                && link.channel_id == channel_id
                && (link.flags & DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE) != 0
            {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Returns true when the descriptor includes a pointer-free bus link sealed
    /// for this descriptor's runtime task.
    #[must_use]
    pub const fn has_sealed_pointer_free_bus_link(
        self,
        task_key: u32,
        owner_hot_path: u32,
        channel_id: u32,
    ) -> bool {
        let mut index = 0;
        while index < self.bus_link_count as usize {
            let link = self.bus_links[index];
            if link.owner_hot_path == owner_hot_path
                && link.channel_id == channel_id
                && link.sealed_for_client(task_key, self.hot_path)
            {
                return true;
            }
            index += 1;
        }
        false
    }
}

#[cfg(test)]
#[allow(clippy::assertions_on_constants)]
mod tests {
    use super::*;

    fn mcs_descriptor() -> DriverRuntimeInitDescriptor {
        DriverRuntimeInitDescriptor::empty().with_mcs_scheduler(
            0,
            10,
            8,
            1,
            2,
            1,
            500,
            10_000,
            0x26e2_0000,
            0x26ed_000b,
        )
    }

    fn direct_genet_descriptor() -> DriverRuntimeInitDescriptor {
        let mut descriptor = mcs_descriptor().with_direct_genet();
        descriptor.hot_path = HOT_PATH_GENET_NIC;
        descriptor.role_bit = 1 << 3;
        descriptor.flags |= DRIVER_RUNTIME_INIT_REQUIRED_FLAGS | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.shared_vaddr_base = 0x70c0_0000;
        descriptor.shared_page_count = DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES as u16;
        // The semantic direct-link range carries the exact CPU mapping. The
        // bounded legacy page array must not reveal physical DMA authority.
        descriptor.shared_pages =
            [DriverRuntimePageDescriptor::empty(); DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES];
        descriptor.resource_range_count = 1;
        descriptor.resource_ranges[0] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED
                | DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_DIRECT_LINK,
            descriptor.shared_vaddr_base,
            0,
            DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT as u64
                * DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES as u64,
            DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT,
            0,
        );
        descriptor
    }

    #[test]
    fn init_descriptor_is_bounded_for_ring_payload() {
        assert_eq!(DRIVER_RUNTIME_INIT_DESCRIPTOR_APERTURE_BYTES, 1664);
        assert!(
            core::mem::size_of::<DriverRuntimeInitDescriptor>()
                <= usize::from(DRIVER_RUNTIME_INIT_DESCRIPTOR_APERTURE_BYTES),
            "descriptor bytes={}",
            core::mem::size_of::<DriverRuntimeInitDescriptor>()
        );
        assert!(
            usize::from(DRIVER_RUNTIME_RING_FRAME_OFFSET)
                + core::mem::size_of::<DriverRuntimeInitDescriptor>()
                <= usize::from(DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET)
        );
        assert_eq!(core::mem::align_of::<DriverRuntimeInitDescriptor>(), 8);
        assert!(DRIVER_RUNTIME_INIT_MAX_DMA_PAGES >= 80);
    }

    #[test]
    fn usb_oldgood_receipt_is_fixed_commit_last_identity_bound_and_fail_closed() {
        assert_eq!(DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_OFFSET, 192);
        assert_eq!(DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_BYTES, 48);
        assert_eq!(core::mem::size_of::<DriverRuntimeUsbOldgoodReceipt>(), 48);
        assert_eq!(core::mem::align_of::<DriverRuntimeUsbOldgoodReceipt>(), 4);
        assert_eq!(
            core::mem::offset_of!(
                DriverRuntimeUsbOldgoodReceipt,
                committed_publication_sequence
            ),
            44
        );
        assert_eq!(DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK, 0x0000_3fff);
        assert!(
            DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_OFFSET + DRIVER_RUNTIME_USB_OLDGOOD_RECEIPT_BYTES
                <= DRIVER_RUNTIME_RING_FRAME_OFFSET
        );

        let staged = DriverRuntimeUsbOldgoodReceipt::new(1, 2, 3, 4, 5);
        assert!(
            !staged.valid(),
            "publication remains uncommitted until the last word"
        );
        let begun = staged.commit();
        assert!(begun.valid());
        assert!(!begun.complete());
        assert_eq!(
            DriverRuntimeUsbOldgoodReceipt::stable_snapshot(begun, begun),
            Some(begun)
        );

        let complete = DriverRuntimeUsbOldgoodReceipt {
            publication_sequence: 14,
            step_mask: DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK,
            topology: 0x1132_0381,
            input_generation: 17,
            committed_publication_sequence: 0,
            ..staged
        }
        .commit();
        assert!(complete.valid());
        assert!(complete.complete());

        assert!(!DriverRuntimeUsbOldgoodReceipt {
            committed_publication_sequence: 13,
            ..complete
        }
        .valid());
        assert!(!DriverRuntimeUsbOldgoodReceipt {
            identity_token: 0,
            ..complete
        }
        .valid());
        assert!(!DriverRuntimeUsbOldgoodReceipt {
            step_mask: DRIVER_RUNTIME_USB_OLDGOOD_STEP_XHCI_READY
                | DRIVER_RUNTIME_USB_OLDGOOD_STEP_ROOT_PORT_RESET,
            topology: 0,
            input_generation: 0,
            committed_publication_sequence: 14,
            ..complete
        }
        .valid());
        assert!(!DriverRuntimeUsbOldgoodReceipt {
            topology: 0,
            ..complete
        }
        .valid());
        assert!(!DriverRuntimeUsbOldgoodReceipt {
            input_generation: 0,
            ..complete
        }
        .valid());

        let poisoned = DriverRuntimeUsbOldgoodReceipt {
            step_mask: DRIVER_RUNTIME_USB_OLDGOOD_STEP_MASK
                | DRIVER_RUNTIME_USB_OLDGOOD_INVALID_ORDER,
            committed_publication_sequence: 0,
            ..complete
        }
        .commit();
        assert!(poisoned.valid());
        assert!(poisoned.poisoned());
        assert!(!poisoned.complete());
        assert_eq!(
            DriverRuntimeUsbOldgoodReceipt::stable_snapshot(complete, poisoned),
            None
        );
        assert!(!DriverRuntimeUsbOldgoodReceipt::zeroed().valid());
    }

    #[test]
    fn cyw43_shared_payload_is_one_exact_backplane_aperture() {
        assert_eq!(DRIVER_RUNTIME_CYW43_BACKPLANE_APERTURE_BYTES, 0x8000);
        assert_eq!(DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES, 32 * 1024);
        assert_eq!(DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES, 8);
        assert_eq!(
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
                + DRIVER_RUNTIME_CYW43_BACKPLANE_APERTURE_BYTES
        );
        assert_eq!(
            DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_OFFSET,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE
                + DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES
        );
        assert_eq!(
            DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_BYTES,
            7 * DRIVER_RUNTIME_RING_PAGE_BYTES
        );
        assert_eq!(
            DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_OFFSET as u32
                + DRIVER_RUNTIME_CYW43_RX_SHARED_PAYLOAD_BYTES as u32,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET as u32
        );
        assert!(DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES <= DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES);
    }

    #[test]
    fn cyw43_rx_batch_geometry_is_exact_and_disjoint_from_sdio_scratch() {
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_FIRST_SHARED_PAGE, 8);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_SHARED_PAGES, 4);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_REQUIRED_SHARED_PAGES, 12);
        assert_eq!(
            DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_END_OFFSET
        );
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_OFFSET, 36_864);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_RECORD_BYTES, 128);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_OFFSET, 36_992);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_STRIDE, 1_536);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_OFFSET, 49_280);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES, 64);
        assert_eq!(DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET, 49_344);
        assert_eq!(DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES, 128);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_END_OFFSET, 53_248);
        assert!(
            DRIVER_RUNTIME_CYW43_RX_BATCH_REQUIRED_SHARED_PAGES
                <= DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES
        );

        let mut index = 0;
        while index < DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP {
            assert_eq!(
                driver_runtime_cyw43_rx_batch_payload_offset(index),
                Some(
                    DRIVER_RUNTIME_CYW43_RX_BATCH_PAYLOAD_OFFSET as u32
                        + index as u32 * DRIVER_RUNTIME_CYW43_RX_BATCH_FRAME_BYTES as u32
                )
            );
            index += 1;
        }
        assert_eq!(
            driver_runtime_cyw43_rx_batch_payload_offset(DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP),
            None
        );
        assert!(
            driver_runtime_cyw43_rx_batch_payload_offset(
                DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP - 1
            )
            .unwrap()
                + DRIVER_RUNTIME_CYW43_RX_BATCH_FRAME_BYTES as u32
                == DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_OFFSET as u32
        );
        assert!(
            DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET as u32
                + DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES as u32
                <= DRIVER_RUNTIME_CYW43_RX_BATCH_END_OFFSET as u32
        );
    }

    #[test]
    fn cyw43_rx_queue_state_is_sequence_last_and_level_triggered() {
        assert_eq!(
            core::mem::size_of::<DriverRuntimeCyw43RxQueueState>(),
            DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES as usize
        );
        assert_eq!(core::mem::align_of::<DriverRuntimeCyw43RxQueueState>(), 4);
        assert!(
            DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_OFFSET + DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_BYTES
                <= DRIVER_RUNTIME_RING_FRAME_OFFSET
        );

        let empty = DriverRuntimeCyw43RxQueueState::empty();
        assert!(empty.valid());
        assert!(!empty.committed());
        assert!(!empty.work_visible());
        assert_eq!(empty.next_commit_sequence(), Some(1));
        assert_eq!(
            DriverRuntimeCyw43RxQueueState::stable_snapshot(
                DriverRuntimeCyw43RxQueueState::zeroed(),
                DriverRuntimeCyw43RxQueueState::zeroed(),
            ),
            Some(empty)
        );

        let staged = DriverRuntimeCyw43RxQueueState {
            generation: 7,
            queue_depth: 3,
            ..empty
        };
        assert!(staged.body_valid());
        assert!(!staged.valid());
        assert_eq!(staged.next_commit_sequence(), None);

        let committed = DriverRuntimeCyw43RxQueueState {
            commit_sequence: 9,
            ..staged
        };
        assert!(committed.valid());
        assert!(committed.committed());
        assert!(committed.work_visible());
        assert_eq!(committed.next_commit_sequence(), Some(10));
        assert_eq!(
            DriverRuntimeCyw43RxQueueState::stable_snapshot(committed, committed),
            Some(committed)
        );

        let changed = DriverRuntimeCyw43RxQueueState {
            queue_depth: 2,
            commit_sequence: 10,
            ..committed
        };
        assert_eq!(
            DriverRuntimeCyw43RxQueueState::stable_snapshot(committed, changed),
            None,
            "a queue update cannot be assembled from two generations"
        );

        let poisoned = DriverRuntimeCyw43RxQueueState {
            flags: DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_FLAG_POISONED,
            recovery_source_line: 39_579,
            commit_sequence: 10,
            ..committed
        };
        assert!(poisoned.valid());
        assert!(poisoned.poisoned());
        assert_eq!(poisoned.recovery_source_line(), Some(39_579));
        assert!(!poisoned.work_visible());

        let missing_source = DriverRuntimeCyw43RxQueueState {
            recovery_source_line: 0,
            ..poisoned
        };
        assert!(!missing_source.body_valid());

        let overflow = DriverRuntimeCyw43RxQueueState {
            queue_depth: DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP as u16 + 1,
            ..committed
        };
        assert!(!overflow.body_valid());
        assert_eq!(
            DriverRuntimeCyw43RxQueueState::stable_snapshot(overflow, overflow),
            None
        );
    }

    #[test]
    fn cyw43_rx_stage_delta_q11_quantization_and_packing_are_exact() {
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SHIFT, 11);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED, 0xffff,);
        assert_eq!(driver_runtime_cyw43_rx_stage_delta_q11(7, 7), 0);
        assert_eq!(
            driver_runtime_cyw43_rx_stage_delta_q11(1, 1 + ((1 << 11) - 1)),
            0,
            "quantization floors sub-Q11 intervals",
        );
        assert_eq!(driver_runtime_cyw43_rx_stage_delta_q11(1, 1 + (1 << 11)), 1,);
        assert_eq!(
            driver_runtime_cyw43_rx_stage_delta_q11(0xffff_f800, 0x0000_0800),
            2,
            "low-word wrap preserves a short modulo-32 interval",
        );
        assert_eq!(
            driver_runtime_cyw43_rx_stage_delta_q11(0, 0xfffe << 11),
            0xfffe,
        );
        assert_eq!(
            driver_runtime_cyw43_rx_stage_delta_q11(0, 0xffff << 11),
            DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
        );
        assert_eq!(
            driver_runtime_cyw43_rx_stage_delta_q11(0, u32::MAX),
            DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
        );

        let packed = driver_runtime_cyw43_rx_stage_deltas_q11_pack(
            0,
            DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
        );
        assert_eq!(
            driver_runtime_cyw43_rx_stage_deltas_q11_source_to_queue(packed),
            0,
            "a raw zero interval remains valid",
        );
        assert_eq!(
            driver_runtime_cyw43_rx_stage_deltas_q11_queue_to_precommit(packed),
            DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
        );
    }

    #[test]
    fn cyw43_rx_batch_requires_exact_slots_and_final_parent_commit() {
        assert_eq!(core::mem::size_of::<DriverRuntimeCyw43RxBatchEntry>(), 8);
        assert_eq!(core::mem::size_of::<DriverRuntimeCyw43RxBatchRecord>(), 128);
        assert_eq!(core::mem::align_of::<DriverRuntimeCyw43RxBatchRecord>(), 4);
        assert_eq!(DRIVER_RUNTIME_CYW43_RX_BATCH_VERSION, 3);
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, entries),
            24
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, source_cntvct_lo),
            88
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, first_data_stage_deltas_q11),
            120
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeCyw43RxBatchRecord, committed_parent_sequence),
            124
        );

        let mut entries =
            [DriverRuntimeCyw43RxBatchEntry::empty(); DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP];
        entries[0] = DriverRuntimeCyw43RxBatchEntry {
            offset: driver_runtime_cyw43_rx_batch_payload_offset(0).unwrap(),
            len: 64,
            flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT | (3 << 8),
        };
        entries[1] = DriverRuntimeCyw43RxBatchEntry {
            offset: driver_runtime_cyw43_rx_batch_payload_offset(1).unwrap(),
            len: DRIVER_RUNTIME_CYW43_RX_BATCH_FRAME_BYTES,
            flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA | (12 << 8),
        };

        let source_cntvct_lo = [0, 0x9abc_def0, 0, 0, 0, 0, 0, 0];

        let staged =
            DriverRuntimeCyw43RxBatchRecord::staged(41, 7, 9, 2, 3, entries, source_cntvct_lo, 0);
        assert!(staged.body_valid());
        assert_eq!(staged.first_data_stage_deltas_q11, 0, "raw zero is valid");
        assert!(!staged.committed());
        assert!(!staged.valid());
        assert_eq!(
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(staged, staged),
            None
        );

        let mut event_only_entries =
            [DriverRuntimeCyw43RxBatchEntry::empty(); DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP];
        event_only_entries[0] = entries[0];
        let event_only_nonzero = DriverRuntimeCyw43RxBatchRecord::staged(
            42,
            7,
            10,
            1,
            0,
            event_only_entries,
            [0; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
            1,
        );
        assert!(
            event_only_nonzero.body_valid(),
            "passive first-data evidence cannot change batch validity",
        );
        assert!(DriverRuntimeCyw43RxBatchRecord::staged(
            42,
            7,
            10,
            1,
            0,
            event_only_entries,
            [0; DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP],
            0,
        )
        .body_valid());

        let committed = staged.commit();
        assert!(committed.body_valid());
        assert!(committed.committed());
        assert!(committed.valid());
        assert_eq!(committed.source_cntvct_lo[0], 0);
        assert_eq!(
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(committed, committed),
            Some(committed)
        );

        let mut prior_version = committed;
        prior_version.version = 2;
        assert!(!prior_version.body_valid());
        assert_eq!(
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(prior_version, prior_version),
            None,
            "a v2 reader/writer mismatch must fail closed",
        );

        let mut changed_source = committed;
        changed_source.source_cntvct_lo[1] ^= 1;
        assert!(changed_source.valid());
        assert_eq!(
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(committed, changed_source),
            None,
            "source-only change must not form one stable snapshot",
        );

        let mut changed_stage_deltas = committed;
        changed_stage_deltas.first_data_stage_deltas_q11 = 0x1234_5678;
        assert!(changed_stage_deltas.valid());
        let degraded_stage_snapshot =
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(committed, changed_stage_deltas)
                .expect("passive stage evidence cannot reject an exact batch");
        assert_eq!(
            degraded_stage_snapshot.first_data_stage_deltas_q11,
            driver_runtime_cyw43_rx_stage_deltas_q11_pack(
                DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
                DRIVER_RUNTIME_CYW43_RX_STAGE_DELTA_Q11_SATURATED,
            ),
            "a torn passive word degrades only that evidence to unknown",
        );

        let queue_state = DriverRuntimeCyw43RxQueueState {
            generation: 7,
            queue_depth: 3,
            commit_sequence: 9,
            ..DriverRuntimeCyw43RxQueueState::empty()
        };
        assert!(committed.valid_for_queue_state(queue_state));
        assert!(committed.valid_for_parent_and_queue_state(41, queue_state));
        assert!(!committed.valid_for_parent_and_queue_state(42, queue_state));
        let later_enqueue = DriverRuntimeCyw43RxQueueState {
            queue_depth: 4,
            commit_sequence: 10,
            ..queue_state
        };
        assert!(
            committed.valid_for_queue_state(later_enqueue),
            "a later same-generation enqueue cannot invalidate an immutable batch"
        );
        assert!(
            !committed.valid_for_queue_state(DriverRuntimeCyw43RxQueueState {
                commit_sequence: 8,
                ..queue_state
            })
        );
        assert!(
            !committed.valid_for_queue_state(DriverRuntimeCyw43RxQueueState {
                generation: 8,
                commit_sequence: 10,
                ..queue_state
            })
        );
        assert!(
            !committed.valid_for_queue_state(DriverRuntimeCyw43RxQueueState {
                flags: DRIVER_RUNTIME_CYW43_RX_QUEUE_STATE_FLAG_POISONED,
                commit_sequence: 10,
                ..queue_state
            })
        );

        let torn_commit = DriverRuntimeCyw43RxBatchRecord {
            committed_parent_sequence: committed.parent_sequence + 1,
            ..committed
        };
        assert!(torn_commit.body_valid());
        assert!(!torn_commit.valid());
        assert_eq!(
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(torn_commit, torn_commit),
            None
        );
        assert_eq!(
            DriverRuntimeCyw43RxBatchRecord::stable_snapshot(staged, committed),
            None,
            "a reader cannot combine staged and committed samples"
        );

        let mut wrong_slot = committed;
        wrong_slot.entries[1].offset += 1;
        assert!(!wrong_slot.body_valid());

        let mut invalid_channel = committed;
        invalid_channel.entries[0].flags = 3;
        assert!(!invalid_channel.body_valid());

        let mut stale_tail = committed;
        stale_tail.entries[2] = DriverRuntimeCyw43RxBatchEntry {
            offset: driver_runtime_cyw43_rx_batch_payload_offset(2).unwrap(),
            len: 1,
            flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL,
        };
        assert!(!stale_tail.body_valid());

        let mut stale_source = committed;
        stale_source.source_cntvct_lo[2] = 1;
        assert!(!stale_source.body_valid());
    }

    #[test]
    fn cyw43_rx_batch_ack_commits_last_and_matches_exact_batch() {
        assert_eq!(core::mem::size_of::<DriverRuntimeCyw43RxBatchAck>(), 64);
        assert_eq!(core::mem::align_of::<DriverRuntimeCyw43RxBatchAck>(), 64);
        assert_eq!(
            core::mem::offset_of!(
                DriverRuntimeCyw43RxBatchAck,
                committed_queue_commit_sequence
            ),
            60
        );

        let empty = DriverRuntimeCyw43RxBatchAck::empty();
        assert!(!empty.body_valid());
        assert!(!empty.valid());
        assert_eq!(
            DriverRuntimeCyw43RxBatchAck::stable_snapshot(empty, empty),
            None
        );

        let mut entries =
            [DriverRuntimeCyw43RxBatchEntry::empty(); DRIVER_RUNTIME_CYW43_RX_BATCH_ENTRY_CAP];
        entries[0] = DriverRuntimeCyw43RxBatchEntry {
            offset: driver_runtime_cyw43_rx_batch_payload_offset(0).unwrap(),
            len: 96,
            flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT | (4 << 8),
        };
        entries[1] = DriverRuntimeCyw43RxBatchEntry {
            offset: driver_runtime_cyw43_rx_batch_payload_offset(1).unwrap(),
            len: 512,
            flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA | (5 << 8),
        };
        let source_cntvct_lo = [0x0102_0304, 0x0506_0708, 0, 0, 0, 0, 0, 0];
        let batch = DriverRuntimeCyw43RxBatchRecord::staged(
            73,
            11,
            19,
            2,
            4,
            entries,
            source_cntvct_lo,
            0x1122_3344,
        )
        .commit();
        assert!(batch.valid());

        let staged = DriverRuntimeCyw43RxBatchAck::staged(
            batch.generation,
            batch.parent_sequence,
            batch.queue_commit_sequence,
            batch.count,
        );
        assert!(staged.body_valid());
        assert!(!staged.valid());
        assert!(!staged.matches_batch(batch));
        assert_eq!(
            DriverRuntimeCyw43RxBatchAck::stable_snapshot(staged, staged),
            None
        );

        let committed = staged.commit();
        assert!(committed.body_valid());
        assert!(committed.valid());
        assert!(committed.matches_batch(batch));
        assert_eq!(
            DriverRuntimeCyw43RxBatchAck::stable_snapshot(committed, committed),
            Some(committed)
        );
        assert_eq!(
            DriverRuntimeCyw43RxBatchAck::stable_snapshot(staged, committed),
            None,
            "a reader cannot combine staged and committed ACK samples"
        );

        let wrong_generation = DriverRuntimeCyw43RxBatchAck {
            generation: committed.generation + 1,
            ..committed
        };
        assert!(wrong_generation.valid());
        assert!(!wrong_generation.matches_batch(batch));
        let wrong_parent = DriverRuntimeCyw43RxBatchAck {
            parent_sequence: committed.parent_sequence + 1,
            ..committed
        };
        assert!(wrong_parent.valid());
        assert!(!wrong_parent.matches_batch(batch));
        let wrong_queue = DriverRuntimeCyw43RxBatchAck::staged(
            committed.generation,
            committed.parent_sequence,
            committed.queue_commit_sequence + 1,
            committed.count,
        )
        .commit();
        assert!(wrong_queue.valid());
        assert!(!wrong_queue.matches_batch(batch));
        let wrong_count = DriverRuntimeCyw43RxBatchAck {
            count: committed.count - 1,
            ..committed
        };
        assert!(wrong_count.valid());
        assert!(!wrong_count.matches_batch(batch));

        let torn_commit = DriverRuntimeCyw43RxBatchAck {
            committed_queue_commit_sequence: committed.queue_commit_sequence + 1,
            ..committed
        };
        assert!(torn_commit.body_valid());
        assert!(!torn_commit.valid());
        assert!(!torn_commit.matches_batch(batch));

        let mut nonzero_reserved = staged;
        nonzero_reserved.reserved[0] = 1;
        assert!(!nonzero_reserved.body_valid());
        assert!(!nonzero_reserved.commit().valid());

        let uncommitted_batch = DriverRuntimeCyw43RxBatchRecord {
            committed_parent_sequence: 0,
            ..batch
        };
        assert!(!committed.matches_batch(uncommitted_batch));
    }

    #[test]
    fn cyw43_dpc_child_timing_layout_is_bounded_and_sequence_last() {
        assert_eq!(DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_OFFSET, 1_920);
        assert_eq!(DRIVER_RUNTIME_SDIO_CHILD_TIMING_MAILBOX_BYTES, 64);
        assert_eq!(
            core::mem::size_of::<DriverRuntimeSdioChildTimingMailbox>(),
            64
        );
        assert_eq!(
            core::mem::offset_of!(
                DriverRuntimeSdioChildTimingMailbox,
                committed_child_sequence
            ),
            60,
        );
        assert_eq!(DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET, 49_472);
        assert_eq!(DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES, 512);
        assert_eq!(
            core::mem::size_of::<DriverRuntimeCyw43DpcChildTimingEntry>(),
            28
        );
        assert_eq!(
            core::mem::size_of::<DriverRuntimeCyw43DpcChildTimingRecord>(),
            512
        );
        assert_eq!(
            core::mem::offset_of!(
                DriverRuntimeCyw43DpcChildTimingRecord,
                committed_publication_sequence
            ),
            508,
        );

        let mut mailbox = DriverRuntimeSdioChildTimingMailbox {
            child_sequence: 7,
            descriptor_fingerprint: 0x1234_5678,
            physical_epoch: 3,
            event_sequence: 9,
            action: 1,
            io_kind: 1,
            io_phase: 4,
            engine: DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_COMMAND as u8,
            flags: DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_PUBLISHED
                | DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_INTAKE
                | DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_ISSUED
                | DRIVER_RUNTIME_SDIO_CHILD_TIMING_FLAG_TERMINAL,
            published_cntvct_lo: 0,
            intake_cntvct_lo: 1,
            issued_cntvct_lo: 2,
            terminal_cntvct_lo: 3,
            ..DriverRuntimeSdioChildTimingMailbox::empty()
        };
        assert!(
            mailbox.body_valid(),
            "raw low-word zero is valid timing evidence"
        );
        assert!(!mailbox.committed());
        mailbox.committed_child_sequence = mailbox.child_sequence;
        assert!(mailbox.committed());
        let mut invalid_mailbox = mailbox;
        invalid_mailbox.action = 20;
        assert!(!invalid_mailbox.committed());
        invalid_mailbox = mailbox;
        invalid_mailbox.io_kind = 7;
        assert!(!invalid_mailbox.committed());
        invalid_mailbox = mailbox;
        invalid_mailbox.io_phase = 5;
        assert!(!invalid_mailbox.committed());
        mailbox.committed_child_sequence = mailbox.child_sequence.wrapping_add(1);
        assert!(!mailbox.committed());

        let flags = DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_PUBLISHED
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_INTAKE
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_ISSUED
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_TERMINAL
            | DRIVER_RUNTIME_CYW43_DPC_CHILD_ENTRY_FLAG_ACCEPTED;
        let entry = DriverRuntimeCyw43DpcChildTimingEntry {
            child_sequence: 7,
            meta: DriverRuntimeCyw43DpcChildTimingEntry::pack_meta(
                1,
                1,
                4,
                DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_COMMAND as u8,
                flags,
            ),
            published_cntvct_lo: u32::MAX - 1,
            intake_cntvct_lo: u32::MAX,
            issued_cntvct_lo: 0,
            terminal_cntvct_lo: 1,
            accepted_cntvct_lo: 2,
        };
        assert!(entry.complete());
        let mut trace = DriverRuntimeCyw43DpcChildTimingRecord {
            publication_sequence: 11,
            physical_epoch: 3,
            event_sequence: 9,
            source_cntvct_lo: u32::MAX - 2,
            queue_commit_cntvct_lo: 3,
            queue_commit_sequence: 5,
            data_len: 64,
            child_count: 1,
            flags: DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_COMPLETE,
            entries: {
                let mut entries = [DriverRuntimeCyw43DpcChildTimingEntry::empty();
                    DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_ENTRY_CAP];
                entries[0] = entry;
                entries
            },
            ..DriverRuntimeCyw43DpcChildTimingRecord::empty()
        };
        assert!(trace.body_valid());
        assert!(!trace.committed());
        trace.committed_publication_sequence = trace.publication_sequence;
        assert!(trace.committed());
        trace.overall_max_source_to_queue_q11 = 1;
        assert!(
            !trace.committed(),
            "an exact trace must carry the selected worst sample"
        );
        trace.overall_max_source_to_queue_q11 = trace.selected_source_to_queue_q11;
        trace.entries[0].accepted_cntvct_lo ^= 1;
        assert!(
            trace.committed(),
            "an internally consistent timing-only change remains passive evidence",
        );
        trace.entries[0].accepted_cntvct_lo = trace.queue_commit_cntvct_lo.wrapping_add(1);
        assert!(
            !trace.committed(),
            "an inconsistent exact timing decomposition must be diagnostic UNKNOWN",
        );
        trace.flags |= DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_FLAG_UNKNOWN;
        assert!(
            trace.committed(),
            "inexact timing remains a committed passive UNKNOWN record",
        );
    }

    #[test]
    fn cyw43_dpc_client_layout_is_bounded_and_sequence_last() {
        assert_eq!(
            DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET,
            DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_OFFSET
                + DRIVER_RUNTIME_CYW43_DPC_CHILD_TIMING_BYTES,
        );
        assert_eq!(DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET, 49_984);
        assert_eq!(DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES, 128);
        assert_eq!(
            DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET + DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES,
            50_112,
        );
        assert!(
            DRIVER_RUNTIME_CYW43_DPC_CLIENT_OFFSET + DRIVER_RUNTIME_CYW43_DPC_CLIENT_BYTES
                <= DRIVER_RUNTIME_CYW43_RX_BATCH_END_OFFSET,
        );
        assert_eq!(
            core::mem::size_of::<DriverRuntimeCyw43DpcClientRecord>(),
            128,
        );
        assert_eq!(
            core::mem::align_of::<DriverRuntimeCyw43DpcClientRecord>(),
            64,
        );
        assert_eq!(
            core::mem::offset_of!(
                DriverRuntimeCyw43DpcClientRecord,
                committed_publication_sequence
            ),
            124,
        );

        let staged = DriverRuntimeCyw43DpcClientRecord {
            publication_sequence: 7,
            physical_epoch: 0x4359_1001,
            consumer_sequence: 23,
            rearms: 5,
            source_samples: 23,
            source_frame: 19,
            source_hostmail: 4,
            turns: 41,
            owner_children: 37,
            owner_turns: 44,
            frames_completed: 19,
            frame_turns: 35,
            frame_owner_turns: 39,
            ..DriverRuntimeCyw43DpcClientRecord::empty()
        };
        assert!(staged.body_valid());
        assert!(!staged.valid());
        let committed = staged.commit();
        assert!(committed.valid());
        assert_eq!(
            DriverRuntimeCyw43DpcClientRecord::from_le_words(committed.to_le_words()),
            committed,
        );
        assert_eq!(
            DriverRuntimeCyw43DpcClientRecord::stable_snapshot(committed, committed),
            Some(committed),
        );
        assert_eq!(
            DriverRuntimeCyw43DpcClientRecord::stable_snapshot(staged, committed),
            None,
        );
        let mut reserved = committed;
        reserved.reserved[0] = 1;
        assert!(!reserved.valid());
    }

    #[test]
    fn cyw43_bus_episode_layout_is_cache_isolated_and_sequence_last() {
        assert_eq!(
            DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET,
            DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_OFFSET + DRIVER_RUNTIME_CYW43_RX_BATCH_ACK_BYTES
        );
        assert!(DRIVER_RUNTIME_CYW43_BUS_EPISODE_OFFSET.is_multiple_of(64));
        assert_eq!(
            core::mem::size_of::<DriverRuntimeCyw43BusEpisodeRecord>(),
            DRIVER_RUNTIME_CYW43_BUS_EPISODE_BYTES as usize
        );
        assert_eq!(
            core::mem::align_of::<DriverRuntimeCyw43BusEpisodeRecord>(),
            64
        );
        assert_eq!(
            core::mem::offset_of!(
                DriverRuntimeCyw43BusEpisodeRecord,
                committed_publication_sequence
            ),
            124
        );

        let empty = DriverRuntimeCyw43BusEpisodeRecord::empty();
        assert!(!empty.body_valid());
        assert!(!empty.valid());

        let mut staged =
            DriverRuntimeCyw43BusEpisodeRecord::staged(DriverRuntimeCyw43BusEpisodeStart {
                publication_sequence: 11,
                episode_sequence: 7,
                logical_generation: 3,
                physical_epoch: 5,
                parent_sequence: 41,
                parent_op: DRIVER_RUNTIME_CYW43_OP_ETH_TX,
                cause: DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_FOREGROUND,
                first_cntvct: 100,
            });
        staged.last_cntvct = 160;
        staged.child_sequence = 17;
        staged.child_code = 1;
        staged.child_engine = DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_PIO;
        staged.child_irq_contract = DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_IRQ158;
        staged.dpc_sequence = 9;
        staged.op8_progress = 2;
        staged.rx_progress = 3;
        staged.tx_progress = 1;
        staged.final_pending_mask = DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RX;
        staged.exit_reason = DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_TERMINAL;
        staged.flags = DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_CHILD_TERMINAL
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_DPC_OBSERVED
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_OP8_PROGRESS
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_RX_PROGRESS
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_TX_PROGRESS;
        assert!(staged.body_valid());
        assert!(!staged.valid());
        assert_eq!(
            DriverRuntimeCyw43BusEpisodeRecord::stable_snapshot(staged, staged),
            None
        );

        let committed = staged.commit();
        assert!(committed.valid());
        assert_eq!(committed.committed_publication_sequence, 11);
        let words = committed.to_le_words();
        assert_eq!(words.len(), DRIVER_RUNTIME_CYW43_BUS_EPISODE_WORDS);
        assert_eq!(words[0], DRIVER_RUNTIME_CYW43_BUS_EPISODE_MAGIC);
        assert_eq!(words[31], committed.publication_sequence);
        assert_eq!(
            DriverRuntimeCyw43BusEpisodeRecord::from_le_words(words),
            committed,
        );
        assert_eq!(
            DriverRuntimeCyw43BusEpisodeRecord::stable_snapshot(committed, committed),
            Some(committed)
        );

        let mut active_child = staged;
        active_child.child_code = 0;
        active_child.child_detail = 0;
        active_child.child_result = 0;
        active_child.flags &= !DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_CHILD_TERMINAL;
        active_child.exit_reason = DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_PREWAIT_CHECKPOINT;
        assert!(active_child.body_valid());
        assert!(active_child.commit().valid());
        active_child.child_engine = DRIVER_RUNTIME_CYW43_BUS_EPISODE_CHILD_ENGINE_NONE;
        active_child.child_irq_contract = 0;
        assert!(!active_child.body_valid());

        let persistent_op11 =
            DriverRuntimeCyw43BusEpisodeRecord::staged(DriverRuntimeCyw43BusEpisodeStart {
                publication_sequence: 12,
                episode_sequence: 8,
                logical_generation: 0,
                physical_epoch: 5,
                parent_sequence: 42,
                parent_op: DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE,
                cause: DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_FOREGROUND,
                first_cntvct: 170,
            });
        assert!(persistent_op11.body_valid());
        assert!(persistent_op11.commit().valid());
    }

    #[test]
    fn cyw43_bus_episode_rejects_torn_and_wrong_version_publications() {
        let mut staged =
            DriverRuntimeCyw43BusEpisodeRecord::staged(DriverRuntimeCyw43BusEpisodeStart {
                publication_sequence: 19,
                episode_sequence: 8,
                logical_generation: 0,
                physical_epoch: 6,
                parent_sequence: 0,
                parent_op: 0,
                cause: DRIVER_RUNTIME_CYW43_BUS_EPISODE_CAUSE_DPC,
                first_cntvct: 200,
            });
        staged.last_cntvct = 230;
        staged.dpc_sequence = 13;
        staged.final_pending_mask = DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_DPC
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_RX
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_ACK_PENDING
            | DRIVER_RUNTIME_CYW43_BUS_EPISODE_PENDING_CARD_INT_MASKED;
        staged.exit_reason = DRIVER_RUNTIME_CYW43_BUS_EPISODE_EXIT_FAIRNESS;
        staged.flags = DRIVER_RUNTIME_CYW43_BUS_EPISODE_FLAG_DPC_OBSERVED;
        let committed = staged.commit();
        assert!(committed.valid());

        let torn = DriverRuntimeCyw43BusEpisodeRecord {
            committed_publication_sequence: committed.publication_sequence + 1,
            ..committed
        };
        assert!(torn.body_valid());
        assert!(!torn.valid());
        assert_eq!(
            DriverRuntimeCyw43BusEpisodeRecord::stable_snapshot(torn, torn),
            None
        );
        assert_eq!(
            DriverRuntimeCyw43BusEpisodeRecord::stable_snapshot(staged, committed),
            None,
            "a reader cannot combine staged and committed publications"
        );

        let wrong_version = DriverRuntimeCyw43BusEpisodeRecord {
            version: DRIVER_RUNTIME_CYW43_BUS_EPISODE_VERSION + 1,
            ..committed
        };
        assert!(!wrong_version.body_valid());
        assert!(!wrong_version.valid());
        assert_eq!(
            DriverRuntimeCyw43BusEpisodeRecord::stable_snapshot(wrong_version, wrong_version),
            None
        );

        let missing_dpc_flag = DriverRuntimeCyw43BusEpisodeRecord {
            flags: 0,
            committed_publication_sequence: 0,
            ..committed
        };
        assert!(!missing_dpc_flag.body_valid());

        let bad_reserved = DriverRuntimeCyw43BusEpisodeRecord {
            reserved: {
                let mut reserved = [0; 28];
                reserved[27] = 1;
                reserved
            },
            committed_publication_sequence: 0,
            ..committed
        };
        assert!(!bad_reserved.body_valid());
    }

    #[test]
    fn bcm2835_dma_tag_identifies_linked_sdio_mmio_authority() {
        assert_eq!(DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA, 13);
        assert_ne!(
            DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA,
            DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST
        );
        let range = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA,
            0x7020_2000,
            0xFE00_7000,
            DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            1,
            2,
        );
        assert!(range.valid());
        assert_eq!(range.first_page_index, 2);
    }

    #[test]
    fn pcie_timer_constants_are_exact_and_disjoint() {
        assert_eq!(DRIVER_RUNTIME_PCIE_OP_ROOT_IDLE_TIMER_ENABLE, 4);
        assert_eq!(DRIVER_RUNTIME_RESOURCE_TAG_PI4_SYSTEM_TIMER, 15);
        assert_eq!(DRIVER_RUNTIME_PI4_SYSTEM_TIMER_PADDR, 0xFE00_3000);
        assert_eq!(DRIVER_RUNTIME_PI4_SYSTEM_TIMER_CLOCK_HZ, 1_000_000);
        assert_eq!(DRIVER_RUNTIME_PCIE_TIMER_IRQ, 99);
        assert_eq!(DRIVER_RUNTIME_PCIE_TIMER_IRQ_BADGE, 1 << 11);
        assert_eq!(DRIVER_RUNTIME_PCIE_TIMER_OWNER_PERIOD_US, 10_000);
        assert_eq!(DRIVER_RUNTIME_PCIE_TIMER_INTERVAL_US, 5_000);
        assert_eq!(DRIVER_RUNTIME_PCIE_TIMER_STATE_OFFSET, 192);
        assert_eq!(DRIVER_RUNTIME_PCIE_TIMER_STATE_BYTES, 40);
        assert_eq!(core::mem::size_of::<DriverRuntimePcieTimerState>(), 40);
        assert_eq!(
            core::mem::offset_of!(DriverRuntimePcieTimerState, committed_publication),
            36
        );
        assert_eq!(
            DRIVER_TASK_CHILD_PCIE_TIMER_IRQ_HANDLER_SLOT,
            DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT
        );
        assert_eq!(
            DRIVER_RUNTIME_PCIE_TIMER_IRQ_BADGE & DRIVER_RUNTIME_RESERVED_ROOT_BADGE,
            0
        );

        let disarmed = DriverRuntimePcieTimerState::staged(
            7,
            11,
            1,
            DRIVER_RUNTIME_PCIE_TIMER_STATE_DISARMED,
            0,
            0,
            0,
        )
        .commit();
        assert!(disarmed.valid());
        assert!(!disarmed.enabled_for(7, 11));

        let enabled = DriverRuntimePcieTimerState::staged(
            7,
            11,
            2,
            DRIVER_RUNTIME_PCIE_TIMER_STATE_ENABLED,
            19,
            5_000,
            0,
        )
        .commit();
        assert!(enabled.enabled_for(7, 11));
        assert!(!enabled.enabled_for(7, 12));
        assert_eq!(
            DriverRuntimePcieTimerState::stable_snapshot(enabled, enabled),
            Some(enabled)
        );
        let mut torn = enabled;
        torn.committed_publication = 0;
        assert_eq!(
            DriverRuntimePcieTimerState::stable_snapshot(enabled, torn),
            None
        );
    }

    #[test]
    fn continuation_grant_fits_reserved_command_slot_and_fingerprints_actions() {
        assert_eq!(core::mem::size_of::<DriverRuntimeContinuationGrant>(), 24);
        assert_eq!(core::mem::align_of::<DriverRuntimeContinuationGrant>(), 4);
        assert_eq!(
            core::mem::size_of::<DriverRuntimeSteadyServiceProgress>(),
            24
        );
        assert_eq!(
            core::mem::align_of::<DriverRuntimeSteadyServiceProgress>(),
            4
        );
        assert_eq!(core::mem::size_of::<DriverRuntimeOneWayWaitReceipt>(), 24);
        assert_eq!(core::mem::align_of::<DriverRuntimeOneWayWaitReceipt>(), 4);
        assert_eq!(
            core::mem::size_of::<DriverRuntimePersistentWaitReceipt>(),
            24
        );
        assert_eq!(
            core::mem::align_of::<DriverRuntimePersistentWaitReceipt>(),
            4
        );
        assert_eq!(DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET, 40);
        assert_eq!(DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES, 24);
        assert_eq!(
            driver_runtime_continuation_grant_action_admitted_id(7),
            Some(DRIVER_RUNTIME_CONTINUATION_GRANT_ACTION_ADMITTED_BIT | 7),
        );
        assert!(driver_runtime_continuation_grant_action_admitted(
            DRIVER_RUNTIME_CONTINUATION_GRANT_ACTION_ADMITTED_BIT | 7,
            7,
        ));
        assert!(!driver_runtime_continuation_grant_action_admitted(7, 7));
        assert_eq!(
            driver_runtime_continuation_grant_action_admitted_id(0),
            None,
        );
        assert_eq!(
            driver_runtime_continuation_grant_action_admitted_id(
                DRIVER_RUNTIME_CONTINUATION_GRANT_ACTION_ADMITTED_BIT,
            ),
            None,
        );
        assert_eq!(
            DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_OFFSET,
            DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET,
        );
        assert_eq!(
            DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_BYTES,
            DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES,
        );
        assert_eq!(
            DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_OFFSET,
            DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET,
        );
        assert_eq!(
            DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_BYTES,
            DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES,
        );
        assert_eq!(
            DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_OFFSET,
            DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET,
        );
        assert_eq!(
            DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_BYTES,
            DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES,
        );
        assert_ne!(
            DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC,
            DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC,
        );
        assert_ne!(
            DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC,
            DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC,
        );
        assert_ne!(
            DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC,
            DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC,
        );
        assert_ne!(
            DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC,
            DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC,
        );
        assert_ne!(
            DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC,
            DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC,
        );
        for other in [
            DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC,
            DRIVER_RUNTIME_STEADY_SERVICE_PROGRESS_MAGIC,
            DRIVER_RUNTIME_PERSISTENT_WAIT_RECEIPT_MAGIC,
            DRIVER_RUNTIME_ONE_WAY_WAIT_RECEIPT_MAGIC,
        ] {
            assert_ne!(DRIVER_RUNTIME_ONE_WAY_WAIT_ACK_MAGIC, other);
        }
        let wait = DriverRuntimeOneWayWaitReceipt::new(7, 0x1234_5678, 0x89ab_cdef, 2);
        assert!(wait.valid());
        assert!(!wait.acknowledged());
        let ack = DriverRuntimeOneWayWaitReceipt {
            magic: DRIVER_RUNTIME_ONE_WAY_WAIT_ACK_MAGIC,
            ..wait
        };
        assert!(!ack.valid());
        assert!(ack.acknowledged());
        assert!(
            usize::from(DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET)
                + core::mem::size_of::<DriverRuntimeContinuationGrant>()
                <= 64
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeContinuationGrant, grant_id),
            16
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeContinuationGrant, consumed_grant_id),
            20
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeSteadyServiceProgress, service_slice),
            16,
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeSteadyServiceProgress, committed_slice),
            20,
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeOneWayWaitReceipt, wait_slice),
            16,
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeOneWayWaitReceipt, committed_wait_slice),
            20,
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimePersistentWaitReceipt, wait_epoch),
            16,
        );
        assert_eq!(
            core::mem::offset_of!(DriverRuntimePersistentWaitReceipt, committed_wait_epoch),
            20,
        );

        let fingerprint =
            driver_runtime_continuation_action_fingerprint(1, 2, 3, 4, 5, 6, 1, 0, 64, 256, 32, 0);
        assert_ne!(fingerprint, 0);
        for (field, mutated) in [
            (
                "opcode",
                driver_runtime_continuation_action_fingerprint(
                    7, 2, 3, 4, 5, 6, 1, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "flags",
                driver_runtime_continuation_action_fingerprint(
                    1, 7, 3, 4, 5, 6, 1, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "arg0",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 7, 4, 5, 6, 1, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "arg1",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 7, 5, 6, 1, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "aux0",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 7, 6, 1, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "aux1",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 7, 1, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "max_ops",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 6, 7, 0, 64, 256, 32, 0,
                ),
            ),
            (
                "max_frames",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 6, 1, 7, 64, 256, 32, 0,
                ),
            ),
            (
                "max_bytes",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 6, 1, 0, 65, 256, 32, 0,
                ),
            ),
            (
                "frame_offset",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 6, 1, 0, 64, 257, 32, 0,
                ),
            ),
            (
                "frame_len",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 6, 1, 0, 64, 256, 33, 0,
                ),
            ),
            (
                "frame_flags",
                driver_runtime_continuation_action_fingerprint(
                    1, 2, 3, 4, 5, 6, 1, 0, 64, 256, 32, 1,
                ),
            ),
        ] {
            assert_ne!(fingerprint, mutated, "{field} must be fingerprinted");
        }
        assert_eq!(
            DriverRuntimeContinuationGrant::new(9, fingerprint, 11, 1),
            DriverRuntimeContinuationGrant {
                magic: DRIVER_RUNTIME_CONTINUATION_GRANT_MAGIC,
                request_sequence: 9,
                action_fingerprint: fingerprint,
                generation: 11,
                grant_id: 1,
                consumed_grant_id: 0,
            }
        );
    }

    #[test]
    fn persistent_wait_receipt_is_exact_commit_last_and_accepts_generation_zero() {
        let fingerprint = driver_runtime_continuation_action_fingerprint(
            1,
            DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY
                | DRIVER_RUNTIME_COMMAND_FLAG_PERSISTENT_TRANSACTION,
            HOT_PATH_CYW43_WIFI,
            DRIVER_RUNTIME_ROLE_NET,
            DRIVER_RUNTIME_CYW43_COMMAND_AUX,
            0,
            DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_OPS,
            DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_FRAMES,
            DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_BYTES,
            u32::from(DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET),
            core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>() as u16,
            0,
        );
        let receipt = DriverRuntimePersistentWaitReceipt::new(73, fingerprint, 0, 1);
        assert!(
            receipt.valid(),
            "logical generation zero is a valid persistent op11 identity"
        );

        assert!(!DriverRuntimePersistentWaitReceipt {
            committed_wait_epoch: 0,
            ..receipt
        }
        .valid());
        assert!(!DriverRuntimePersistentWaitReceipt {
            committed_wait_epoch: 2,
            ..receipt
        }
        .valid());
        assert!(!DriverRuntimePersistentWaitReceipt {
            action_fingerprint: 0,
            ..receipt
        }
        .valid());
        assert!(!DriverRuntimePersistentWaitReceipt {
            wait_epoch: 0,
            committed_wait_epoch: 0,
            ..receipt
        }
        .valid());
        assert_eq!(DriverRuntimePersistentWaitReceipt::empty().magic, 0);
    }

    #[test]
    fn cyw43_root_continuation_identity_is_generation_and_sequence_independent() {
        assert!(driver_runtime_is_cyw43_root_continuation(
            HOT_PATH_CYW43_WIFI,
            DRIVER_RUNTIME_ROLE_NET,
            DRIVER_RUNTIME_CYW43_COMMAND_AUX,
        ));
        assert!(!driver_runtime_is_cyw43_root_continuation(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_ROLE_NET,
            DRIVER_RUNTIME_CYW43_COMMAND_AUX,
        ));
        assert!(!driver_runtime_is_cyw43_root_continuation(
            HOT_PATH_CYW43_WIFI,
            1 << 4,
            DRIVER_RUNTIME_CYW43_COMMAND_AUX,
        ));
        assert!(!driver_runtime_is_cyw43_root_continuation(
            HOT_PATH_CYW43_WIFI,
            DRIVER_RUNTIME_ROLE_NET,
            DRIVER_RUNTIME_ENGINE_INIT_AUX,
        ));
    }

    #[test]
    fn counter_snapshot_is_fixed_layout_and_non_authority_bearing() {
        assert_eq!(core::mem::size_of::<DriverRuntimeCounterSnapshot>(), 256);
        assert_eq!(core::mem::align_of::<DriverRuntimeCounterSnapshot>(), 8);
        assert_eq!(DRIVER_RUNTIME_COUNTER_MAGIC, 0x4452_4354);
        assert_eq!(DRIVER_RUNTIME_COUNTER_VERSION, 1);

        let empty = DriverRuntimeCounterSnapshot::empty();
        assert!(!empty.valid());
        assert_eq!(empty.magic, DRIVER_RUNTIME_COUNTER_MAGIC);
        assert_eq!(
            empty.len as usize,
            core::mem::size_of::<DriverRuntimeCounterSnapshot>()
        );

        let snapshot = DriverRuntimeCounterSnapshot::for_hot_path(
            HOT_PATH_CYW43_WIFI,
            DRIVER_RUNTIME_COUNTER_FLAG_ROOT_SNAPSHOT,
            73,
        );
        assert!(snapshot.valid());
        assert_eq!(snapshot.hot_path, HOT_PATH_CYW43_WIFI);
        assert_eq!(snapshot.sequence, 73);

        let mut invalid = snapshot;
        invalid.flags = 1 << 31;
        assert!(!invalid.valid());
        invalid = snapshot;
        invalid.reserved = 1;
        assert!(!invalid.valid());
    }

    #[test]
    fn cadence_record_is_fixed_commit_last_and_non_authority_bearing() {
        assert_eq!(core::mem::size_of::<DriverRuntimeCadenceRecord>(), 48);
        assert_eq!(core::mem::align_of::<DriverRuntimeCadenceRecord>(), 8);
        assert_eq!(DRIVER_RUNTIME_CADENCE_OFFSET, 144);
        assert_eq!(DRIVER_RUNTIME_CADENCE_MAGIC, 0x4452_4344);
        assert_eq!(DRIVER_RUNTIME_CADENCE_VERSION, 2);

        let staged = DriverRuntimeCadenceRecord::staged(
            7,
            DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_DONE,
            1_000,
            2_000,
            64 * 1024,
            320 * 1024,
            DRIVER_RUNTIME_CADENCE_EXIT_PROGRESS,
            DRIVER_RUNTIME_CADENCE_FLAG_WORK_REMAINS | DRIVER_RUNTIME_CADENCE_FLAG_WORK_BYTES,
        );
        assert!(!staged.valid());

        let mut committed = staged;
        committed.committed_sequence = committed.sequence;
        assert!(committed.valid());

        let mut invalid = committed;
        invalid.work_completed = invalid.work_total + 1;
        assert!(!invalid.valid());
        invalid = committed;
        invalid.flags = 1 << 15;
        assert!(!invalid.valid());

        let mut with_previous = DriverRuntimeCadenceRecord::staged_with_previous_entry(
            8,
            DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_DONE,
            2_000,
            1_000,
            2_500,
            64 * 1024,
            320 * 1024,
            DRIVER_RUNTIME_CADENCE_EXIT_PROGRESS,
            DRIVER_RUNTIME_CADENCE_FLAG_WORK_REMAINS
                | DRIVER_RUNTIME_CADENCE_FLAG_WORK_BYTES
                | DRIVER_RUNTIME_CADENCE_FLAG_PREVIOUS_ENTRY_VALID,
        );
        with_previous.committed_sequence = with_previous.sequence;
        assert!(with_previous.valid());
        assert_eq!(with_previous.previous_entry_cntvct_lo, 1_000);
        assert_eq!(with_previous.last_cntvct_lo, 2_500);
    }

    #[test]
    fn serial_rx_state_is_fixed_commit_last_and_non_authority_bearing() {
        assert_eq!(core::mem::size_of::<DriverRuntimeSerialRxState>(), 48);
        assert_eq!(core::mem::align_of::<DriverRuntimeSerialRxState>(), 8);
        assert_eq!(DRIVER_RUNTIME_SERIAL_RX_STATE_OFFSET, 192);
        assert_eq!(DRIVER_RUNTIME_SERIAL_RX_STATE_BYTES, 48);
        assert_eq!(
            core::mem::offset_of!(DriverRuntimeSerialRxState, committed_publication),
            44
        );
        assert!(
            DRIVER_RUNTIME_SERIAL_RX_STATE_OFFSET + DRIVER_RUNTIME_SERIAL_RX_STATE_BYTES
                <= DRIVER_RUNTIME_RING_FRAME_OFFSET
        );

        let staged = DriverRuntimeSerialRxState {
            magic: DRIVER_RUNTIME_SERIAL_RX_STATE_MAGIC,
            version: DRIVER_RUNTIME_SERIAL_RX_STATE_VERSION,
            len: DRIVER_RUNTIME_SERIAL_RX_STATE_BYTES,
            publication: 7,
            irq_wakes: 11,
            irq_acks: 10,
            irq_ack_failures: 1,
            hardware_overrun_events: 2,
            queue_full_events: 3,
            received_bytes: 64,
            queued_bytes: 4,
            flags: 0,
            last_cntvct_lo: 0x1234,
            committed_publication: 0,
        };
        assert!(!staged.valid());

        let mut committed = staged;
        committed.committed_publication = committed.publication;
        assert!(committed.valid());

        let mut invalid = committed;
        invalid.publication = 0;
        invalid.committed_publication = 0;
        assert!(!invalid.valid());
        invalid = committed;
        invalid.flags = 1 << 15;
        assert!(!invalid.valid());
    }

    #[test]
    fn serial_spsc_header_is_exact_generation_bound_and_commit_paired() {
        assert_eq!(core::mem::size_of::<DriverRuntimeSerialSpscHeader>(), 64);
        assert_eq!(core::mem::align_of::<DriverRuntimeSerialSpscHeader>(), 64);
        assert_eq!(DRIVER_RUNTIME_SERIAL_SPSC_SHARED_PAGES, 4);
        assert_eq!(DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY, 8128);

        let tx = DriverRuntimeSerialSpscHeader::empty(
            7,
            DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME,
        );
        assert!(tx.valid_for(7, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME));
        assert_eq!(tx.occupancy(), Some(0));
        assert!(!tx.valid_for(8, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME));
        assert!(!tx.valid_for(7, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_RUNTIME_TO_ROOT));

        let mut wrapped = tx;
        wrapped.consumer_index = u32::MAX - 3;
        wrapped.consumer_commit = wrapped.consumer_index;
        wrapped.producer_index = 4;
        wrapped.producer_commit = wrapped.producer_index;
        assert_eq!(wrapped.occupancy(), Some(8));
        assert!(wrapped.valid_for(7, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME));

        let mut torn = wrapped;
        torn.producer_commit = torn.producer_commit.wrapping_sub(1);
        assert_eq!(torn.occupancy(), None);
        assert!(!torn.valid_for(7, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME));

        let mut overfull = tx;
        overfull.producer_index = DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY as u32 + 1;
        overfull.producer_commit = overfull.producer_index;
        assert_eq!(overfull.occupancy(), None);
        assert!(!overfull.valid_for(7, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME));

        let poisoned = DriverRuntimeSerialSpscHeader {
            flags: DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME
                | DRIVER_RUNTIME_SERIAL_SPSC_FLAG_POISONED,
            ..tx
        };
        assert!(!poisoned.valid_for(7, DRIVER_RUNTIME_SERIAL_SPSC_FLAG_ROOT_TO_RUNTIME));
    }

    #[test]
    fn serial_spsc_post_commit_rechecks_close_both_wake_races() {
        let capacity = DRIVER_RUNTIME_SERIAL_SPSC_CAPACITY as u32;

        assert!(driver_runtime_serial_spsc_data_doorbell_due(0, 19, 19));
        assert!(driver_runtime_serial_spsc_data_doorbell_due(8, 19, 19));
        assert!(!driver_runtime_serial_spsc_data_doorbell_due(8, 19, 11));

        assert_eq!(
            driver_runtime_serial_spsc_consumer_post_commit(8, 11, 19, 23),
            Some((false, true))
        );
        assert_eq!(
            driver_runtime_serial_spsc_consumer_post_commit(capacity, 11, 19, 19),
            Some((true, false))
        );
        assert_eq!(
            driver_runtime_serial_spsc_consumer_post_commit(
                capacity - 4,
                11,
                19,
                11u32.wrapping_add(capacity),
            ),
            Some((true, true))
        );
        assert_eq!(
            driver_runtime_serial_spsc_consumer_post_commit(8, u32::MAX - 3, 4, 7,),
            Some((false, true))
        );
        assert_eq!(
            driver_runtime_serial_spsc_consumer_post_commit(8, 11, 20, 20),
            None
        );
        assert_eq!(
            driver_runtime_serial_spsc_consumer_post_commit(
                8,
                11,
                19,
                19u32.wrapping_add(capacity).wrapping_add(1),
            ),
            None
        );
    }

    #[test]
    fn dpc_event_ring_is_fixed_bounded_and_sequence_checked() {
        assert_eq!(core::mem::size_of::<DriverRuntimeDpcEventEntry>(), 16);
        assert_eq!(core::mem::size_of::<DriverRuntimeDpcEventRing>(), 96);
        assert_eq!(DRIVER_RUNTIME_DPC_EVENT_RING_VERSION, 3);
        assert_eq!(DRIVER_RUNTIME_INIT_VERSION, 13);
        assert_eq!(
            DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET + DRIVER_RUNTIME_DPC_EVENT_RING_BYTES,
            DRIVER_RUNTIME_RING_FRAME_OFFSET
        );

        let ring = DriverRuntimeDpcEventRing::empty(7);
        assert!(ring.valid());

        let mut invalid = ring;
        invalid.producer = DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH as u32 + 1;
        assert!(!invalid.valid());
        invalid = ring;
        invalid.entries[0].flags = 1 << 15;
        assert!(!invalid.valid());
    }

    #[test]
    fn sdio_physical_lifetime_record_occupies_reserved_gap_and_rejects_torn_samples() {
        assert_eq!(
            DRIVER_RUNTIME_RING_PROGRESS_OFFSET + DRIVER_RUNTIME_RING_PROGRESS_BYTES,
            DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_OFFSET
        );
        assert_eq!(
            DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_OFFSET
                + DRIVER_RUNTIME_SDIO_PHYSICAL_LIFETIME_BYTES,
            DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET
        );
        assert_eq!(
            core::mem::size_of::<DriverRuntimeSdioPhysicalLifetimeRecord>(),
            16
        );

        let empty = DriverRuntimeSdioPhysicalLifetimeRecord::empty();
        assert!(empty.valid());
        assert!(!empty.active());
        assert_eq!(empty.next_epoch(), Some(1));
        assert_eq!(
            DriverRuntimeSdioPhysicalLifetimeRecord::stable_snapshot(
                DriverRuntimeSdioPhysicalLifetimeRecord::zeroed(),
                DriverRuntimeSdioPhysicalLifetimeRecord::zeroed(),
            ),
            Some(empty),
        );

        let active = DriverRuntimeSdioPhysicalLifetimeRecord {
            begun_epoch: 1,
            ..empty
        };
        assert!(active.valid());
        assert!(active.active());
        assert_eq!(
            DriverRuntimeSdioPhysicalLifetimeRecord::stable_snapshot(empty, active),
            None,
            "a changed sequence-last begun epoch is not a stable snapshot",
        );

        let completed = DriverRuntimeSdioPhysicalLifetimeRecord {
            completed_epoch: 1,
            ..active
        };
        assert!(completed.valid());
        assert!(!completed.active());
        assert_eq!(completed.next_epoch(), Some(2));

        let mut impossible = completed;
        impossible.failed_epoch = 1;
        assert!(!impossible.valid());
        assert_eq!(
            DriverRuntimeSdioPhysicalLifetimeRecord::stable_snapshot(impossible, impossible),
            None,
        );
    }

    #[test]
    fn sdio_clock_snapshot_is_disjoint_and_requires_complete_stable_readback() {
        assert_eq!(
            core::mem::size_of::<DriverRuntimeSdioClockSnapshot>(),
            usize::from(DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_BYTES)
        );
        assert!(
            DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET
                + core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>() as u16
                <= DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET
        );
        assert_eq!(DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET % 64, 0);
        assert!(
            DRIVER_RUNTIME_CYW43_COMMAND_DESCRIPTOR_OFFSET + 64
                <= DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET
        );
        assert_eq!(
            DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_OFFSET + DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_BYTES,
            DRIVER_RUNTIME_SDIO_DEADLINE_ARM_OFFSET
        );

        let snapshot = DriverRuntimeSdioClockSnapshot {
            magic: DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_MAGIC,
            version: DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_VERSION,
            len: DRIVER_RUNTIME_SDIO_CLOCK_SNAPSHOT_BYTES,
            sequence: 9,
            physical_lifetime_epoch: 3,
            requested_clock_hz: 50_000_000,
            base_clock_hz: 250_000_000,
            effective_clock_hz: 41_666_666,
            timer_clock_hz: 54_000_000,
            divider: 6,
            clock_control: DriverRuntimeSdioClockSnapshot::CLOCK_CONTROL_INTERNAL_ENABLE
                | DriverRuntimeSdioClockSnapshot::CLOCK_CONTROL_INTERNAL_STABLE
                | DriverRuntimeSdioClockSnapshot::CLOCK_CONTROL_CARD_ENABLE,
            host_control: 0x02,
            cccr_speed: DriverRuntimeSdioClockSnapshot::CCCR_SPEED_EHS,
            cccr_interface: DriverRuntimeSdioClockSnapshot::CCCR_INTERFACE_WIDTH_4BIT,
            flags: DriverRuntimeSdioClockSnapshot::FLAG_REQUEST_VALID
                | DriverRuntimeSdioClockSnapshot::FLAG_CLOCK_READBACK_VALID
                | DriverRuntimeSdioClockSnapshot::FLAG_INTERNAL_CLOCK_STABLE
                | DriverRuntimeSdioClockSnapshot::FLAG_CARD_CLOCK_ENABLED
                | DriverRuntimeSdioClockSnapshot::FLAG_CARD_HIGH_SPEED
                | DriverRuntimeSdioClockSnapshot::FLAG_HOST_WIDTH_4BIT
                | DriverRuntimeSdioClockSnapshot::FLAG_CCCR_SPEED_VALID
                | DriverRuntimeSdioClockSnapshot::FLAG_CCCR_INTERFACE_VALID,
            reserved: 0,
        };
        assert!(snapshot.valid());
        assert!(snapshot.gate4_ready());
        assert_eq!(
            DriverRuntimeSdioClockSnapshot::stable_snapshot(snapshot, snapshot),
            Some(snapshot)
        );

        let mut torn = snapshot;
        torn.sequence += 1;
        assert_eq!(
            DriverRuntimeSdioClockSnapshot::stable_snapshot(snapshot, torn),
            None
        );
        torn = snapshot;
        torn.clock_control &= !DriverRuntimeSdioClockSnapshot::CLOCK_CONTROL_CARD_ENABLE;
        assert!(!torn.valid());
    }

    #[test]
    fn sdio_deadline_arm_fills_ring_tail_and_rejects_torn_identity() {
        assert_eq!(
            DRIVER_RUNTIME_SDIO_DEADLINE_ARM_OFFSET + DRIVER_RUNTIME_SDIO_DEADLINE_ARM_BYTES,
            DRIVER_RUNTIME_CYW43_SDPCM_TX_FRAME_OFFSET
        );
        assert_eq!(
            core::mem::size_of::<DriverRuntimeSdioDeadlineArm>(),
            usize::from(DRIVER_RUNTIME_SDIO_DEADLINE_ARM_BYTES)
        );
        assert!(!DriverRuntimeSdioDeadlineArm::empty().valid());

        let staged = DriverRuntimeSdioDeadlineArm::staged(3, 0x8000_0042, 0x1234_5678_9abc_def0);
        assert!(staged.body_valid());
        assert!(!staged.valid());
        assert_eq!(staged.expiry_ticks(), 0x1234_5678_9abc_def0);
        let committed = staged.commit().expect("valid deadline body commits");
        assert!(committed.valid());
        assert_eq!(
            DriverRuntimeSdioDeadlineArm::stable_snapshot(committed, committed),
            Some(committed)
        );

        let mut torn = committed;
        torn.committed_request_sequence = 0;
        assert_eq!(
            DriverRuntimeSdioDeadlineArm::stable_snapshot(committed, torn),
            None
        );
        torn = committed;
        torn.request_sequence = torn.request_sequence.wrapping_add(1);
        assert!(!torn.valid());
        assert_eq!(
            DriverRuntimeSdioDeadlineArm::stable_snapshot(torn, torn),
            None
        );
    }

    #[test]
    fn dpc_event_ring_accepts_every_known_state_flag() {
        let mut ring = DriverRuntimeDpcEventRing::empty(7);
        for flag in [
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OVERRUN,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_ACK_PENDING,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_POISONED,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_CARD_IRQ_MASKED,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OWNER_ACTIVE,
        ] {
            ring.flags = flag;
            assert!(ring.valid(), "known DPC ring flag 0x{flag:08x}");
        }
        ring.flags = DRIVER_RUNTIME_DPC_EVENT_RING_KNOWN_FLAGS;
        assert!(ring.valid());
    }

    #[test]
    fn dpc_event_ring_rejects_unknown_state_flags() {
        let mut ring = DriverRuntimeDpcEventRing::empty(7);
        ring.flags = DRIVER_RUNTIME_DPC_EVENT_RING_KNOWN_FLAGS | (1 << 31);
        assert!(!ring.valid());
    }

    #[test]
    fn dpc_event_ring_accepts_saturated_ack_failure_count() {
        let mut ring = DriverRuntimeDpcEventRing::empty(7);
        ring.ack_failures = u32::MAX;
        assert!(ring.valid());
    }

    #[test]
    fn genet_completion_result_packs_bounded_diagnostics() {
        let result =
            driver_runtime_genet_completion_result(DriverRuntimeGenetCompletionResultParts {
                tx_free: 70,
                tx_in_flight: 69,
                rx_queue_count: 35,
                rx_queue_high_water: 34,
                rx_max_drained_per_turn: 33,
                rx_drain_budget_hit: true,
                rx_byte_budget_hit: true,
                rx_overflow_seen: true,
                command_rx_drain_seen: true,
            });

        assert_eq!(
            result,
            u32::MAX,
            "the independent contract assigns every packed bit exactly once",
        );
        assert!(driver_runtime_genet_result_is_packed(result));
        assert_eq!(
            driver_runtime_genet_result_tx_free(result),
            DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK as u16
        );
        assert_eq!(
            driver_runtime_genet_result_tx_in_flight(result),
            DRIVER_RUNTIME_GENET_RESULT_SIX_BIT_MASK as u16
        );
        assert_eq!(
            driver_runtime_genet_result_rx_queue_count(result),
            DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK as u16
        );
        assert_eq!(
            driver_runtime_genet_result_rx_queue_high_water(result),
            DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK as u16
        );
        assert_eq!(
            driver_runtime_genet_result_rx_max_drained_per_turn(result),
            DRIVER_RUNTIME_GENET_RESULT_FIVE_BIT_MASK as u16
        );
        assert!(driver_runtime_genet_result_rx_drain_budget_hit(result));
        assert!(driver_runtime_genet_result_rx_byte_budget_hit(result));
        assert!(driver_runtime_genet_result_rx_overflow_seen(result));
        assert!(driver_runtime_genet_result_command_rx_drain_seen(result));

        let result =
            driver_runtime_genet_completion_result(DriverRuntimeGenetCompletionResultParts {
                tx_free: 32,
                tx_in_flight: 1,
                rx_queue_count: 3,
                rx_queue_high_water: 8,
                rx_max_drained_per_turn: 16,
                rx_drain_budget_hit: false,
                rx_byte_budget_hit: true,
                rx_overflow_seen: false,
                command_rx_drain_seen: false,
            });
        assert_eq!(driver_runtime_genet_result_tx_free(result), 32);
        assert_eq!(driver_runtime_genet_result_tx_in_flight(result), 1);
        assert_eq!(driver_runtime_genet_result_rx_queue_count(result), 3);
        assert_eq!(driver_runtime_genet_result_rx_queue_high_water(result), 8);
        assert_eq!(
            driver_runtime_genet_result_rx_max_drained_per_turn(result),
            16
        );
        assert!(!driver_runtime_genet_result_rx_drain_budget_hit(result));
        assert!(driver_runtime_genet_result_rx_byte_budget_hit(result));
        assert!(!driver_runtime_genet_result_rx_overflow_seen(result));
        assert!(!driver_runtime_genet_result_command_rx_drain_seen(result));
        assert_eq!(
            result & (1 << DRIVER_RUNTIME_GENET_RESULT_COMMAND_RX_DRAIN_SEEN_SHIFT),
            0,
            "legacy packed results leave the additive command-route bit clear"
        );
    }

    #[test]
    fn genet_completion_route_bit_is_independent_of_legacy_overflow() {
        // These literal protocol words are independent of the packing shifts:
        // bit 31 is the existing packed marker, bit 29 is legacy overflow, and
        // bit 30 is the additive same-owner command-route discriminator.
        const LEGACY_OVERFLOW_ONLY: u32 = 0xa000_0000;
        const COMMAND_ROUTE_ONLY: u32 = 0xc000_0000;

        assert!(driver_runtime_genet_result_is_packed(LEGACY_OVERFLOW_ONLY));
        assert!(driver_runtime_genet_result_rx_overflow_seen(
            LEGACY_OVERFLOW_ONLY
        ));
        assert!(!driver_runtime_genet_result_command_rx_drain_seen(
            LEGACY_OVERFLOW_ONLY
        ));

        assert!(driver_runtime_genet_result_is_packed(COMMAND_ROUTE_ONLY));
        assert!(!driver_runtime_genet_result_rx_overflow_seen(
            COMMAND_ROUTE_ONLY
        ));
        assert!(driver_runtime_genet_result_command_rx_drain_seen(
            COMMAND_ROUTE_ONLY
        ));
    }

    #[test]
    fn empty_descriptor_needs_role_and_buffers_before_valid() {
        let descriptor = DriverRuntimeInitDescriptor::empty();
        assert!(!descriptor.valid());
        assert_eq!(descriptor.magic, DRIVER_RUNTIME_INIT_MAGIC);
        assert_eq!(descriptor.version, DRIVER_RUNTIME_INIT_VERSION);
        assert!(!descriptor.mcs_scheduler_valid());
    }

    #[test]
    fn mcs_scheduler_inventory_is_exact_and_badge_domains_are_disjoint() {
        let descriptor = mcs_descriptor();
        assert!(descriptor.mcs_scheduler_valid());
        assert_eq!(
            descriptor.command_reply_slot,
            DRIVER_RUNTIME_COMMAND_REPLY_SLOT
        );
        assert_eq!(
            descriptor.completion_notification_slot,
            DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT
        );
        assert_eq!(
            descriptor.root_control_wake_notification_slot,
            DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT
        );
        assert_eq!(descriptor.max_inflight_commands, 1);
        assert_ne!(descriptor.command_badge, descriptor.completion_badge);
        assert_ne!(descriptor.command_badge, descriptor.standard_fault_badge);
        assert_ne!(descriptor.command_badge, descriptor.timeout_fault_badge);

        let mut invalid = descriptor;
        invalid.command_reply_slot = DRIVER_RUNTIME_COMMAND_REPLY_SLOT + 1;
        assert!(!invalid.mcs_scheduler_valid());
        invalid = descriptor;
        invalid.scheduler_flags &= !DRIVER_RUNTIME_MCS_FLAG_ONE_INFLIGHT;
        assert!(!invalid.mcs_scheduler_valid());
        invalid = descriptor;
        invalid.completion_badge = invalid.command_badge;
        assert!(!invalid.mcs_scheduler_valid());
        invalid = descriptor;
        invalid.root_control_wake_notification_slot = 0;
        assert!(!invalid.mcs_scheduler_valid());
        invalid = descriptor;
        invalid.root_control_wake_notification_slot =
            DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT + 1;
        assert!(!invalid.mcs_scheduler_valid());
    }

    #[test]
    fn valid_descriptor_requires_pointer_free_shared_and_bus_flags() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_GENET_NIC;
        descriptor.role_bit = 1 << 3;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x4000_0000);
        assert!(descriptor.valid());

        descriptor.flags &= !DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE;
        assert!(!descriptor.valid());
    }

    #[test]
    fn root_wake_notification_is_optional_exact_and_cyw43_only() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_CYW43_WIFI;
        descriptor.role_bit = 1 << 4;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x4000_0000);

        assert!(descriptor.valid(), "generic zero wake pair remains valid");

        descriptor.root_wake_notification_slot = DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT;
        descriptor.root_wake_notification_badge = DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE;
        assert!(descriptor.valid(), "exact CYW43 wake pair accepted");

        descriptor.root_wake_notification_badge = 0;
        assert!(!descriptor.valid(), "half-populated wake pair rejected");

        descriptor.root_wake_notification_badge = DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE;
        descriptor.root_wake_notification_slot =
            DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT + 1;
        assert!(!descriptor.valid(), "wrong wake slot rejected");

        descriptor.root_wake_notification_slot = DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT;
        descriptor.root_wake_notification_badge =
            DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE + 1;
        assert!(!descriptor.valid(), "wrong wake badge rejected");

        descriptor.root_wake_notification_badge = DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE;
        descriptor.hot_path = HOT_PATH_GENET_NIC;
        assert!(!descriptor.valid(), "non-CYW43 wake authority rejected");
    }

    #[test]
    fn direct_genet_descriptor_is_exact_cpu_only_and_genet_only() {
        assert_eq!(
            core::mem::size_of::<DriverRuntimeDirectGenetDescriptor>(),
            16
        );
        assert_eq!(core::mem::size_of::<DriverRuntimeInitDescriptor>(), 1600);
        assert_eq!(DRIVER_RUNTIME_CHILD_DIRECT_GENET_PEER_NOTIFICATION_SLOT, 8);
        assert_eq!(DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT, 12);
        assert!(DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT < 16);
        assert_ne!(
            DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT,
            DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT
        );
        assert_ne!(
            DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT,
            DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT
        );
        assert_eq!(DRIVER_RUNTIME_GENET_DIRECT_LINK_NOTIFICATION_BADGE, 1 << 8);
        assert_eq!(DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT, 32);

        let descriptor = direct_genet_descriptor();
        assert!(descriptor.direct_genet.valid());
        assert!(descriptor.direct_genet_link_valid());
        assert!(descriptor.valid());

        let mut invalid = descriptor;
        invalid.direct_genet.flags &= !DRIVER_RUNTIME_DIRECT_GENET_FLAG_CPU_ONLY;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.direct_genet.shared_page_count -= 1;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.direct_genet.page_bytes /= 2;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.direct_genet.peer_notification_slot += 1;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.direct_genet.peer_notification_badge <<= 1;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.flags |= 1 << 31;
        assert!(!invalid.valid(), "unknown init authority must fail closed");
        invalid = descriptor;
        invalid.hot_path = HOT_PATH_SERIAL_CONSOLE;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.resource_ranges[0].flags |= DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.resource_ranges[0].flags |= DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.resource_ranges[0].paddr = 0x5000_0000;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.resource_ranges[0].flags |= 1 << 15;
        assert!(!invalid.valid(), "unknown range authority must fail closed");
        invalid = descriptor;
        invalid.resource_ranges[0].reserved = 1;
        assert!(!invalid.valid(), "reserved range authority must stay zero");
        invalid = descriptor;
        invalid.resource_ranges[0].page_count -= 1;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.resource_ranges[0].bytes -= DRIVER_RUNTIME_DIRECT_GENET_PAGE_BYTES as u64;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.resource_ranges[0].first_page_index = 1;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.shared_page_count -= 1;
        assert!(!invalid.valid());
        invalid = descriptor;
        invalid.shared_pages[7] = DriverRuntimePageDescriptor::new(0x5000_7000);
        assert!(
            !invalid.valid(),
            "legacy shared-page physical authority is forbidden"
        );

        let mut extra_shared = descriptor;
        extra_shared.resource_range_count = 2;
        extra_shared.resource_ranges[1] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
            DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL,
            0x70e0_0000,
            0x5200_0000,
            DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            1,
            0,
        );
        assert!(!extra_shared.valid());
    }

    #[test]
    fn direct_genet_authority_cannot_hide_in_an_absent_descriptor() {
        let descriptor = direct_genet_descriptor();

        let mut absent = descriptor;
        absent.flags &= !DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET;
        absent.direct_genet = DriverRuntimeDirectGenetDescriptor::empty();
        assert!(
            !absent.direct_genet_link_valid(),
            "tag 14 remains authoritative"
        );

        absent.resource_ranges[0].tag = DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL;
        assert!(
            !absent.direct_genet_link_valid(),
            "CPU-only range remains direct authority"
        );

        absent.resource_ranges[0].flags &= !DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY;
        assert!(absent.direct_genet_link_valid());

        let mut half_present = absent;
        half_present.flags |= DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET;
        assert!(!half_present.direct_genet_link_valid());
    }

    #[test]
    fn direct_genet_handoff_completion_is_generation_and_frame_exact() {
        let generation = 0x1122_3344_5566_7788;
        let sequence = 27;
        let token = driver_runtime_direct_genet_handoff_token(generation);
        assert_ne!(token, 0);
        assert_ne!(
            token,
            driver_runtime_direct_genet_handoff_token(generation + 1)
        );
        assert!(driver_runtime_direct_genet_handoff_completion_exact(
            sequence,
            sequence,
            DRIVER_RUNTIME_COMPLETION_PROGRESS,
            DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
            token,
            0,
            0,
            0,
            generation,
        ));

        for exact in [
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence + 1,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS + 1,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY + 1,
                token,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token ^ 1,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token,
                1,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token,
                0,
                1,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token,
                0,
                0,
                1,
                generation,
            ),
            driver_runtime_direct_genet_handoff_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                driver_runtime_direct_genet_handoff_token(0),
                0,
                0,
                0,
                0,
            ),
        ] {
            assert!(!exact);
        }

        assert!(
            driver_runtime_direct_genet_handoff_quiescing_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_IDLE,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING,
                token,
                0,
                0,
                0,
                generation,
            )
        );
        for exact in [
            driver_runtime_direct_genet_handoff_quiescing_completion_exact(
                sequence,
                sequence + 1,
                DRIVER_RUNTIME_COMPLETION_IDLE,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING,
                token,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_quiescing_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_PROGRESS,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING,
                token,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_quiescing_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_IDLE,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY,
                token,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_quiescing_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_IDLE,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING,
                token ^ 1,
                0,
                0,
                0,
                generation,
            ),
            driver_runtime_direct_genet_handoff_quiescing_completion_exact(
                sequence,
                sequence,
                DRIVER_RUNTIME_COMPLETION_IDLE,
                DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_QUIESCING,
                token,
                0,
                1,
                0,
                generation,
            ),
        ] {
            assert!(!exact);
        }
    }

    #[test]
    fn runtime_irq_descriptors_require_distinct_handler_slots_and_badge_bits() {
        let mut descriptor = mcs_descriptor();
        descriptor.irq_count = 2;
        descriptor.irqs[0] = DriverRuntimeIrqDescriptor {
            irq: DRIVER_RUNTIME_SDIO_IRQ,
            badge: DRIVER_RUNTIME_SDIO_IRQ_BADGE,
            handler_slot: DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT,
            notification_slot: DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            trigger: DRIVER_RUNTIME_IRQ_TRIGGER_LEVEL,
            flags: 0,
            reserved: 0,
        };
        descriptor.irqs[1] = DriverRuntimeIrqDescriptor {
            irq: DRIVER_RUNTIME_SDIO_DMA_IRQ,
            badge: DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE,
            handler_slot: DRIVER_TASK_CHILD_SDIO_DMA_IRQ_HANDLER_SLOT,
            notification_slot: DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            trigger: DRIVER_RUNTIME_IRQ_TRIGGER_LEVEL,
            flags: 0,
            reserved: 0,
        };
        assert!(descriptor.valid_irqs());

        let mut duplicate_handler = descriptor;
        duplicate_handler.irqs[1].handler_slot = DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT;
        assert!(!duplicate_handler.valid_irqs());

        let mut aliasing_badge = descriptor;
        aliasing_badge.irqs[1].badge = DRIVER_RUNTIME_SDIO_IRQ_BADGE;
        assert!(!aliasing_badge.valid_irqs());
    }

    #[test]
    fn valid_for_resources_rejects_count_mismatch() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_PCIE_ROOT;
        descriptor.role_bit = 1 << 5;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.mmio_page_count = 2;
        descriptor.shared_page_count = 1;
        descriptor.mmio_pages[0] = DriverRuntimePageDescriptor::new(0xFD50_0000);
        descriptor.mmio_pages[1] = DriverRuntimePageDescriptor::new(0xFD50_1000);
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x5000_0000);

        assert!(descriptor.valid_for_resources(HOT_PATH_PCIE_ROOT, 1 << 5, 2, 0, 1));
        assert!(!descriptor.valid_for_resources(HOT_PATH_PCIE_ROOT, 1 << 5, 1, 0, 1));
    }

    #[test]
    fn resource_ranges_can_describe_large_mmio_without_page_array_growth() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_USB_KEYBOARD;
        descriptor.role_bit = 1 << 1;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
            | DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x4000_0000);
        descriptor.mmio_page_count = DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES as u16;
        for index in 0..DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES {
            descriptor.mmio_pages[index] = DriverRuntimePageDescriptor::new(
                0x0000_0006_0000_0000usize + index * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as usize,
            );
        }
        descriptor.resource_range_count = 1;
        descriptor.resource_ranges[0] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
            0x7020_0000,
            0x0000_0006_0000_0000,
            512 * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            512,
            0,
        );

        assert!(descriptor.valid());
        assert_eq!(
            descriptor.resource_pages_by_kind(DRIVER_RUNTIME_RESOURCE_KIND_MMIO),
            512
        );
        assert!(descriptor.has_resource_range(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI
        ));
        assert!(descriptor.valid_for_resources(HOT_PATH_USB_KEYBOARD, 1 << 1, 512, 0, 1));
        assert!(!descriptor.valid_for_resources(HOT_PATH_USB_KEYBOARD, 1 << 1, 16, 0, 1));
    }

    #[test]
    fn resource_ranges_can_describe_large_dma_and_shared_budgets() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_GENET_NIC;
        descriptor.role_bit = 1 << 3;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
            | DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED
            | DRIVER_RUNTIME_INIT_FLAG_DMA_PADDRS;
        descriptor.mmio_page_count = 6;
        descriptor.dma_page_count = DRIVER_RUNTIME_INIT_MAX_DMA_PAGES as u16;
        descriptor.shared_page_count = DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES as u16;
        for index in 0..6 {
            descriptor.mmio_pages[index] =
                DriverRuntimePageDescriptor::new(0xfd58_0000usize + index * 0x1000);
        }
        for index in 0..DRIVER_RUNTIME_INIT_MAX_DMA_PAGES {
            descriptor.dma_pages[index] =
                DriverRuntimePageDescriptor::new(0x4000_0000usize + index * 0x1000);
        }
        for index in 0..DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES {
            descriptor.shared_pages[index] =
                DriverRuntimePageDescriptor::new(0x5000_0000usize + index * 0x1000);
        }
        descriptor.resource_range_count = 3;
        descriptor.resource_ranges[0] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_0000,
            0xfd58_0000,
            6 * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            6,
            0,
        );
        descriptor.resource_ranges[1] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_DMA,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
            0x7080_0000,
            0x4000_0000,
            512 * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            512,
            0,
        );
        descriptor.resource_ranges[2] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE
                | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
            DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL,
            0x70c0_0000,
            0x5000_0000,
            32 * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            32,
            0,
        );

        assert!(descriptor.valid());
        assert_eq!(
            descriptor.resource_pages_by_kind(DRIVER_RUNTIME_RESOURCE_KIND_DMA),
            512
        );
        assert_eq!(
            descriptor.resource_pages_by_kind(DRIVER_RUNTIME_RESOURCE_KIND_SHARED),
            32
        );
        assert!(descriptor.valid_for_resources(HOT_PATH_GENET_NIC, 1 << 3, 6, 512, 32));
    }

    #[test]
    fn bus_links_are_pointer_free_and_owner_checked() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_CYW43_WIFI;
        descriptor.role_bit = 1 << 3;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
            | DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x4000_0000);
        descriptor.bus_link_count = 1;
        descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        );

        assert!(descriptor.valid());
        assert!(descriptor.has_bus_link_to(HOT_PATH_SDIO_HOST));
        assert!(descriptor.has_pointer_free_bus_link(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        ));
        assert!(!descriptor.sealed_identity_self_consistent());
        let sealed = descriptor.with_sealed_identity(
            4,
            driver_runtime_artifact_hash("cohesix/bin/pi4-driver-cyw43"),
        );
        assert!(sealed.sealed_identity_valid_for_task(4));
        assert!(!sealed.sealed_identity_valid_for_task(3));
        assert_ne!(sealed.bus_links[0].epoch, 0);
        assert_ne!(sealed.bus_links[0].token, 0);
        assert!(sealed.has_sealed_pointer_free_bus_link(
            4,
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        ));
        assert!(!sealed.has_sealed_pointer_free_bus_link(
            3,
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        ));
        assert!(!descriptor.has_pointer_free_bus_link(
            HOT_PATH_PCIE_ROOT,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        ));
        descriptor.bus_links[0].flags &= !DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE;
        assert!(!descriptor.valid());
        assert!(!descriptor.has_pointer_free_bus_link(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        ));

        descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            0,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        );
        assert!(!descriptor.valid());
        descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
            DRIVER_RUNTIME_RING_PAGE_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        );
        assert!(!descriptor.valid());
        descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
            8 * 1024,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        );
        assert!(!descriptor.valid());
        descriptor.bus_links[0].shared_len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32 - 1;
        assert!(!descriptor.valid());
        descriptor.bus_links[0].shared_len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32 + 1;
        assert!(!descriptor.valid());
        descriptor.bus_links[0].shared_len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32;
        descriptor.bus_links[0].shared_offset =
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32 + 1;
        assert!(!descriptor.valid());
        descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_PCIE_ROOT,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        );
        assert!(!descriptor.valid());
    }

    #[test]
    fn cyw43_sdio_bus_link_supports_reciprocal_notification_dpc_descriptors() {
        assert_ne!(DRIVER_RUNTIME_RESERVED_ROOT_BADGE, 0);
        assert_ne!(DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE, 0);
        assert_ne!(DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE, 0);
        assert_ne!(
            DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE,
            DRIVER_RUNTIME_SDIO_IRQ_BADGE
        );
        assert_ne!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE,
            DRIVER_RUNTIME_SDIO_IRQ_BADGE
        );
        assert_ne!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE,
            DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE
        );
        assert_eq!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE & DRIVER_RUNTIME_SDIO_IRQ_BADGE,
            0,
            "the client-to-owner badge must remain bitwise disjoint from the SDIO IRQ"
        );
        assert_eq!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE
                & DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE,
            0,
            "the reciprocal peer badges must remain bitwise disjoint"
        );
        assert_eq!(
            DRIVER_RUNTIME_RESERVED_ROOT_BADGE
                & (DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE
                    | DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE
                    | DRIVER_RUNTIME_SDIO_IRQ_BADGE),
            0,
            "the reserved root bit must remain outside every service badge"
        );
        let client = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        )
        .with_notification_dpc(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT,
            0x4359_5301,
        );
        assert!(client.valid());
        assert!(client.notification_dpc_valid());

        let owner = DriverRuntimeBusLinkDescriptor::new(
            HOT_PATH_SDIO_HOST,
            DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
            DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        )
        .with_notification_dpc(
            HOT_PATH_CYW43_WIFI,
            DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT,
            0x4359_5301,
        );
        assert!(owner.valid());
        assert!(owner.notification_dpc_valid());
        assert_eq!(client.shared_epoch, owner.shared_epoch);

        let client_sealed = client.with_sealed_identity(4, HOT_PATH_CYW43_WIFI);
        let owner_sealed = owner.with_sealed_identity(7, HOT_PATH_SDIO_HOST);
        assert_ne!(client_sealed.epoch, owner_sealed.epoch);
        assert_eq!(client_sealed.shared_epoch, owner_sealed.shared_epoch);

        let mut invalid = owner;
        invalid.peer_notification_slot = DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT;
        assert!(!invalid.valid());
    }

    #[test]
    fn resource_range_at_requires_exact_vaddr_and_minimum_pages() {
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
        descriptor.hot_path = HOT_PATH_GENET_NIC;
        descriptor.role_bit = 1 << 3;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
            | DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x5000_0000);
        descriptor.resource_range_count = 1;
        descriptor.resource_ranges[0] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_0000,
            0xfd58_0000,
            6 * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            6,
            0,
        );

        assert!(descriptor.has_resource_range_at(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_0000,
            6
        ));
        assert!(descriptor.has_resource_range_at_with_flags(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_0000,
            6,
            DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
        ));
        assert!(!descriptor.has_resource_range_at_with_flags(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_0000,
            6,
            DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED
        ));
        assert!(!descriptor.has_resource_range_at(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_1000,
            6
        ));
        assert!(!descriptor.has_resource_range_at(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
            0x7020_0000,
            7
        ));
    }

    #[test]
    fn hdmi_ready_requires_framebuffer_flag_and_geometry() {
        let mut descriptor = mcs_descriptor();
        descriptor.hot_path = HOT_PATH_HDMI_TEXT;
        descriptor.role_bit = 1 << 2;
        descriptor.flags = DRIVER_RUNTIME_INIT_REQUIRED_FLAGS | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = DriverRuntimePageDescriptor::new(0x4000_0000);
        descriptor.framebuffer = DriverRuntimeFramebufferDescriptor {
            vaddr: DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
            paddr: 0x3000_0000,
            width: 640,
            height: 480,
            pitch: 640 * 4,
            format: DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
        };
        assert!(!descriptor.hdmi_ready());
        descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER;
        assert!(descriptor.hdmi_ready());
        descriptor.framebuffer.pitch = 0;
        assert!(!descriptor.hdmi_ready());
        descriptor.framebuffer.pitch = 640 * 4;
        descriptor.framebuffer.format = 0;
        assert!(!descriptor.hdmi_ready());
        descriptor.framebuffer.format = DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888;
        descriptor.framebuffer.vaddr = DRIVER_RUNTIME_FRAMEBUFFER_VADDR - 0x1000;
        assert!(!descriptor.hdmi_ready());
    }

    #[test]
    fn sdio_command_descriptor_validates_cmd52_and_cmd53_bounds() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CMD53_READ,
            function: 2,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT,
            addr: 0x1000,
            data_offset: 256,
            len: 512,
            block_size: 512,
            block_count: 0,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT,
            reserved: 0,
            timeout_us: 1000,
        };
        assert!(descriptor.valid());

        descriptor.function = 8;
        assert!(!descriptor.valid());
        descriptor.function = 2;
        descriptor.addr = 1 << 17;
        assert!(!descriptor.valid());
        descriptor.addr = 0x1000;
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE;
        descriptor.len = 2;
        descriptor.block_size = 0;
        assert!(!descriptor.valid());
        descriptor.len = 1;
        assert!(descriptor.valid());

        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_READ;
        descriptor.data_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET - 1;
        assert!(!descriptor.valid());
        descriptor.data_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        descriptor.len = DRIVER_RUNTIME_RING_PAGE_BYTES;
        assert!(!descriptor.valid());

        descriptor.data_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        descriptor.len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES;
        descriptor.block_size = 0;
        descriptor.block_count = 0;
        assert!(descriptor.valid());
        descriptor.len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES + 1;
        assert!(!descriptor.valid());

        descriptor.len = 0;
        descriptor.block_size = 512;
        descriptor.block_count = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES / descriptor.block_size;
        assert!(descriptor.valid());
        descriptor.data_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE + 1;
        assert!(!descriptor.valid());
        descriptor.data_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        descriptor.block_count = descriptor.block_count.saturating_add(1);
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_pre_tx_dpc_fence_is_scoped_to_function2_cmd53_writes() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE,
            function: 2,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT,
            addr: 0x8000,
            data_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            len: 64,
            block_size: 64,
            block_count: 1,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT
                | DriverRuntimeSdioCommandDescriptor::FLAG_PRE_TX_DPC_FENCE,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(descriptor.valid());

        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_READ;
        assert!(!descriptor.valid());
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE;
        descriptor.function = 1;
        assert!(!descriptor.valid());
        descriptor.function = 2;
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE;
        descriptor.len = 1;
        descriptor.block_size = 0;
        descriptor.block_count = 0;
        assert!(!descriptor.valid());
        descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
            addr: 0x4359_5302,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_DPC_FORCE_SOURCE_PROBE
                | DriverRuntimeSdioCommandDescriptor::FLAG_PRE_TX_DPC_FENCE,
            timeout_us: 1_000,
            ..DriverRuntimeSdioCommandDescriptor::empty()
        };
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_steady_service_lease_is_scoped_to_typed_data_plane_children() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CMD53_READ,
            function: 2,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT,
            addr: 0,
            data_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            len: 64,
            block_size: 64,
            block_count: 1,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(descriptor.valid());

        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD52_READ;
        descriptor.function = 1;
        descriptor.addr = 0x1000;
        descriptor.len = 1;
        descriptor.block_size = 0;
        descriptor.block_count = 0;
        assert!(descriptor.valid());
        descriptor.reserved = 1;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE,
            addr: 0x4359_5302,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE,
            timeout_us: 1_000,
            ..DriverRuntimeSdioCommandDescriptor::empty()
        };
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
            addr: 50_000_000,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE,
            timeout_us: 1_000,
            ..DriverRuntimeSdioCommandDescriptor::empty()
        };
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE,
            function: 2,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT,
            addr: 0,
            data_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            len: 64,
            block_size: 64,
            block_count: 1,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT
                | DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE
                | DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_TX_SERVICE_LEASE,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(descriptor.valid());
        descriptor.flags &= !DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE;
        assert!(!descriptor.valid());
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE;
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_READ;
        assert!(!descriptor.valid());
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE;
        descriptor.function = 1;
        assert!(!descriptor.valid());
        descriptor.function = 2;
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_DPC_SOURCE_W1C_REARM;
        assert!(!descriptor.valid());
    }

    #[test]
    fn cyw43_persistent_parent_budget_and_timeout_are_canonical() {
        assert_eq!(DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_OPS, 192);
        assert_eq!(DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_FRAMES, 64);
        assert_eq!(DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_BYTES, 65_536);
        assert_eq!(
            DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_TIMEOUT_US,
            30_000_000
        );
        let derived_minimum_us = 2_500_000 + 20_560_000 + 2_500_000;
        assert!(DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_TIMEOUT_US >= derived_minimum_us);
        assert!(
            DRIVER_RUNTIME_CYW43_PERSISTENT_PARENT_OPS
                < DRIVER_RUNTIME_CYW43_PARENT_MAX_SDIO_ACTIONS
        );
    }

    #[test]
    fn sdio_persistent_transaction_marker_is_scoped_to_one_linked_primitive() {
        assert_eq!(
            DRIVER_RUNTIME_COMMAND_FLAG_PERSISTENT_TRANSACTION
                & DRIVER_RUNTIME_COMMAND_FLAG_STEADY_SERVICE_LEASE,
            0,
        );

        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE,
            function: 2,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT,
            addr: 0,
            data_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            len: 64,
            block_size: 64,
            block_count: 1,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT
                | DriverRuntimeSdioCommandDescriptor::FLAG_PRE_TX_DPC_FENCE
                | DriverRuntimeSdioCommandDescriptor::FLAG_PERSISTENT_TRANSACTION,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(descriptor.valid());

        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_READ;
        descriptor.function = 1;
        descriptor.flags &= !DriverRuntimeSdioCommandDescriptor::FLAG_PRE_TX_DPC_FENCE;
        assert!(descriptor.valid());
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_HOST_HIGH_SPEED;
        assert!(!descriptor.valid());
        descriptor.flags &= !DriverRuntimeSdioCommandDescriptor::FLAG_HOST_HIGH_SPEED;

        descriptor.function = 0;
        assert!(!descriptor.valid());
        descriptor.function = 1;
        descriptor.reserved = 1;
        assert!(!descriptor.valid());
        descriptor.reserved = 0;
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE,
            addr: 0x4359_5302,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_PERSISTENT_TRANSACTION,
            timeout_us: 1_000,
            ..DriverRuntimeSdioCommandDescriptor::empty()
        };
        assert!(descriptor.valid());
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_DPC_FORCE_SOURCE_PROBE;
        assert!(descriptor.valid());
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
            addr: 50_000_000,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_PERSISTENT_TRANSACTION,
            timeout_us: 1_000,
            ..DriverRuntimeSdioCommandDescriptor::empty()
        };
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_dpc_source_w1c_rearm_requires_exact_event_bound_write_shape() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE,
            function: 1,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT,
            addr: 0x10_020,
            data_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            len: 4,
            block_size: 0,
            block_count: 0,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT
                | DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE
                | DriverRuntimeSdioCommandDescriptor::FLAG_DPC_SOURCE_W1C_REARM,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(descriptor.valid());

        descriptor.flags &= !DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE;
        assert!(!descriptor.valid());
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_STEADY_SERVICE_LEASE;
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_READ;
        assert!(!descriptor.valid());
        descriptor.op = DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE;
        descriptor.function = 2;
        assert!(!descriptor.valid());
        descriptor.function = 1;
        descriptor.len = 8;
        assert!(!descriptor.valid());
        descriptor.len = 4;
        descriptor.flags &= !DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT;
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_command_descriptor_validates_host_config_bounds() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
            addr: 50_000_000,
            data_offset: 0,
            len: 0,
            block_size: 0,
            block_count: 0,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT
                | DriverRuntimeSdioCommandDescriptor::FLAG_HOST_HIGH_SPEED,
            reserved: 0,
            timeout_us: 1000,
        };
        assert!(descriptor.valid());

        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_HOST_CCCR_SPEED_VALID
            | DriverRuntimeSdioCommandDescriptor::FLAG_HOST_CCCR_INTERFACE_VALID;
        descriptor.reserved = u16::from(DriverRuntimeSdioClockSnapshot::CCCR_SPEED_EHS)
            | (u16::from(DriverRuntimeSdioClockSnapshot::CCCR_INTERFACE_WIDTH_4BIT) << 8);
        assert!(descriptor.valid());
        descriptor.reserved &= !u16::from(DriverRuntimeSdioClockSnapshot::CCCR_SPEED_EHS);
        assert!(!descriptor.valid());
        descriptor.reserved = u16::from(DriverRuntimeSdioClockSnapshot::CCCR_SPEED_EHS)
            | (u16::from(DriverRuntimeSdioClockSnapshot::CCCR_INTERFACE_WIDTH_4BIT) << 8);

        descriptor.addr = 100_000_001;
        assert!(!descriptor.valid());
        descriptor.addr = 50_000_000;
        descriptor.len = 1;
        assert!(!descriptor.valid());
        descriptor.len = 0;
        descriptor.data_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_command_descriptor_validates_raw_card_commands() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_CARD_COMMAND,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY,
            addr: 0x0001_0000,
            data_offset: 0,
            len: 7,
            block_size: 0,
            block_count: 0,
            flags: 0,
            reserved: 0,
            timeout_us: 1000,
        };
        assert!(descriptor.valid());

        descriptor.len = 64;
        assert!(!descriptor.valid());
        descriptor.len = 7;
        descriptor.function = 1;
        assert!(!descriptor.valid());
        descriptor.function = 0;
        descriptor.flags = DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT;
        assert!(!descriptor.valid());
        descriptor.flags = 0;
        descriptor.data_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        assert!(!descriptor.valid());
        descriptor.data_offset = 0;
        descriptor.block_count = 1;
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_command_descriptor_reserves_but_rejects_retired_generation_reset() {
        assert_eq!(
            core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>(),
            24
        );
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_GENERATION_RESET,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
            addr: 0x4359_5302,
            data_offset: 0,
            len: 0,
            block_size: 0,
            block_count: 0,
            flags: 0,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(!descriptor.valid());
        descriptor.addr = 0;
        assert!(!descriptor.valid());
        descriptor.addr = 0x4359_5302;
        descriptor.len = 1;
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_command_descriptor_reserves_but_rejects_retired_generation_commit() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_GENERATION_COMMIT,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
            addr: 0x4359_5302,
            data_offset: 0,
            len: 0,
            block_size: 0,
            block_count: 0,
            flags: 0,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(!descriptor.valid());
        descriptor.addr = 0;
        assert!(!descriptor.valid());
        descriptor.addr = 0x4359_5302;
        descriptor.function = 1;
        assert!(!descriptor.valid());
    }

    #[test]
    fn sdio_command_descriptor_validates_generation_bound_dpc_activation() {
        let mut descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_DPC_ACTIVATE,
            function: 0,
            response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
            addr: 0x4359_5302,
            data_offset: 0,
            len: 0,
            block_size: 0,
            block_count: 0,
            flags: 0,
            reserved: 0,
            timeout_us: 1_000,
        };
        assert!(descriptor.valid());

        descriptor.addr = 0;
        assert!(!descriptor.valid());
        descriptor.addr = 0x4359_5302;
        descriptor.len = 1;
        assert!(!descriptor.valid());
        descriptor.len = 0;
        descriptor.flags = DriverRuntimeSdioCommandDescriptor::FLAG_DPC_FORCE_SOURCE_PROBE;
        assert!(descriptor.valid());
        descriptor.flags |= DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT;
        assert!(!descriptor.valid());
        descriptor.flags = DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT;
        assert!(!descriptor.valid());
    }

    #[test]
    fn cyw43_command_descriptor_validates_canonical_shared_payload_bounds() {
        assert_eq!(DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK & 0xfff0, 0);
        assert_eq!(DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT, 8);
        assert_eq!(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK
                & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK,
            0
        );

        let mut descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            flags: 0,
            target_addr: 0x0019_8000,
            payload_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            payload_len: 512,
            total_len: 4096,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        };
        assert!(descriptor.valid());
        descriptor.flags = 1 << 0;
        assert!(!descriptor.valid());
        descriptor.flags = (1 << 0) << 1;
        assert!(!descriptor.valid());
        descriptor.flags = 0;

        descriptor.payload_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        assert!(!descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        descriptor.payload_len = 0;
        assert!(!descriptor.valid());
        descriptor.payload_len = 512;
        descriptor.total_len = 128;
        assert!(!descriptor.valid());
        descriptor.total_len = 4096;
        assert!(descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE + 1;
        assert!(!descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        descriptor.payload_len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES;
        descriptor.total_len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32;
        assert!(descriptor.valid());
        descriptor.payload_len = DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES + 1;
        descriptor.total_len = descriptor.payload_len as u32;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor::empty();
        descriptor.op = DRIVER_RUNTIME_CYW43_OP_RX_POLL;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
            | DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN;
        assert!(descriptor.valid());
        descriptor.flags = 1 << 0;
        assert!(!descriptor.valid());
        descriptor.flags = 0;
        descriptor.payload_len = 1;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor::empty();
        descriptor.op = DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN;
        assert!(!descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
            flags: DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER,
            payload_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            payload_len: 16,
            total_len: 16,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
            | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
            | DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE;
        assert!(!descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN;
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        assert!(descriptor.valid());
        descriptor.payload_len = DRIVER_RUNTIME_CYW43_COMMAND_TX_SHARED_PAYLOAD_BYTES + 1;
        descriptor.total_len = descriptor.payload_len as u32;
        assert!(!descriptor.valid());
        descriptor.payload_len = 16;
        descriptor.total_len = 16;
        descriptor.payload_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        assert!(!descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        descriptor.flags = 1 << 0;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE,
            flags: DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER,
            payload_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            payload_len: 16,
            total_len: 16,
            arg0: 2,
            arg1: 1,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
            | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
            | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
            | DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE;
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
            | DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE;
        assert!(!descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
            | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
            | DRIVER_RUNTIME_CYW43_FLAG_JOIN_PRE_TX_DPC_FENCE;
        descriptor.payload_len = 0;
        assert!(!descriptor.valid());
        descriptor.payload_len = 16;
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        assert!(descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        assert!(!descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE;
        descriptor.flags = 1 << 0;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_ETH_TX,
            flags: 0,
            payload_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            payload_len: 64,
            total_len: 64,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        assert!(descriptor.valid());
        descriptor.flags = DRIVER_RUNTIME_CYW43_FLAG_STEADY_TX_SERVICE_LEASE;
        assert!(descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE + 1;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
            flags: DRIVER_RUNTIME_CYW43_FLAG_STEADY_TX_SERVICE_LEASE,
            payload_offset: DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
            payload_len: 64,
            total_len: 64,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        assert!(!descriptor.valid());
    }

    #[test]
    fn cyw43_armcr4_reset_result_round_trips_edge_attempt_and_readback() {
        let result = driver_runtime_cyw43_armcr4_reset_result(
            DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_CLEAR_WRITE,
            17,
            Some(0x01),
        );
        assert_eq!(
            driver_runtime_cyw43_armcr4_reset_result_edge(result),
            DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_CLEAR_WRITE
        );
        assert_eq!(driver_runtime_cyw43_armcr4_reset_result_attempt(result), 17);
        assert_eq!(
            driver_runtime_cyw43_armcr4_reset_result_readback(result),
            Some(0x01)
        );

        let result = driver_runtime_cyw43_armcr4_reset_result(
            DRIVER_RUNTIME_CYW43_ARMCR4_RESET_EDGE_PRERESET_FLUSH,
            0,
            None,
        );
        assert_eq!(
            driver_runtime_cyw43_armcr4_reset_result_readback(result),
            None
        );
    }

    #[test]
    fn pair_restart_marker_is_out_of_band_and_phase_is_distinct() {
        assert_eq!(DRIVER_RUNTIME_CYW43_PARENT_MAX_SDIO_ACTIONS, 1_024);
        assert_eq!(DRIVER_RUNTIME_TASK_KEY_RESTART_FLAG, 1 << 31);
        assert_eq!(DRIVER_RUNTIME_TASK_KEY_RESTART_FLAG & 0xff, 0);
        assert_ne!(
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_PAIR_RESTART_REQUIRED,
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_TIMEOUT,
        );
        assert_ne!(
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_PAIR_RESTART_REQUIRED,
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_REPLY,
        );
        let retained_owner_phases = [
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_WAKE_RETAINED,
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_WAIT_BEGIN,
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_READY,
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_REJECTED,
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_ACCEPTED,
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_GRANT_ACK_FAILED,
            DRIVER_RUNTIME_RING_PROGRESS_SDIO_OWNER_COMMAND_ADMITTED,
        ];
        assert_eq!(retained_owner_phases, [458, 459, 460, 461, 462, 463, 464]);
        assert!(retained_owner_phases
            .windows(2)
            .all(|window| window[0] < window[1]),);
        let root_grant_phases = [
            DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_WAIT_BEGIN,
            DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_READY,
            DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_REJECTED,
            DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_ACCEPTED,
            DRIVER_RUNTIME_RING_PROGRESS_ROOT_GRANT_ACK_FAILED,
        ];
        assert_eq!(root_grant_phases, [465, 466, 467, 468, 469]);
        assert!(root_grant_phases
            .windows(2)
            .all(|window| window[0] < window[1]),);
    }

    #[test]
    fn pullup_progress_preserves_legacy_values_and_uses_a_distinct_skip_value() {
        assert_eq!(
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_PULLUP_CLEAR,
            451
        );
        assert_eq!(
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_PULLUP_FAULT_CONTAINED,
            452
        );
        assert_eq!(
            DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_PULLUP_SKIPPED,
            457
        );
    }

    #[test]
    fn sdio_fault_frame_dispositions_are_disjoint_and_exhaustive() {
        assert_ne!(DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_CONTAINED, 0);
        assert_ne!(DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_OWNER_PATH_POISONED, 0);
        assert_eq!(
            DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_CONTAINED
                & DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_OWNER_PATH_POISONED,
            0
        );
        assert_eq!(
            DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_MASK,
            DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_CONTAINED
                | DRIVER_RUNTIME_SDIO_FAULT_FRAME_FLAG_OWNER_PATH_POISONED
        );
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_MAGIC, 0x5344_494f);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_VERSION, 3);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_BYTES, 116);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_WORDS, 29);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_ARG_OFFSET, 8);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_FAILURE_OFFSET, 40);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_CS_OFFSET, 56);
        assert_eq!(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_CONBLK_OFFSET, 60);
        assert_eq!(
            DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_DMA_DEBUG_OFFSET + 4,
            usize::from(DRIVER_RUNTIME_SDIO_FAULT_TELEMETRY_BYTES)
        );
    }

    #[test]
    fn sdio_request_child_lifetime_covers_linux_request_and_containment() {
        assert_eq!(DRIVER_RUNTIME_SDIO_REQUEST_TIMEOUT_US, 10_000_000);
        assert_eq!(DRIVER_RUNTIME_SDIO_INHIBIT_TIMEOUT_US, 10_000);
        assert_eq!(DRIVER_RUNTIME_SDIO_CONTAINMENT_TIMEOUT_US, 220_000);
        assert_eq!(DRIVER_RUNTIME_SDIO_TRANSFER_ATTEMPT_LIMIT, 2);
        assert_eq!(DRIVER_RUNTIME_SDIO_CONTAINMENT_ATTEMPT_LIMIT, 2);
        assert_eq!(DRIVER_RUNTIME_SDIO_SERVICE_MAX_OPS, 256);
        assert_eq!(DRIVER_RUNTIME_SDIO_SERVICE_MAX_BYTES, 65_536);
        assert_eq!(DRIVER_RUNTIME_SDIO_SERVICE_MAX_FRAMES, 64);
        assert_eq!(DRIVER_RUNTIME_SDIO_LINK_REQUEST_MAX_FRAMES, 1);
        assert_eq!(DRIVER_RUNTIME_CYW43_SDIO_CHILD_WORST_CASE_US, 20_560_000);
    }
}
