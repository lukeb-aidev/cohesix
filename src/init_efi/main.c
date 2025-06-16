// CLASSIFICATION: COMMUNITY
// Filename: main.c v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-27
// SPDX-License-Identifier: MIT
#include <efi.h>
#include <efilib.h>

EFI_STATUS
efi_main(EFI_HANDLE image, EFI_SYSTEM_TABLE *systab) {
    InitializeLib(image, systab);
    Print(L"Init EFI running\n");
    return EFI_SUCCESS;
}
