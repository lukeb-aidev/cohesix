// Author: Lukas Bower
// Purpose: Define pointer-free ABI records shared by Pi 4 root and driver runtimes.
// Copyright 2026 Lukas Bower

#![no_std]
#![deny(unsafe_code)]

/// Magic value for a pointer-free driver runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_MAGIC: u32 = 0x4452_4934;
/// Runtime descriptor layout version.
pub const DRIVER_RUNTIME_INIT_VERSION: u16 = 5;
/// Magic value for a sealed runtime identity inside an init descriptor.
pub const DRIVER_RUNTIME_IDENTITY_MAGIC: u32 = 0x4452_4944;
const DRIVER_RUNTIME_IDENTITY_HASH_SEED: u32 = 0x811c_9dc5;
const DRIVER_RUNTIME_IDENTITY_HASH_PRIME: u32 = 0x0100_0193;
/// Command `aux0` value used to submit a runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_AUX: u32 = 0x4452_494e;
/// Command `aux0` value used to ask a linked runtime to instantiate its engine state.
pub const DRIVER_RUNTIME_ENGINE_INIT_AUX: u32 = 0x454e_474e;
/// Local-seat USB/HDMI init command used by the root ring client.
pub const DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX: u32 = 0x4c53_494e;
/// Serial-console service command that samples the mini-UART transmitter-idle bit.
pub const DRIVER_RUNTIME_SERIAL_TX_IDLE_AUX: u32 = 0x5345_5244;

const fn driver_runtime_nonzero_hash(hash: u32) -> u32 {
    if hash == 0 {
        DRIVER_RUNTIME_IDENTITY_MAGIC
    } else {
        hash
    }
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
/// Maximum reciprocal SDIO actions retained by one immutable CYW43 parent command.
///
/// Root uses the same bound to cap child-completion deadline renewals, so a
/// multi-action Linux-shaped operation can outlive each legal child request
/// without turning progress into an unbounded parent lease.
pub const DRIVER_RUNTIME_CYW43_PARENT_MAX_SDIO_ACTIONS: u16 = 1_024;
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
/// CYW43 positive detail: a control Function 2 TX retry recovered a transfer fault.
pub const DRIVER_RUNTIME_CYW43_CONTROL_DETAIL_TX_F2_RETRY_RECOVERED: u16 = 0x5801;
/// CYW43 positive detail: an event/data frame interrupted a retained control exchange.
///
/// The exchange remains active in the isolated runtime. Root must route the
/// frame and resubmit the identical BCDC command identity; the runtime must not
/// transmit the request a second time.
pub const DRIVER_RUNTIME_CYW43_CONTROL_DETAIL_INTERLEAVED_FRAME: u16 = 0x5802;
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
/// PCIe runtime command operation: read one 32-bit xHCI/VL805 register.
pub const DRIVER_RUNTIME_PCIE_OP_PORT_READ: u16 = 1;
/// PCIe runtime command operation: write one 32-bit xHCI/VL805 register.
pub const DRIVER_RUNTIME_PCIE_OP_PORT_WRITE: u16 = 2;
/// PCIe runtime command operation: flush posted writes.
pub const DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH: u16 = 3;
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
/// the sealed init descriptor within the 1,536-byte command-frame budget.
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
/// Fixed offset of the runtime progress marker in one ring page.
pub const DRIVER_RUNTIME_RING_PROGRESS_OFFSET: u16 = 128;
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
/// Bytes in the runtime progress marker.
pub const DRIVER_RUNTIME_RING_PROGRESS_BYTES: u16 = 16;
/// Runtime progress-marker magic.
pub const DRIVER_RUNTIME_RING_PROGRESS_MAGIC: u32 = 0x4452_5047;
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
pub const DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RECV_READY: u32 = 201;
/// Linked runtime completed an intake poll without consuming a new command.
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
};
/// First child CSpace slot reserved for driver-owned IRQ handler caps.
pub const DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT: u32 = 4;
/// Child CSpace slot containing each runtime's local notification receive cap.
pub const DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT: u32 = 3;
/// BCM2711 auxiliary mini-UART interrupt used by the isolated serial runtime.
pub const DRIVER_RUNTIME_SERIAL_IRQ: u32 = 125;
/// Nonzero notification badge bound to [`DRIVER_RUNTIME_SERIAL_IRQ`].
pub const DRIVER_RUNTIME_SERIAL_IRQ_BADGE: u32 = DRIVER_RUNTIME_SERIAL_IRQ + 1;
/// CYW43 child CSpace slot containing its send-only root RX-wake notification cap.
pub const DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT: u32 = 11;
/// Exact badge delivered to root for a CYW43 private RX queue empty-to-nonempty edge.
pub const DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE: u32 = 1;
/// BCM2711 SDIO host interrupt used by the CYW43 card function.
pub const DRIVER_RUNTIME_SDIO_IRQ: u32 = 158;
/// Nonzero notification badge bound to [`DRIVER_RUNTIME_SDIO_IRQ`].
pub const DRIVER_RUNTIME_SDIO_IRQ_BADGE: u32 = DRIVER_RUNTIME_SDIO_IRQ + 1;
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

