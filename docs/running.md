# Running the server

The server keeps all its data in one SQLite file, and [config/](../config/README.md) holds its settings.

## On your own network

The server listens on every address by default, so other machines on the network can open
http://this-machine:8652/. To keep it running, install
[deploy/autorumble-local.service](../deploy/autorumble-local.service) as described at the top of that file.

To fill the rumbles, [tools/local_clients.sh](../tools/local_clients.sh) runs RoboRumble clients on the same
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

## In public

[deploy/](../deploy/) has a systemd unit and an nginx configuration. Change `listen` in
`config/server.toml` to `127.0.0.1:8652`, so that everything goes through nginx.

To back up the database while the server runs:

```bash
sqlite3 data/autorumble.db ".backup backup.db"
```

## Tests

```bash
cargo test
```

Each module tests its own logic, and [tests/http.rs](../tests/http.rs) runs the whole server through its
routes. `scoring.rs` checks its numbers against values computed separately from LiteRumble's formulas.
`python3 tools/test_license_census.py` tests the license census.

[tools/crosscheck_literumble.py](../tools/crosscheck_literumble.py) goes further: it feeds the same random
battles to a real LiteRumble and to this server, and compares every score of every robot and pairing.
