# Flags and logos

The images shown next to robot names. [config/logos.csv](../config/logos.csv) says which one each top-level
package uses, with lines like `cs,flag_usa`, and names match whatever their case. An image is a PNG of at most
64x64 pixels and 32 KB, and its name can have letters, digits and `_`. Flags are named `flag_` and a country
code, and everything else `logo_` and a name.

## Where these came from

The images are LiteRumble's `flags/*.gif` converted to PNG, and only the ones the list uses were copied.
`logo_pez` is the wiki's `MAN` code, the anarcho-capitalist flag that PEZ asked for, and `logo_wiki` is its `WIKI`
code, the United Nations flag, for wiki robots.

The list started as a copy of [RoboRumble/Country Flags](https://robowiki.net/wiki/RoboRumble/Country_Flags)
on 2026-10-31. It leaves out the three packages marked `PHL`, which has no flag image, and the two marked
`BLANK`, and gives `cs` and `chase` the US flag in place of the wiki's `JPN`.

The BSD license of this repository does not cover these images. LiteRumble does not state a license for them,
and its wiki page gives CC BY-NC-SA 3.0 for the project.
