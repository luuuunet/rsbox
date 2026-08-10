#!/usr/bin/env python3
"""RST stability probe against Japan node via local mixed proxy."""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from datetime import datetime, timezone

PROXY = os.environ.get("RST_PROXY", "http://127.0.0.1:17944")
TIMEOUT = float(os.environ.get("RST_TIMEOUT", "25"))
OUT_JSON = os.environ.get("RST_REPORT", os.path.join(os.environ.get("TEMP", "."), "rst-stability-report.json"))


@dataclass
class Sample:
    name: str
    ok: bool
    status: int | None = None
    seconds: float = 0.0
    bytes: int = 0
    error: str = ""


@dataclass
class Report:
    started_at: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())
    samples: list[dict] = field(default_factory=list)
    summary: dict = field(default_factory=dict)


def fetch(url: str, name: str, timeout: float = TIMEOUT) -> Sample:
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({"http": PROXY, "https": PROXY}))
    t0 = time.perf_counter()
    try:
        req = urllib.request.Request(url, method="GET", headers={"User-Agent": "rst-stability/1.0"})
        with opener.open(req, timeout=timeout) as resp:
            data = resp.read()
            dt = time.perf_counter() - t0
            return Sample(name=name, ok=True, status=getattr(resp, "status", 200), seconds=dt, bytes=len(data))
    except Exception as ex:  # noqa: BLE001
        dt = time.perf_counter() - t0
        status = None
        if isinstance(ex, urllib.error.HTTPError):
            status = ex.code
            # some endpoints return non-2xx but connection worked
            if 200 <= ex.code < 500:
                return Sample(name=name, ok=True, status=ex.code, seconds=dt, bytes=0, error=str(ex))
        return Sample(name=name, ok=False, status=status, seconds=dt, error=f"{type(ex).__name__}: {ex}")


def curl_speed(url: str, name: str, max_time: int = 35) -> Sample:
    cmd = [
        "curl.exe",
        "-x",
        PROXY,
        url,
        "-o",
        "NUL",
        "-s",
        "-w",
        "%{http_code} %{time_total} %{size_download} %{speed_download}",
        "--connect-timeout",
        "20",
        "-m",
        str(max_time),
    ]
    t0 = time.perf_counter()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=max_time + 10)
        dt = time.perf_counter() - t0
        parts = (p.stdout or "").strip().split()
        if len(parts) >= 4:
            code = int(parts[0])
            seconds = float(parts[1])
            size = int(float(parts[2]))
            speed = float(parts[3])
            ok = code in (200, 204) and size >= 0
            return Sample(
                name=name,
                ok=ok,
                status=code,
                seconds=seconds,
                bytes=size,
                error="" if ok else f"curl_exit={p.returncode} speed={speed}",
            )
        return Sample(name=name, ok=False, seconds=dt, error=f"bad curl out: {p.stdout!r} err={p.stderr!r}")
    except Exception as ex:  # noqa: BLE001
        return Sample(name=name, ok=False, seconds=time.perf_counter() - t0, error=str(ex))


def add(report: Report, s: Sample) -> None:
    d = {
        "name": s.name,
        "ok": s.ok,
        "status": s.status,
        "seconds": round(s.seconds, 3),
        "bytes": s.bytes,
        "Mbps": round((s.bytes * 8 / s.seconds) / 1e6, 3) if s.ok and s.seconds > 0 and s.bytes > 0 else None,
        "error": s.error,
    }
    report.samples.append(d)
    flag = "OK " if s.ok else "FAIL"
    extra = f" {d['Mbps']}Mbps" if d["Mbps"] else ""
    print(f"[{flag}] {s.name}: status={s.status} t={d['seconds']}s bytes={s.bytes}{extra} {s.error}", flush=True)


