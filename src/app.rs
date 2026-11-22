//! Application layer: builds the read model from the database, and runs every change
//! inside a transaction that ends by replaying the affected product.

use std::collections::HashMap;

use anyhow::anyhow;
use chrono::{Days, NaiveDate, Utc};
use chrono_tz::Tz;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

use crate::db::{self, BottleRow, DoseBounds, ProductIngredientRow, ProductInput, ProductRow};
use crate::domain::projection::{self, Projection, QueueEntry};
use crate::domain::rate::{self, Pause};
use crate::domain::replay::{self, BottleSpec, Event, EventKind, ReplayError};
use crate::domain::{BottleStatus, ProductStatus, Schedule};

pub const UNITS: [&str; 8] = [
    "capsule", "softgel", "tablet", "gummy", "scoop", "ml", "g", "drop",
];
pub const KINDS: [(&str, &str); 7] = [
    ("vitamin", "Vitamin"),
    ("mineral", "Mineral"),
    ("herb", "Herb"),
    ("amino_acid", "Amino acid"),
    ("fatty_acid", "Fatty acid"),
    ("probiotic", "Probiotic"),
    ("other", "Other"),
];

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    /// A rule the request broke; the message is shown to the user.
    #[error("{0}")]
    Rule(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

impl From<ReplayError> for AppError {
    fn from(e: ReplayError) -> Self {
        Self::Rule(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn rule<T>(msg: impl Into<String>) -> AppResult<T> {
    Err(AppError::Rule(msg.into()))
}

// ---- Read model -------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Product {
    pub id: i64,
    pub brand: String,
    pub name: String,
    pub strength: String,
    pub unit: String,
    pub schedule: Schedule,
    pub status: ProductStatus,
}

#[derive(Debug, Clone)]
pub struct Bottle {
    pub id: i64,
    pub initial_count: u32,
    pub expires_on: NaiveDate,
    pub added_on: NaiveDate,
    pub status: BottleStatus,
    pub remaining: u32,
    pub started_on: Option<NaiveDate>,
    pub finished_on: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
pub struct ProductIngredient {
    pub ingredient_id: i64,
    pub name: String,
    pub kind: String,
    pub amount: Option<String>,
}

/// Everything the screens need to know about one product, as of `today`.
#[derive(Debug, Clone)]
pub struct Summary {
    pub product: Product,
    /// Every bottle, in expiry order.
    pub bottles: Vec<Bottle>,
    /// Started bottle, then new bottles in the order they will be opened.
    pub queue: Vec<(Bottle, Projection)>,
    pub actual_rate: Option<f64>,
    pub last_dose: Option<NaiveDate>,
    /// `(dose id, units)` for doses dated today, oldest first.
    pub doses_today: Vec<(i64, u32)>,
    pub ingredients: Vec<ProductIngredient>,
}

impl Summary {
    pub fn at_risk(&self) -> impl Iterator<Item = &(Bottle, Projection)> {
        self.queue.iter().filter(|(_, p)| p.at_risk())
    }

    pub fn is_due(&self, today: NaiveDate) -> bool {
        self.product.status == ProductStatus::Active
            && self.product.schedule.is_due(self.last_dose, today)
    }
}

fn parse_status<T>(s: &str, parse: fn(&str) -> Option<T>) -> AppResult<T> {
    parse(s).ok_or_else(|| AppError::Internal(anyhow!("unknown status {s:?} in database")))
}

fn non_negative(n: i32) -> u32 {
    u32::try_from(n).unwrap_or(0)
}

fn to_bottle(b: BottleRow) -> AppResult<Bottle> {
    Ok(Bottle {
        id: b.id,
        initial_count: non_negative(b.initial_count),
        expires_on: b.expires_on,
        added_on: b.added_on,
        status: parse_status(&b.status, BottleStatus::parse)?,
        remaining: non_negative(b.remaining),
        started_on: b.started_on,
        finished_on: b.finished_on,
    })
}

struct ProductData {
    bottles: Vec<BottleRow>,
    doses: Vec<db::DoseRow>,
    pauses: Vec<Pause>,
    ingredients: Vec<ProductIngredientRow>,
}

fn summarize(
    p: ProductRow,
    data: ProductData,
    bounds: Option<&DoseBounds>,
    today: NaiveDate,
) -> AppResult<Summary> {
    let product = Product {
        id: p.id,
        brand: p.brand,
        name: p.name,
        strength: p.strength,
        unit: p.unit,
        schedule: Schedule {
            units: non_negative(p.dose_units),
            every_days: non_negative(p.dose_every_days).max(1),
        },
        status: parse_status(&p.status, ProductStatus::parse)?,
    };
    let bottles = data
        .bottles
        .into_iter()
        .map(to_bottle)
        .collect::<AppResult<Vec<_>>>()?;

    let doses: Vec<(NaiveDate, u32)> = data
        .doses
        .iter()
        .map(|d| (d.taken_on, non_negative(d.units)))
        .collect();
    let actual_rate = rate::actual_rate(bounds.map(|b| b.first), &doses, &data.pauses, today);
    let effective_rate = rate::effective_rate(product.status, product.schedule, actual_rate);

    let entries = bottles
        .iter()
        .map(|b| QueueEntry {
            id: b.id,
            status: b.status,
            remaining: b.remaining,
            expires_on: b.expires_on,
            added_on: b.added_on,
        })
        .collect();
    let order = projection::queue(entries);
    let projections = projection::project(&order, effective_rate, today);
    let queue = projections
        .into_iter()
        .filter_map(|p| {
            bottles
                .iter()
                .find(|b| b.id == p.id)
                .map(|b| (b.clone(), p))
        })
        .collect();

    Ok(Summary {
        product,
        bottles,
        queue,
        actual_rate,
        last_dose: bounds.map(|b| b.last),
        doses_today: data
            .doses
            .iter()
            .filter(|d| d.taken_on == today)
            .map(|d| (d.id, non_negative(d.units)))
            .collect(),
        ingredients: data
            .ingredients
            .into_iter()
            .map(|i| ProductIngredient {
                ingredient_id: i.ingredient_id,
                name: i.name,
                kind: i.kind,
                amount: i.amount,
            })
            .collect(),
    })
}

fn group<T>(rows: Vec<T>, key: impl Fn(&T) -> i64) -> HashMap<i64, Vec<T>> {
    let mut map: HashMap<i64, Vec<T>> = HashMap::new();
    for row in rows {
        map.entry(key(&row)).or_default().push(row);
    }
    map
}

// ---- Store --------------------------------------------------------------------

#[derive(Clone)]
pub struct Store {
    pub pool: PgPool,
    pub tz: Tz,
}

impl Store {
    /// The calendar date in the configured time zone.
    pub fn today(&self) -> NaiveDate {
        Utc::now().with_timezone(&self.tz).date_naive()
    }

    pub async fn summaries(&self, today: NaiveDate) -> AppResult<Vec<Summary>> {
        let pool = &self.pool;
        let since = today - Days::new(rate::WINDOW_DAYS + 1);
        let (products, bottles, doses, bounds, pauses, ingredients) = tokio::try_join!(
            db::products(pool),
            db::bottles(pool),
            db::doses_since(pool, since),
            db::dose_bounds(pool),
            db::pauses(pool),
            db::product_ingredients(pool),
        )?;
        let mut bottles = group(bottles, |b| b.product_id);
        let mut doses = group(doses, |d| d.product_id);
        let mut pauses = group(pauses, |p| p.product_id);
        let mut ingredients = group(ingredients, |i| i.product_id);
        let bounds: HashMap<i64, DoseBounds> =
            bounds.into_iter().map(|b| (b.product_id, b)).collect();

        products
            .into_iter()
            .map(|p| {
                let id = p.id;
                let data = ProductData {
                    bottles: bottles.remove(&id).unwrap_or_default(),
                    doses: doses.remove(&id).unwrap_or_default(),
                    pauses: pauses
                        .remove(&id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| Pause {
                            from: p.paused_on,
                            until: p.resumed_on,
                        })
                        .collect(),
                    ingredients: ingredients.remove(&id).unwrap_or_default(),
                };
                summarize(p, data, bounds.get(&id), today)
            })
            .collect()
    }

    // ---- Changes ----

    /// Starts a transaction holding the product's row lock.
    async fn begin(&self, product_id: i64) -> AppResult<Transaction<'static, Postgres>> {
        let mut tx = self.pool.begin().await?;
        if !db::lock_product(&mut tx, product_id).await? {
            return Err(AppError::NotFound);
        }
        Ok(tx)
    }

    /// Replays the product and commits, or rolls back if the replay breaks a rule.
    async fn commit_replayed(
        mut tx: Transaction<'static, Postgres>,
        product_id: i64,
    ) -> AppResult<()> {
        replay_product(&mut tx, product_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn create_product(&self, input: &ProductInput<'_>) -> AppResult<i64> {
        let mut tx = self.pool.begin().await?;
        let id = db::insert_product(&mut tx, input)
            .await
            .map_err(duplicate_product)?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn update_product(&self, id: i64, input: &ProductInput<'_>) -> AppResult<()> {
        let mut tx = self.begin(id).await?;
        db::update_product(&mut tx, id, input)
            .await
            .map_err(duplicate_product)?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_product(&self, id: i64) -> AppResult<()> {
        let mut tx = self.begin(id).await?;
        db::delete_product(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_status(
        &self,
        id: i64,
        status: ProductStatus,
        today: NaiveDate,
    ) -> AppResult<()> {
        let mut tx = self.begin(id).await?;
        db::set_product_status(&mut tx, id, status.as_str()).await?;
        match status {
            ProductStatus::Active => db::close_pause(&mut tx, id, today).await?,
            ProductStatus::Paused | ProductStatus::Archived => {
                db::open_pause(&mut tx, id, today).await?
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn add_bottles(
        &self,
        product_id: i64,
        count: u32,
        expires_on: NaiveDate,
        quantity: u32,
        already_open: bool,
        today: NaiveDate,
    ) -> AppResult<()> {
        if already_open && quantity != 1 {
            return rule("Add an already-open bottle on its own, with what's left in it.");
        }
        let mut tx = self.begin(product_id).await?;
        for _ in 0..quantity {
            let id =
                db::insert_bottle(&mut tx, product_id, count as i32, expires_on, today).await?;
            if already_open {
                db::insert_bottle_event(&mut tx, id, "start", today, None).await?;
            }
        }
        Self::commit_replayed(tx, product_id).await
    }

    pub async fn update_bottle(
        &self,
        bottle_id: i64,
        count: u32,
        expires_on: NaiveDate,
    ) -> AppResult<i64> {
        let product_id = found(db::product_of_bottle(&self.pool, bottle_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::update_bottle(&mut tx, bottle_id, count as i32, expires_on).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    pub async fn delete_bottle(&self, bottle_id: i64) -> AppResult<i64> {
        let product_id = found(db::product_of_bottle(&self.pool, bottle_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::delete_bottle(&mut tx, bottle_id).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    /// Override: start this bottle instead of the one the queue would open.
    pub async fn start_bottle(&self, bottle_id: i64, today: NaiveDate) -> AppResult<i64> {
        let product_id = found(db::product_of_bottle(&self.pool, bottle_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::insert_bottle_event(&mut tx, bottle_id, "start", today, None).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    pub async fn correct_count(&self, bottle_id: i64, count: u32, on: NaiveDate) -> AppResult<i64> {
        let product_id = found(db::product_of_bottle(&self.pool, bottle_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::insert_bottle_event(&mut tx, bottle_id, "set_count", on, Some(count as i32)).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    pub async fn delete_bottle_event(&self, event_id: i64) -> AppResult<i64> {
        let product_id = found(db::product_of_bottle_event(&self.pool, event_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::delete_bottle_event(&mut tx, event_id).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    pub async fn log_dose(&self, product_id: i64, units: u32, on: NaiveDate) -> AppResult<()> {
        let mut tx = self.begin(product_id).await?;
        db::insert_dose(&mut tx, product_id, on, units as i32).await?;
        Self::commit_replayed(tx, product_id).await
    }

    pub async fn update_dose(&self, dose_id: i64, units: u32, on: NaiveDate) -> AppResult<i64> {
        let product_id = found(db::product_of_dose(&self.pool, dose_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::update_dose(&mut tx, dose_id, on, units as i32).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    pub async fn delete_dose(&self, dose_id: i64) -> AppResult<i64> {
        let product_id = found(db::product_of_dose(&self.pool, dose_id).await?)?;
        let mut tx = self.begin(product_id).await?;
        db::delete_dose(&mut tx, dose_id).await?;
        Self::commit_replayed(tx, product_id).await?;
        Ok(product_id)
    }

    pub async fn add_ingredient(
        &self,
        product_id: i64,
        name: &str,
        kind: &str,
        amount: Option<&str>,
    ) -> AppResult<()> {
        let mut tx = self.begin(product_id).await?;
        let ingredient_id = db::upsert_ingredient(&mut tx, name, kind).await?;
        db::link_ingredient(&mut tx, product_id, ingredient_id, amount).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn remove_ingredient(&self, product_id: i64, ingredient_id: i64) -> AppResult<()> {
        let mut tx = self.begin(product_id).await?;
        db::unlink_ingredient(&mut tx, product_id, ingredient_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_ingredient(&self, ingredient_id: i64) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        if !db::delete_unused_ingredient(&mut tx, ingredient_id).await? {
            return rule("That ingredient is still used by a product.");
        }
        tx.commit().await?;
        Ok(())
    }
}

fn found(product_id: Option<i64>) -> AppResult<i64> {
    product_id.ok_or(AppError::NotFound)
}

fn duplicate_product(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            AppError::Rule("You already have a product with that brand, name and strength.".into())
        }
        _ => e.into(),
    }
}

/// Recomputes and caches every bottle's state for one product (ADR 0002).
async fn replay_product(conn: &mut PgConnection, product_id: i64) -> AppResult<()> {
    let specs = db::bottle_specs(conn, product_id).await?;
    let doses = db::product_doses(conn, product_id).await?;
    let bottle_events = db::product_bottle_events(conn, product_id).await?;

    let specs: Vec<BottleSpec> = specs
        .into_iter()
        .map(|b| BottleSpec {
            id: b.id,
            initial_count: non_negative(b.initial_count),
            expires_on: b.expires_on,
            added_on: b.added_on,
        })
        .collect();
    let mut events: Vec<Event> = doses
        .iter()
        .map(|d| Event {
            on: d.taken_on,
            seq: d.created_at.timestamp_micros(),
            kind: EventKind::Dose {
                units: non_negative(d.units),
            },
        })
        .collect();
    for e in &bottle_events {
        let kind = match (e.kind.as_str(), e.count) {
            ("start", _) => EventKind::Start {
                bottle: e.bottle_id,
            },
            ("set_count", Some(count)) => EventKind::SetCount {
                bottle: e.bottle_id,
                count: non_negative(count),
            },
            (kind, _) => {
                return Err(AppError::Internal(anyhow!(
                    "bad bottle event {} ({kind})",
                    e.id
                )))
            }
        };
        events.push(Event {
            on: e.on_date,
            seq: e.created_at.timestamp_micros(),
            kind,
        });
    }

    let states = replay::replay(&specs, &events)?;
    // Save the started bottle last so the one-started-per-product index never sees two.
    let (started, rest): (Vec<_>, Vec<_>) = states
        .into_iter()
        .partition(|s| s.status == BottleStatus::Started);
    for s in rest.iter().chain(&started) {
        db::save_bottle_state(
            conn,
            s.id,
            s.status.as_str(),
            s.remaining as i32,
            s.started_on,
            s.finished_on,
        )
        .await?;
    }
    Ok(())
}