/// Durable authority for exactly one retained-command continuation quantum.
///
/// `grant_id` is the sequence-last commit word. Producers publish zero there,
/// then the immutable request identity, and finally a nonzero monotonically
/// increasing ID. `consumed_grant_id` is written only by the consumer after it
/// spends that grant on one arbitration quantum. Producers re-signal an
/// unacknowledged ID rather than overwriting it. Consumers accept the record
/// only when both reads of the ID match and its request/fingerprint/generation
/// match the retained command. A notification is therefore only a wake hint;
/// its coalesced badge cannot create, duplicate, or mutate foreground authority.
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
    /// Consumer-published ID of the most recently spent grant.
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
/// SDIO-owner CSpace slot containing the send-only CYW43 notification cap.
pub const DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT: u32 = 10;
/// USB-local virtual address where root maps the PCIe owner command ring.
pub const DRIVER_RUNTIME_BUS_LINK_PCIE_RING_VADDR: u64 = 0x70e0_1000;
/// CYW43-local virtual address where root maps the SDIO owner command ring.
pub const DRIVER_RUNTIME_BUS_LINK_SDIO_RING_VADDR: u64 = 0x70e0_0000;
/// Command flag: root delivered this turn with send-only IPC and expects no reply cap.
pub const DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY: u16 = 1 << 13;

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
pub const DRIVER_RUNTIME_DPC_EVENT_RING_VERSION: u16 = 2;
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
/// Complete set of flags admitted by [`DriverRuntimeDpcEventRing::valid`].
pub const DRIVER_RUNTIME_DPC_EVENT_RING_KNOWN_FLAGS: u32 =
    DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OVERRUN
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_ACK_PENDING
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_POISONED
        | DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_CARD_IRQ_MASKED;
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

