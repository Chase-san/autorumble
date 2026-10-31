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

- The source of every class must be in the jar.
- The robot must have a permissive license, stated in the jar or on its RoboWiki page. The wiki page is
  taken as true unless the jar says something that conflicts with it, and a conflict goes to a reviewer.
- A robot without any license is out, even with its source in the jar. Its license cannot be assumed, even
  if it took part in earlier competitions.

| Licenses | Ruling |
|---|---|
| MIT, MIT-0, BSD, zlib, public domain, RWPCL, RWLPCL, and others of the same kind, such as Apache, ISC, CC0, the Unlicense, WTFPL, CC-BY and "Attribution" | Allowed |
| Share-alike licenses with no condition on how a robot competes: GPL, LGPL, EPL, CC-BY-SA | Allowed |
| Permission in the author's own words, such as "use the code as you see fit" | Allowed, once a reviewer has read it |
| The Kawigi Public License (KPL), and the Nat Public License (NPL, also NPRL or NRPL) | Not allowed, because of conditions such as derived robots not being allowed to beat the original |
| Any other condition on how a robot can compete, such as a ban on competitions | Not allowed |
| CC-BY-ND and CC-BY-NC, which do not allow reuse in general | Not allowed |
| Only the words "Open Source", or a term nothing defines | Not a license |
| ACCSL | No ruling yet, and no robot uses it |

A license counts for a robot when it covers the robot's own code: in its main class, a license file, or
at least half of its source files. Someone else's license on one borrowed file does not license the whole
robot, except for the RWPCL, which requires a robot using its code to be released under it. A license that
is not allowed counts anywhere in the jar, because its conditions apply to any robot using that code.

The source of a library a robot carries can be missing if the robot's own source is complete and the
library's source is public.

`tools/license_census.py` sorts robots by these rules and records the evidence for each one, and
`participants/review/` holds the results and a reviewer's rulings.

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
