// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::ipcbuf_view::IpcBufView;

static mut IPCBUF_BACKING: [u8; IpcBufView::PAGE_LEN] = [0u8; IpcBufView::PAGE_LEN];

#[test]
fn ipcbuf_prefix_is_clamped_to_page() {
    let view = unsafe { IpcBufView::new(IPCBUF_BACKING.as_ptr()) };
    let oversized = view.prefix(1 << 20);
    assert_eq!(oversized.len(), IpcBufView::PAGE_LEN);

    let exact = view.prefix(IpcBufView::PAGE_LEN);
    assert_eq!(exact.len(), IpcBufView::PAGE_LEN);
}
