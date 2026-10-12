#!/usr/bin/env python3
"""Print a short report about this machine."""
import os
import platform
import shutil
import subprocess


def run(cmd):
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=10)
        return out.stdout.strip() if out.returncode == 0 else None
    except Exception:
        return None


def gib(n):
    return f"{n / 2**30:.1f} GiB"


def memory():
    system = platform.system()
    if system == "Linux":
        try:
            info = dict(line.split(":", 1) for line in open("/proc/meminfo"))
            total = int(info["MemTotal"].split()[0]) * 1024
            free = int(info["MemAvailable"].split()[0]) * 1024
            return f"{gib(total)} total, {gib(free)} available"
        except Exception:
            return None
    if system == "Darwin":
        total = run(["sysctl", "-n", "hw.memsize"])
        return f"{gib(int(total))} total" if total else None
    if system == "Windows":
        total = run(["powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"])
        return f"{gib(int(total))} total" if total else None
    return None


def main():
    unknown = "could not read"
    print(f"OS: {platform.system()} {platform.release()} ({platform.machine()})")
    print(f"CPU: {os.cpu_count()} cores")
    print(f"Memory: {memory() or unknown}")
    try:
        disk = shutil.disk_usage(os.getcwd())
        print(f"Disk (workspace volume): {gib(disk.free)} free of {gib(disk.total)}")
    except Exception:
        print(f"Disk: {unknown}")
    if platform.system() != "Windows":
        print(f"Uptime: {run(['uptime']) or unknown}")
        top = run(["ps", "-Ao", "pcpu,pmem,comm", "-r"]) or run(["ps", "-eo", "pcpu,pmem,comm", "--sort=-pcpu"])
        if top:
            print("Busiest processes (cpu%, mem%, command):")
            for line in top.splitlines()[1:6]:
                print("  " + line.strip())
        else:
            print(f"Processes: {unknown}")


if __name__ == "__main__":
    main()
