# A made-up Apple Studio Display for vm-monitor-brightness-test.nix.
#
# `edid` prints an EDID like a 2022 Studio Display's (APP, product 0xAE3A,
# named StudioDisplay, with a DisplayID container ID; the serial and the
# container ID are made up) for drm.edid_firmware.
#
# `serve` makes a USB device on dummy_hcd through FunctionFS with the
# Studio Display's IDs (05ac:1114): interfaces 0-7, a HID at 5 with a
# vendor report and the brightness HID at 7 with the descriptor asdbctl
# records (feature report 1: 32-bit brightness, 400-60000, and a 16-bit
# vendor field). It answers GET_REPORT with the level it holds and keeps
# what SET_REPORT sends in /run/fake-studio-display/level.

import os
import struct
import subprocess
import sys

GADGET = "/sys/kernel/config/usb_gadget/studio"
FFS = "/dev/ffs-studio"
STATE = "/run/fake-studio-display"

BRIGHTNESS = bytes.fromhex(
    "05 80 09 01 a1 01 85 01 06 82 00 09 10 16 90 01 27 60 ea 00 00 67 e1 00 00 01"
    " 55 0e 75 20 95 01 b1 42 05 0f 09 50 15 00 26 20 4e 66 10 01 55 0d 75 10 b1 42"
    " 06 82 00 09 10 16 90 01 27 60 ea 00 00 67 e1 00 00 01 55 0e 75 20 95 01 81 02"
    " c0"
)
# A vendor collection with one feature byte, report 2.
OTHER = bytes.fromhex("06 00 ff 09 01 a1 01 85 02 15 00 26 ff 00 75 08 95 01 b1 02 c0")
REPORTS = {5: OTHER, 7: BRIGHTNESS}


def checksum(block):
    return (-sum(block)) & 0xFF


def edid():
    e = bytearray(128)
    e[0:8] = bytes([0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0])
    maker = (1 << 10) | (16 << 5) | 16  # "APP"
    e[8:10] = maker.to_bytes(2, "big")
    e[10:12] = (0xAE3A).to_bytes(2, "little")
    e[12:16] = (0x12345678).to_bytes(4, "little")
    e[16], e[17] = 7, 2022 - 1990
    e[18], e[19] = 1, 4
    e[20] = 0xB5  # digital, 10 bits a colour, DisplayPort
    e[21], e[22] = 60, 37
    e[23] = 120  # gamma 2.2
    e[24] = 0x02  # the first timing is the native one
    for i in range(38, 54):
        e[i] = 1
    # 1280x800 at 60 Hz (CVT reduced blanking), 71 MHz.
    ha, hb, va, vb = 1280, 160, 800, 23
    hso, hsw, vso, vsw = 48, 32, 3, 6
    e[54:56] = (7100).to_bytes(2, "little")
    e[56], e[57], e[58] = ha & 0xFF, hb & 0xFF, (ha >> 8) << 4 | hb >> 8
    e[59], e[60], e[61] = va & 0xFF, vb & 0xFF, (va >> 8) << 4 | vb >> 8
    e[62], e[63] = hso & 0xFF, hsw & 0xFF
    e[64] = (vso & 0xF) << 4 | vsw & 0xF
    e[65] = (hso >> 8) << 6 | (hsw >> 8) << 4 | (vso >> 4) << 2 | vsw >> 4
    e[66], e[67], e[68] = 0x54, 0x74, 0x21  # 596 x 372 mm
    e[71] = 0x1A
    name = b"StudioDisplay"
    e[72:90] = bytes([0, 0, 0, 0xFC, 0]) + name.ljust(13, b" ")
    e[90 + 3] = 0x10
    e[108 + 3] = 0x10
    e[126] = 1
    e[127] = checksum(e[:127])
    container = bytes.fromhex("0123456789abcdef0123456789abcdef")
    blocks = bytes([0x29, 0x00, 16]) + container
    section = bytearray([0x12, len(blocks), 0x00, 0x00]) + blocks
    section.append(checksum(section))
    ext = bytearray([0x70]) + section
    ext.extend(bytes(127 - len(ext)))
    ext.append(checksum(ext))
    return bytes(e) + bytes(ext)


