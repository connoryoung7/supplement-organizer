# Design

What the app does and the rules it follows. Vocabulary is defined in [`CONTEXT.md`](../CONTEXT.md); the reasoning behind the less obvious choices is in [`docs/adr/`](./adr/).

## Goal

Minimise the number of **expired bottles**: bottles whose expiry date passes while they still hold units. The app does this by keeping every product's bottles in the right order, and by flagging, as early as the arithmetic allows, the bottles that are going to expire anyway — so you can decide what to do (take more, give it away, stop buying).

The app only orders and warns. It never suggests a different dose ([ADR 0001](./adr/0001-earliest-expiry-ordering.md)).

## Model

```
Ingredient ──< ProductIngredient >── Product ──< Bottle
                (optional amount)       │
                                        ├──< Dose
                                        └──< Pause
Bottle ──< BottleEvent   (start override | count correction)
```

| Entity | Holds |
|---|---|
| **Ingredient** | name, kind (vitamin, mineral, herb, amino acid, fatty acid, probiotic, other) |
| **Product** | brand, name, strength, unit, schedule (X units every Y days), status (active, paused, archived) |
| **ProductIngredient** | which ingredients a product contains, with an optional free-text amount. Grouping only. |
| **Bottle** | initial count, expiry date, date added. Status, remaining units, started/finished dates are *derived* (below). |
| **Dose** | product, date taken, units |
| **BottleEvent** | an override (start this bottle) or a count correction (this bottle holds N), dated |
| **Pause** | a date range in which a product was paused or archived |

## Rules

### Bottle lifecycle

- A bottle is **new → started → finished**. "Expired" is not a status; it is derived from dates.
- A product has **at most one started bottle**.
- A dose is drawn from the started bottle. If it empties the bottle, the bottle is finished and the rest of the dose comes out of the next bottle.
- When a bottle finishes, the next bottle — **the new bottle with the earliest expiry date** (ties: oldest first) — starts automatically.
- **Override**: any new bottle can be started instead. If another bottle is already started, it goes back to new, which is only allowed if no dose has been drawn from it.
- **Count correction** on a started bottle sets its remaining units; zero finishes it early. A correction above zero on a finished bottle re-opens it, which pushes the bottle that had started after it back to new (only allowed if nothing has been drawn from that one yet).
- A bottle can be added already started, with its current count, if the product has no started bottle.
- A dose can't draw from a bottle that hadn't been added yet on the dose's date. Doses dated before *any* bottle of the product was added (backfilled history from before you started tracking) count toward the actual rate but draw from no bottle — an already-open bottle's count already reflects them.

Bottle state is computed by **replaying** the product's doses and bottle events in date order ([ADR 0002](./adr/0002-bottle-state-by-replay.md)). Every change — logging, editing or deleting a dose, overriding, correcting, adding or deleting a bottle — replays the product, and is rejected if the replay breaks a rule above (for example, deleting a bottle that later doses were drawn from, with nothing else to draw from instead).

### Rate

- **Planned rate** = X ÷ Y units per day.
- **Actual rate** = units taken ÷ active days, over the 30 days ending yesterday, counting only days on or after the first dose that were not paused. It is used once there are at least 14 such days.
- **Effective rate** = 0 if the product is paused or archived; otherwise the actual rate if available, else the planned rate.

### Projection and at-risk

For each product, walk the queue — started bottle first, then new bottles by expiry date — consuming each at the effective rate, starting today. Each bottle gets a projected start and finish date. A bottle is **at risk** when it would still hold units at the end of its expiry date; the app shows how many. Bottles of paused and archived products are always at risk (rate 0) and are labelled as such. No time-based "expiring soon" flag, and no "rate needed to finish in time".

### Due

A product is due today when it is active and either has never been dosed or its last dose was at least Y days ago.

### Scorecard

Counts expired bottles (expiry date passed while units remained — including bottles finished *after* expiry), all-time and for the last 12 months, against bottles finished on time.

## Screens

| Screen | Purpose |
|---|---|
| **Today** (`/`) | Products due today, each with one tap to log its planned dose; doses logged today with undo; what's coming up; at-risk count |
| **Products** (`/products`) | Every product with bottles on hand, rate and risk; filter by ingredient |
| **Product** (`/products/{id}`) | Queue with projections; add bottles; override, correct, finish, edit, delete bottles; log a dose with any amount and date; edit or delete past doses; ingredients; pause, resume, archive |
| **At risk** (`/at-risk`) | Every at-risk bottle across products, soonest expiry first; scorecard |
| **Ingredients** (`/ingredients`) | Ingredients by kind, linking to the products that contain them |

The UI is server-rendered (Askama templates, htmx `hx-boost`), mobile-first, and installable to a phone's home screen. Every action is a plain form post, so it works without JavaScript. No notifications.

## Operations

- **Storage**: Postgres via `sqlx`, connection from `DATABASE_URL`. Migrations are embedded and run at startup. Queries are compile-time checked; offline metadata lives in `.sqlx/` (regenerate with `just prepare`).
- **"Today"** is the calendar date in the `TZ` environment variable's time zone (IANA name, default `UTC`).
- **Access**: the app has no login. It must be deployed behind Tailscale or Dokploy basic auth ([ADR 0003](./adr/0003-no-in-app-auth.md)).
- **Backups**: Dokploy's scheduled backups of the Postgres service.
