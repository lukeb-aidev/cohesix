// CLASSIFICATION: COMMUNITY
// Filename: main.c v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-30
// SPDX-License-Identifier: MIT
#include <efi.h>
#include <efilib.h>
#include <string.h>

EFI_STATUS
efi_main(EFI_HANDLE image, EFI_SYSTEM_TABLE *systab) {
    InitializeLib(image, systab);
    Print(L"Init EFI running\n");

    EFI_STATUS status;
    EFI_GUID fs_guid = SIMPLE_FILE_SYSTEM_PROTOCOL;
    EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *fs;
    status = uefi_call_wrapper(systab->BootServices->HandleProtocol, 3,
                               image, &fs_guid, (void **)&fs);
    if (EFI_ERROR(status)) {
        Print(L"[init] FileSystem protocol unavailable\n");
        return status;
    }

    EFI_FILE_HANDLE root, file;
    status = uefi_call_wrapper(fs->OpenVolume, 2, fs, &root);
    if (EFI_ERROR(status)) {
        Print(L"[init] Failed to open volume\n");
        return status;
    }

    status = uefi_call_wrapper(root->Open, 5, root, &file,
                               L"\\etc\\init.cfg", EFI_FILE_MODE_READ, 0);
    if (EFI_ERROR(status)) {
        Print(L"[init] warning: /etc/init.cfg not found\n");
        return EFI_SUCCESS;
    }

    CHAR8 buf[256];
    UINTN sz = sizeof(buf) - 1;
    status = uefi_call_wrapper(file->Read, 3, file, &sz, buf);
    buf[sz] = '\0';
    uefi_call_wrapper(file->Close, 1, file);
    if (EFI_ERROR(status)) {
        Print(L"[init] failed to read /etc/init.cfg\n");
        return EFI_ABORTED;
    }

    if (strstr((CHAR8 *)buf, "init_mode") == NULL ||
        strstr((CHAR8 *)buf, "start_services") == NULL) {
        Print(L"[init] missing required keys in /etc/init.cfg\n");
        return EFI_ABORTED;
    }

    Print(L"[init] configuration OK\n");
    return EFI_SUCCESS;
}
