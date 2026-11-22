-- Ingredients: general substances, used only to group products.
CREATE TABLE ingredients (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT NOT NULL CHECK (btrim(name) <> ''),
    kind       TEXT NOT NULL CHECK (kind IN ('vitamin', 'mineral', 'herb', 'amino_acid', 'fatty_acid', 'probiotic', 'other')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX ingredients_name_key ON ingredients (lower(name));

-- Products: one brand + name + strength. Bottles of a product are interchangeable.
CREATE TABLE products (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    brand           TEXT NOT NULL CHECK (btrim(brand) <> ''),
    name            TEXT NOT NULL CHECK (btrim(name) <> ''),
    strength        TEXT NOT NULL DEFAULT '',
    unit            TEXT NOT NULL CHECK (unit IN ('capsule', 'softgel', 'tablet', 'gummy', 'scoop', 'ml', 'g', 'drop')),
    dose_units      INTEGER NOT NULL CHECK (dose_units > 0),
    dose_every_days INTEGER NOT NULL CHECK (dose_every_days > 0),
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'archived')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX products_identity_key ON products (lower(brand), lower(name), lower(strength));

CREATE TABLE product_ingredients (
    product_id    BIGINT NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    ingredient_id BIGINT NOT NULL REFERENCES ingredients (id) ON DELETE RESTRICT,
    amount        TEXT,
    PRIMARY KEY (product_id, ingredient_id)
);
CREATE INDEX product_ingredients_ingredient ON product_ingredients (ingredient_id);

-- Stretches in which a product was paused or archived. `paused_on` is paused;
-- `resumed_on` is the first active day again.
CREATE TABLE product_pauses (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    paused_on  DATE NOT NULL,
    resumed_on DATE CHECK (resumed_on >= paused_on)
);
CREATE UNIQUE INDEX product_pauses_one_open ON product_pauses (product_id) WHERE resumed_on IS NULL;

-- Bottles. initial_count / expires_on / added_on are facts; status, remaining,
-- started_on and finished_on are derived by replaying doses and bottle events
-- (docs/adr/0002-bottle-state-by-replay.md) and cached here.
CREATE TABLE bottles (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id    BIGINT NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    initial_count INTEGER NOT NULL CHECK (initial_count > 0),
    expires_on    DATE NOT NULL,
    added_on      DATE NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    status        TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'started', 'finished')),
    remaining     INTEGER NOT NULL CHECK (remaining >= 0),
    started_on    DATE,
    finished_on   DATE
);
CREATE INDEX bottles_product ON bottles (product_id);
CREATE UNIQUE INDEX bottles_one_started_per_product ON bottles (product_id) WHERE status = 'started';

-- Overrides ("start this bottle") and count corrections ("this bottle holds N").
CREATE TABLE bottle_events (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    bottle_id  BIGINT NOT NULL REFERENCES bottles (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('start', 'set_count')),
    on_date    DATE NOT NULL,
    count      INTEGER CHECK (count >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((kind = 'set_count') = (count IS NOT NULL))
);
CREATE INDEX bottle_events_bottle ON bottle_events (bottle_id);

CREATE TABLE doses (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    taken_on   DATE NOT NULL,
    units      INTEGER NOT NULL CHECK (units > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX doses_product_taken ON doses (product_id, taken_on);
