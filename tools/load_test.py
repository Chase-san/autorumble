#!/usr/bin/env python3
"""Measures the server under many results: upload rate, memory, database size and restart time.

It starts its own server on a throwaway database, so it does not touch a running rumble. The rate is
printed for each million results, to show whether uploads slow down as the database grows.

usage: tools/load_test.py <server binary> <work directory> <robots> <results>
"""
import pathlib
import random
import shutil
import subprocess
import sys
import time
import urllib.request

server, work, robots, results = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
PORT = 18654
BASE = f"http://127.0.0.1:{PORT}"
BATCH = 1000
PAIRING = "/BattleDetails?game=load&name=load.Robot0001%201.0&opponent=load.Robot0000%201.0"

if work.exists():
    shutil.rmtree(work)
work.mkdir(parents=True)
names = [f"load.Robot{i:04d} 1.0" for i in range(robots)]
(work / "list.txt").write_text("".join(f"{name},http://example.com/x.jar\n" for name in names))
(work / "config.toml").write_text(
    f'listen = "127.0.0.1:{PORT}"\ndatabase = "{work}/load.db"\nlogos = "{work}/logos"\n'
    f'standings_seconds = 10\nallowed_clients = ["1.11.2"]\n'
    f'[[rumble]]\nname = "load"\ntitle = "Load"\nfield = "800x600"\nrounds = 35\n'
    f'participants = {{ file = "{work}/list.txt" }}\n'
)


def start():
    process = subprocess.Popen([str(server), str(work / "config.toml")], stderr=subprocess.DEVNULL)
    began = time.time()
    while True:
        try:
            urllib.request.urlopen(BASE + "/robots.txt", timeout=1)
            return process, time.time() - began
        except OSError:
            time.sleep(0.02)


def memory_mb(process):
    for line in pathlib.Path(f"/proc/{process.pid}/status").read_text().splitlines():
        if line.startswith("VmRSS"):
            return int(line.split()[1]) / 1024
    return 0


def database_mb():
    return sum(path.stat().st_size for path in work.glob("load.db*")) / 1e6


def page_seconds(path):
    began = time.time()
    urllib.request.urlopen(BASE + path).read()
    return time.time() - began


process, _ = start()
# Times count back from now, one millisecond per result, since the server refuses a battle from the
# future. That leaves room for about 80 million results before they count as too old.
battle_time = int(time.time() * 1000)
sent = 0
stored = 0
mark = time.time()
while sent < results:
    lines = []
    for _ in range(BATCH):
        battle_time -= 1
        first, second = random.sample(names, 2)
        score = random.randint(0, 5000)
        lines.append(
            f"game=load&version=1&client=1.11.2&teams=NOT&melee=NOT&rounds=35&field=800x600&user=Load&time={battle_time}"
            f"&fname={first}&fscore={score}&fbulletd=1&fsurvival=1&sname={second}&sscore={5000 - score}&sbulletd=1&ssurvival=1"
        )
    request = urllib.request.Request(BASE + "/UploadedResultsBatch", "\n".join(lines).encode())
    reply = urllib.request.urlopen(request).read().decode()
    stored += sum(1 for line in reply.splitlines() if line == "OK stored")
    sent += len(lines)
    # A refused result is fast, so refusals would look like a very high upload rate.
    refused = [line for line in reply.splitlines() if line.startswith(("REJECT", "RETRY"))]
    if refused:
        process.terminate()
        sys.exit(f"stopped at {sent} results, the server refused some: {refused[0]}")
    if sent % 1_000_000 == 0:
        rate = 1_000_000 / (time.time() - mark)
        mark = time.time()
        print(f"{sent // 1_000_000:4}M: {rate:7.0f} results/s, memory {memory_mb(process):5.0f} MB, "
              f"database {database_mb():7.0f} MB, rankings page {page_seconds('/Rankings?game=load'):.3f}s, "
              f"robot page {page_seconds('/BotDetails?game=load&name=load.Robot0001%201.0'):.3f}s, "
              f"pairing page {page_seconds(PAIRING):.3f}s, all its battles {page_seconds(PAIRING + '&limit=100000000'):.3f}s", flush=True)

print(f"{robots} robots, {stored} of {sent} results stored", flush=True)
process.terminate()
process.wait()
print(f"database: {database_mb():.0f} MB, {database_mb() * 1e6 / max(stored, 1):.0f} bytes per result", flush=True)

process, startup = start()
print(f"restart, reading every result: {startup:.1f}s, then {memory_mb(process):.0f} MB", flush=True)
process.terminate()
process.wait()
shutil.rmtree(work)
