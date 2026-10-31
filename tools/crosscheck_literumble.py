#!/usr/bin/env python3
"""Checks this server's scores against LiteRumble's on the same random battles.

LiteRumble's definitions are what the scores here are meant to follow, so it makes a good test
oracle. This script feeds the same results to a throwaway LiteRumble and a throwaway AutoRumble,
then compares every score of every robot and every pairing. It exits with an error on any
difference larger than LiteRumble's two-decimal output can explain.

LiteRumble needs Linux, and Flask, Flask-Compress and numpy:

    python3 -m venv venv
    venv/bin/pip install Flask==3.0.3 Flask-Compress==1.15 numpy
    git clone https://github.com/robo-code/literumble.git
    cargo build
    venv/bin/python tools/crosscheck_literumble.py --literumble literumble --server target/debug/autorumble
"""
import argparse
import itertools
import json
import os
import pathlib
import random
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request

PORT = 18653
BASE = f"http://127.0.0.1:{PORT}"
CLIENT = "1.11.1"
ROUNDS = 35

# LiteRumble prints two decimals, so two rounded numbers can differ this much from exact ones.
TOLERANCE = 0.0051


def robot_names(count):
    return [f"check.Robot{i:03d} 1.{i}" for i in range(count)]


def result_line(game, melee, field, first, second, first_share, time_ms):
    """One result as the Java client sends it."""
    total = random.randint(1000, 6000)
    first_score = int(total * first_share)
    first_firsts = round(ROUNDS * first_share)
    return (
        f"game={game}&version=1&client={CLIENT}&teams=NOT&melee={melee}&rounds={ROUNDS}"
        f"&field={field}&user=Crosscheck&time={time_ms}"
        f"&fname={first}&fscore={first_score}&fbulletd={first_score // 2}&fsurvival={first_firsts}"
        f"&sname={second}&sscore={total - first_score}&sbulletd={(total - first_score) // 2}"
        f"&ssurvival={ROUNDS - first_firsts}"
    )


def share(strengths, first, second):
    """A noisy share of the score for the first robot, never exactly even."""
    base = strengths[first] / (strengths[first] + strengths[second])
    return min(0.97, max(0.03, random.gauss(base, 0.08)))


def duel_results(game, names, strengths, now_ms):
    """Most pairings get a few battles and some get none, so robots have different pairings."""
    lines = []
    for first, second in itertools.combinations(names, 2):
        if random.random() < 0.85:
            for _ in range(random.randint(1, 5)):
                now_ms += 1
                a, b = (first, second) if random.random() < 0.5 else (second, first)
                lines.append(result_line(game, "NOT", "800x600", a, b, share(strengths, a, b), now_ms))
    return lines


def melee_results(game, names, strengths, battles, now_ms):
    """Random groups of six, each with one result per pair of robots."""
    lines = []
    for _ in range(battles):
        for first, second in itertools.combinations(random.sample(names, 6), 2):
            now_ms += 1
            lines.append(result_line(game, "YES", "800x800", first, second, share(strengths, first, second), now_ms))
    return lines


def start_autorumble(server, directory, rumbles):
    sections = []
    for game, field, melee_bots, names in rumbles:
        listing = directory / f"{game}.txt"
        listing.write_text("".join(f"{name},http://example.com/x.jar\n" for name in names))
        melee = f"melee_bots = {melee_bots}\n" if melee_bots else ""
        sections.append(
            f'[[rumble]]\nname = "{game}"\ntitle = "{game}"\nfield = "{field}"\nrounds = {ROUNDS}\n'
            f'{melee}participants = {{ file = "{listing}" }}\n'
        )
    config = directory / "autorumble.toml"
    config.write_text(
        f'listen = "127.0.0.1:{PORT}"\ndatabase = "{directory}/auto.db"\nlogos = "{directory}/logos"\n'
        f'standings_seconds = 0\nallowed_clients = ["{CLIENT}"]\n' + "".join(sections)
    )
    process = subprocess.Popen([server, str(config)], stderr=subprocess.DEVNULL)
    for _ in range(100):
        try:
            urllib.request.urlopen(BASE + "/robots.txt", timeout=1)
            return process
        except OSError:
            time.sleep(0.1)
    process.kill()
    sys.exit("AutoRumble did not start")


def post_batches(lines):
    for start in range(0, len(lines), 1000):
        body = "\n".join(lines[start:start + 1000]).encode()
        reply = urllib.request.urlopen(urllib.request.Request(BASE + "/UploadedResultsBatch", body)).read().decode()
        statuses = [line for line in reply.splitlines() if not line.startswith("[")]
        bad = [line for line in statuses if line != "OK stored"]
        if bad or len(statuses) != len(lines[start:start + 1000]):
            sys.exit(f"AutoRumble refused results: {bad[:3]}")


