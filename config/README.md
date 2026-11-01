# Configuration

`server.toml` has the server's own settings. `rumbles/` defines the rumbles, with one file for each.
`logos.csv` says which flag or logo each robot's top-level package uses, with a `package,image` line for each
and nothing else. The images are in `logos/`, and the server reads this list again every few minutes.

## Defining a rumble

The file's name is the rumble's name, which is lowercase letters and digits. A client uploads to it by
naming its own configuration file the same, so `rumbles/autorumble.toml` goes with a client's
`roborumble/autorumble.txt`.

| Setting | Meaning |
|---|---|
| `title` | The name shown on the pages |
| `field` | The battlefield as `WIDTHxHEIGHT` |
| `rounds` | Rounds per battle |
| `melee_bots` | Robots per battle in a melee rumble. Leave it out for 1v1 |
| `hide_names` | `true` if robots only see their enemies as `#0`, `#1` and so on |
| `clear_data` | `true` if clients must delete the data robots saved before each battle |
| `participants` | Where the list comes from, like `{ file = "participants/autorumble.txt" }` |

To add a rumble, add a file here and restart the server. Its scores start from nothing, and the rumble
appears on the pages. To change a format, make a new rumble instead of editing a file, because the results
already stored were played under the old format.

## What the server discards

A result is only stored if it matches its rumble's definition. The server discards any result:

- for a rumble with no file here
- with the wrong `rounds`, `field` or melee setting
- that does not match `hide_names` or `clear_data`
- from a Robocode version that is not in `allowed_clients`
- with a robot that is not on the rumble's participant list
- more than `max_battle_age_hours` old, or from the future
- with impossible numbers, or a robot fighting itself

It tells the client that it discarded the result, so the client does not send it again.
