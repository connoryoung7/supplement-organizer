# Bottle state is derived by replaying doses and bottle events

A bottle's status, remaining units and start/finish dates are not edited directly. They are recomputed by replaying the product's doses, overrides and count corrections in date order, and the result is cached on the `bottles` row (where a partial unique index enforces one started bottle per product). We chose this because doses can be backdated, edited and deleted: mutating stored counts would require hand-written reversal logic for every edit — un-finishing a bottle, un-starting the next one — and that logic is where counts silently drift.

## Consequences

- Any change that would make the history inconsistent (e.g. deleting a bottle that later doses were drawn from) is rejected by the replay, not by ad-hoc checks.
- Auto-start only considers bottles that had been added by the date in question, so adding a bottle later never rewrites which bottle was opened in the past.
