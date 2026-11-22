# Supplement Organizer

Tracks the supplement bottles one person owns, the doses they take from them, and which bottles are on course to expire before they are finished — so that as few bottles as possible expire.

## Language

### Things you own

**Ingredient**:
A general substance a product contains, such as Magnesium or Vitamin D3. Used only for grouping products.
_Avoid_: Supplement (for this level), nutrient, compound

**Ingredient kind**:
The broad family an ingredient belongs to: vitamin, mineral, herb, amino acid, fatty acid, probiotic, or other.
_Avoid_: Category, type

**Product**:
A specific supplement as sold — one brand, one name, one strength — containing zero or more ingredients. Bottles of the same product are interchangeable; bottles of different products never are.
_Avoid_: Supplement (ambiguous), item, SKU

**Bottle**:
One physical container of a product, holding a whole number of units and carrying an expiry date.
_Avoid_: Instance, container, jar, pack

**Unit**:
The countable thing a product is measured in — capsule, softgel, tablet, gummy, scoop, ml, g, or drop. Counts and doses are whole numbers of units.
_Avoid_: Serving, pill

**Expiry date**:
The last day a bottle is at full efficacy. A label that gives only a month means the last day of that month.
_Avoid_: Best-by, use-by

### Taking supplements

**Dose**:
A recorded number of units of a product taken on a given day.
_Avoid_: Log, intake, serving

**Schedule**:
How much of a product you plan to take, expressed as "X units every Y days".
_Avoid_: Regimen, plan

**Due**:
A product is due when its schedule's interval has passed since its last dose.

**Planned rate**:
Units per day implied by the schedule.

**Actual rate**:
Units per day observed from doses over the last 30 non-paused days, available once there are about two weeks of history.

**Paused**:
A product you have temporarily stopped taking. It is never due, and paused days do not count toward its actual rate.
_Avoid_: Inactive, on hold

**Archived**:
A product you have stopped taking for good. It is shown only while it still has unfinished bottles.
_Avoid_: Deleted, discontinued

### Bottle lifecycle

**New**:
A bottle that has not been opened.
_Avoid_: Unopened, sealed

**Started**:
The one open bottle of a product that doses come out of. A product has at most one started bottle.
_Avoid_: Open, active, current

**Finished**:
A bottle that is empty. Finishing is the only way a bottle leaves the queue — expired bottles are still taken until finished.
_Avoid_: Used up, done, discarded

**Queue**:
A product's started bottle followed by its new bottles in expiry-date order — the order they will be taken.

**Override**:
Starting a different new bottle than the one the queue would open next, to match what was physically opened.
_Avoid_: Swap, manual start

**Count correction**:
Setting a bottle's remaining units to what is actually in it. A correction to zero finishes it early; a correction above zero on a finished bottle re-opens it.
_Avoid_: Adjustment, recount

### Expiry

**Expired bottle**:
A bottle whose expiry date passed while it still had units in it. Minimising expired bottles is the goal of the whole app.
_Avoid_: Wasted bottle, bad bottle

**Projection**:
The predicted start and finish date of each bottle in a product's queue, at the product's rate.
_Avoid_: Forecast, estimate

**At risk**:
A bottle whose projection finishes after its expiry date, with the number of units that would be left at expiry.
_Avoid_: Expiring soon, warning
