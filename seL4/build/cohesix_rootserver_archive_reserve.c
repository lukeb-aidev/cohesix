/* Author: Lukas Bower */
/* Purpose: Reserve validated elfloader archive capacity for the Cohesix root task. */
/* Copyright 2026 Lukas Bower */
__attribute__((used, section(".rodata.cohesix_archive_reserve")))
const unsigned char cohesix_rootserver_archive_reserve[2097152] = {0x43};
