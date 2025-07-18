// CLASSIFICATION: COMMUNITY
// Filename: COHESIX_AARCH64_BUILD.md
// Author: Lukas Bower
// Date Modified: 2027-12-28

_Step 1: Environment setup assumed complete (details managed elsewhere)._

Cohesix Githib Repository Notes
- seL4 source files sould be stored in "third_party/seL4"
- Web Codex cannot push binary files, so complete and tested build scripts must be provided in the "third_party/seL4" directory.

Specific Steps to Building a Custom Rust Root Server on seL4 (AArch64 on QEMU)

Step 1: Create the Rust Root Server Application
	1.	Set up a Rust crate for no-std: Your root task will run without an OS, so configure your Rust crate accordingly. In Cargo.toml, disable the standard library and choose an appropriate crate type. For example:

[package]
name = "myrootserver"
edition = "2021"
# ... other package settings ...

[lib]  # If building as staticlib for linking into C, or use [bin] for standalone
crate-type = ["staticlib"]

[dependencies]
sel4-sys = { git = "https://github.com/AmbiML/sparrow-kata" }  # example using sel4-sys crate [oai_citation:9‡antmicro.com](https://antmicro.com/blog/2022/08/running-rust-programs-in-sel4/#:~:text=In%20order%20to%20start%20a,file%2C%20as%20a%20dependency)

Here we include the sel4-sys crate (or an equivalent) which provides Rust bindings to the seL4 kernel API. This crate will generate the necessary syscall interfaces and constants based on seL4 headers, ensuring your Rust code can invoke seL4 system calls safely ￼ ￼. Also add any other dependencies you need. Use #![no_std] in your Rust code since no standard library is available on seL4 ￼. Also define a panic handler in Rust (e.g., loop forever or call seL4_DebugHalt) to handle any panics ￼.

	2.	Write the entry point in Rust: In a freestanding environment, you typically either define a custom _start or use the provided runtime. Since we link against sel4runtime, we can let it call our main. In Rust, declare your main with C linkage and proper signature so that sel4runtime can find it. For example, in your src/main.rs:

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use sel4_sys::*;  // from sel4-sys crate, provides seL4 API functions and types

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // handle panic (possibly print to kernel debug port)
    unsafe { seL4_DebugPutChar(b'!' as _) };
    loop {}
}

// Define main with C ABI so that sel4runtime's startup code can call it:
#[no_mangle]
pub extern "C" fn main() -> i32 {
    // Example: obtain BootInfo and print a hello message
    let bootinfo = unsafe { &*seL4_GetBootInfo() };
    seL4_DebugPutChar(b'H' as _); seL4_DebugPutChar(b'i' as _);  // minimal "Hi"
    // ... (your root server logic here) ...

    // Prevent returning normally – suspend thread to avoid fault
    unsafe { seL4_TCB_Suspend(seL4_CapInitThreadTCB) };
    0
}

Here we use seL4_GetBootInfo() (through sel4-sys) to get a pointer to the kernel-provided BootInfo structure, and we use seL4_DebugPutChar for output (since initially no drivers are set up). We also explicitly suspend the initial thread at the end instead of returning. This is important: if main returns, the root thread will exit without cleanup, likely causing a fault or kernel panic (as seen in the Hello World tutorial when it simply returns ￼). By suspending itself (the initial thread), we avoid leaving an active thread running out of main. In future, you may handle termination more gracefully (e.g. start a idle loop or system monitor thread), but for now ensure the root thread doesn’t fall off the end.
Common Pitfall: Forgetting to mark main as extern "C" and no_mangle will result in the symbol not being found by the C runtime. Ensure these attributes are present so that the linker can find your main symbol. Also, double-check that you included a panic handler (or panic = "abort" in Cargo profiles) to avoid linking in unwanted runtime parts.

	3.	Ensure compatibility with seL4’s expectations: The sel4-sys crate uses build-time environment variables to locate seL4 headers and configuration. Set SEL4_INCLUDE_DIRS to the path of the kernel’s libsel4/include directory (or set SEL4_PREFIX to the kernel build output prefix) so that the crate’s build script can find the headers ￼. This allows it to generate code matching your kernel’s configuration (for example, the crate will know if MCS is on, which affects syscall numbers, etc.). In your build environment (or in the CMake file), export these variables. For instance:

export SEL4_INCLUDE_DIRS="$PWD/../kernel/libsel4/include"

The Rust build script will also typically link in libsel4.a automatically or generate equivalent syscall stubs. If your approach instead is to link to the actual libsel4.a, you can use a build.rs to instruct rustc to link the library:

println!("cargo:rustc-link-search=native={}", env!("SEL4_LIB_PATH"));
println!("cargo:rustc-link-lib=static=sel4");
println!("cargo:rustc-link-lib=static=sel4runtime");

(assuming you built libsel4 and sel4runtime via CMake and have their path in an env var). However, if using sel4-sys, it may generate all necessary symbols internally using inline assembly for syscalls, meaning linking against libsel4.a might not be required. Consult the crate’s documentation for how it provides the seL4 API. In summary, ensure your Rust compilation knows about the seL4 configuration and has access to the libsel4 headers – otherwise you could encounter mismatches in system call numbers or missing definitions.

	4.	Cross-compile the Rust code for AArch64: Use Rust’s nightly if required (for no_std), and a target specification for bare-metal AArch64. You can use the target triple aarch64-unknown-none (or the more specific custom target from the sel4-sys project, e.g., aarch64-sel4.json). In your .cargo/config.toml, you might specify:

[build]
target = "aarch64-unknown-none.json"  # custom target with "os": "none" and appropriate features

The Colias seL4 Rust training provides an example target spec and config that you can adapt ￼. Ensure the target architecture and pointer width match your seL4 kernel (64-bit ARM). Compile the Rust crate with cargo build --release --target aarch64-unknown-none (the build system can do this for you in the next step). This should produce either a static library (libmyrootserver.a) or a binary ELF, depending on your crate type. We will integrate that into the final image next.

Step 2: Link the Root Server with seL4 Libraries
	1.	Integrate Rust build into CMake: Rather than building the Rust code entirely standalone, it’s best to invoke it as part of the CMake build to ensure proper ordering and linking. You have a few options:
	•	Use a CMake external project or custom command to call Cargo. For example, you can add in your CMakeLists.txt:

find_package(CMakeRust OPTIONAL_COMPONENTS)  # if using a CMake module for Rust
cargo_build(NAME myrootserver_rust 
            SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR} 
            TARGET_ARCH "aarch64-unknown-none") [oai_citation:17‡dornerworks.com](https://www.dornerworks.com/blog/strengthen-your-sel4-userspace-code-with-rust/#:~:text=The%20CMakeLists,and%20which%20architecture%20to%20target) [oai_citation:18‡dornerworks.com](https://www.dornerworks.com/blog/strengthen-your-sel4-userspace-code-with-rust/#:~:text=Building%20the%20application%20is%20handled,the%20other%20seL4%20applications%20are)
# This will build the Rust crate in the current projects directory targeting AArch64.

Make sure the target architecture for cargo matches (you might use aarch64-unknown-linux-gnu for staticlib no_std as a workaround, as shown in a DornerWorks example ￼, or better a proper none target). After this, you’ll have a Rust staticlib or object file output.

	•	Alternatively, compile the Rust code to an ELF binary separately and have the build system treat it as an input file for the image (advanced use). In that case you wouldn’t add_executable in CMake but would package the pre-built ELF in the boot image. However, the simpler method is to produce a static library and link it with a small C stub or use linkers directly.

	2.	Link Rust code with the rootserver target: If you built a static library (libmyrootserver.a), you can link it into the CMake executable target. For example:

target_link_libraries(myrootserver sel4runtime sel4 sel4muslcsys sel4platsupport)
target_link_libraries(myrootserver myrootserver_rust)  # link the Rust static lib

In the Hello World tutorial, the root task links against sel4runtime, sel4, a C library (muslc) and various seL4 userland support libs ￼. For a pure Rust task, you may not need muslc or sel4muslcsys unless you intend to use C library functions (like printf). You do need sel4runtime and sel4 (or their Rust equivalents) to resolve the startup symbols and seL4 syscall stubs. If your Rust code uses sel4-sys, ensure its build script either replaced libsel4 or you link against libsel4.a produced by the kernel build ￼. The sel4runtime.a provides the _sel4_start assembly routine that sets up the initial stack and calls main in the root task ￼ – linking this is crucial for correct bootstrapping. (If you skip it, your root server ELF might not have the proper entry point that seL4 expects, leading to a runtime fault.)

	3.	Use correct linker scripts/addresses: The seL4 build system will handle most of this. By default, user executables are linked to run in their own address space starting at a low virtual address (often 0x400000 for ELF). You typically do not need a custom linker script for the root server if using the standard build, but ensure that the ELF sections won’t overlap with kernel reserved regions. The build system will inform elfloader about where it can load the root server. If you manually specify a linker script (for advanced use), pick a reasonable virtual base (e.g. 0x400000 or 0x1000000) that leaves room at address 0 for catching null pointer dereferences. In most cases, using the default is fine. For instance, in a Genode integration, they set the link address to 0x01000000 to avoid overlap ￼ – but in our case, the seL4 toolchain’s default of 0x400000 for user ELFs (with PIE disabled) works and was used in sel4test (as seen in QEMU logs, the root task was loaded at vaddr 0x400000) ￼.
	4.	Common Mistake – missing symbols: If you encounter undefined references during linking, it usually means you missed a library. For example, if you call certain seL4 utility functions (like those from sel4platsupport or sel4utils), be sure to link those libs as well. If Rust code uses printf from C, link against sel4muslcsys/muslc as the HelloWorld did ￼. Double-check that sel4runtime is linked after your Rust objects if using the C library initialization (to pull in the right _start). The order is generally handled by CMake.

At this stage, your build system should produce the myrootserver executable (an ELF) and the kernel.elf. Next, we’ll package them into a bootable image.

Step 3: Package the Kernel, Root Server, and DTB into a Boot Image
	1.	Combine binaries into a CPIO archive: seL4 uses a simple archive (CPIO) as the boot image, containing the kernel and userland ELF(s) and optionally a device tree. The build system’s DeclareRootserver automatically creates a target for this archive. After a successful build (ninja in the build directory), you should find an image named something like myrootserver-image-arm-qemu-arm-virt (the exact name may vary). This is typically an ELF file that actually contains the embedded archive and the elfloader. For example, in sel4test, the final image sel4test-driver-image-arm-qemu-arm-virt contains the ELF-loader and an embedded CPIO with files kernel and sel4test-driver ￼. You can list sections of the image or use cpio to inspect it if needed.
	2.	Understand the boot sequence: On ARM platforms (including QEMU virt), the boot flow is:
	•	QEMU (or U-Boot/UEFI) loads the elfloader (if using the combined image approach). This elfloader is a small program that runs at a very early stage.
	•	The elfloader finds the embedded CPIO archive within itself and loads the kernel and root server images into memory ￼. It also finds the device tree blob (DTB) either passed by the bootloader or inside the archive ￼.
	•	The elfloader then jumps to the kernel’s entry point, passing it the location of the loaded user images and the DTB.
	•	The kernel boots, then creates the initial thread (root server) and starts it.
In our case, the build system produces an ELF that is the elfloader with the archive. You do not need to manually create the archive; CMake’s MakeCPIO and related functions handle it ￼. Just ensure you used DeclareRootserver so that those rules were generated.
	3.	Include the device tree (DTB): QEMU’s virt board requires a device tree to describe devices like the UART, GIC, etc. The elfloader will search for a DTB in the provided boot info. You have two ways to supply it:
	•	QEMU can provide a DTB via -dtb flag. However, the simpler method in the seL4 context is to embed the DTB in the CPIO. The build system often automatically includes the platform’s default DTB. For instance, if none is passed from firmware, elfloader logs “No DTB passed in from boot loader. Looking for DTB in CPIO…found at … Loaded DTB” ￼.
	•	If your build did not automatically include the DTB, you can add it. Locate the QEMU virt DTB (often provided in the kernel repo or generate from QEMU). Then in CMake, you could use DeclareRootserver’s advanced usage or MakeCPIO to include a file. E.g.,

MakeCPIO(mycpio archive.cpio kernel.elf myrootserver.elf qemu-virt.dtb)

and then instruct elfloader to use that archive (this is usually abstracted by rootserver_image target). In practice, if you target qemu_arm_virt, the build should pick up a default DTB for you. Verify that your QEMU log shows the DTB was loaded (as above).

	4.	Verify the archive content names: By convention, the kernel is stored under the name “kernel” in the CPIO, and the root server ELF is stored under a name matching your target (e.g. “myrootserver”). The elfloader will look for the file named "kernel" and load it at the correct physical address for seL4, and will load the file corresponding to the root task and any other modules. The DeclareRootserver macro ensures these names are set correctly (it uses the target name for the rootserver file). If you were to manually craft or modify the archive, maintain these names. (In the sel4test example, the root server was named “sel4test-driver” in the archive ￼, and elfloader accordingly loaded sel4test-driver as the user program.)
	5.	Avoid sel4test fallback: If you see sel4test output or behavior when booting, it means the wrong image is running. Double-check that the image you run is the one containing your root server, not a pre-built sel4test image. Also ensure that SEL4_DEFAULT_IMAGE or similar CMake cache isn’t pointing to sel4test. Ideally, start the build directory fresh after removing sel4test references. Setting the root server via CMake as we did will prevent any default fallback. (In older setups, one might accidentally run sel4test-image because it was last built – be mindful when using the simulate script or QEMU to point to the correct file under images/.)

Step 4: Run and Verify on QEMU
	1.	Run the image using QEMU: The build should have produced a convenience script called simulate in your build directory if you used GenerateSimulateScript. You can run ./simulate to launch QEMU with the correct parameters ￼. Alternatively, run QEMU manually, for example:

qemu-system-aarch64 -machine virt -cpu cortex-a53 -nographic -m 512M \
    -kernel images/myrootserver-image-arm-qemu-arm-virt

(The -kernel here is pointing to the combined ELF that includes elfloader+CPIO. QEMU will load it at the default address, and the embedded elfloader will take over.) You should also add -serial mon:stdio to ensure the output appears in your terminal.

	2.	Observe the boot output: On a successful boot, you’ll see messages from the elfloader and kernel, for example:

ELF-loader started on CPU: ARM Ltd. Cortex-A53
... 
Loaded DTB from 0x... 
ELF-loading image 'kernel' ... 
ELF-loading image 'myrootserver' ... 
Enabling MMU and paging  
Jumping to kernel-image entry point...  
Bootstrapping kernel  
Booting all finished, dropped to user space  

This indicates the kernel was loaded and started the user space. After that, any output from your root server should appear. If you used seL4_DebugPutChar or the Rust debug_print! macros from the sel4 crate, you might see characters. For example, if your root server prints a message, it should appear after the “dropped to user space”. In the Hello World tutorial, the output was: “Hello, World!” followed by a notice about a fault (because it returned) ￼. In your case, if you suspended the thread, you might instead see your message and then the system will hang idle (since the root thread is no longer running, and seL4 has nothing else to do).

	3.	Debugging tips: If nothing prints from your root server:
	•	Make sure you built a debug-enabled kernel or enabled kernel printing. By default, on the QEMU platform, the build turns on a simplistic serial driver that connects printf to the QEMU console using seL4_DebugPutChar ￼. Ensure LibSel4PlatSupportUseDebug is ON so that any printf or debug_printf in early boot goes to the QEMU console. (The buildsystem typically does this for simulation by default ￼.)
	•	You can instrument the code with more seL4_DebugPutChar calls (which write directly to the kernel console). These are handy in no_std Rust (e.g., wrap it in a Rust print! macro using the debug port) ￼.
	•	If the system crashes early (before your code runs), consider enabling kernel debug builds (-DRELEASE=OFF in init-build.sh) to get kernel assertions. A common crash in the root server startup is forgetting the correct startup routine (leading to a bad instruction or fault immediately). If you see a fault address like 0, it might mean _sel4_start wasn’t linked properly or your main returned unexpectedly ￼. Recheck that sel4runtime is linked and that you didn’t override the entry point.
	4.	Confirm BootInfo usage: If you want to verify that you received the BootInfo, you can print some values from it (e.g., the number of untyped objects or RAM available). This will confirm that your root task has the proper data. The BootInfo is passed by the kernel to the root task’s register (or as argument to _sel4_start). In our Rust setup, we retrieved it with seL4_GetBootInfo(). Ensure that function returned a valid pointer (non-null). You might print bootinfo->nodeID or similar as a simple check.

At this point, you have a running custom root server on seL4! The remaining steps cover proper system setup and future expansion (like memory management and UEFI boot).

Step 5: Implement Root Server Memory Management and Object Allocation

Once your root server is running, it’s responsible for setting up the user-level environment. Here are best practices for managing memory and kernel objects in the root task:
	1.	Understand seL4_BootInfo: The BootInfo structure (accessible via seL4_GetBootInfo()) contains crucial information: it has an array of untyped memory caps (i.e., chunks of physical memory you can allocate from), initial CSpace and VSpace setup, an initial thread TCB cap, and other capabilities (IRQ control, etc.) ￼. Your root server should use this BootInfo to guide memory allocation. For instance, bootinfo->untyped.start and .end tell you the CSpace slots where untyped caps reside, and the untypedSizeList gives the size of each untyped. A typical strategy is to iterate over these untypeds to build a memory allocator.
	2.	Initialize an allocator for untypeds: You can use seL4’s support libraries or write your own allocator:
	•	Using seL4 utils/libs: The library libsel4utils provides functions to bootstrap an allocator from BootInfo. For example, in C, one might use sel4utils_bootstrap_allocator or the allocman library (from seL4_libs) to manage untypeds. These take the BootInfo and create a simple allocator that can dish out memory for new objects. If you prefer Rust, you may either wrap these C functions via FFI or use a Rust allocator crate designed for seL4 (if available).
	•	Custom allocation: A straightforward approach is to pick the largest untyped you have and carve from it for your objects. However, a better approach is to implement a buddy allocator or slab allocator across all untypeds. The goal is to be able to allocate new frames, CNodes, TCBs, etc., by retyping portions of these untyped caps using seL4_Untyped_Retype. For example, to create a new 4K frame, you find an untyped of at least size 12 (2^12 bytes) and call seL4_Untyped_Retype on it to produce a frame object.
	•	Accounting for device memory: Some untypeds may correspond to device memory regions (if your platform has them). These are marked in BootInfo’s untypedIsDevice array. If using an allocator like allocman, you should register those as device-untypeds so that regular allocation doesn’t use them for normal RAM. On QEMU virt, most untypeds will be regular RAM, but be mindful on real hardware.
	3.	Allocate kernel objects through retyping: seL4’s model requires you to “retype” untyped memory into kernel objects (threads, endpoints, page tables, etc.). As the root task, you have the authority to do this. Best practice is to encapsulate these operations in allocator functions. For example, to create a new TCB for a thread, allocate some untyped memory of the appropriate size (e.g., 1 slot from a 4K untyped for a TCB) using your allocator logic, then call seL4_Untyped_Retype to get a TCB cap. Similarly, allocate a small CNode (e.g., 4 or 5 slots) for the thread’s CSpace, etc. Manage the bookkeeping: each retype consumes some portion of an untyped. If using allocman, it will track used portions; if doing manually, you may mark that region as used (perhaps by splitting the untyped capability if partially used – note that seL4 untyped caps can be split by retyping part of them into a smaller untyped cap).
	4.	Use capability space wisely: The root task starts with a CNode (the root CNode) that has a lot of free slots (BootInfo tells you how many). Use these slots to store any new caps you allocate. A simple approach is to keep an index of the next free slot in your CSpace for each new object cap you create. For better structure, you might create a second-level CNode (like a “object table”) to store caps if you plan to delegate to other processes. In any case, ensure you don’t reuse slots erroneously – a capability management library (like libsel4allocman or sel4utils CSpace ops) can help manage slot allocation, but you can also manage an array of booleans or a pointer for the next free slot as an extremely simple method (adequate in early development).
	5.	Avoid preallocating everything upfront (if not needed): Older examples sometimes retyped every untyped into a large pool of objects at startup (frames, page tables, etc.). This can be inflexible and wasteful. Instead, allocate on demand. If you know you will, say, create 2 threads and a few endpoints, you can allocate just those objects. Modern approaches (like in seL4 Core Platform) use a dynamic allocator to ask the kernel for more memory if needed (through a memory server or “root proxy”). The sel4test code was updated to allocate additional resources dynamically rather than all upfront ￼. For a simple system, you can decide at startup what to create, but design your allocator so it can hand out more if needed when your system grows (e.g., if you implement dynamic user-level heap, paging, etc.).
	6.	Initialize critical services: The root task typically also sets up essential services: e.g., a simple scheduler (if you’ll create multiple threads), an IRQ handler thread if you need interrupts, or a pager if using demand paging. At a minimum, consider setting up a fault handler for the root thread (by creating an endpoint and setting the root TCB’s fault EP to it). This way, if your root thread ever faults (e.g., due to a programming error), the kernel will send a fault message to that endpoint rather than killing the system immediately. You can then decide to handle or print the fault. In early stages you might skip this, but it’s good practice as you develop more complex logic.
	7.	Freeing resources: seL4 does not provide automatic garbage collection of kernel objects. If you destroy an object, you must explicitly delete its cap and potentially recycle the underlying memory (untyped). This is advanced (requires CNode delete and maybe recycling untyped, which can only be done if no live objects exist in it). For now, assume objects you create live for the system’s lifetime, or reboot to reclaim memory. Just be aware that leaking caps/objects in the root task can eventually exhaust memory or slots.
	8.	Example – allocate a new endpoint in Rust: Suppose you want to create an IPC endpoint for communication:

// Assume we have a function to allocate an untyped of size = seL4_EndpointBits (4).
let ut_cap = alloc_untyped(seL4_EndpointBits);  // returns a cap to an untyped memory of size 2^4 = 16 bytes
let mut endpoint_cap: seL4_CPtr = 0;
let ret = unsafe {
    seL4_Untyped_Retype(ut_cap, /* type */ seL4_ObjectType_seL4_EndpointObject as usize,
                         /* size_bits */ 0, /* root cnode */ seL4_CapInitThreadCNode,
                         /* destCNodeDepth */ 32, 
                         /* destSlot */ free_slot, /* destThread */ 0, /* numObjects */ 1)
};
if ret != 0 {
    // handle error
} else {
    endpoint_cap = free_slot;
    free_slot += 1;
}

This demonstrates the flow: find appropriate untyped, call Retype to an endpoint (size_bits 0 because endpoint object is 1 slot in that untyped), place the new cap in an empty CSpace slot. In practice, you’d wrap this in safer abstractions. The sel4-sys crate will have definitions for seL4_ObjectType_* enums and the function seL4_Untyped_Retype. Always check the return code (0 indicates success). Repeat similar steps for TCBs (which then need seL4_TCB_Configure and setting up IPC buffer frames, etc.), pages (frames), and so on.

	9.	Leverage existing frameworks when possible: As your project grows, you might consider using higher-level frameworks like CapDL (Capability Distribution Language) or CAmkES for complex setups – but since the goal here is from scratch, stick to manual management initially. Another emerging option is the seL4 Core Platform’s runtime in Rust, which might provide utilities for threads and allocations in a Rust-friendly way ￼ ￼. Keep an eye on such tools if manual management becomes too cumbersome.

In summary, the root server should use the BootInfo to take control of memory and capabilities. Allocate what you need, and establish any fundamental services (threads, communication endpoints) your system will use. By following these practices, you ensure your root task can reliably create and manage objects without running out of resources unexpectedly.

Step 6: Prepare for UEFI Boot and Hardware Deployment

Eventually, you may want to run your system on real hardware with UEFI firmware (or via a UEFI bootloader like GRUB on x86). Ensuring compatibility involves a few considerations:
	1.	Use the ELF-loader’s UEFI support (on ARM): The seL4 elfloader we used in QEMU can be built as an EFI application. The elfloader already contains code to interface with UEFI – it has an entry point _gnuefi_start and will relocate itself as needed for UEFI environments ￼ ￼. To take advantage of this, you would build the image in EFI mode. In the CMake build system, this might be as simple as enabling an EFI target for elfloader (for example, some projects provide an EFI configuration option or a separate target like elfloader-image-efi). Check the seL4 documentation or elfloader repo for how to build it as *.efi. When built, you will get a .efi binary that you can directly execute from a UEFI shell or boot manager. The elfloader will handle loading the kernel and rootserver just as it did under QEMU, and it can even take a DTB from the UEFI if provided (it looks for a configuration table entry for DTB) ￼ ￼.
	2.	Multiboot on x86 (GRUB): If your target is x86_64 with UEFI, note that GRUB on UEFI can boot multiboot modules in a multiboot2-compliant way. The seL4 kernel on x86 includes a Multiboot header, so GRUB can load it and the root server module. In that scenario, you might not use the elfloader (GRUB assumes its role). Instead, GRUB reads a multiboot config: one module is the kernel ELF, the second is the root server ELF, plus a device tree (or ACPI is used on x86). For ARM UEFI, Multiboot is less common; using the elfloader as an EFI binary is the typical approach. If using GRUB/Multiboot, ensure your kernel ELF has the multiboot2 header (seL4 does on x86) and that GRUB’s config lists your rootserver ELF as module with modulenounzip (because seL4 expects it uncompressed).
	3.	UEFI Boot process: On ARM boards with UEFI (e.g., Raspberry Pi with UEFI firmware, or QEMU’s EDK2), you would copy the .efi file to the EFI partition and execute it. That .efi is the elfloader+CPIO image. The UEFI will load it into memory and jump to _gnuefi_start. At that point, the process is the same as QEMU: elfloader loads kernel and rootserver from its embedded archive and starts the kernel ￼. One difference: on real hardware, you’ll need a real device tree for that board. You can often obtain the device’s DTB (sometimes UEFI will provide an ACPI instead – seL4 currently expects a DTB on ARM, so you might need to include a DTB for your board). Some UEFI firmwares for ARM allow passing a DTB to non-Linux OS via a configuration. If not, you can embed the DTB in the CPIO like with QEMU. Ensure the DTB matches the hardware.
	4.	Serial and console under UEFI: On QEMU virt, we rely on the default PL011 at 0x09000000. On a real board, make sure the DTB describes the correct UART. UEFI might initialize the UART, but after seL4 starts, you’ll likely still use seL4_DebugPutChar until you set up a driver. If you want to use UEFI services (like console output) after boot, that’s not possible directly – once seL4 runs, UEFI runtime services are not accessible (seL4 is not an OS with UEFI support built-in). So plan to use seL4-native drivers for any output or storage you need on hardware.
	5.	Secure boot considerations: If you intend to use UEFI Secure Boot or measured boot, you would sign the EFI binary (elfloader image) and deploy keys accordingly. That’s beyond our scope, but keep in mind if you go that route, your image should be one monolithic EFI executable (which it is, in this case).
	6.	Testing on hardware: Before attempting real hardware, test your EFI image under QEMU’s OVMF (UEFI BIOS). QEMU can boot with -bios /path/to/OVMF.fd and then you can use -device loader,file=myrootserver.efi or interact with the UEFI shell to run it. This will mimic how the image runs on a UEFI system. Watch for any differences: e.g., some UEFI firmwares on ARM may not pass a DTB automatically. The elfloader log will tell you if it found a DTB or not. For example, a UEFI boot log on x86 might show multiboot info being parsed ￼, or on ARM an absence of DTB that you must resolve.
	7.	Ensure alignment with hardware kernel configuration: When moving to actual hardware, double-check kernel configuration options like KernelARMPlatform (it must match the board, e.g., qemu-arm-virt vs raspberrypi4 etc.), and possibly rebuild the kernel for that platform. The userland should ideally be platform-agnostic (except for device drivers), but if you wrote any device-specific code, adjust for the real board. Additionally, if your hardware requires the use of EL2 (hyp mode) or has secure vs non-secure world requirements, configure seL4 appropriately (for instance, some platforms might need CONFIG_ARM_HYP_MODE).

By following these guidelines, your project’s design will remain compatible with UEFI and other bootloaders. The key is that our approach – using the elfloader and a CPIO archive – is general: on ARM hardware, we can package an EFI loader ￼, and on x86 we rely on multiboot. Both methods have been used in practice to boot seL4 on PCs and boards via UEFI ￼ ￼. Your buildsystem can produce the necessary images; it’s mostly a matter of selecting the right build target (e.g., an EFI image) and providing the correct device description (DTB or ACPI).

⸻

Recap: We built a Rust-based root server and integrated it with seL4 on QEMU. We ensured correct linking to seL4 libraries (using libsel4 and sel4runtime to get the system calls and startup code ￼), packaged the kernel and root task into a bootable image, and saw how to avoid the common pitfall of falling back to the default test image by explicitly specifying our root server. We also discussed how to manage memory and capabilities in the root task (using BootInfo to handle untyped memory and create new objects), and how to plan for running the system on real hardware using UEFI boot mechanisms. By following this step-by-step guide, you can create a custom seL4 system with your own root server, started from scratch but using the best practices from seL4’s examples and recent developments. Good luck with your seL4 project!

Sources:
	•	seL4 Tutorial: Hello World (build system and root task setup) ￼ ￼
	•	seL4 Build System documentation (CMake integration and DeclareRootserver) ￼ ￼
	•	DornerWorks blog – Rust in seL4 userspace (integrating Cargo build with CMake) ￼ ￼
	•	Antmicro blog – Running Rust on seL4 (sel4-sys crate usage) ￼ ￼
	•	seL4 elfloader documentation (boot sequence and EFI support) ￼ ￼
	•	QEMU/sel4test boot log (CPIO contents: kernel, rootserver, DTB) ￼.