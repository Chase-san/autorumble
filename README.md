# AutoRumble

A small rumble server for [Robocode](https://robocode.sourceforge.io/), written in Rust. Clients run
battles and upload the results, and the server stores them, works out the scores and serves the
rankings. It speaks the same protocol as [LiteRumble](https://robowiki.net/wiki/LiteRumble), so the
standard RoboRumble client works with it.

It hosts these rumbles, all with enemy names hidden from the robots:

| Rumble | Who can enter | Format |
|---|---|---|
| `autorumble` | Open source robots designed mostly by AI | 1v1, 800x600, 35 rounds |
| `automelee6` | The same | 6 robots, 800x800, 35 rounds |
| `openrumble` | Open source robots | 1v1, 800x600, 35 rounds |
| `openmelee6` | The same | 6 robots, 800x800, 35 rounds |

[docs/rules.md](docs/rules.md) has the entry rules. [docs/protocol.md](docs/protocol.md) has the upload
format. [docs/scoring.html](docs/scoring.html) defines the scores.

## Running it

```bash
cargo run --release
```

Then open http://127.0.0.1:8652/. [config/](config/) holds the settings and the rumble definitions, and
[its README](config/README.md) explains them. The server keeps all its data in one SQLite file.

### On your own network

The server listens on every address by default, so other machines on the network can open
http://this-machine:8652/. To keep it running, install
[deploy/autorumble-local.service](deploy/autorumble-local.service) as described at the top of that file.

To fill the rumbles, [tools/local_clients.sh](tools/local_clients.sh) runs RoboRumble clients on the same
machine:

```bash
tools/local_clients.sh setup path/to/robocode ~/robocode/robots
```

```bash
tools/local_clients.sh start 3
```

Robots that are not hosted anywhere go in the `robots/` folder, which the server offers at
`/robots/name_version.jar`, and their line in a participant list points there. Clients download them like
any other robot.

### In public

[deploy/](deploy/) has a systemd unit and an nginx configuration. Change `listen` in
`config/server.toml` to `127.0.0.1:8652`, so that everything goes through nginx.

To back up the database while the server runs:

```bash
sqlite3 data/autorumble.db ".backup backup.db"
```

## How it works

```
 Robocode clients                      web browsers
       | upload results                     | read pages
       v                                    v
    web.rs  ------- job and reply ----->  pages.rs
       |                                    ^
       v                                    | latest snapshot
    engine.rs (one thread owns all state) --+
       |            |             |
       v            v             v
    store.rs     scoring.rs    priority.rs
    (SQLite)     (the scores)  (what to fight next)
```

The design rests on three decisions:

- **One thread owns everything that changes.** Web handlers send uploads to the engine thread and wait
  for the answer, and pages read an immutable snapshot that the engine publishes. No other locks are
  needed. Uploads waiting together are stored in one transaction, so single uploads get the speed of
  batches.
- **Every result is kept, and nothing derived is stored.** The `battle` table is the only record, and
  the scores are rebuilt from it on every start. Changing a formula applies to all past battles, and
  removing a bad uploader's results is a `DELETE` and a restart. The primary key identifies a result,
  so sending one twice is harmless, and clients can retry freely.
- **Scores are one plain function.** `Standings::compute` turns the battle totals into every number the
  pages show, every few seconds. There is no separate batch step to keep in line.

| File | What it does |
|---|---|
| `src/main.rs` | Starts the server |
| `src/config.rs` | The configuration file |
| `src/web.rs` | The HTTP routes |
| `src/pages.rs` | The HTML pages and their JSON versions |
| `src/engine.rs` | The thread that owns the state, and the checks on each upload |
| `src/protocol.rs` | The text format of uploads and replies |
| `src/scoring.rs` | All the score calculations |
| `src/priority.rs` | Which battles to ask clients for |
| `src/store.rs` | The database |
| `src/participants.rs` | Reading participant lists |
| `src/logos.rs` | Flags and logos |
| `config/` | The server's settings and the rumble definitions |
| `participants/` | The participant lists and the license record |

## Participants

The lists are the files in [participants/](participants/), with one `name version,url` per line and
nothing else, so entering a robot is a reviewed change to this repository. The server reads them again
every few minutes, and removing a line retires a robot. Its battles stay in the database, and adding the
line back restores them.

[participants/licenses.tsv](participants/licenses.tsv) is the license record: for each robot that is in, its
license, who decided it and on what words, any AI declaration, and the SHA-256 of the exact jar.

Each rumble says where its list comes from in its file in `config/rumbles/`. To use a web page later, such as a
RoboWiki page, add a variant to `Source` in [src/participants.rs](src/participants.rs). The parser
already reads the wiki's format, clients always download the list from this server, and a failed fetch
keeps the last good list.

### Sorting robots for OpenRumble

[tools/license_census.py](tools/license_census.py) sorts robots by [the rules](docs/rules.md): whether the
source is in the jar, and what license the jar and the robot's wiki page state. It only reads jars, and
never extracts or runs them. [participants/review/robots.tsv](participants/review/robots.tsv) is its output,
with a verdict and the evidence for every robot, and
[participants/review/overrides.tsv](participants/review/overrides.tsv) holds a reviewer's rulings.

To run it again, fetch the wiki pages into a cache first. The wiki limits requests, and has blocked
addresses before, so [tools/wiki_fetch.py](tools/wiki_fetch.py) uses one bulk export every 2 minutes:

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

### Checking robots with hidden names

A robot can break when it cannot see real names, so it is worth checking new ones.
[tools/name_check.sh](tools/name_check.sh) runs each listed robot against fixed opponents with names hidden
and shown, and [tools/name_check_report.py](tools/name_check_report.py) shows which scores differ and which
errors only appear with names hidden:

```bash
tools/name_check.sh path/to/robocode ~/robocode/robots 36
```

```bash
tools/name_check_report.py ~/name-check/results.tsv
```

A short battle is noisy, so run anything that looks wrong again with more rounds before believing it, using a
jobs file as described at the top of the script.

## Flags and logos

[config/logos.csv](config/logos.csv) says which image each robot's top-level package uses, with lines like
`cs,flag_usa`, and the images are in [logos/](logos/). So an author can have a country's flag or their own logo,
and changing one is a line in the list. An image is a PNG of at most 64x64 pixels and 32 KB. The server checks
these limits when it reads the file, and ignores a file that fails. [logos/README.md](logos/README.md) says
where the flags came from.

## Clients

The configurations in [client/](client/) go in the `roborumble` folder of a Robocode installation.
Set `USER` and replace `localhost:8652` with the server's address. The launcher for a rumble is a copy of
`roborumble.sh` that names its configuration file, which keeps the Java options the robot sandbox needs:

```bash
sed 's#roborumble/roborumble.txt#roborumble/autorumble.txt#' roborumble.sh > autorumble.sh
```

The rumbles need a client that supports `HIDENAMES` and `CLEARDATA`, which released versions of Robocode do
not have yet. `RESULTSBATCHURL` is optional: without it, the client uploads one result at a time.

## Tests

```bash
cargo test
```

Each module tests its own logic, and [tests/http.rs](tests/http.rs) runs the whole server through its
routes. `scoring.rs` checks its numbers against values computed separately from LiteRumble's formulas.
`python3 tools/test_license_census.py` tests the license census.

[tools/crosscheck_literumble.py](tools/crosscheck_literumble.py) goes further: it feeds the same random
battles to a real LiteRumble and to this server, and compares every score of every robot and pairing.

## How far it scales

The number of battles barely matters. Each pairing is a fixed set of totals, so more battles do not make
the scores slower to compute or take more memory, and the pages are served from memory. These are from
[tools/load_test.py](tools/load_test.py) with 650 robots, limited to 1 GB of memory on an NVMe disk:

| Results stored | Uploads per second | Memory | Database | Page time |
|---|---|---|---|---|
| 1 million | 29,000 | 130 MB | 56 MB | 3 ms |
| 10 million | 33,000 | 157 MB | 484 MB | 3 ms |
| 25 million | 19,000 to 27,000 | 159 MB | 1.2 GB | 3 ms |

What does grow with battles is the disk, at 48 bytes per result, and the time to restart, about 0.7 seconds per
million results, because the server reads every result when it starts. 32 clients produce about 1.3 million
results a day, so a year of that is about 22 GB and a five-minute restart. Keeping a snapshot of the totals
would remove the wait if it ever matters.

The number of robots does matter, because the standings compare every robot with every other.
[examples/scale.rs](examples/scale.rs) times one calculation, which takes the same time at any number of battles:

| Robots | Standings | Priority battles | Peak memory |
|---|---|---|---|
| 148 | 7 ms | 2 ms | 6 MB |
| 650 | 204 ms | 57 ms | 80 MB |
| 1,216 | 740 ms | 190 ms | 284 MB |
| 3,000 | 6 s | 1.4 s | 1.8 GB |

Uploads wait during this calculation, which runs at most every `standings_seconds`. Raise that setting for a
very large rumble.

## Credits

The scores and the protocol follow [LiteRumble](https://github.com/robo-code/literumble) by Julian Kent
(Skilgannon). This is a new implementation and shares no code with it. See [LICENSE](LICENSE).
