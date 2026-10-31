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
cargo run --release -- autorumble.toml
```

Then open http://127.0.0.1:8652/. [autorumble.toml](autorumble.toml) documents every setting. It keeps
all its data in one SQLite file, and [deploy/](deploy/) has a systemd unit and an nginx configuration
for running it in public.

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
| `src/logos.rs` | Author logos |

## Participants

The lists are the files in [participants/](participants/), so entering a robot is a reviewed change to
this repository. The server reads them again every few minutes, and removing a line retires a robot.
Its battles stay in the database, and adding the line back restores them.

Each rumble says where its list comes from in `autorumble.toml`. To use a web page later, such as a
RoboWiki page, add a variant to `Source` in [src/participants.rs](src/participants.rs). The parser
already reads the wiki's format, clients always download the list from this server, and a failed fetch
keeps the last good list.

## Logos

Logos replace LiteRumble's country flags. A logo is a PNG of at most 64x64 pixels and 32 KB, named after
the robot's top-level package, like `logos/claude.png`. It shows for all robots in that package.
The server checks these limits when it reads the file, and ignores a file that fails.

## Clients

The configurations in [client/](client/) go in the `roborumble` folder of a Robocode installation.
Set `USER` and replace `localhost:8652` with the server's address. The launcher for a rumble is a copy of
`roborumble.sh` that names its configuration file, which keeps the Java options the robot sandbox needs:

```bash
sed 's#roborumble/roborumble.txt#roborumble/autorumble.txt#' roborumble.sh > autorumble.sh
```

The rumbles need a client that supports `HIDEENEMYNAMES`, which released versions of Robocode do not have
yet. `RESULTSBATCHURL` is optional: without it, the client uploads one result at a time.

## Tests

```bash
cargo test
```

Each module tests its own logic, and [tests/http.rs](tests/http.rs) runs the whole server through its
routes. `scoring.rs` checks its numbers against values computed separately from LiteRumble's formulas.

[tools/crosscheck_literumble.py](tools/crosscheck_literumble.py) goes further: it feeds the same random
battles to a real LiteRumble and to this server, and compares every score of every robot and pairing.

## Credits

The scores and the protocol follow [LiteRumble](https://github.com/robo-code/literumble) by Julian Kent
(Skilgannon). This is a new implementation and shares no code with it. See [LICENSE](LICENSE).
