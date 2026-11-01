#!/usr/bin/env python3
"""Summarizes the results of tools/name_check.sh: which robots act differently with names hidden.

Each robot fought the same opponents with names hidden and shown. A robot that breaks on hidden names
prints errors only when they are hidden, or scores far less. The opponents are only there to measure
the robot, so the noise of a few rounds is expected, and the report shows how large it is. A match-up
run more than once is averaged.

usage: tools/name_check_report.py results.tsv
"""
import collections
import statistics
import sys

sys.stdout.reconfigure(errors="replace")
battles = collections.defaultdict(lambda: {"hidden": [], "shown": []})
failed = []
# Only a newline ends a line, because an error message a robot printed can hold a carriage return.
for line in open(sys.argv[1], encoding="utf-8", errors="replace", newline="\n"):
    robot, mode, opponents, share, firsts, errors, first_error = (line.rstrip("\n").split("\t") + [""] * 7)[:7]
    if mode == "failed":
        failed.append((robot, opponents, first_error))
    else:
        battles[robot, opponents][mode].append((float(share) if share else None, int(errors or 0), first_error))


def summary(runs):
    """The mean score share, the total errors and the first error of a mode's battles."""
    shares = [run[0] for run in runs if run[0] is not None]
    message = next((run[2] for run in runs if run[2]), "")
    return (statistics.mean(shares) if shares else None), sum(run[1] for run in runs), message


print(f"{len({robot for robot, _ in battles})} robots, {len(battles)} match-ups, {len(failed)} that did not finish")
for robot, opponents, why in failed:
    print(f"  did not finish: {robot} against {opponents}: {why}")

differences = []
only_hidden, only_shown, both = [], [], []
for (robot, opponents), modes in sorted(battles.items()):
    if not modes["hidden"] or not modes["shown"]:
        continue
    hidden, shown = summary(modes["hidden"]), summary(modes["shown"])
    if hidden[0] is not None and shown[0] is not None:
        differences.append((shown[0] - hidden[0], robot, opponents, hidden[0], shown[0]))
    if hidden[1] and not shown[1]:
        only_hidden.append((robot, opponents, hidden[1], hidden[2]))
    elif shown[1] and not hidden[1]:
        only_shown.append((robot, opponents, shown[1], shown[2]))
    elif hidden[1] and shown[1]:
        both.append((robot, opponents, hidden[1], shown[1], hidden[2]))

gaps = [difference[0] for difference in differences]
print(f"\nscore share, shown minus hidden, over {len(gaps)} match-ups: mean {statistics.mean(gaps):+.2f}, "
      f"median {statistics.median(gaps):+.2f}, standard deviation {statistics.pstdev(gaps):.2f}")

print("\nerrors only with names hidden:")
for robot, opponents, count, error in only_hidden:
    print(f"  {robot} against {opponents[:24]}: {count} errors: {error[:110]}")
print("\nerrors only with names shown:")
for robot, opponents, count, error in only_shown:
    print(f"  {robot} against {opponents[:24]}: {count} errors: {error[:110]}")
print("\nerrors in both:")
for robot, opponents, hidden_count, shown_count, error in both:
    print(f"  {robot} against {opponents[:24]}: {hidden_count} hidden, {shown_count} shown: {error[:100]}")

differences.sort(reverse=True)
print("\nscored less with names hidden:")
for gap, robot, opponents, hidden, shown in differences[:25]:
    print(f"  {gap:+6.1f}  {robot:38} {hidden:5.1f} hidden, {shown:5.1f} shown, against {opponents[:24]}")
print("\nscored more with names hidden:")
for gap, robot, opponents, hidden, shown in differences[-8:]:
    print(f"  {gap:+6.1f}  {robot:38} {hidden:5.1f} hidden, {shown:5.1f} shown, against {opponents[:24]}")
