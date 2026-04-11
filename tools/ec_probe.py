#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""
Razer EC HID Firmware Probe — Blade 16 2023 (Windows, no external deps)

Uses Windows built-in HID APIs via ctypes (hid.dll + setupapi.dll + kernel32).
No external Python packages required.

Usage (run as Administrator):
    uv run tools/ec_probe.py
    uv run tools/ec_probe.py --fan-temp         # focused fan + temp probe
    uv run tools/ec_probe.py --dump 0x0D 0x88   # dump single command

Output: tools/razer_ec_map.json + tools/razer_ec_map.txt

Protocol — RazerPacket (91 bytes, HID feature report):
    [0]        report_id      = 0x00
    [1]        status         0x02=OK  0x05=unsupported
    [2]        transaction_id = 0xFF (Blade laptops)
    [3..4]     remaining_packets (u16 BE)
    [5]        protocol_type  = 0x00
    [6]        data_size
    [7]        command_class
    [8]        command_id
    [9..88]    args[0..79]
    [89]       crc  (XOR bytes 2..88)
    [90]       reserved = 0x00
"""

import argparse, ctypes, ctypes.wintypes, json, struct, sys, time
from pathlib import Path

# ─── Windows HID via ctypes ───────────────────────────────────────────────────

kernel32 = ctypes.windll.kernel32
hid_dll  = ctypes.windll.hid
setupapi = ctypes.windll.setupapi

GENERIC_READ         = 0x80000000
GENERIC_WRITE        = 0x40000000
FILE_SHARE_READ      = 0x00000001
FILE_SHARE_WRITE     = 0x00000002
OPEN_EXISTING        = 3
DIGCF_PRESENT        = 0x00000002
DIGCF_DEVICEINTERFACE= 0x00000010
INVALID_HANDLE       = -1


class GUID(ctypes.Structure):
    _fields_ = [("Data1", ctypes.c_ulong), ("Data2", ctypes.c_ushort),
                ("Data3", ctypes.c_ushort), ("Data4", ctypes.c_ubyte * 8)]


class SP_DEVICE_INTERFACE_DATA(ctypes.Structure):
    _fields_ = [("cbSize", ctypes.c_ulong), ("InterfaceClassGuid", GUID),
                ('Flags', ctypes.c_ulong), ('Reserved', ctypes.c_size_t)]  # ULONG_PTR



def _setup_argtypes() -> None:
    """Set correct restype/argtypes for 64-bit Windows pointer safety."""
    setupapi.SetupDiGetClassDevsW.restype          = ctypes.c_void_p
    setupapi.SetupDiDestroyDeviceInfoList.argtypes  = [ctypes.c_void_p]
    setupapi.SetupDiEnumDeviceInterfaces.argtypes   = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(GUID),
        ctypes.c_ulong, ctypes.POINTER(SP_DEVICE_INTERFACE_DATA)]
    # 3rd arg is PSP_DEVICE_INTERFACE_DETAIL_DATA — use c_void_p for raw buffer
    setupapi.SetupDiGetDeviceInterfaceDetailW.argtypes = [
        ctypes.c_void_p, ctypes.POINTER(SP_DEVICE_INTERFACE_DATA),
        ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.c_void_p]


_setup_argtypes()


def _hid_guid() -> GUID:
    g = GUID()
    hid_dll.HidD_GetHidGuid(ctypes.byref(g))
    return g


def enumerate_hid_paths(vendor_id: int) -> list[str]:
    guid  = _hid_guid()
    hset  = setupapi.SetupDiGetClassDevsW(
        ctypes.byref(guid), None, None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE)
    if not hset or hset == ctypes.c_void_p(-1).value:
        return []
    paths, idx = [], 0
    iface = SP_DEVICE_INTERFACE_DATA()
    iface.cbSize = ctypes.sizeof(SP_DEVICE_INTERFACE_DATA)
    req   = ctypes.c_ulong(0)
    while setupapi.SetupDiEnumDeviceInterfaces(
            hset, None, ctypes.byref(guid), idx, ctypes.byref(iface)):
        # Two-step: get required size, then fill buffer
        setupapi.SetupDiGetDeviceInterfaceDetailW(
            hset, ctypes.byref(iface), None, 0, ctypes.byref(req), None)
        sz = req.value
        if sz >= 6:
            buf = (ctypes.c_byte * sz)()
            # cbSize = 8 on 64-bit Windows (DWORD field offset before WCHAR[])
            struct.pack_into('<I', buf, 0, 8)
            if setupapi.SetupDiGetDeviceInterfaceDetailW(
                    hset, ctypes.byref(iface), buf, sz, None, None):
                path = ctypes.wstring_at(ctypes.addressof(buf) + 4)
                if f"vid_{vendor_id:04x}" in path.lower():
                    paths.append(path)
        idx += 1
    setupapi.SetupDiDestroyDeviceInfoList(hset)
    return paths


def open_hid(path: str) -> int:
    h = kernel32.CreateFileW(path, GENERIC_READ | GENERIC_WRITE,
                             FILE_SHARE_READ | FILE_SHARE_WRITE,
                             None, OPEN_EXISTING, 0, None)
    if h == INVALID_HANDLE:
        raise OSError(f"CreateFileW error {kernel32.GetLastError()}: {path}")
    return h


def close_hid(h: int) -> None:
    kernel32.CloseHandle(h)


def set_feature(h: int, data: bytes) -> bool:
    buf = (ctypes.c_ubyte * len(data))(*data)
    return bool(hid_dll.HidD_SetFeature(h, buf, len(data)))


def get_feature(h: int, size: int) -> bytes | None:
    buf = (ctypes.c_ubyte * size)()
    if hid_dll.HidD_GetFeature(h, buf, size):
        return bytes(buf)
    return None


# ─── Razer packet helpers ─────────────────────────────────────────────────────

REPORT_SIZE = 91
TXID        = 0xFF
STATUS_OK   = 0x02
STATUS_UNSUPPORTED = 0x05
SNAME = {0x00:"NEW",0x01:"BUSY",0x02:"OK",0x03:"FAIL",0x04:"TIMEOUT",0x05:"UNSUPPORTED"}

KNOWN: dict[tuple[int,int], str] = {
    (0x02, 0x06): "set_fn_swap",          (0x02, 0x86): "get_fn_swap",
    (0x03, 0x00): "set_logo_led_state",   (0x03, 0x0A): "set_keyboard_effect",
    (0x07, 0x12): "set_bho",              (0x07, 0x92): "get_bho",
    (0x0D, 0x01): "set_fan_rpm",          (0x0D, 0x02): "set_power_mode",
    (0x0D, 0x07): "set_boost",            (0x0D, 0x81): "get_fan_setpoint (RPM/100)",
    (0x0D, 0x82): "get_power_mode",       (0x0D, 0x86): "? thermal/unknown",
    (0x0D, 0x87): "get_boost",            (0x0D, 0x88): "get_fan_tachometer (RPM/100)",
    (0x0E, 0x04): "set_brightness",       (0x0E, 0x84): "get_brightness",
}


def _crc(p: bytearray) -> int:
    r = 0
    for i in range(2, 88):
        r ^= p[i]
    return r


def build(cls: int, cmd: int, ds: int, args: list[int]) -> bytes:
    p = bytearray(REPORT_SIZE)
    p[2] = TXID; p[6] = ds; p[7] = cls; p[8] = cmd
    for i, v in enumerate(args[:80]):
        p[9+i] = v & 0xFF
    p[89] = _crc(p)
    return bytes(p)


def send(h: int, cls: int, cmd: int, ds: int = 2,
         args: list[int] | None = None, retries: int = 3) -> bytes | None:
    if args is None:
        args = [0x00, 0x01]
    pkt = build(cls, cmd, ds, args)
    for attempt in range(retries):
        try:
            if not set_feature(h, pkt):
                time.sleep(0.002 * (1 << attempt)); continue
            time.sleep(0.003)
            resp = get_feature(h, REPORT_SIZE)
            if resp and len(resp) == REPORT_SIZE:
                return resp
        except Exception:
            pass
        time.sleep(0.001 * (1 << attempt))
    return None


def parse(resp: bytes) -> dict:
    return {"status": resp[1], "sname": SNAME.get(resp[1], hex(resp[1])),
            "cls": resp[7], "cmd": resp[8], "args": list(resp[9:89])}


# ─── Probe modes ─────────────────────────────────────────────────────────────

def probe_range(h: int, classes: list[int], ids: list[int],
                base_args: list[int], ds: int, verbose: bool) -> list[dict]:
    results = []
    total, done = len(classes) * len(ids), 0
    for cls in classes:
        for cmd in ids:
            done += 1
            resp = send(h, cls, cmd, ds=ds, args=base_args)
            if resp is None:
                continue
            r = parse(resp)
            if r["status"] == STATUS_OK:
                nz  = {i: r["args"][i] for i in range(80) if r["args"][i]}
                key = KNOWN.get((cls, cmd), "")
                results.append({"class": f"0x{cls:02X}", "id": f"0x{cmd:02X}",
                                 "known": key, "args": r["args"], "nonzero": nz})
                print(f"  [{done}/{total}] 0x{cls:02X}/0x{cmd:02X} OK  nz={nz}"
                      + (f"  [{key}]" if key else ""))
            elif verbose and r["status"] not in (STATUS_UNSUPPORTED,):
                print(f"  [{done}/{total}] 0x{cls:02X}/0x{cmd:02X} {r['sname']}")
            if done % 20 == 0 and not verbose:
                print(f"  ... {done}/{total} ({100*done//total}%)", end="\r", flush=True)
    print()
    return results


def fan_temp_probe(h: int) -> None:
    print("\n=== FOCUSED: class 0x0D, all zone variants ===")
    for zone in [0x00, 0x01, 0x02]:
        print(f"\n  Zone {zone}:")
        for cmd in range(0x100):
            resp = send(h, 0x0D, cmd, ds=2, args=[0x00, zone])
            if resp is None: continue
            r = parse(resp)
            if r["status"] == STATUS_OK:
                nz  = {i: v for i, v in enumerate(r["args"]) if v}
                key = KNOWN.get((0x0D, cmd), "")
                print(f"    0x0D/0x{cmd:02X} zone={zone} nz={nz}"
                      + (f"  [{key}]" if key else ""))

    print("\n=== Scan all classes for temperature-scale (20-100) single-byte responses ===")
    for cls in range(0x20):
        for cmd in range(0x80, 0xA0):
            resp = send(h, cls, cmd, ds=2, args=[0x00, 0x01])
            if resp is None: continue
            r = parse(resp)
            if r["status"] == STATUS_OK:
                nz       = {i: v for i, v in enumerate(r["args"]) if v}
                temp_like= {i: v for i, v in nz.items() if 20 <= v <= 100}
                if temp_like:
                    key = KNOWN.get((cls, cmd), "")
                    print(f"  0x{cls:02X}/0x{cmd:02X} TEMP-LIKE={temp_like} all={nz}"
                          + (f"  [{key}]" if key else ""))


def dump_cmd(h: int, cls: int, cmd: int) -> None:
    print(f"\n=== Dump 0x{cls:02X}/0x{cmd:02X} ===")
    for zone in range(3):
        for ds in [0, 2, 4]:
            args = [0x00, zone] + [0] * max(0, ds-2)
            resp = send(h, cls, cmd, ds=ds, args=args)
            if resp is None:
                print(f"  zone={zone} ds={ds} -> no resp"); continue
            r = parse(resp)
            nz = {i: v for i, v in enumerate(r["args"]) if v}
            print(f"  zone={zone} ds={ds} {r['sname']}  nz={nz or '(all zero)'}")


# ─── main ─────────────────────────────────────────────────────────────────────

def range_list(s: str) -> list[int]:
    s = s.strip()
    if '-' in s and not s.startswith('-'):
        lo, hi = s.split('-', 1)
        return list(range(int(lo, 0), int(hi, 0) + 1))
    return [int(x.strip(), 0) for x in s.split(',')]


def main() -> None:
    ap = argparse.ArgumentParser(description="Razer EC HID probe (Windows, no deps)")
    ap.add_argument("--classes",  default="0x00-0x1F")
    ap.add_argument("--ids",      default="0x80-0xFF")
    ap.add_argument("--fan-temp", action="store_true")
    ap.add_argument("--dump",     nargs=2, metavar=("CLASS", "ID"))
    ap.add_argument("--args",     default="0x00,0x01")
    ap.add_argument("--data-size",type=lambda x: int(x, 0), default=2)
    ap.add_argument("-v",         action="store_true", dest="verbose")
    args = ap.parse_args()

    RAZER_VID = 0x1532
    print("Razer EC HID Probe\n==================")
    paths = enumerate_hid_paths(RAZER_VID)
    if not paths:
        print("ERROR: No Razer HID device found. Run as Administrator.")
        sys.exit(1)
    paths.sort(reverse=True)   # highest interface first (usually control interface)
    for p in paths[:5]:
        print(f"  {p}")
    if len(paths) > 5:
        print(f"  ... and {len(paths)-5} more")

    handle = None
    for path in paths:
        try:
            handle = open_hid(path)
            print(f"\nOpened: {path}\n")
            break
        except OSError as e:
            print(f"  Skip: {e}")

    if handle is None:
        print("ERROR: Could not open any interface. Run as Administrator.")
        sys.exit(1)

    try:
        if args.dump:
            dump_cmd(handle, int(args.dump[0], 0), int(args.dump[1], 0))
            return
        if args.fan_temp:
            fan_temp_probe(handle)
            return

        classes   = range_list(args.classes)
        ids       = range_list(args.ids)
        base_args = [int(x.strip(), 0) for x in args.args.split(',')]
        print(f"Probing {len(classes)} classes × {len(ids)} IDs = "
              f"{len(classes)*len(ids)} combinations\n")

        results = probe_range(handle, classes, ids, base_args,
                              args.data_size, args.verbose)

        out = Path(__file__).parent
        (out / "razer_ec_map.json").write_text(json.dumps(results, indent=2))
        lines = [f"Razer EC probe — {len(results)} responding commands\n",
                 "="*60+"\n\n"]
        for r in results:
            k = f"  [{r['known']}]" if r["known"] else ""
            lines.append(f"{r['class']}/{r['id']}{k}\n  nz={r['nonzero']}\n\n")
        (out / "razer_ec_map.txt").write_text("".join(lines))
        print(f"\nSaved razer_ec_map.json + razer_ec_map.txt  ({len(results)} hits)")
    finally:
        close_hid(handle)


if __name__ == "__main__":
    main()
