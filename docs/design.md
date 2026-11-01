# How it works

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
| `src/participants.rs` | Reading participant lists from files and web servers |
| `src/logos.rs` | Flags and logos |
| `config/` | The server's settings and the rumble definitions |
| `participants/` | The participant lists and the license record |

## How far it scales

The number of battles barely matters. Each pairing is a fixed set of totals, so more battles do not make
the scores slower to compute or take more memory, and the pages are served from memory. These are from
[tools/load_test.py](../tools/load_test.py) with 650 robots, limited to 1 GB of memory on an NVMe disk:

| Results stored | Uploads per second | Memory | Database | Page time |
|---|---|---|---|---|
| 1 million | 29,000 | 130 MB | 56 MB | 3 ms |
| 10 million | 33,000 | 157 MB | 484 MB | 3 ms |
| 25 million | 19,000 to 27,000 | 159 MB | 1.2 GB | 3 ms |

The page of a pairing's battles reads that one pairing from the database through its primary key. It takes about
5 ms for the 1000 battles it shows by default, and about 60 ms for 16,000 battles.

What does grow with battles is the disk, at 48 bytes per result, and the time to restart, about 0.7 seconds per
million results, because the server reads every result when it starts. 32 clients produce about 1.3 million
results a day, so a year of that is about 22 GB and a five-minute restart. Keeping a snapshot of the totals
would remove the wait if it ever matters.

The number of robots does matter, because the standings compare every robot with every other.
[examples/scale.rs](../examples/scale.rs) times one calculation, which takes the same time at any number of battles:

| Robots | Standings | Priority battles | Peak memory |
|---|---|---|---|
| 148 | 7 ms | 2 ms | 6 MB |
| 650 | 204 ms | 57 ms | 80 MB |
| 1,216 | 740 ms | 190 ms | 284 MB |
| 3,000 | 6 s | 1.4 s | 1.8 GB |

Uploads wait during this calculation, which runs at most every `standings_seconds`. Raise that setting for a
very large rumble.
