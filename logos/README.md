# Flags and logos

The images shown next to robot names. [config/logos.csv](../config/logos.csv) says which one each top-level
package uses, with lines like `cs,flag_usa`. An image is a PNG of at most 64x64 pixels and 32 KB, and its
name can have letters, digits and `_`. Flags are named `flag_` and a country code, and anything else is an
author's own logo.

## Where these came from

The `flag_*.png` files are LiteRumble's `flags/*.gif` converted to PNG. `flag_man` is the anarcho-capitalist
flag and `flag_wiki` the United Nations flag, which the RoboWiki uses for robots with no country and for wiki
robots. Only the flags that the list uses were copied.

The list started as a copy of [RoboRumble/Country Flags](https://robowiki.net/wiki/RoboRumble/Country_Flags)
on 2026-10-31. It leaves out the three packages marked `PHL`, which has no flag image, and the two marked
`BLANK`, and gives `cs` and `chase` the US flag in place of the wiki's `JPN`.

The BSD license of this repository does not cover these flags. LiteRumble does not state a license for them,
and its wiki page gives CC BY-NC-SA 3.0 for the project.
