# Companion dashboard status fields

Penguin Pass can publish a few more facts on the [companion dashboard](companion-dashboard.md).
They cover a structured workout, a timed segment, drafting, fish, a
powerup, and a course ridden in reverse. Each one is optional. A
`RideStatus` payload from before these fields still parses, and the page
draws nothing extra while they are empty.

`GET /api/status` and the `event: status` stream carry the same object.
`GET /api/telemetry` is unchanged.

| Field | JSON | When the page shows it |
| --- | --- | --- |
| `workout` | object or null | A workout card: the block label, target watts, an `ERG` badge when `erg_active` is true, a `MM:SS` countdown, and a thin progress bar. |
| `segment` | object or null | A segment banner: name, elapsed time, progress, and a delta chip. Negative `delta_s` is green (ahead), positive is red (behind), and null is `FIRST TRY`. |
| `draft_saving_pct` | number or null | `DRAFT -27%` when the number is above 0. Zero and null stay hidden. |
| `fish_collected` | number or null | `FISH 12` when the count is above 0. |
| `fish_combo` | number or null | Appended as `· x4` when fish have been collected and the combo is above 0. |
| `active_powerup` | object or null | `FEATHER 14s`, `AERO 11s`, or `BURST 8s` while one is running. |
| `course_reversed` | bool, default false | A `REVERSE` pill when true. |

`workout` is `name`, `block_label`, `target_w`, `remaining_s`, `progress`
(0 to 1), and `erg_active`.

`segment` is `name`, `elapsed_s`, `progress` (0 to 1), and `delta_s`.

`active_powerup` is `name` and `remaining_s`.

Open `?preview=features` on the dashboard URL to see the cards with sample
numbers. `?preview=reconnecting` still shows a dropped stream.
