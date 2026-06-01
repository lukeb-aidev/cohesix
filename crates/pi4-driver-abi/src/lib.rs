// Author: Lukas Bower
// Purpose: Define pointer-free ABI records shared by Pi 4 root and driver runtimes.
// Copyright 2026 Lukas Bower

#![no_std]
#![deny(unsafe_code)]

/// Magic value for a pointer-free driver runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_MAGIC: u32 = 0x4452_4934;
/// Runtime descriptor layout version.
pub const DRIVER_RUNTIME_INIT_VERSION: u16 = 2;
/// Command `aux0` value used to submit a runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_AUX: u32 = 0x4452_494e;
/// Command `aux0` value used to ask a linked runtime to instantiate its engine state.
pub const DRIVER_RUNTIME_ENGINE_INIT_AUX: u32 = 0x454e_474e;
/// Local-seat USB/HDMI init command used by the root ring client.
pub const DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX: u32 = 0x4c53_494e;
/// USB runtime init detail: xHCI controller reached run state, no keyboard endpoint yet.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY: u16 = 0x0201;
/// USB runtime init detail: xHCI controller and boot keyboard endpoint are ready.
pub const DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY: u16 = 0x0202;
/// USB service detail: keyboard endpoint is armed, but no interrupt report has arrived.
pub const DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING: u16 = 0x0203;
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
/// GENET/CYW43 network init command used by the root ring client.
pub const DRIVER_RUNTIME_NET_INIT_AUX: u32 = 0x494e_4954;
/// CYW43 command descriptor submission marker used in `aux0`.
pub const DRIVER_RUNTIME_CYW43_COMMAND_AUX: u32 = 0x4359_5734;
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
/// SDIO bus-owner operation: poll interrupt status.
pub const DRIVER_RUNTIME_SDIO_OP_POLL_IRQ: u16 = 5;
/// SDIO bus-owner operation: apply host-controller clock and bus-width state.
pub const DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG: u16 = 6;
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
pub const DRIVER_RUNTIME_INIT_MAX_DMA_PAGES: usize = 64;
/// Maximum root/driver shared pages carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES: usize = 16;
/// Maximum IRQ descriptors carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_IRQS: usize = 4;
/// Maximum bus-link descriptors carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_BUS_LINKS: usize = 2;
/// Maximum semantic resource ranges carried in one init descriptor.
pub const DRIVER_RUNTIME_INIT_MAX_RESOURCE_RANGES: usize = 8;
/// Runtime resource descriptors use 4 KiB pages.
pub const DRIVER_RUNTIME_RESOURCE_PAGE_BYTES: u64 = 4096;
/// Fixed offset of the root/runtime payload area in one ring page.
pub const DRIVER_RUNTIME_RING_FRAME_OFFSET: u16 = 256;
/// Bytes in one command/completion ring page.
pub const DRIVER_RUNTIME_RING_PAGE_BYTES: u16 = 4096;
/// First child CSpace slot reserved for driver-owned IRQ handler caps.
pub const DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT: u32 = 4;
/// Child CSpace slot where USB receives the PCIe/VL805 bus-owner endpoint cap.
pub const DRIVER_RUNTIME_BUS_LINK_PCIE_ENDPOINT_SLOT: u32 = 9;
/// Child CSpace slot where CYW43 receives the SDIO bus-owner endpoint cap.
pub const DRIVER_RUNTIME_BUS_LINK_SDIO_ENDPOINT_SLOT: u32 = 8;
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

