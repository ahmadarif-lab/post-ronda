#!/usr/bin/env python3
"""Compare installed macOS clients with disposable profiles, using only the stdlib.

Measures fresh-profile launch to first visible window and idle process-tree RSS/CPU.
First window is not an assertion that the editor is ready. See docs/benchmark.md.
"""

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone


DEFAULT_APPS = ["/Applications/Post Ronda.app", "/Applications/Bruno.app",
                "/Applications/Postman.app"]


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def cpu_seconds(value):
    days, _, clock = value.rpartition("-")
    total = 0.0
    for part in clock.split(":"):
        total = total * 60 + float(part)
    return total + (int(days) * 86400 if days else 0)


def processes():
    rows = {}
    for line in command("ps", "-axo", "pid=,ppid=,rss=,time=,comm=").splitlines():
        pid, parent, rss, cpu, executable = line.strip().split(None, 4)
        rows[int(pid)] = {"pid": int(pid), "parent": int(parent),
                          "rss_kib": int(rss), "cpu_seconds": cpu_seconds(cpu),
                          "executable": executable}
    return rows


def app_processes(rows, bundle, root=None):
    # Bundle matching includes Electron helpers reparented outside the root tree.
    found = {pid for pid, row in rows.items()
             if row["executable"].startswith(str(bundle) + "/")}
    if root in rows:
        found.add(root)
    while True:
        children = {pid for pid, row in rows.items() if row["parent"] in found}
        if children <= found:
            return [rows[pid] for pid in sorted(found)]
        found |= children


class Windows:
    """Public CoreGraphics window metadata; no screenshot or Accessibility access."""

    def __init__(self):
        self.cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
        self.cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
        self.cg.CGWindowListCopyWindowInfo.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
        self.cg.CGWindowListCopyWindowInfo.restype = ctypes.c_void_p
        self.cf.CFPropertyListCreateData.argtypes = [ctypes.c_void_p, ctypes.c_void_p,
                                                    ctypes.c_long, ctypes.c_ulong,
                                                    ctypes.c_void_p]
        self.cf.CFPropertyListCreateData.restype = ctypes.c_void_p
        self.cf.CFDataGetLength.argtypes = [ctypes.c_void_p]
        self.cf.CFDataGetLength.restype = ctypes.c_long
        self.cf.CFDataGetBytePtr.argtypes = [ctypes.c_void_p]
        self.cf.CFDataGetBytePtr.restype = ctypes.c_void_p
        self.cf.CFRelease.argtypes = [ctypes.c_void_p]

    def visible(self, pid):
        windows = self.cg.CGWindowListCopyWindowInfo(1 | 16, 0)
        if not windows:
            return None
        data = None
        try:
            data = self.cf.CFPropertyListCreateData(None, windows, 100, 0, None)
            if not data:
                return None
            raw = ctypes.string_at(self.cf.CFDataGetBytePtr(data),
                                   self.cf.CFDataGetLength(data))
            for window in plistlib.loads(raw):
                bounds = window.get("kCGWindowBounds", {})
                if (window.get("kCGWindowOwnerPID") == pid
                        and window.get("kCGWindowLayer") == 0
                        and bounds.get("Width", 0) >= 300
                        and bounds.get("Height", 0) >= 200):
                    return {"width": bounds["Width"], "height": bounds["Height"]}
        finally:
            if data:
                self.cf.CFRelease(data)
            self.cf.CFRelease(windows)
        return None


def metadata(bundle):
    with (bundle / "Contents/Info.plist").open("rb") as file:
        info = plistlib.load(file)
    executable = bundle / "Contents/MacOS" / info["CFBundleExecutable"]
    electron = (bundle / "Contents/Frameworks/Electron Framework.framework").exists()
    digest = hashlib.sha256()
    with executable.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return {"name": bundle.stem, "bundle": str(bundle),
            "version": info.get("CFBundleShortVersionString"),
            "executable": str(executable), "electron": electron,
            "executable_sha256": digest.hexdigest(),
            "architecture": command("file", "-b", str(executable)),
            "bundle_allocated_kib": int(command("du", "-sk", str(bundle)).split()[0])}


