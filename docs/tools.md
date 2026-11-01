# Tools for sorting and checking robots

## Sorting robots for OpenRumble

[tools/license_census.py](../tools/license_census.py) sorts robots by [the rules](rules.md): whether the
source is in the jar, and what license the jar and the robot's wiki page state. It only reads jars, and
never extracts or runs them. [participants/review/robots.tsv](../participants/review/robots.tsv) is its output,
with a verdict and the evidence for every robot, and
[participants/review/overrides.tsv](../participants/review/overrides.tsv) holds a reviewer's rulings.
[participants/licenses.tsv](../participants/licenses.tsv) is the license record: for each robot that is in, its
license, who decided it and on what words, any AI declaration, and the SHA-256 of the exact jar.

To run it again, fetch the wiki pages into a cache first. The wiki limits requests, and has blocked
addresses before, so [tools/wiki_fetch.py](../tools/wiki_fetch.py) uses one bulk export every 2 minutes:

```bash
python3 tools/wiki_fetch.py pages.json titles.txt
```

```bash
python3 tools/license_census.py --list 1v1.txt --list melee.txt --jars robots --wiki pages.json \
  --overrides participants/review/overrides.tsv --ai participants/review/ai.tsv \
  --out participants/review/robots.tsv --write-licenses participants/licenses.tsv \
  --write-list 1v1=participants/openrumble.txt --write-list melee=participants/openmelee6.txt
```

`titles.txt` has one wiki page title per line, normally each robot's class name. The name of each `--list`
file becomes a column in the output and the label for `--write-list`.

## Checking robots with hidden names

A robot can break when it cannot see real names, so it is worth checking new ones.
[tools/name_check.sh](../tools/name_check.sh) runs each listed robot against fixed opponents with names hidden
and shown, and [tools/name_check_report.py](../tools/name_check_report.py) shows which scores differ and which
errors only appear with names hidden:

```bash
tools/name_check.sh path/to/robocode ~/robocode/robots 36
```

```bash
tools/name_check_report.py ~/name-check/results.tsv
```

A short battle is noisy, so run anything that looks wrong again with more rounds before believing it, using a
jobs file as described at the top of the script.
