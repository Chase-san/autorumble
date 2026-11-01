# Configuration

`server.toml` has the server's own settings. `rumbles/` defines the rumbles, with one file for each.
`logos.csv` says which flag or logo each robot's top-level package uses, with a `package,image` line for each
and nothing else. Names match whatever their case. The images are in `logos/`, and the server reads this list
again every few minutes.

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
| `participants` | Where the list comes from, as below |

To add a rumble, add a file here and restart the server. Its scores start from nothing, and the rumble
appears on the pages. To change a format, make a new rumble instead of editing a file, because the results
already stored were played under the old format.

## Where a participant list comes from

A list has one `name version,url` line per robot. It can be a file, or a page on a web server:

```toml
participants = { file = "participants/openrumble.txt" }
```

```toml
participants = { url = "https://robowiki.net/wiki/RoboRumble/Participants?action=raw" }
```

On a page, only the lines between `<pre>` and `</pre>` count. To use other tags, set `open` and `close`, which can
be any text. That lets one page hold the lists of several rumbles:

```toml
participants = { url = "https://example.com/lists", open = "<!-- openrumble -->", close = "<!-- /openrumble -->" }
```

The server only fetches a page while clients are active, which means uploading results or downloading a list. With
no clients, it sends no requests at all, apart from one at startup if a rumble has no saved list yet. It fetches each
page at most once a minute, however many rumbles use it. To fetch less often, raise `participants_seconds`.

If a page cannot be fetched, has lost its tags or has no robots, the rumble keeps the list it has. The last list
fetched is saved next to the database, so restarting does not need the site to be up. Until a rumble's first list
arrives, clients hold on to their results and send them again later.

Clients always download the list from this server, so they do not need to know where it came from.

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
