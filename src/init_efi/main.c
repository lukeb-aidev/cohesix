// CLASSIFICATION: COMMUNITY
// Filename: main.c v0.5
// Author: Lukas Bower
// Date Modified: 2026-09-08
// SPDX-License-Identifier: MIT
#include <efi.h>
#include <efilib.h>
#include <string.h>
#include <stdio.h>

void *__stack_chk_guard = 0;
void __stack_chk_fail(void) { while (1); }

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

    CHAR16 config_path[] = L"\\etc\\cohesix\\config.yaml";
    status = uefi_call_wrapper(root->Open, 5, root, &file,
                               config_path, EFI_FILE_MODE_READ, 0);

    if (EFI_ERROR(status)) {
        CHAR8 role[64] = "default";
        EFI_STATUS rs = uefi_call_wrapper(root->Open, 5, root, &file,
                                          L"\\srv\\cohrole", EFI_FILE_MODE_READ, 0);
        if (!EFI_ERROR(rs)) {
            UINTN rsz = sizeof(role) - 1;
            if (EFI_ERROR(uefi_call_wrapper(file->Read, 3, file, &rsz, role))) {
                Print(L"[init] failed reading cohrole\n");
            }
            role[rsz] = '\0';
            uefi_call_wrapper(file->Close, 1, file);
            /*
             * gnu-efi defines CHAR8 as unsigned char, while standard C
             * string helpers expect char*. Cast explicitly to avoid
             * signedness warnings on some toolchains.
             */
            CHAR8 *nl = (CHAR8 *)strchr((char *)role, '\n');
            if (nl) *nl = '\0';
        } else {
            Print(L"[init] /srv/cohrole missing; using default role\n");
        }

        CHAR8 path_ascii[128];
        /* Cast role so snprintf sees a char* argument. */
        snprintf((char *)path_ascii, sizeof(path_ascii), "\\\roles\\%s\\config.yaml", (char *)role);
        CHAR16 path[128];
        for (int i = 0; path_ascii[i]; i++)
            path[i] = (CHAR16)path_ascii[i];
        /* strlen expects char*; cast accordingly. */
        path[strlen((char *)path_ascii)] = L'\0';

        status = uefi_call_wrapper(root->Open, 5, root, &file,
                                   path, EFI_FILE_MODE_READ, 0);
        if (EFI_ERROR(status)) {
            Print(L"[init] no configuration found\n");
            return EFI_SUCCESS;
        }
    }

    CHAR8 buf[128];
    UINTN sz = sizeof(buf) - 1;
    status = uefi_call_wrapper(file->Read, 3, file, &sz, buf);
    buf[sz] = '\0';
    uefi_call_wrapper(file->Close, 1, file);
    if (EFI_ERROR(status)) {
        Print(L"[init] failed to read role config\n");
    } else {
        Print(L"[init] loaded role config: %a\n", buf);
    }

    return EFI_SUCCESS;
}
