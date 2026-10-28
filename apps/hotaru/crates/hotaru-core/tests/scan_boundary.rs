//! A SIMD overread at a slice tail must fault this test, not quietly read a
//! vector's spare capacity. The adjacent virtual page is deliberately PROT_NONE.
#![cfg(target_os = "linux")]
use hotaru_core::Terminal;

#[test]
fn printable_scanner_never_reads_the_guard_page() {
    // SAFETY: sysconf has no pointer arguments and the positive result is checked.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page > 0);
    let page = page as usize;
    // SAFETY: create a private anonymous mapping; MAP_FAILED is checked before use.
    let raw = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            page * 2,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(raw, libc::MAP_FAILED);
    struct Mapping(*mut libc::c_void, usize);
    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: Mapping owns exactly this mmap region and drops once.
            assert_eq!(unsafe { libc::munmap(self.0, self.1) }, 0);
        }
    }
    let _mapping = Mapping(raw, page * 2);
    // SAFETY: pointer stays inside mapping; the second page is aligned and live.
    assert_eq!(
        unsafe { libc::mprotect(raw.cast::<u8>().add(page).cast(), page, libc::PROT_NONE) },
        0
    );
    // SAFETY: first page is exclusively owned, writable and initialized here.
    let memory = unsafe { std::slice::from_raw_parts_mut(raw.cast::<u8>(), page) };
    memory.fill(b'x');
    for len in 0..=128 {
        let bytes = &memory[page - len..];
        let mut terminal = Terminal::new(160, 2, 0);
        terminal.feed(bytes);
        assert_eq!(terminal.snapshot().cursor.col, len);
    }
    // Exercise the UTF-8-candidate scanner as well as the ASCII scanner.
    for len in 2..=128 {
        memory[page - len] = 0xc3;
        memory[page - len + 1] = 0xa9;
        let mut terminal = Terminal::new(160, 2, 0);
        terminal.feed(&memory[page - len..]);
        assert_eq!(terminal.snapshot().cursor.col, len - 1);
        memory[page - len] = b'x';
        memory[page - len + 1] = b'x';
    }
}
