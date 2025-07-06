// CLASSIFICATION: COMMUNITY
// Filename: main.c v0.4
// Author: Lukas Bower
// Date Modified: 2025-09-08
// SPDX-License-Identifier: MIT
#include <efi.h>
#include <efiprot.h>
#include <efilib.h>
#include "sha1.h"

#define EFI_FILE_MODE_READ   0x0000000000000001ULL
#define EFI_FILE_MODE_WRITE  0x0000000000000002ULL
#define EFI_FILE_MODE_CREATE 0x8000000000000000ULL

static void log_message(EFI_FILE_HANDLE root, const CHAR16 *msg)
{
    EFI_FILE_HANDLE log;
    CHAR16 log_path[] = L"\\boot.log";
    EFI_STATUS s = uefi_call_wrapper(root->Open, 5, root, &log, log_path,
                                     EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE |
                                         EFI_FILE_MODE_CREATE,
                                     0);
    if (EFI_ERROR(s))
        return;
    uefi_call_wrapper(log->SetPosition, 2, log, (UINT64)-1);
    UINTN len = StrLen(msg) * sizeof(CHAR16);
    uefi_call_wrapper(log->Write, 3, log, &len, (void *)msg);
    uefi_call_wrapper(log->Close, 1, log);
}

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

    // Verify kernel hash before loading
    EFI_FILE_HANDLE kfile;
    status = uefi_call_wrapper(root->Open, 5, root, &kfile, L"\\kernel.elf",
                               EFI_FILE_MODE_READ, 0);
    if (EFI_ERROR(status)) {
        Print(L"kernel.elf missing\n");
        log_message(root, L"kernel.elf missing\n");
        return status;
    }
    sha1_ctx ctx;
    sha1_init(&ctx);
    UINT8 buf[512];
    for (;;) {
        UINTN sz = sizeof(buf);
        status = uefi_call_wrapper(kfile->Read, 3, kfile, &sz, buf);
        if (EFI_ERROR(status)) {
            Print(L"kernel read error\n");
            log_message(root, L"kernel read error\n");
            uefi_call_wrapper(kfile->Close, 1, kfile);
            return status;
        }
        if (sz == 0)
            break;
        sha1_update(&ctx, buf, sz);
    }
    uefi_call_wrapper(kfile->Close, 1, kfile);
    UINT8 digest[20];
    sha1_final(&ctx, digest);
    char actual_hex[41];
    sha1_to_hex(digest, actual_hex);
    EFI_FILE_HANDLE hashf;
    status = uefi_call_wrapper(root->Open, 5, root, &hashf, L"\\kernel.sha1",
                               EFI_FILE_MODE_READ, 0);
    if (EFI_ERROR(status)) {
        Print(L"kernel.sha1 missing\n");
        log_message(root, L"kernel.sha1 missing\n");
        return status;
    }
    char expected[41];
    UINTN hsz = sizeof(expected) - 1;
    status = uefi_call_wrapper(hashf->Read, 3, hashf, &hsz, expected);
    uefi_call_wrapper(hashf->Close, 1, hashf);
    expected[hsz] = '\0';
    for (UINTN i = 0; i < hsz; i++) {
        if (expected[i] == '\n' || expected[i] == '\r') {
            expected[i] = '\0';
            break;
        }
    }
    if (CompareMem(expected, actual_hex, 40) != 0) {
        Print(L"kernel hash mismatch\n");
        log_message(root, L"kernel hash mismatch\n");
        return EFI_SECURITY_VIOLATION;
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
    Print(L"Launching kernel.elf...\n");

    status = uefi_call_wrapper(system_table->BootServices->StartImage, 3,
                               kernel_image, NULL, NULL);
    if (EFI_ERROR(status)) {
        Print(L"Failed to start kernel. %r\n", status);
        return status;
    }
    Print(L"Kernel launched.\n");
    return status;
}