/// Required descriptor flags for any acceptance-eligible hardware runtime.
pub const DRIVER_RUNTIME_INIT_REQUIRED_FLAGS: u32 = DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE
    | DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS
    | DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING
    | DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN;

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
    /// Reserved for alignment and future fields.
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
    /// A Function-2 CMD53 write must sample the host `CARD_INT` source at the
    /// final pre-issue boundary and defer without issuing when it is asserted.
    pub const FLAG_PRE_TX_DPC_FENCE: u16 = 1 << 4;

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
                    && self.reserved == 0
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
                    && (self.flags == 0 || self.flags == Self::FLAG_DPC_FORCE_SOURCE_PROBE)
                    && self.reserved == 0))
            && (!pre_tx_dpc_fence
                || (self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE && self.function == 2))
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
        known_kind
            && self.vaddr != 0
            && self.paddr != 0
            && self.bytes != 0
            && self.page_count != 0
            && self.bytes <= max_bytes
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
    /// Reserved for alignment and future fixed-layout fields.
    pub reserved0: u16,
    /// Child CSpace slot containing a send-only root wake cap, or zero when absent.
    pub root_wake_notification_slot: u32,
    /// Exact badge on the root wake cap, or zero when absent.
    pub root_wake_notification_badge: u32,
    /// [`DRIVER_RUNTIME_IDENTITY_MAGIC`] when root sealed this descriptor.
    pub identity_magic: u32,
    /// Stable driver-task key supplied in the runtime entry register.
    pub task_key: u32,
    /// Hash of the generated runtime artifact contract selected by root.
    pub artifact_hash: u32,
    /// Sealed token over task key, artifact hash, hot path, and role bit.
    pub identity_token: u32,
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
            reserved0: 0,
            root_wake_notification_slot: 0,
            root_wake_notification_badge: 0,
            identity_magic: 0,
            task_key: 0,
            artifact_hash: 0,
            identity_token: 0,
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
        let mut index = 0usize;
        while index < self.bus_link_count as usize {
            self.bus_links[index] =
                self.bus_links[index].with_sealed_identity(task_key, self.hot_path);
            index += 1;
        }
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
            && self.shared_page_count != 0
            && (self.mmio_page_count as usize) <= DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES
            && (self.dma_page_count as usize) <= DRIVER_RUNTIME_INIT_MAX_DMA_PAGES
            && (self.shared_page_count as usize) <= DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES
            && (self.irq_count as usize) <= DRIVER_RUNTIME_INIT_MAX_IRQS
            && (self.bus_link_count as usize) <= DRIVER_RUNTIME_INIT_MAX_BUS_LINKS
            && (self.resource_range_count as usize) <= DRIVER_RUNTIME_INIT_MAX_RESOURCE_RANGES
            && self.root_wake_notification_valid()
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

    #[test]
    fn init_descriptor_is_bounded_for_ring_payload() {
        assert!(core::mem::size_of::<DriverRuntimeInitDescriptor>() <= 1536);
        assert_eq!(core::mem::align_of::<DriverRuntimeInitDescriptor>(), 8);
        assert!(DRIVER_RUNTIME_INIT_MAX_DMA_PAGES >= 80);
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
    fn continuation_grant_fits_reserved_command_slot_and_fingerprints_actions() {
        assert_eq!(core::mem::size_of::<DriverRuntimeContinuationGrant>(), 24);
        assert_eq!(core::mem::align_of::<DriverRuntimeContinuationGrant>(), 4);
        assert_eq!(DRIVER_RUNTIME_CONTINUATION_GRANT_OFFSET, 40);
        assert_eq!(DRIVER_RUNTIME_CONTINUATION_GRANT_BYTES, 24);
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
    fn dpc_event_ring_is_fixed_bounded_and_sequence_checked() {
        assert_eq!(core::mem::size_of::<DriverRuntimeDpcEventEntry>(), 16);
        assert_eq!(core::mem::size_of::<DriverRuntimeDpcEventRing>(), 96);
        assert_eq!(DRIVER_RUNTIME_DPC_EVENT_RING_VERSION, 2);
        assert_eq!(DRIVER_RUNTIME_INIT_VERSION, 5);
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
    fn dpc_event_ring_accepts_every_known_state_flag() {
        let mut ring = DriverRuntimeDpcEventRing::empty(7);
        for flag in [
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_OVERRUN,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_ACK_PENDING,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_POISONED,
            DRIVER_RUNTIME_DPC_EVENT_RING_FLAG_CARD_IRQ_MASKED,
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
            });

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
    }

    #[test]
    fn empty_descriptor_needs_role_and_buffers_before_valid() {
        let descriptor = DriverRuntimeInitDescriptor::empty();
        assert!(!descriptor.valid());
        assert_eq!(descriptor.magic, DRIVER_RUNTIME_INIT_MAGIC);
        assert_eq!(descriptor.version, DRIVER_RUNTIME_INIT_VERSION);
    }

    #[test]
    fn valid_descriptor_requires_pointer_free_shared_and_bus_flags() {
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
    fn valid_for_resources_rejects_count_mismatch() {
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
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
        descriptor.payload_offset = DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE + 1;
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
