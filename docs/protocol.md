# The upload protocol

These are the routes the Robocode client uses. The first three match LiteRumble, so a standard client
only needs its URLs changed.

## A result

One line of `key=value` pairs joined by `&`. A melee battle sends one result for each pair of robots.
The values are not URL-encoded, so robot names contain plain spaces.

```
game=autorumble&version=1&client=1.11.2&teams=NOT&melee=NOT&rounds=35&field=800x600&user=Chase
&time=1793491199000&fname=a.One 1.0&fscore=3000&fbulletd=900&fsurvival=20
&sname=b.Two 2.0&sscore=1000&sbulletd=300&ssurvival=15&hidenames=YES
```

(Shown on three lines here. It is one line in the request.)

| Field | Meaning |
|---|---|
| `game` | The rumble, which is the name of the client's configuration file |
| `version` | The upload format, always `1` |
| `client` | The Robocode version |
| `teams`, `melee` | `YES` or `NOT` |
| `rounds`, `field` | The battle's format, like `35` and `800x600` |
| `user` | Who ran the battle |
| `time` | When the battle ended, in milliseconds since the Unix epoch |
| `fname`, `sname` | The two robots |
| `fscore`, `sscore` | Their scores |
| `fbulletd`, `sbulletd` | Their bullet damage |
| `fsurvival`, `ssurvival` | The rounds they finished first |
| `hidenames` | `YES` if enemy names were hidden from the robots |

The server rejects a result when the settings do not match the rumble, when a robot is not on the
participant list, or when the battle is more than a day old.

## POST /UploadedResults

One result per request. The reply has lines of text:

- A line containing `OK` means the client is finished with this result: it was stored, it was stored
  before, or it was rejected (`OK. ERROR: reason`).
- Any other status means the result was not stored for now, so the client keeps it and sends it again
  with its next upload (`RETRY: reason`).
- A line like `[a.One_1.0,b.Two_2.0]` is a priority battle for the client to run soon.

## POST /UploadedResultsBatch

Many results in one request, one per line, up to 1000 lines. A larger request gets HTTP status 413.

The reply has exactly one status line per result, in the same order:

| Status | Meaning | What the client does |
|---|---|---|
| `OK text` | Stored, or stored before | Forgets the result |
| `REJECT reason` | Will never be accepted | Logs the reason and forgets the result |
| `RETRY reason` | Not stored for now | Sends the result again later |

Priority battles follow as lines like `[a.One_1.0,b.Two_2.0]`. A client treats a missing or unknown
status as `RETRY`, and any error response as `RETRY` for the whole request. This is safe because a
result sent twice is only stored once.

## GET /RatingsFile?version=1&game=NAME

One line per robot with battles: `a.One_1.0=APS,battles,latest battle`.

## GET /Participants?game=NAME

The participant list between `<pre>` tags, for the client's `PARTICIPANTSURL`.

## GET or POST /RemoveOldParticipant

Answers `OK` and does nothing. Standard clients call this for robots they no longer see on the list,
but here the server's own list decides, so a client with an old list cannot retire anyone.

## Other routes

| Route | What it is |
|---|---|
| `GET /` | The list of rumbles |
| `GET /Rankings?game=NAME` | The rankings. Takes `order`, `limit` and `api=1` for JSON |
| `GET /BotDetails?game=NAME&name=ROBOT` | One robot's pairings. Takes `order` and `api=1` for JSON |
| `GET /BotCompare?game=NAME&bota=ROBOT&botb=ROBOT` | Two robots side by side. Takes `order` |
| `GET /RumbleStats` | Uploads by rumble and by uploader |
| `GET /ScoreExplanation` | What each score means |
| `GET /logos/PACKAGE.png` | An author's logo |
