//! GET handlers. Each builds a flat view model of display strings for its template.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use chrono::NaiveDate;
use serde::Deserialize;

use super::format::{self, units};
use super::{render, Layout};
use crate::app::{AppError, AppResult, Bottle, Store, Summary, KINDS, UNITS};
use crate::db;
use crate::domain::projection::Projection;
use crate::domain::scorecard::{self, Scorecard};
use crate::domain::{BottleStatus, ProductStatus};

#[derive(Deserialize)]
pub struct Flash {
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct ProductsQuery {
    error: Option<String>,
    ingredient: Option<i64>,
}

fn layout(active: &'static str, error: Option<String>, summaries: &[Summary]) -> Layout {
    Layout {
        active,
        error,
        at_risk: summaries.iter().map(|s| s.at_risk().count()).sum(),
    }
}

fn title(s: &Summary) -> String {
    s.product.name.clone()
}

fn subtitle(s: &Summary) -> String {
    format::product_subtitle(&s.product.brand, &s.product.strength)
}

fn plural_unit(s: &Summary) -> String {
    format::plural(&s.product.unit, 2.0)
}

/// What the at-risk badge on a bottle says, if anything.
fn risk_text(s: &Summary, b: &Bottle, p: &Projection, today: NaiveDate) -> Option<String> {
    if !p.at_risk() {
        return None;
    }
    let left = units(p.left_at_expiry, &s.product.unit);
    Some(if b.expires_on < today {
        format!("Expired with {left} left")
    } else {
        match s.product.status {
            ProductStatus::Active => format!("~{left} left at expiry"),
            ProductStatus::Paused => format!("Paused · {left} will expire"),
            ProductStatus::Archived => format!("Archived · {left} will expire"),
        }
    })
}

fn projection_text(s: &Summary, b: &Bottle, p: &Projection, today: NaiveDate) -> String {
    match (p.start, p.finish) {
        (Some(_), Some(finish)) if b.status == BottleStatus::Started => {
            format!("Runs out around {}", format::relative_date(finish, today))
        }
        (Some(start), Some(finish)) => format!(
            "Opens around {}, runs out around {}",
            format::relative_date(start, today),
            format::relative_date(finish, today)
        ),
        _ => match s.product.status {
            ProductStatus::Active => "Not reached at the current rate".into(),
            ProductStatus::Paused => "Not being taken while paused".into(),
            ProductStatus::Archived => "Not being taken (archived)".into(),
        },
    }
}

// ---- Today --------------------------------------------------------------------

pub struct DueItem {
    id: i64,
    title: String,
    subtitle: String,
    dose_label: String,
    dose_units: u32,
    note: Option<String>,
    can_log: bool,
}

pub struct DoneItem {
    id: i64,
    title: String,
    taken_label: String,
    undo_id: i64,
}

pub struct UpcomingItem {
    id: i64,
    title: String,
    when: String,
}

#[derive(Template)]
#[template(path = "today.html")]
pub struct TodayPage {
    layout: Layout,
    date_label: String,
    has_products: bool,
    due: Vec<DueItem>,
    done: Vec<DoneItem>,
    upcoming: Vec<UpcomingItem>,
}

fn bottle_note(s: &Summary) -> Option<String> {
    let dose = s.product.schedule.units;
    let mut queue = s.queue.iter().map(|(b, _)| b);
    let first = queue.next()?;
    let expiry = |b: &Bottle| format::date(b.expires_on);
    Some(match first.status {
        BottleStatus::Started if first.remaining <= dose => match queue.next() {
            Some(next) => format!(
                "Finishes this bottle, then open the one expiring {}",
                expiry(next)
            ),
            None => format!("Finishes your last bottle ({} left)", first.remaining),
        },
        BottleStatus::Started => format!("From the open bottle · {} left", first.remaining),
        _ => format!("Open the bottle expiring {}", expiry(first)),
    })
}

pub async fn today(State(store): State<Store>, Query(q): Query<Flash>) -> AppResult<Response> {
    let today = store.today();
    let summaries = store.summaries(today).await?;
    let mut due = Vec::new();
    let mut done = Vec::new();
    let mut upcoming = Vec::new();

    for s in summaries
        .iter()
        .filter(|s| s.product.status == ProductStatus::Active)
    {
        let unit = &s.product.unit;
        if let Some(&(undo_id, _)) = s.doses_today.last() {
            let taken: u32 = s.doses_today.iter().map(|&(_, u)| u).sum();
            done.push(DoneItem {
                id: s.product.id,
                title: title(s),
                taken_label: units(taken, unit),
                undo_id,
            });
        } else if s.is_due(today) {
            let on_hand: u32 = s.queue.iter().map(|(b, _)| b.remaining).sum();
            due.push(DueItem {
                id: s.product.id,
                title: title(s),
                subtitle: subtitle(s),
                dose_label: units(s.product.schedule.units, unit),
                dose_units: s.product.schedule.units,
                note: if on_hand == 0 {
                    Some("No bottles left — add one".into())
                } else {
                    bottle_note(s)
                },
                can_log: on_hand > 0,
            });
        } else {
            let next = s.product.schedule.next_due(s.last_dose, today);
            upcoming.push((
                next,
                UpcomingItem {
                    id: s.product.id,
                    title: title(s),
                    when: format::relative_date(next, today),
                },
            ));
        }
    }
    upcoming.sort_by_key(|(next, _)| *next);

    render(&TodayPage {
        layout: layout("today", q.error, &summaries),
        date_label: today.format("%A %-d %B").to_string(),
        has_products: !summaries.is_empty(),
        due,
        done,
        upcoming: upcoming.into_iter().map(|(_, item)| item).collect(),
    })
}

// ---- Products -------------------------------------------------------------------

pub struct ProductItem {
    id: i64,
    title: String,
    subtitle: String,
    stock: String,
    rate: String,
    at_risk: usize,
}

pub struct ProductSection {
    title: &'static str,
    items: Vec<ProductItem>,
}

#[derive(Template)]
#[template(path = "products.html")]
pub struct ProductsPage {
    layout: Layout,
    filter: Option<String>,
    sections: Vec<ProductSection>,
}

fn stock(s: &Summary) -> String {
    let unit = &s.product.unit;
    let mut parts = Vec::new();
    if let Some((b, _)) = s
        .queue
        .iter()
        .find(|(b, _)| b.status == BottleStatus::Started)
    {
        parts.push(format!(
            "{} left in the open bottle",
            units(b.remaining, unit)
        ));
    }
    let new = s
        .queue
        .iter()
        .filter(|(b, _)| b.status == BottleStatus::New)
        .count();
    match new {
        0 => {}
        1 => parts.push("1 new bottle".into()),
        n => parts.push(format!("{n} new bottles")),
    }
    if parts.is_empty() {
        "No bottles".into()
    } else {
        parts.join(" · ")
    }
}

pub async fn products(
    State(store): State<Store>,
    Query(q): Query<ProductsQuery>,
) -> AppResult<Response> {
    let today = store.today();
    let summaries = store.summaries(today).await?;
    let filter = match q.ingredient {
        Some(id) => {
            let name = summaries
                .iter()
                .flat_map(|s| &s.ingredients)
                .find(|i| i.ingredient_id == id)
                .map(|i| i.name.clone())
                .ok_or(AppError::NotFound)?;
            Some((id, name))
        }
        None => None,
    };

    let mut sections = vec![
        ProductSection {
            title: "Taking",
            items: vec![],
        },
        ProductSection {
            title: "Paused",
            items: vec![],
        },
        ProductSection {
            title: "Archived",
            items: vec![],
        },
    ];
    for s in &summaries {
        if let Some((id, _)) = &filter {
            if !s.ingredients.iter().any(|i| i.ingredient_id == *id) {
                continue;
            }
        }
        let archived = s.product.status == ProductStatus::Archived;
        if archived && s.queue.is_empty() && filter.is_none() {
            continue; // archived products disappear once their bottles are gone
        }
        let section = match s.product.status {
            ProductStatus::Active => 0,
            ProductStatus::Paused => 1,
            ProductStatus::Archived => 2,
        };
        sections[section].items.push(ProductItem {
            id: s.product.id,
            title: title(s),
            subtitle: subtitle(s),
            stock: stock(s),
            rate: format::schedule(s.product.schedule, &s.product.unit),
            at_risk: s.at_risk().count(),
        });
    }
    sections.retain(|s| !s.items.is_empty());

    render(&ProductsPage {
        layout: layout("products", q.error, &summaries),
        filter: filter.map(|(_, name)| name),
        sections,
    })
}

// ---- Product --------------------------------------------------------------------

pub struct QueueRow {
    id: i64,
    label: String,
    count: String,
    expires: String,
    expired: bool,
    opened: Option<String>,
    projection: String,
    risk: Option<String>,
    is_new: bool,
    is_started: bool,
    remaining: u32,
}

pub struct FinishedRow {
    id: i64,
    expires: String,
    finished: String,
    expired: bool,
}

pub struct HistoryRow {
    date: String,
    text: String,
    edit_url: Option<String>,
    delete_url: String,
}

pub struct IngredientChip {
    id: i64,
    name: String,
    kind: &'static str,
    amount: Option<String>,
}

#[derive(Template)]
#[template(path = "product.html")]
pub struct ProductPage {
    layout: Layout,
    id: i64,
    title: String,
    subtitle: String,
    schedule: String,
    status: &'static str,
    rate_planned: String,
    rate_actual: Option<String>,
    rate_used: &'static str,
    queue: Vec<QueueRow>,
    finished: Vec<FinishedRow>,
    today_iso: String,
    dose_units: u32,
    unit_plural: String,
    ingredients: Vec<IngredientChip>,
    ingredient_names: Vec<String>,
    kinds: Vec<(&'static str, &'static str)>,
    history: Vec<HistoryRow>,
}

pub async fn product(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Query(q): Query<Flash>,
) -> AppResult<Response> {
    let today = store.today();
    let summaries = store.summaries(today).await?;
    let s = summaries
        .iter()
        .find(|s| s.product.id == id)
        .ok_or(AppError::NotFound)?;
    let unit = &s.product.unit;
    let (doses, events, all_ingredients) = tokio::try_join!(
        db::recent_doses(&store.pool, id, 30),
        db::recent_bottle_events(&store.pool, id, 30),
        db::ingredients(&store.pool),
    )?;

    let queue = s
        .queue
        .iter()
        .enumerate()
        .map(|(i, (b, p))| QueueRow {
            id: b.id,
            label: match (b.status, i) {
                (BottleStatus::Started, _) => "Open".into(),
                (_, 0) => "Next".into(),
                (_, n) => format!("{} in line", ordinal(n + 1)),
            },
            count: if b.status == BottleStatus::Started {
                format!("{} of {} left", b.remaining, units(b.initial_count, unit))
            } else {
                units(b.remaining, unit)
            },
            expires: format::date(b.expires_on),
            expired: b.expires_on < today,
            opened: b.started_on.map(|d| format::relative_date(d, today)),
            projection: projection_text(s, b, p, today),
            risk: risk_text(s, b, p, today),
            is_new: b.status == BottleStatus::New,
            is_started: b.status == BottleStatus::Started,
            remaining: b.remaining,
        })
        .collect();

    let mut finished: Vec<_> = s
        .bottles
        .iter()
        .filter(|b| b.status == BottleStatus::Finished)
        .collect();
    finished.sort_by_key(|b| std::cmp::Reverse(b.finished_on));
    let finished = finished
        .into_iter()
        .map(|b| FinishedRow {
            id: b.id,
            expires: format::date(b.expires_on),
            finished: b.finished_on.map(format::date).unwrap_or_default(),
            expired: scorecard::is_expired(b.expires_on, b.finished_on, today),
        })
        .collect();

    let bottle_expiry = |bottle_id: i64| {
        s.bottles
            .iter()
            .find(|b| b.id == bottle_id)
            .map(|b| format::date(b.expires_on))
            .unwrap_or_default()
    };
    let mut history: Vec<(NaiveDate, i64, HistoryRow)> = doses
        .iter()
        .map(|d| {
            let row = HistoryRow {
                date: format::relative_date(d.taken_on, today),
                text: format!("Took {}", units(d.units.max(0) as u32, unit)),
                edit_url: Some(format!("/doses/{}/edit", d.id)),
                delete_url: format!("/doses/{}/delete", d.id),
            };
            (d.taken_on, d.created_at.timestamp_micros(), row)
        })
        .collect();
    for e in &events {
        let bottle = bottle_expiry(e.bottle_id);
        let text = match (e.kind.as_str(), e.count) {
            ("start", _) => format!("Started the bottle expiring {bottle}"),
            ("set_count", Some(0)) => format!("Marked the bottle expiring {bottle} finished"),
            ("set_count", Some(n)) => format!(
                "Counted {} in the bottle expiring {bottle}",
                units(n.max(0) as u32, unit)
            ),
            _ => continue,
        };
        let row = HistoryRow {
            date: format::relative_date(e.on_date, today),
            text,
            edit_url: None,
            delete_url: format!("/bottle-events/{}/delete", e.id),
        };
        history.push((e.on_date, e.created_at.timestamp_micros(), row));
    }
    history.sort_by_key(|(on, seq, _)| std::cmp::Reverse((*on, *seq)));
    history.truncate(30);

    render(&ProductPage {
        layout: layout("products", q.error, &summaries),
        id,
        title: title(s),
        subtitle: subtitle(s),
        schedule: format::schedule(s.product.schedule, unit),
        status: s.product.status.as_str(),
        rate_planned: format::rate(s.product.schedule.planned_rate(), unit),
        rate_actual: s.actual_rate.map(|r| format::rate(r, unit)),
        rate_used: match (s.product.status, s.actual_rate) {
            (ProductStatus::Active, Some(_)) => "Projections use your actual rate.",
            (ProductStatus::Active, None) => {
                "Projections use your planned rate until there are two weeks of doses."
            }
            _ => "Projections assume you're not taking it.",
        },
        queue,
        finished,
        today_iso: today.to_string(),
        dose_units: s.product.schedule.units,
        unit_plural: plural_unit(s),
        ingredients: s
            .ingredients
            .iter()
            .map(|i| IngredientChip {
                id: i.ingredient_id,
                name: i.name.clone(),
                kind: format::kind(&i.kind),
                amount: i.amount.clone(),
            })
            .collect(),
        ingredient_names: all_ingredients.into_iter().map(|i| i.name).collect(),
        kinds: KINDS.to_vec(),
        history: history.into_iter().map(|(_, _, row)| row).collect(),
    })
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

// ---- Forms ------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "product_form.html")]
pub struct ProductFormPage {
    layout: Layout,
    id: Option<i64>,
    brand: String,
    name: String,
    strength: String,
    dose_units: u32,
    every_days: u32,
    units: Vec<UnitOption>,
}

pub struct UnitOption {
    value: &'static str,
    label: String,
    selected: bool,
}

fn unit_options(selected: &str) -> Vec<UnitOption> {
    UNITS
        .iter()
        .map(|&value| UnitOption {
            value,
            label: format::plural(value, 2.0),
            selected: value == selected,
        })
        .collect()
}

pub async fn new_product(
    State(store): State<Store>,
    Query(q): Query<Flash>,
) -> AppResult<Response> {
    let summaries = store.summaries(store.today()).await?;
    render(&ProductFormPage {
        layout: layout("products", q.error, &summaries),
        id: None,
        brand: String::new(),
        name: String::new(),
        strength: String::new(),
        dose_units: 1,
        every_days: 1,
        units: unit_options("capsule"),
    })
}

pub async fn edit_product(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Query(q): Query<Flash>,
) -> AppResult<Response> {
    let summaries = store.summaries(store.today()).await?;
    let p = &summaries
        .iter()
        .find(|s| s.product.id == id)
        .ok_or(AppError::NotFound)?
        .product;
    render(&ProductFormPage {
        layout: layout("products", q.error.clone(), &summaries),
        id: Some(id),
        brand: p.brand.clone(),
        name: p.name.clone(),
        strength: p.strength.clone(),
        dose_units: p.schedule.units,
        every_days: p.schedule.every_days,
        units: unit_options(&p.unit),
    })
}

#[derive(Template)]
#[template(path = "bottle_form.html")]
pub struct BottleFormPage {
    layout: Layout,
    id: i64,
    product_id: i64,
    product_title: String,
    count: u32,
    expires: String,
    added: String,
    unit_plural: String,
}

pub async fn edit_bottle(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Query(q): Query<Flash>,
) -> AppResult<Response> {
    let summaries = store.summaries(store.today()).await?;
    let (s, b) = summaries
        .iter()
        .find_map(|s| s.bottles.iter().find(|b| b.id == id).map(|b| (s, b)))
        .ok_or(AppError::NotFound)?;
    render(&BottleFormPage {
        layout: layout("products", q.error.clone(), &summaries),
        id,
        product_id: s.product.id,
        product_title: title(s),
        count: b.initial_count,
        expires: b.expires_on.to_string(),
        added: format::date(b.added_on),
        unit_plural: plural_unit(s),
    })
}

#[derive(Template)]
#[template(path = "dose_form.html")]
pub struct DoseFormPage {
    layout: Layout,
    id: i64,
    product_id: i64,
    product_title: String,
    units: u32,
    taken_on: String,
    today_iso: String,
    unit_plural: String,
}

pub async fn edit_dose(
    State(store): State<Store>,
    Path(id): Path<i64>,
    Query(q): Query<Flash>,
) -> AppResult<Response> {
    let today = store.today();
    let dose = db::dose(&store.pool, id).await?.ok_or(AppError::NotFound)?;
    let summaries = store.summaries(today).await?;
    let s = summaries
        .iter()
        .find(|s| s.product.id == dose.product_id)
        .ok_or(AppError::NotFound)?;
    render(&DoseFormPage {
        layout: layout("products", q.error.clone(), &summaries),
        id,
        product_id: s.product.id,
        product_title: title(s),
        units: dose.units.max(0) as u32,
        taken_on: dose.taken_on.to_string(),
        today_iso: today.to_string(),
        unit_plural: plural_unit(s),
    })
}

// ---- At risk --------------------------------------------------------------------

pub struct RiskRow {
    product_id: i64,
    title: String,
    subtitle: String,
    bottle: String,
    expires: String,
    risk: String,
    projection: String,
}

#[derive(Template)]
#[template(path = "at_risk.html")]
pub struct AtRiskPage {
    layout: Layout,
    rows: Vec<RiskRow>,
    card: Scorecard,
}

pub async fn at_risk(State(store): State<Store>, Query(q): Query<Flash>) -> AppResult<Response> {
    let today = store.today();
    let summaries = store.summaries(today).await?;
    let mut rows: Vec<(NaiveDate, RiskRow)> = Vec::new();
    for s in &summaries {
        for (b, p) in s.at_risk() {
            rows.push((
                b.expires_on,
                RiskRow {
                    product_id: s.product.id,
                    title: title(s),
                    subtitle: subtitle(s),
                    bottle: if b.status == BottleStatus::Started {
                        "Open bottle".into()
                    } else {
                        "New bottle".into()
                    },
                    expires: format::date(b.expires_on),
                    risk: risk_text(s, b, p, today).unwrap_or_default(),
                    projection: projection_text(s, b, p, today),
                },
            ));
        }
    }
    rows.sort_by_key(|(expires, _)| *expires);
    let card = scorecard::scorecard(
        summaries
            .iter()
            .flat_map(|s| s.bottles.iter().map(|b| (b.expires_on, b.finished_on))),
        today,
    );
    render(&AtRiskPage {
        layout: layout("at-risk", q.error, &summaries),
        rows: rows.into_iter().map(|(_, row)| row).collect(),
        card,
    })
}

// ---- Ingredients ----------------------------------------------------------------

pub struct IngredientItem {
    id: i64,
    name: String,
    product_count: i64,
}

pub struct IngredientGroup {
    label: &'static str,
    items: Vec<IngredientItem>,
}

#[derive(Template)]
#[template(path = "ingredients.html")]
pub struct IngredientsPage {
    layout: Layout,
    groups: Vec<IngredientGroup>,
}

pub async fn ingredients(
    State(store): State<Store>,
    Query(q): Query<Flash>,
) -> AppResult<Response> {
    let today = store.today();
    let (summaries, ingredients) = tokio::try_join!(store.summaries(today), async {
        db::ingredients(&store.pool).await.map_err(AppError::from)
    })?;
    let groups = KINDS
        .iter()
        .map(|&(kind, label)| IngredientGroup {
            label,
            items: ingredients
                .iter()
                .filter(|i| i.kind == kind)
                .map(|i| IngredientItem {
                    id: i.id,
                    name: i.name.clone(),
                    product_count: i.product_count,
                })
                .collect(),
        })
        .filter(|g| !g.items.is_empty())
        .collect();
    render(&IngredientsPage {
        layout: layout("ingredients", q.error, &summaries),
        groups,
    })
}
