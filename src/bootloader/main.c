// CLASSIFICATION: COMMUNITY
// Filename: main.c v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22
// SPDX-License-Identifier: MIT
#include <efi.h>
#include <efilib.h>

EFI_STATUS EFIAPI efi_main(EFI_HANDLE image, EFI_SYSTEM_TABLE *system_table) {
    InitializeLib(image, system_table);
    Print(L"Starting Cohesix EFI loader...\n");
    Print(L"Booting Cohesix from UEFI...\n");

    EFI_STATUS status;
    EFI_GUID loaded_image_guid = LOADED_IMAGE_PROTOCOL;
    EFI_LOADED_IMAGE *loaded_image;
    status = uefi_call_wrapper(system_table->BootServices->HandleProtocol, 3,
                               image, &loaded_image_guid, (void **)&loaded_image);
    if (EFI_ERROR(status)) {
        Print(L"LoadedImage protocol failed\n");
        return status;
    }

    EFI_GUID fs_guid = SIMPLE_FILE_SYSTEM_PROTOCOL;
    EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *fs;
    status = uefi_call_wrapper(system_table->BootServices->HandleProtocol, 3,
                               loaded_image->DeviceHandle, &fs_guid, (void **)&fs);
    if (EFI_ERROR(status)) {
        Print(L"FileSystem protocol failed\n");
        return status;
    }

    EFI_FILE_HANDLE root;
    status = uefi_call_wrapper(fs->OpenVolume, 2, fs, &root);
    if (EFI_ERROR(status)) {
        Print(L"OpenVolume failed\n");
        return status;
    }

    EFI_DEVICE_PATH *kernel_path = FileDevicePath(loaded_image->DeviceHandle, L"kernel.elf");
    EFI_HANDLE kernel_image;
    status = uefi_call_wrapper(system_table->BootServices->LoadImage, 6,
                               FALSE, image, kernel_path, NULL, 0, &kernel_image);
    if (EFI_ERROR(status)) {
        FreePool(kernel_path);
        kernel_path = FileDevicePath(loaded_image->DeviceHandle, L"init.elf");
        status = uefi_call_wrapper(system_table->BootServices->LoadImage, 6,
                                   FALSE, image, kernel_path, NULL, 0, &kernel_image);
        if (EFI_ERROR(status)) {
            Print(L"Kernel not found!\n");
            return status;
        }
    }
    FreePool(kernel_path);
    Print(L"kernel.elf loaded successfully\n");

    status = uefi_call_wrapper(system_table->BootServices->StartImage, 3,
                               kernel_image, NULL, NULL);
    if (EFI_ERROR(status)) {
        Print(L"Kernel start failed: %r\n", status);
    }
    return status;
}