def main() -> int:
    report = Report()
    print(f"proxy={PROXY}", flush=True)

    # 1) smoke
    for name, url in [
        ("google_204", "https://www.google.com/generate_204"),
        ("cloudflare_trace", "https://1.1.1.1/cdn-cgi/trace"),
        ("ifconfig", "https://ifconfig.me"),
        ("youtube_home", "https://www.youtube.com/"),
        ("ytimg", "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"),
    ]:
        add(report, fetch(url, name))

    # 2) latency burst (20 samples)
    lats = []
    ok_n = 0
    for i in range(20):
        s = fetch("https://www.google.com/generate_204", f"lat_{i+1}", timeout=15)
        add(report, s)
        if s.ok:
            ok_n += 1
            lats.append(s.seconds)

    # 3) concurrent (8 parallel)
    print("--- concurrent x8 ---", flush=True)
    urls = [
        ("c_google", "https://www.google.com/generate_204"),
        ("c_cf", "https://1.1.1.1/cdn-cgi/trace"),
        ("c_yt", "https://www.youtube.com/"),
        ("c_ytimg", "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"),
        ("c_google2", "https://www.google.com/generate_204"),
        ("c_ip", "https://ifconfig.me"),
        ("c_cf2", "https://cloudflare.com/cdn-cgi/trace"),
        ("c_yt2", "https://www.youtube.com/"),
    ]
    with ThreadPoolExecutor(max_workers=8) as ex:
        futs = {ex.submit(fetch, url, name): name for name, url in urls}
        for fut in as_completed(futs):
            add(report, fut.result())

    # 4) sustained download (~15-30s)
    print("--- sustained download ---", flush=True)
    add(
        report,
        curl_speed(
            "https://speed.cloudflare.com/__down?bytes=25000000",
            "cf_down_25MB",
            max_time=60,
        ),
    )
    add(
        report,
        curl_speed(
            "https://speed.cloudflare.com/__down?bytes=50000000",
            "cf_down_50MB",
            max_time=90,
        ),
    )

    # 5) keep-alive loop 60s
    print("--- keep-alive 60s ---", flush=True)
    ka_ok = 0
    ka_fail = 0
    ka_lat = []
    end = time.time() + 60
    i = 0
    while time.time() < end:
        i += 1
        s = fetch("https://www.google.com/generate_204", f"ka_{i}", timeout=12)
        add(report, s)
        if s.ok:
            ka_ok += 1
            ka_lat.append(s.seconds)
        else:
            ka_fail += 1
        time.sleep(2)

    # summary
    total = len(report.samples)
    oks = sum(1 for s in report.samples if s["ok"])
    fails = total - oks
    report.summary = {
        "total": total,
        "ok": oks,
        "fail": fails,
        "success_rate": round(oks / total * 100, 2) if total else 0,
        "latency_n": len(lats),
        "latency_ok": ok_n,
        "latency_avg_ms": round(sum(lats) / len(lats) * 1000, 1) if lats else None,
        "latency_p95_ms": round(sorted(lats)[max(0, int(len(lats) * 0.95) - 1)] * 1000, 1) if lats else None,
        "latency_max_ms": round(max(lats) * 1000, 1) if lats else None,
        "keepalive_ok": ka_ok,
        "keepalive_fail": ka_fail,
        "keepalive_avg_ms": round(sum(ka_lat) / len(ka_lat) * 1000, 1) if ka_lat else None,
        "exit_ip": next((s.get("error") for s in report.samples if s["name"] == "ifconfig" and not s["ok"]), None),
    }
    # capture exit IP from successful ifconfig body via re-fetch
    ip_s = fetch("https://ifconfig.me", "exit_ip_final")
    add(report, ip_s)
    # ifconfig returns body as IP; we didn't store body — refetch with curl
    try:
        p = subprocess.run(
            ["curl.exe", "-x", PROXY, "https://ifconfig.me", "-s", "--connect-timeout", "15", "-m", "20"],
            capture_output=True,
            text=True,
            timeout=25,
        )
        report.summary["exit_ip"] = (p.stdout or "").strip()
    except Exception as ex:  # noqa: BLE001
        report.summary["exit_ip_error"] = str(ex)

    speeds = [s["Mbps"] for s in report.samples if s.get("Mbps")]
    if speeds:
        report.summary["speed_max_Mbps"] = max(speeds)
        report.summary["speed_avg_Mbps"] = round(sum(speeds) / len(speeds), 3)

    with open(OUT_JSON, "w", encoding="utf-8") as f:
        json.dump(
            {
                "started_at": report.started_at,
                "proxy": PROXY,
                "summary": report.summary,
                "samples": report.samples,
            },
            f,
            indent=2,
            ensure_ascii=False,
        )

    print("\n==== SUMMARY ====", flush=True)
    print(json.dumps(report.summary, indent=2, ensure_ascii=False), flush=True)
    print(f"report: {OUT_JSON}", flush=True)
    return 0 if fails == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
