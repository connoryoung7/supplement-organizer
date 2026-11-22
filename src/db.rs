//! Every SQL statement in the app. Reads take the pool; writes take a transaction's
//! connection so they compose with `app::Store::replay_product`.

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgConnection, PgPool};

pub type DbResult<T> = Result<T, sqlx::Error>;

pub struct ProductRow {
    pub id: i64,
    pub brand: String,
    pub name: String,
    pub strength: String,
    pub unit: String,
    pub dose_units: i32,
    pub dose_every_days: i32,
    pub status: String,
}

pub struct BottleRow {
    pub id: i64,
    pub product_id: i64,
    pub initial_count: i32,
    pub expires_on: NaiveDate,
    pub added_on: NaiveDate,
    pub status: String,
    pub remaining: i32,
    pub started_on: Option<NaiveDate>,
    pub finished_on: Option<NaiveDate>,
}

pub struct DoseRow {
    pub id: i64,
    pub product_id: i64,
    pub taken_on: NaiveDate,
    pub units: i32,
    pub created_at: DateTime<Utc>,
}

pub struct DoseBounds {
    pub product_id: i64,
    pub first: NaiveDate,
    pub last: NaiveDate,
}

pub struct PauseRow {
    pub product_id: i64,
    pub paused_on: NaiveDate,
    pub resumed_on: Option<NaiveDate>,
}

pub struct BottleEventRow {
    pub id: i64,
    pub bottle_id: i64,
    pub kind: String,
    pub on_date: NaiveDate,
    pub count: Option<i32>,
    pub created_at: DateTime<Utc>,
}

pub struct ProductIngredientRow {
    pub product_id: i64,
    pub ingredient_id: i64,
    pub name: String,
    pub kind: String,
    pub amount: Option<String>,
}

pub struct IngredientRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub product_count: i64,
}

pub struct ProductInput<'a> {
    pub brand: &'a str,
    pub name: &'a str,
    pub strength: &'a str,
    pub unit: &'a str,
    pub dose_units: i32,
    pub dose_every_days: i32,
}

// ---- Reads ---------------------------------------------------------------

pub async fn products(pool: &PgPool) -> DbResult<Vec<ProductRow>> {
    sqlx::query_as!(
        ProductRow,
        "SELECT id, brand, name, strength, unit, dose_units, dose_every_days, status
         FROM products ORDER BY lower(name), lower(brand), lower(strength)"
    )
    .fetch_all(pool)
    .await
}

pub async fn bottles(pool: &PgPool) -> DbResult<Vec<BottleRow>> {
    sqlx::query_as!(
        BottleRow,
        "SELECT id, product_id, initial_count, expires_on, added_on, status, remaining, started_on, finished_on
         FROM bottles ORDER BY expires_on, added_on, id"
    )
    .fetch_all(pool)
    .await
}

pub async fn doses_since(pool: &PgPool, since: NaiveDate) -> DbResult<Vec<DoseRow>> {
    sqlx::query_as!(
        DoseRow,
        "SELECT id, product_id, taken_on, units, created_at FROM doses
         WHERE taken_on >= $1 ORDER BY taken_on, created_at",
        since
    )
    .fetch_all(pool)
    .await
}

pub async fn dose_bounds(pool: &PgPool) -> DbResult<Vec<DoseBounds>> {
    sqlx::query_as!(
        DoseBounds,
        r#"SELECT product_id, min(taken_on) AS "first!", max(taken_on) AS "last!"
           FROM doses GROUP BY product_id"#
    )
    .fetch_all(pool)
    .await
}

pub async fn pauses(pool: &PgPool) -> DbResult<Vec<PauseRow>> {
    sqlx::query_as!(
        PauseRow,
        "SELECT product_id, paused_on, resumed_on FROM product_pauses"
    )
    .fetch_all(pool)
    .await
}