def measure(app, args, windows, run):
    bundle = Path(app["bundle"])
    if app_processes(processes(), bundle):
        raise RuntimeError(f'{app["name"]} is already running; close it before measuring')
    with tempfile.TemporaryDirectory(prefix="post-ronda-benchmark-") as temporary:
        profile = Path(temporary)
        env = os.environ.copy()
        launch = [app["executable"]]
        if app["electron"]:
            launch.append(f"--user-data-dir={profile / 'electron'}")
        elif app["name"] == "Post Ronda":
            env["POST_RONDA_STATE_DIR"] = str(profile / "state")
            launch.append(str(profile / "project"))
        else:
            raise RuntimeError("Only Post Ronda and Electron clients support profile isolation here")
        # No dev automation/capture settings: those can force repainting or mock requests.
        for key in list(env):
            if key.startswith("POST_RONDA_") and key != "POST_RONDA_STATE_DIR":
                del env[key]
        with (profile / "app.log").open("wb") as log:
            started = time.monotonic()
            proc = subprocess.Popen(launch, env=env, stdout=log, stderr=log,
                                    start_new_session=True)
            try:
                bounds = None
                while time.monotonic() - started < args.window_timeout:
                    if proc.poll() is not None:
                        raise RuntimeError(f'{app["name"]} exited before showing a window')
                    bounds = windows.visible(proc.pid)
                    if bounds:
                        break
                    time.sleep(0.05)
                if bounds is None:
                    raise RuntimeError(f'{app["name"]}: no visible window; keep the desktop unlocked')
                first_window = time.monotonic() - started
                time.sleep(args.warmup)
                previous = {p["pid"]: p["cpu_seconds"]
                            for p in app_processes(processes(), bundle, proc.pid)}
                previous_at = time.monotonic()
                samples = []
                for _ in range(args.samples):
                    time.sleep(args.interval)
                    rows = app_processes(processes(), bundle, proc.pid)
                    now = time.monotonic()
                    if proc.poll() is not None or not rows:
                        raise RuntimeError(f'{app["name"]} exited during sampling')
                    cpu = sum(max(0, p["cpu_seconds"] - previous.get(p["pid"], 0))
                              for p in rows)
                    samples.append({"elapsed_seconds": round(now - started, 4),
                                    "rss_mib": sum(p["rss_kib"] for p in rows) / 1024,
                                    "cpu_percent_one_core": cpu / (now - previous_at) * 100,
                                    "processes": [{k: p[k] for k in
                                                   ("pid", "rss_kib", "cpu_seconds")}
                                                  for p in rows]})
                    previous = {p["pid"]: p["cpu_seconds"] for p in rows}
                    previous_at = now
                result = {"app": app["name"], "run": run,
                          "first_window_seconds": round(first_window, 4),
                          "window_points": bounds, "samples": samples}
                print(f'{app["name"]} run {run}: window {first_window:.3f}s, '
                      f'RSS median {statistics.median(s["rss_mib"] for s in samples):.1f} MiB',
                      flush=True)
                return result
            finally:
                # Only the new process group belongs to this benchmark.
                if proc.poll() is None:
                    os.killpg(proc.pid, signal.SIGTERM)
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(proc.pid, signal.SIGKILL)
                        proc.wait(timeout=5)


def summary(report):
    lines = ["| Client | First window (s) | Idle RSS (MiB) | Idle CPU (% of one core) | Bundle (MiB) |",
             "| --- | ---: | ---: | ---: | ---: |"]
    for app in report["apps"]:
        runs = [r for r in report["runs"] if r["app"] == app["name"]]
        window = statistics.median(r["first_window_seconds"] for r in runs)
        rss = statistics.median(statistics.median(s["rss_mib"] for s in r["samples"])
                                for r in runs)
        cpu = statistics.median(statistics.mean(s["cpu_percent_one_core"] for s in r["samples"])
                                for r in runs)
        lines.append(f'| {app["name"]} {app["version"]} | {window:.3f} | {rss:.1f} | '
                     f'{cpu:.2f} | {app["bundle_allocated_kib"] / 1024:.2f} |')
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", action="append", help=".app path; repeat for each client")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--warmup", type=float, default=15, help="seconds after first window")
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--interval", type=float, default=1)
    parser.add_argument("--window-timeout", type=float, default=30)
    parser.add_argument("--output", type=Path, default=Path("target/benchmark-desktop.json"))
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("This sampler currently supports macOS only")
    if min(args.runs, args.samples, args.interval, args.window_timeout) <= 0 or args.warmup < 0:
        parser.error("runs, samples, interval and timeout must be positive; warmup must be >= 0")
    apps = [metadata(Path(p).resolve()) for p in (args.app or DEFAULT_APPS)]
    for app in apps:
        if app_processes(processes(), Path(app["bundle"])):
            parser.error(f'{app["name"]} is running; close it first to protect your session')
    windows = Windows()
    report = {"schema_version": 1, "recorded_at": datetime.now(timezone.utc).isoformat(),
              "scenario": "fresh-profile-first-window-and-idle",
              "system": {"os": command("sw_vers", "-productVersion"),
                         "os_build": command("sw_vers", "-buildVersion"),
                         "architecture": platform.machine(),
                         "model": command("sysctl", "-n", "hw.model"),
                         "memory_bytes": int(command("sysctl", "-n", "hw.memsize"))},
              "config": {"runs": args.runs, "warmup_seconds": args.warmup,
                         "samples": args.samples, "interval_seconds": args.interval,
                         "window_poll_seconds": 0.05}, "apps": apps, "runs": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for run in range(args.runs):
        # Rotate order to reduce a consistent first/last-client bias.
        order = apps[run % len(apps):] + apps[:run % len(apps)]
        for app in order:
            report["runs"].append(measure(app, args, windows, run + 1))
            args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(summary(report))
    print(f"Raw results: {args.output}")


if __name__ == "__main__":
    main()