def interfaces(interval):
    out = b""
    endpoint = 1
    for i in range(8):
        if i in REPORTS:
            desc = REPORTS[i]
            out += struct.pack("<9B", 9, 4, i, 0, 1, 3, 0, 0, 0)
            out += struct.pack("<BBHBBBH", 9, 0x21, 0x0111, 0, 1, 0x22, len(desc))
            out += struct.pack("<BBBBHB", 7, 5, 0x80 | endpoint, 3, 16, interval)
            endpoint += 1
        else:
            out += struct.pack("<9B", 9, 4, i, 0, 0, 0xFF, 0, 0, 0)
    return out


def descriptors():
    # FunctionFS's v2 format: full and high speed, 12 descriptors each.
    body = struct.pack("<II", 12, 12) + interfaces(1) + interfaces(4)
    return struct.pack("<III", 3, 12 + len(body), 1 | 2) + body


def write(path, value):
    with open(path, "w") as f:
        f.write(value)


def save(level):
    write(STATE + "/level.new", str(level))
    os.rename(STATE + "/level.new", STATE + "/level")


def setup():
    for module in ["dummy_hcd", "libcomposite", "usb_f_fs"]:
        subprocess.run(["modprobe", module], check=True)
    os.makedirs(GADGET + "/strings/0x409")
    write(GADGET + "/idVendor", "0x05ac")
    write(GADGET + "/idProduct", "0x1114")
    write(GADGET + "/bcdDevice", "0x0100")
    write(GADGET + "/bcdUSB", "0x0200")
    write(GADGET + "/strings/0x409/manufacturer", "Apple Inc.")
    write(GADGET + "/strings/0x409/product", "Studio Display")
    write(GADGET + "/strings/0x409/serialnumber", "FAKE0000")
    os.makedirs(GADGET + "/configs/c.1/strings/0x409")
    write(GADGET + "/configs/c.1/strings/0x409/configuration", "Studio Display")
    os.makedirs(GADGET + "/functions/ffs.studio")
    os.symlink(GADGET + "/functions/ffs.studio", GADGET + "/configs/c.1/ffs.studio")
    os.makedirs(FFS, exist_ok=True)
    subprocess.run(["mount", "-t", "functionfs", "studio", FFS], check=True)
    ep0 = os.open(FFS + "/ep0", os.O_RDWR)
    os.write(ep0, descriptors())
    os.write(ep0, struct.pack("<IIII", 2, 16, 0, 0))
    write(GADGET + "/UDC", sorted(os.listdir("/sys/class/udc"))[0])
    return ep0


def get_report(interface, value, level):
    kind, report = value >> 8, value & 0xFF
    if interface == 7 and report == 1:
        data = bytes([1]) + level.to_bytes(4, "little")
        return data + bytes(2) if kind == 3 else data
    if interface == 5 and report == 2:
        return bytes([2, 0])
    return None


def stall(ep0, incoming):
    # FunctionFS stalls a request read the wrong way.
    try:
        if incoming:
            os.read(ep0, 0)
        else:
            os.write(ep0, b"")
    except OSError:
        pass


def serve():
    os.makedirs(STATE, exist_ok=True)
    level = 30200
    save(level)
    ep0 = setup()
    print("the Studio Display is plugged in", flush=True)
    while True:
        event = os.read(ep0, 12)
        if len(event) < 12 or event[8] != 4:
            continue
        kind, request, value, index, length = struct.unpack("<BBHHH", event[:8])
        interface = index & 0xFF
        if kind == 0x81 and request == 6 and value >> 8 == 0x22 and interface in REPORTS:
            os.write(ep0, REPORTS[interface][:length])
        elif kind == 0xA1 and request == 1:
            report = get_report(interface, value, level)
            if report is None:
                stall(ep0, True)
            else:
                os.write(ep0, report[:length])
        elif kind == 0x21 and request == 9:
            data = os.read(ep0, length)
            if interface == 7 and data[:1] == b"\x01" and len(data) >= 5:
                level = int.from_bytes(data[1:5], "little")
                save(level)
                print(f"brightness set to {level}", flush=True)
        elif kind == 0x21 and request == 0x0A:
            os.read(ep0, 0)
        else:
            stall(ep0, bool(kind & 0x80))


if __name__ == "__main__":
    if sys.argv[1:] == ["edid"]:
        sys.stdout.buffer.write(edid())
    else:
        serve()
