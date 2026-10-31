"""Inspect a generated fixture's GPT and embedded x86 initramfs without mounting."""

import struct
import sys
import uuid
import zlib
from pathlib import Path


def check(image, initrd):
    data = Path(image).read_bytes()
    sector = 512
    for lba in (1, len(data) // sector - 1):
        header = bytearray(data[lba * sector : (lba + 1) * sector])
        assert header[:8] == b"EFI PART"
        size, crc = struct.unpack_from("<II", header, 12)
        struct.pack_into("<I", header, 16, 0)
        assert zlib.crc32(header[:size]) == crc
        entries_lba, count, entry_size, entries_crc = struct.unpack_from("<QIII", header, 72)
        assert (count, entry_size) == (128, 128), "vboot requires 128 GPT entries"
        entries = data[entries_lba * sector : entries_lba * sector + count * entry_size]
        assert zlib.crc32(entries) == entries_crc

    # Check the primary entries, including that every allocated region is disjoint.
    entries = data[2 * sector : 34 * sector]
    regions = []
    for offset in range(0, len(entries), 128):
        entry = entries[offset : offset + 128]
        if entry[:16] == bytes(16):
            continue
        first, last = struct.unpack_from("<QQ", entry, 32)
        assert first >= 34 and first <= last < len(data) // sector - 33
        assert all(last < a or first > b for a, b in regions)
        regions.append((first, last))

    first, last = struct.unpack_from("<QQ", entries, 32)
    # ISO filesystem must be recognizable from its partition, not just the disk.
    pvd = (first + 64) * sector
    assert data[pvd : pvd + 7] == b"\x01CD001\x01"
    assert data[pvd + 40 : pvd + 72].rstrip() == b"SEKKA_GOOGLEBOOK"

    entry = entries[128:256]
    assert uuid.UUID(bytes_le=entry[:16]) == uuid.UUID("fe3a2a5d-4f32-41a7-b725-accc3285a309")
    first, last, attrs = struct.unpack_from("<QQQ", entry, 32)
    assert (attrs >> 48) & 0x1FF == 0x10F, "priority 15, successful, no attempt limit"
    blob = data[first * sector : (last + 1) * sector]
    assert blob[:8] == b"CHROMEOS"
    keyblock_size, = struct.unpack_from("<Q", blob, 16)
    body_load, bootloader_addr, bootloader_size = struct.unpack_from("<QQQ", blob, keyblock_size + 48)
    bootloader_offset = bootloader_addr - body_load + 0x10000
    params_offset = bootloader_offset - 0x1000
    assert blob[params_offset + 0x202 : params_offset + 0x206] == b"HdrS"
    ramdisk_addr, ramdisk_size = struct.unpack_from("<II", blob, params_offset + 0x218)
    expected = Path(initrd).read_bytes()
    assert ramdisk_addr == bootloader_addr
    assert ramdisk_size == len(expected) <= bootloader_size
    assert blob[bootloader_offset : bootloader_offset + ramdisk_size] == expected


if __name__ == "__main__":
    check(*sys.argv[1:])
