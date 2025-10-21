
.section ._archive_cpio,"aw"
.globl _archive_start, _archive_start_end
_archive_start:
.incbin "archive.archive.o.cpio"
_archive_start_end:

