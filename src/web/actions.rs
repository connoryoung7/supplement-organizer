//! POST handlers. Each validates its form, runs one change, and redirects back.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::Form;
use chrono::NaiveDate;
use serde::Deserialize;

use super::{respond, safe_back};
use crate::app::{rule, AppResult, Store, KINDS, UNITS};
use crate::db::ProductInput;
use crate::domain::{parse_expiry, ProductStatus};

fn int(raw: &str, what: &str, min: u32, max: u32) -> AppResult<u32> {
    match raw.trim().parse::<u32>() {
        Ok(n) if (min..=max).contains(&n) => Ok(n),
        _ => rule(format!(
            "{what} must be a whole number from {min} to {max}."
        )),
    }
}

/// A date from an `<input type=date>`, defaulting to today; never in the future.
fn day(raw: Option<&str>, today: NaiveDate) -> AppResult<NaiveDate> {
    let raw = raw.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return Ok(today);
    }
    let Ok(d) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") else {
        return rule("Enter the date as YYYY-MM-DD.");
    };
    if d > today {
        return rule("That date is in the future.");
    }
    Ok(d)
}

fn expiry(raw: &str) -> AppResult<NaiveDate> {
    match parse_expiry(raw) {
        Some(d) => Ok(d),
        None => rule("Enter the expiry date as printed, e.g. 03/2027 or 2027-03-15."),
    }
}

fn product_page(id: i64) -> String {
    format!("/products/{id}")
}

// ---- Products -----------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProductForm {
    brand: String,
    name: String,
    #[serde(default)]
    strength: String,
    unit: String,
    dose_units: String,
    dose_every_days: String,
}

struct ValidProduct {
    brand: String,
    name: String,
    strength: String,
    unit: String,
    dose_units: u32,
    dose_every_days: u32,
}

impl ValidProduct {
    fn input(&self) -> ProductInput<'_> {
        ProductInput {
            brand: &self.brand,
            name: &self.name,
            strength: &self.strength,
            unit: &self.unit,
            dose_units: self.dose_units as i32,
            dose_every_days: self.dose_every_days as i32,
        }
    }
}

fn validate_product(f: &ProductForm) -> AppResult<ValidProduct> {
    let brand = f.brand.trim();
    let name = f.name.trim();
    if brand.is_empty() || name.is_empty() {
        return rule("Brand and name are required.");
    }
    if !UNITS.contains(&f.unit.as_str()) {
        return rule("Pick a unit from the list.");
    }
    Ok(ValidProduct {
        brand: brand.into(),
        name: name.into(),
        strength: f.strength.trim().into(),
        unit: f.unit.clone(),
        dose_units: int(&f.dose_units, "Units per dose", 1, 1000)?,
        dose_every_days: int(&f.dose_every_days, "Days between doses", 1, 365)?,
    })
}

pub async fn create_product(
    State(store): State<Store>,
    Form(f): Form<ProductForm>,
) -> AppResult<Response> {
    let result = async {
        let p = validate_product(&f)?;
        store.create_product(&p.input()).await
    }
    .await;
    respond(result, product_page, "/products/new")
}

pub async fn update_product(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<ProductForm>,
) -> AppResult<Response> {
    let result = async {
        let p = validate_product(&f)?;
        store.update_product(id, &p.input()).await
    }
    .await;
    respond(
        result,
        |_| product_page(id),
        &format!("/products/{id}/edit"),
    )
}

pub async fn delete_product(
    State(store): State<Store>,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    respond(
        store.delete_product(id).await,
        |_| "/products".into(),
        &product_page(id),
    )
}

#[derive(Deserialize)]
pub struct StatusForm {
    status: String,
}

pub async fn set_status(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<StatusForm>,
) -> AppResult<Response> {
    let result = async {
        let Some(status) = ProductStatus::parse(&f.status) else {
            return rule("Unknown status.");
        };
        store.set_status(id, status, store.today()).await
    }
    .await;
    respond(result, |_| product_page(id), &product_page(id))
}

// ---- Doses --------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DoseForm {
    units: String,
    taken_on: Option<String>,
    back: Option<String>,
}

pub async fn log_dose(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<DoseForm>,
) -> AppResult<Response> {
    let back = safe_back(f.back.as_deref(), &product_page(id));
    let result = async {
        let today = store.today();
        let units = int(&f.units, "Units", 1, 1000)?;
        store
            .log_dose(id, units, day(f.taken_on.as_deref(), today)?)
            .await
    }
    .await;
    respond(result, |_| back.clone(), &back)
}