pub async fn recent_doses(pool: &PgPool, product_id: i64, limit: i64) -> DbResult<Vec<DoseRow>> {
    sqlx::query_as!(
        DoseRow,
        "SELECT id, product_id, taken_on, units, created_at FROM doses
         WHERE product_id = $1 ORDER BY taken_on DESC, created_at DESC LIMIT $2",
        product_id,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn dose(pool: &PgPool, id: i64) -> DbResult<Option<DoseRow>> {
    sqlx::query_as!(
        DoseRow,
        "SELECT id, product_id, taken_on, units, created_at FROM doses WHERE id = $1",
        id
    )
    .fetch_optional(pool)
    .await
}

pub async fn recent_bottle_events(
    pool: &PgPool,
    product_id: i64,
    limit: i64,
) -> DbResult<Vec<BottleEventRow>> {
    sqlx::query_as!(
        BottleEventRow,
        "SELECT e.id, e.bottle_id, e.kind, e.on_date, e.count, e.created_at
         FROM bottle_events e JOIN bottles b ON b.id = e.bottle_id
         WHERE b.product_id = $1 ORDER BY e.on_date DESC, e.created_at DESC LIMIT $2",
        product_id,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn product_ingredients(pool: &PgPool) -> DbResult<Vec<ProductIngredientRow>> {
    sqlx::query_as!(
        ProductIngredientRow,
        "SELECT pi.product_id, pi.ingredient_id, i.name, i.kind, pi.amount
         FROM product_ingredients pi JOIN ingredients i ON i.id = pi.ingredient_id
         ORDER BY lower(i.name)"
    )
    .fetch_all(pool)
    .await
}

pub async fn ingredients(pool: &PgPool) -> DbResult<Vec<IngredientRow>> {
    sqlx::query_as!(
        IngredientRow,
        r#"SELECT i.id AS "id!", i.name AS "name!", i.kind AS "kind!", count(pi.product_id) AS "product_count!"
           FROM ingredients i LEFT JOIN product_ingredients pi ON pi.ingredient_id = i.id
           GROUP BY i.id ORDER BY lower(i.name)"#
    )
    .fetch_all(pool)
    .await
}

/// Which product a bottle, dose or bottle event belongs to.
pub async fn product_of_bottle(pool: &PgPool, bottle_id: i64) -> DbResult<Option<i64>> {
    sqlx::query_scalar!("SELECT product_id FROM bottles WHERE id = $1", bottle_id)
        .fetch_optional(pool)
        .await
}

pub async fn product_of_dose(pool: &PgPool, dose_id: i64) -> DbResult<Option<i64>> {
    sqlx::query_scalar!("SELECT product_id FROM doses WHERE id = $1", dose_id)
        .fetch_optional(pool)
        .await
}

pub async fn product_of_bottle_event(pool: &PgPool, event_id: i64) -> DbResult<Option<i64>> {
    sqlx::query_scalar!(
        "SELECT b.product_id FROM bottle_events e JOIN bottles b ON b.id = e.bottle_id WHERE e.id = $1",
        event_id
    )
    .fetch_optional(pool)
    .await
}

// ---- Replay inputs (inside a transaction) --------------------------------

/// Locks the product row so concurrent changes to one product replay one at a time.
pub async fn lock_product(conn: &mut PgConnection, product_id: i64) -> DbResult<bool> {
    let found = sqlx::query_scalar!(
        "SELECT id FROM products WHERE id = $1 FOR UPDATE",
        product_id
    )
    .fetch_optional(conn)
    .await?;
    Ok(found.is_some())
}

pub struct BottleSpecRow {
    pub id: i64,
    pub initial_count: i32,
    pub expires_on: NaiveDate,
    pub added_on: NaiveDate,
}

pub async fn bottle_specs(
    conn: &mut PgConnection,
    product_id: i64,
) -> DbResult<Vec<BottleSpecRow>> {
    sqlx::query_as!(
        BottleSpecRow,
        "SELECT id, initial_count, expires_on, added_on FROM bottles WHERE product_id = $1",
        product_id
    )
    .fetch_all(conn)
    .await
}

pub async fn product_doses(conn: &mut PgConnection, product_id: i64) -> DbResult<Vec<DoseRow>> {
    sqlx::query_as!(
        DoseRow,
        "SELECT id, product_id, taken_on, units, created_at FROM doses WHERE product_id = $1",
        product_id
    )
    .fetch_all(conn)
    .await
}

pub async fn product_bottle_events(
    conn: &mut PgConnection,
    product_id: i64,
) -> DbResult<Vec<BottleEventRow>> {
    sqlx::query_as!(
        BottleEventRow,
        "SELECT e.id, e.bottle_id, e.kind, e.on_date, e.count, e.created_at
         FROM bottle_events e JOIN bottles b ON b.id = e.bottle_id WHERE b.product_id = $1",
        product_id
    )
    .fetch_all(conn)
    .await
}

pub async fn save_bottle_state(
    conn: &mut PgConnection,
    id: i64,
    status: &str,
    remaining: i32,
    started_on: Option<NaiveDate>,
    finished_on: Option<NaiveDate>,
) -> DbResult<()> {
    sqlx::query!(
        "UPDATE bottles SET status = $2, remaining = $3, started_on = $4, finished_on = $5 WHERE id = $1",
        id,
        status,
        remaining,
        started_on,
        finished_on
    )
    .execute(conn)
    .await?;
    Ok(())
}

// ---- Writes ---------------------------------------------------------------

pub async fn insert_product(conn: &mut PgConnection, p: &ProductInput<'_>) -> DbResult<i64> {
    sqlx::query_scalar!(
        "INSERT INTO products (brand, name, strength, unit, dose_units, dose_every_days)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        p.brand,
        p.name,
        p.strength,
        p.unit,
        p.dose_units,
        p.dose_every_days
    )
    .fetch_one(conn)
    .await
}

pub async fn update_product(
    conn: &mut PgConnection,
    id: i64,
    p: &ProductInput<'_>,
) -> DbResult<()> {
    sqlx::query!(
        "UPDATE products SET brand = $2, name = $3, strength = $4, unit = $5, dose_units = $6, dose_every_days = $7
         WHERE id = $1",
        id,
        p.brand,
        p.name,
        p.strength,
        p.unit,
        p.dose_units,
        p.dose_every_days
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn delete_product(conn: &mut PgConnection, id: i64) -> DbResult<()> {
    sqlx::query!("DELETE FROM products WHERE id = $1", id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn set_product_status(conn: &mut PgConnection, id: i64, status: &str) -> DbResult<()> {
    sqlx::query!("UPDATE products SET status = $2 WHERE id = $1", id, status)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn open_pause(conn: &mut PgConnection, product_id: i64, on: NaiveDate) -> DbResult<()> {
    sqlx::query!(
        "INSERT INTO product_pauses (product_id, paused_on) VALUES ($1, $2)
         ON CONFLICT (product_id) WHERE resumed_on IS NULL DO NOTHING",
        product_id,
        on
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn close_pause(conn: &mut PgConnection, product_id: i64, on: NaiveDate) -> DbResult<()> {
    sqlx::query!(
        "UPDATE product_pauses SET resumed_on = greatest($2, paused_on)
         WHERE product_id = $1 AND resumed_on IS NULL",
        product_id,
        on
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn insert_bottle(
    conn: &mut PgConnection,
    product_id: i64,
    count: i32,
    expires_on: NaiveDate,
    added_on: NaiveDate,
) -> DbResult<i64> {
    sqlx::query_scalar!(
        "INSERT INTO bottles (product_id, initial_count, expires_on, added_on, remaining)
         VALUES ($1, $2, $3, $4, $2) RETURNING id",
        product_id,
        count,
        expires_on,
        added_on
    )
    .fetch_one(conn)
    .await
}

pub async fn update_bottle(
    conn: &mut PgConnection,
    id: i64,
    count: i32,
    expires_on: NaiveDate,
) -> DbResult<()> {
    sqlx::query!(
        "UPDATE bottles SET initial_count = $2, expires_on = $3 WHERE id = $1",
        id,
        count,
        expires_on
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn delete_bottle(conn: &mut PgConnection, id: i64) -> DbResult<()> {
    sqlx::query!("DELETE FROM bottles WHERE id = $1", id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn insert_bottle_event(
    conn: &mut PgConnection,
    bottle_id: i64,
    kind: &str,
    on: NaiveDate,
    count: Option<i32>,
) -> DbResult<()> {
    sqlx::query!(
        "INSERT INTO bottle_events (bottle_id, kind, on_date, count) VALUES ($1, $2, $3, $4)",
        bottle_id,
        kind,
        on,
        count
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn delete_bottle_event(conn: &mut PgConnection, id: i64) -> DbResult<()> {
    sqlx::query!("DELETE FROM bottle_events WHERE id = $1", id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn insert_dose(
    conn: &mut PgConnection,
    product_id: i64,
    on: NaiveDate,
    units: i32,
) -> DbResult<()> {
    sqlx::query!(
        "INSERT INTO doses (product_id, taken_on, units) VALUES ($1, $2, $3)",
        product_id,
        on,
        units
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn update_dose(
    conn: &mut PgConnection,
    id: i64,
    on: NaiveDate,
    units: i32,
) -> DbResult<()> {
    sqlx::query!(
        "UPDATE doses SET taken_on = $2, units = $3 WHERE id = $1",
        id,
        on,
        units
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn delete_dose(conn: &mut PgConnection, id: i64) -> DbResult<()> {
    sqlx::query!("DELETE FROM doses WHERE id = $1", id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn upsert_ingredient(conn: &mut PgConnection, name: &str, kind: &str) -> DbResult<i64> {
    sqlx::query_scalar!(
        "INSERT INTO ingredients (name, kind) VALUES ($1, $2)
         ON CONFLICT ((lower(name))) DO UPDATE SET name = ingredients.name
         RETURNING id",
        name,
        kind
    )
    .fetch_one(conn)
    .await
}

pub async fn link_ingredient(
    conn: &mut PgConnection,
    product_id: i64,
    ingredient_id: i64,
    amount: Option<&str>,
) -> DbResult<()> {
    sqlx::query!(
        "INSERT INTO product_ingredients (product_id, ingredient_id, amount) VALUES ($1, $2, $3)
         ON CONFLICT (product_id, ingredient_id) DO UPDATE SET amount = EXCLUDED.amount",
        product_id,
        ingredient_id,
        amount
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn unlink_ingredient(
    conn: &mut PgConnection,
    product_id: i64,
    ingredient_id: i64,
) -> DbResult<()> {
    sqlx::query!(
        "DELETE FROM product_ingredients WHERE product_id = $1 AND ingredient_id = $2",
        product_id,
        ingredient_id
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Deletes an ingredient only if no product uses it. Returns whether it was deleted.
pub async fn delete_unused_ingredient(conn: &mut PgConnection, id: i64) -> DbResult<bool> {
    let done = sqlx::query!(
        "DELETE FROM ingredients WHERE id = $1
         AND NOT EXISTS (SELECT 1 FROM product_ingredients WHERE ingredient_id = $1)",
        id
    )
    .execute(conn)
    .await?;
    Ok(done.rows_affected() > 0)
}