/// Bus link flag: child runtime issues requests to the linked bus owner.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT: u32 = 1 << 0;
/// Bus link flag: channel carries only pointer-free ring offsets/lengths.
pub const DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE: u32 = 1 << 1;
/// Bus link channel id for USB using the PCIe/VL805 owner.
pub const DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE: u32 = 1;
/// Bus link channel id for CYW43 using the SDIO owner.
pub const DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO: u32 = 2;

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
    /// SDIO register/window address.
    pub addr: u32,
    /// Data payload offset inside the fixed command ring page.
    pub data_offset: u16,
    /// Data bytes for byte-mode transfers.
    pub len: u16,
    /// Block size for CMD53 block-mode transfers.
    pub block_size: u16,
    /// Block count for CMD53 block-mode transfers.
    pub block_count: u16,
    /// Bit 0 requests incrementing CMD53 address mode.
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
            || self.op == DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG;
        let host_config = self.op == DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG;
        let known_response = self.response_kind == DRIVER_RUNTIME_SDIO_RESP_NONE
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_OCR
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_SHORT
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY
            || self.response_kind == DRIVER_RUNTIME_SDIO_RESP_LONG;
        let cmd52 = self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE;
        let cmd53 = self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE;
        let read_result = self.op == DRIVER_RUNTIME_SDIO_OP_CMD52_READ
            || self.op == DRIVER_RUNTIME_SDIO_OP_POLL_IRQ;
        let effective_len = if read_result {
            1
        } else if host_config {
            0
        } else if self.block_count != 0 {
            (self.block_count as u32).saturating_mul(self.block_size as u32)
        } else {
            self.len as u32
        };
        let payload_end = self.data_offset as u32 + effective_len;
        known_op
            && known_response
            && self.function <= 7
            && (host_config || self.addr < (1 << 17))
            && (!host_config
                || (self.function == 0
                    && self.response_kind == DRIVER_RUNTIME_SDIO_RESP_NONE
                    && self.data_offset == 0
                    && self.len == 0
                    && self.block_size == 0
                    && self.block_count == 0
                    && self.reserved == 0
                    && self.addr <= 100_000_000))
            && (!cmd52 || (self.len == 1 && self.block_count == 0 && self.block_size == 0))
            && (!cmd53
                || ((self.len != 0 || self.block_count != 0)
                    && (self.block_count == 0
                        || (self.block_size != 0
                            && self.block_size <= 512
                            && self.block_count <= 511))))
            && (host_config
                || (effective_len != 0
                    && self.data_offset >= DRIVER_RUNTIME_RING_FRAME_OFFSET
                    && payload_end <= DRIVER_RUNTIME_RING_PAGE_BYTES as u32))
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
    /// Payload offset inside the fixed command ring page.
    pub payload_offset: u16,
    /// Payload bytes carried in this command.
    pub payload_len: u16,
    /// Total stream length for chunked transfers.
    pub total_len: u32,
    /// Operation-specific argument.
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
            || self.op == DRIVER_RUNTIME_CYW43_OP_RX_POLL;
        let carries_payload = self.op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
            || self.op == DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
            || self.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
            || self.op == DRIVER_RUNTIME_CYW43_OP_ETH_TX;
        let zero_payload = self.op == DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
            || self.op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP
            || self.op == DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL
            || self.op == DRIVER_RUNTIME_CYW43_OP_RELEASE
            || self.op == DRIVER_RUNTIME_CYW43_OP_RX_POLL;
        let payload_end = self.payload_offset as u32 + self.payload_len as u32;
        known_op
            && ((carries_payload
                && self.payload_len != 0
                && self.payload_offset >= DRIVER_RUNTIME_RING_FRAME_OFFSET
                && payload_end <= DRIVER_RUNTIME_RING_PAGE_BYTES as u32)
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
    /// Reserved for alignment and future fields.
    pub reserved: u32,
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
            reserved: 0,
        }
    }

    /// Returns true when the link contains a bounded pointer-free channel.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.owner_hot_path >= HOT_PATH_SERIAL_CONSOLE
            && self.owner_hot_path <= HOT_PATH_PCIE_ROOT
            && self.channel_id != 0
            && (self.flags & DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE) != 0
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
            && self.valid_bus_links()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_descriptor_is_bounded_for_ring_payload() {
        assert!(core::mem::size_of::<DriverRuntimeInitDescriptor>() <= 1536);
        assert_eq!(core::mem::align_of::<DriverRuntimeInitDescriptor>(), 8);
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
            0,
            DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as u32,
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
        );

        assert!(descriptor.valid());
        assert!(descriptor.has_bus_link_to(HOT_PATH_SDIO_HOST));
        assert!(descriptor.has_pointer_free_bus_link(
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
    fn cyw43_command_descriptor_validates_ring_payload_bounds() {
        let mut descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            flags: 0,
            target_addr: 0x0019_8000,
            payload_offset: DRIVER_RUNTIME_RING_FRAME_OFFSET,
            payload_len: 512,
            total_len: 4096,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        };
        assert!(descriptor.valid());

        descriptor.payload_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET - 1;
        assert!(!descriptor.valid());
        descriptor.payload_offset = DRIVER_RUNTIME_RING_FRAME_OFFSET;
        descriptor.payload_len = 0;
        assert!(!descriptor.valid());
        descriptor.payload_len = 512;
        descriptor.total_len = 128;
        assert!(!descriptor.valid());
        descriptor.total_len = 4096;
        descriptor.payload_offset = DRIVER_RUNTIME_RING_PAGE_BYTES - 128;
        descriptor.payload_len = 129;
        assert!(!descriptor.valid());

        descriptor = DriverRuntimeCyw43CommandDescriptor::empty();
        descriptor.op = DRIVER_RUNTIME_CYW43_OP_RX_POLL;
        assert!(descriptor.valid());
        descriptor.payload_len = 1;
        assert!(!descriptor.valid());
    }
}
