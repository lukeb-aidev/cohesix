// Author: Lukas Bower
// Purpose: Define pointer-free ABI records shared by Pi 4 root and driver runtimes.
// Copyright 2026 Lukas Bower

#![no_std]
#![deny(unsafe_code)]

/// Magic value for a pointer-free driver runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_MAGIC: u32 = 0x4452_4934;
/// Runtime descriptor layout version.
pub const DRIVER_RUNTIME_INIT_VERSION: u16 = 1;
/// Command `aux0` value used to submit a runtime initialization descriptor.
pub const DRIVER_RUNTIME_INIT_AUX: u32 = 0x4452_494e;
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
/// First child CSpace slot reserved for driver-owned IRQ handler caps.
pub const DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT: u32 = 4;

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
        self.vaddr != 0
            && self.width != 0
            && self.height != 0
            && self.pitch != 0
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
        self.valid()
            && self.hot_path == hot_path
            && self.role_bit == role_bit
            && self.mmio_page_count == mmio_pages
            && self.dma_page_count == dma_pages
            && self.shared_page_count == shared_pages
    }

    /// Returns true when this descriptor is eligible to back HDMI ownership.
    #[must_use]
    pub const fn hdmi_ready(self) -> bool {
        self.valid()
            && self.hot_path == HOT_PATH_HDMI_TEXT
            && (self.flags & DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER) != 0
            && self.framebuffer.valid()
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
}
