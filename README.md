# AutoRumble

A small server for [Robocode](https://robocode.sourceforge.io/) rumbles, written in Rust. Clients run battles
and upload the results, and the server scores them and shows the rankings.

| Rumble | Who can enter | Format |
|---|---|---|
| `openrumble` | Open source robots | 1v1, 800x600 |
| `openmelee6` | Open source robots | 6 robots, 800x800 |
| `autorumble` | Open source robots designed mostly by AI | 1v1, 800x600 |
| `automelee6` | Open source robots designed mostly by AI | 6 robots, 800x800 |

All of them hide enemy names from the robots, and delete the data robots save between battles.

## Run the server

```bash
cargo run --release
```

Then open http://127.0.0.1:8652/. The settings and rumbles are in [config/](config/README.md).

## Run a client

You need a Robocode client that supports `HIDENAMES` and `CLEARDATA`, which released versions do not have yet.

1. Copy a file from [client/](client/) to the `roborumble` folder of your Robocode installation.
2. In that file, set `USER` and replace `localhost:8652` with the server's address.
3. Make a launcher for it, and run that:

```bash
sed 's#roborumble/roborumble.txt#roborumble/openrumble.txt#' roborumble.sh > openrumble.sh
```

## Enter a robot

Add a line to the rumble's list in [participants/](participants/). [The rules](docs/rules.md) say what a robot needs.

```
claude.Umbra 0.5,https://example.com/claude.Umbra_0.5.jar
```

## More

- [Entry rules](docs/rules.md)
- [Settings, rumbles and participant lists](config/README.md)
- [Running it on a network or in public](docs/running.md)
- [How it works and how far it scales](docs/design.md)
- [Tools for sorting and checking robots](docs/tools.md)
- [The upload protocol](docs/protocol.md) and [the scores](docs/scoring.html)

The scores and protocol follow [LiteRumble](https://github.com/robo-code/literumble) by Julian Kent (Skilgannon).
This is a new implementation and shares no code with it. See [LICENSE](LICENSE).