def auto_json(path):
    return json.load(urllib.request.urlopen(BASE + path))


class LiteRumble:
    """LiteRumble in this process, on a throwaway database."""

    def __init__(self, checkout, database):
        os.environ["LITERUMBLE_DB"] = str(database)
        sys.path.insert(0, str(checkout))
        os.chdir(checkout)
        import wsgi_local  # registers its fake App Engine modules, so it has to come first
        import background

        self.background = background
        self.client = wsgi_local.app.test_client()

    def upload(self, lines):
        for index, line in enumerate(lines):
            response = self.client.post("/UploadedResults", data=line + "\n").get_data(as_text=True)
            if "OK" not in response or "discarded" in response:
                sys.exit(f"LiteRumble refused a result: {response}")
            if index % 50 == 49:
                self.background.drain(120)
        self.background.drain(120)

    def batch_rankings(self):
        self.background.tick("/QueueHourlyBatchRankings")
        self.background.drain(600)

    def json(self, path):
        return json.loads(self.client.get(path).get_data(as_text=True))


class Comparison:
    def __init__(self):
        self.compared = 0
        self.largest = 0.0
        self.failures = []

    def check(self, where, field, expected, actual):
        difference = abs(float(expected) - float(actual))
        self.compared += 1
        self.largest = max(self.largest, difference)
        if difference > TOLERANCE:
            self.failures.append(f"{where} {field}: LiteRumble {expected}, AutoRumble {actual}")


def compare_rumble(game, lite, comparison):
    """Compares a rumble's rankings, then every pairing of every robot."""
    lite_rows = {row["name"]: row for row in lite.json(f"/Rankings?game={game}&api=1")}
    auto_rows = {row["name"]: row for row in auto_json(f"/Rankings?game={game}&api=1")}
    if lite_rows.keys() != auto_rows.keys():
        comparison.failures.append(f"{game}: different robots")
        return
    for name, expected in lite_rows.items():
        actual = auto_rows[name]
        for field in ("APS", "APS_CI", "PWIN", "ANPP", "vote", "survival", "pairings", "battles"):
            comparison.check(f"{game} {name}", field, expected[field], actual[field])

        query = urllib.parse.quote(name)
        lite_pairs = {p["name"]: p for p in lite.json(f"/BotDetails?game={game}&name={query}&api=1")["pairingsList"]}
        auto_pairs = {p["name"]: p for p in auto_json(f"/BotDetails?game={game}&name={query}&api=1")["pairingsList"]}
        if lite_pairs.keys() != auto_pairs.keys():
            comparison.failures.append(f"{game} {name}: different pairings")
            continue
        for opponent, expected_pair in lite_pairs.items():
            for field in ("APS", "APS_CI", "NPP", "survival", "KNNPBI", "battles"):
                comparison.check(f"{game} {name} vs {opponent}", field, expected_pair[field], auto_pairs[opponent][field])


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--literumble", required=True, type=pathlib.Path)
    parser.add_argument("--server", required=True, type=pathlib.Path)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--duel-robots", type=int, default=40)
    parser.add_argument("--melee-robots", type=int, default=15)
    parser.add_argument("--melee-battles", type=int, default=100)
    args = parser.parse_args()
    random.seed(args.seed)

    duel_names = robot_names(args.duel_robots)
    melee_names = robot_names(args.melee_robots)
    strengths = {name: random.uniform(0.5, 2.0) for name in duel_names}
    strengths.update({name: random.uniform(0.5, 2.0) for name in melee_names if name not in strengths})
    now_ms = int(time.time() * 1000) - 3_600_000
    duel = duel_results("checkduel", duel_names, strengths, now_ms)
    melee = melee_results("checkmelee", melee_names, strengths, args.melee_battles, now_ms)
    print(f"{len(duel)} duel results and {len(melee)} melee results")

    server = args.server.resolve()
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        autorumble = start_autorumble(
            str(server),
            directory,
            [("checkduel", "800x600", None, duel_names), ("checkmelee", "800x800", 6, melee_names)],
        )
        try:
            post_batches(duel + melee)
            lite = LiteRumble(args.literumble.resolve(), directory / "lite.db")
            lite.upload(duel)
            lite.upload(melee)
            lite.batch_rankings()

            comparison = Comparison()
            compare_rumble("checkduel", lite, comparison)
            compare_rumble("checkmelee", lite, comparison)
        finally:
            autorumble.terminate()

    print(f"compared {comparison.compared} numbers, largest difference {comparison.largest:.4f}")
    for failure in comparison.failures[:30]:
        print(failure)
    if comparison.failures:
        sys.exit(f"{len(comparison.failures)} differences")
    print("every score matches")


if __name__ == "__main__":
    main()