pub async fn update_dose(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<DoseForm>,
) -> AppResult<Response> {
    let result = async {
        let today = store.today();
        let units = int(&f.units, "Units", 1, 1000)?;
        store
            .update_dose(id, units, day(f.taken_on.as_deref(), today)?)
            .await
    }
    .await;
    respond(result, product_page, &format!("/doses/{id}/edit"))
}

#[derive(Deserialize)]
pub struct BackForm {
    back: Option<String>,
}

pub async fn delete_dose(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<BackForm>,
) -> AppResult<Response> {
    let result = store.delete_dose(id).await;
    let back = f.back.clone();
    let err_back = safe_back(f.back.as_deref(), "/");
    respond(
        result,
        move |product_id| safe_back(back.as_deref(), &product_page(product_id)),
        &err_back,
    )
}

// ---- Bottles ------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BottlesForm {
    count: String,
    expires: String,
    quantity: Option<String>,
    already_open: Option<String>,
}

pub async fn add_bottles(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<BottlesForm>,
) -> AppResult<Response> {
    let result = async {
        let count = int(&f.count, "Count", 1, 100_000)?;
        let expires_on = expiry(&f.expires)?;
        let quantity = match f.quantity.as_deref().map(str::trim) {
            None | Some("") => 1,
            Some(q) => int(q, "Number of bottles", 1, 50)?,
        };
        store
            .add_bottles(
                id,
                count,
                expires_on,
                quantity,
                f.already_open.is_some(),
                store.today(),
            )
            .await
    }
    .await;
    respond(result, |_| product_page(id), &product_page(id))
}

#[derive(Deserialize)]
pub struct BottleForm {
    count: String,
    expires: String,
}

pub async fn update_bottle(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<BottleForm>,
) -> AppResult<Response> {
    let result = async {
        let count = int(&f.count, "Count", 1, 100_000)?;
        store.update_bottle(id, count, expiry(&f.expires)?).await
    }
    .await;
    respond(result, product_page, &format!("/bottles/{id}/edit"))
}

pub async fn delete_bottle(State(store): State<Store>, Path(id): Path<i64>) -> AppResult<Response> {
    respond(
        store.delete_bottle(id).await,
        product_page,
        &format!("/bottles/{id}/edit"),
    )
}

#[derive(Deserialize)]
pub struct StartForm {
    on: Option<String>,
    back: Option<String>,
}

pub async fn start_bottle(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<StartForm>,
) -> AppResult<Response> {
    let result = async {
        store
            .start_bottle(id, day(f.on.as_deref(), store.today())?)
            .await
    }
    .await;
    let err_back = safe_back(f.back.as_deref(), "/products");
    respond(result, product_page, &err_back)
}

#[derive(Deserialize)]
pub struct CountForm {
    count: String,
    on: Option<String>,
    back: Option<String>,
}

pub async fn correct_count(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<CountForm>,
) -> AppResult<Response> {
    let result = async {
        let count = int(&f.count, "Count", 0, 100_000)?;
        store
            .correct_count(id, count, day(f.on.as_deref(), store.today())?)
            .await
    }
    .await;
    let err_back = safe_back(f.back.as_deref(), "/products");
    respond(result, product_page, &err_back)
}

pub async fn delete_bottle_event(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<BackForm>,
) -> AppResult<Response> {
    let err_back = safe_back(f.back.as_deref(), "/products");
    respond(store.delete_bottle_event(id).await, product_page, &err_back)
}

// ---- Ingredients --------------------------------------------------------------

#[derive(Deserialize)]
pub struct IngredientForm {
    name: String,
    kind: String,
    amount: Option<String>,
}

pub async fn add_ingredient(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Form(f): Form<IngredientForm>,
) -> AppResult<Response> {
    let result = async {
        let name = f.name.trim();
        if name.is_empty() {
            return rule("Ingredient name is required.");
        }
        if !KINDS.iter().any(|(k, _)| *k == f.kind) {
            return rule("Pick a kind from the list.");
        }
        let amount = f.amount.as_deref().map(str::trim).filter(|a| !a.is_empty());
        store.add_ingredient(id, name, &f.kind, amount).await
    }
    .await;
    respond(result, |_| product_page(id), &product_page(id))
}

pub async fn remove_ingredient(
    State(store): State<Store>,
    Path((id, ingredient_id)): Path<(i64, i64)>,
) -> AppResult<Response> {
    respond(
        store.remove_ingredient(id, ingredient_id).await,
        |_| product_page(id),
        &product_page(id),
    )
}

pub async fn delete_ingredient(
    State(store): State<Store>,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    respond(
        store.delete_ingredient(id).await,
        |_| "/ingredients".into(),
        "/ingredients",
    )
}
