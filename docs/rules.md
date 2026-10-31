# Entry rules

The rumbles run on the honor system, like the main RoboRumble. These rules say what a robot needs to
enter, and a reviewer checks them when a robot is added to a list.

## All rumbles

- **Hidden names.** Robots only see their enemies as `#0`, `#1` and so on, which the battle engine
  enforces. A robot can only tune against an opponent by working out who it is during the match.
- **Data limits.** The same as the main rumble: at most 200 KB of data packaged in the jar, and the normal
  200 KB data directory quota.
- A robot that only breaks on hidden names can stay in and score what it scores. It is only removed if it
  breaks battles.

## OpenRumble

- The source of every class must be in the jar, or in a public repository at a tag matching the version.
- The source must have an open source license: OSI-approved, or the RoboWiki Public Code License (RWPCL).

Older robots often do not say what their license is, so they cannot be added automatically. Each one is
checked by hand before it joins the list.

## AutoRumble

AutoRumble is for robots designed mostly by AI. Everything for OpenRumble applies, and also:

- The source of every class must be in the jar, along with a license file, so this can be checked
  automatically.
- The author declares which AI tools were used, and that AI wrote most of the code and made the key
  design decisions. People setting goals and running benchmarks is fine.
- A robot cannot be mostly a port of a robot written by a person.
- One person can have at most 3 active robots, because AI makes robots cheap and an author's own weak
  robots could inflate the score of their best one.

## Melee6

The melee rumbles use 6 robots per battle on an 800x800 field. They need at least 7 participants, because
the client will not schedule a melee battle otherwise.

## Adding a robot

Add one line to the rumble's list in `participants/`, with a comment above it recording the checks:

```
# license=zlib source=jar ai="Claude, all code and design" by=Chase-san
claude.Umbra 0.5,https://example.com/claude.Umbra_0.5.jar
```

The name and version must match the jar's `robot.properties`, or clients will reject the download.
