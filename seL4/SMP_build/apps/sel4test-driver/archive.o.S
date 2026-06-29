
.section ._archive_cpio,"aw"
.globl _cpio_archive, _cpio_archive_end
_cpio_archive:
.incbin "archive.archive.o.cpio"
_cpio_archive_end:

