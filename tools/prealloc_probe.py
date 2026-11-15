#!/usr/bin/env python3
"""Does fdatasync get cheaper if the journal file is preallocated?

The journal appends: every commit grows the file, so every fdatasync also has
to make the new size durable (a file-system journal commit). This probe
compares that with overwriting blocks that were written with zeros and
flushed beforehand, where fdatasync has only data to flush.

It is an experiment for a design decision (docs/adr/ADR-012), not part of the
product. It writes to the given directory and removes its files.

usage: prealloc_probe.py <directory on the device to test> [--out result.json]
"""
import json
import os
import subprocess
import sys
import time


def pct(values, q):
    v = sorted(values)
    return round(v[min(len(v) - 1, int(len(v) * q))])


def run(directory, name, prealloc, n=1500, size=1400, gap=0.005):
    path = os.path.join(directory, f"prealloc-probe-{os.getpid()}.bin")
    fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_TRUNC, 0o600)
    try:
        if prealloc:
            os.write(fd, b"\0" * (n * size))
            os.fsync(fd)
            dfd = os.open(directory, os.O_RDONLY)
            os.fsync(dfd)
            os.close(dfd)
        block = b"\x5a" * size
        times = []
        for i in range(n):
            t0 = time.perf_counter()
            os.pwrite(fd, block, i * size)
            os.fdatasync(fd)
            times.append((time.perf_counter() - t0) * 1e6)
            time.sleep(gap)  # about 200 commits per second
    finally:
        os.close(fd)
        os.unlink(path)
    return {
        "name": name,
        "unit": "us",
        "samples": n,
        "bytes_per_write": size,
        "p50": pct(times, 0.50),
        "p90": pct(times, 0.90),
        "p99": pct(times, 0.99),
        "max": round(max(times)),
    }


def sh(cmd):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True).stdout.strip()


def main():
    args = sys.argv[1:]
    if not args or args[0].startswith("-"):
        print(__doc__)
        return 2
    directory = args[0]
    out = args[args.index("--out") + 1] if "--out" in args else None
    os.makedirs(directory, exist_ok=True)
    results = []
    for round_no in (1, 2, 3):
        for name, prealloc in (("append, file grows", False), ("overwrite preallocated zeros", True)):
            r = run(directory, name, prealloc)
            r["round"] = round_no
            results.append(r)
            print(json.dumps(r))
    report = {
        "label": "prealloc-probe",
        "taken_at_us": int(time.time() * 1e6),
        "what": "write 1400 bytes then fdatasync, 1500 times at about 200 per second: appending to a growing file "
        "versus overwriting blocks that were zero-filled and flushed beforehand",
        "tool": "tools/prealloc_probe.py (Python; not part of the product, no source digest)",
        "environment": {
            "df": sh(f"df -h {directory} | tail -1"),
            "mount": sh(f"findmnt -no SOURCE,FSTYPE,OPTIONS -T {directory}"),
            "loadavg": sh("cut -d' ' -f1-3 /proc/loadavg"),
            "busiest_processes": sh("ps -eo pcpu,comm --sort=-pcpu | sed -n '2,4p' | tr -s ' ' | tr '\\n' ';'"),
            "kernel": sh("uname -r"),
        },
        "results": results,
    }
    if out:
        with open(out, "w") as f:
            json.dump(report, f, indent=2)
        print(f"# written {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
