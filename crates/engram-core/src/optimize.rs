//! FSRS parameter optimization from review history.

use chrono::{DateTime, Utc};
use fsrs::{compute_parameters, ComputeParametersInput, FSRSItem, FSRSReview};

use crate::collection::Collection;
use crate::error::{Error, Result};
use crate::model::Rating;

const META_FSRS_PARAMS: &str = "fsrs_params";
/// Anki's guidance: need a few hundred reviews before personalize is useful.
pub const MIN_REVIEWS_FOR_OPTIMIZE: usize = 200;

#[derive(Debug, Clone)]
pub struct OptimizeReport {
    pub review_count: usize,
    pub item_count: usize,
    pub card_count: usize,
    pub params: Vec<f32>,
    pub log_loss: Option<f32>,
}

impl Collection {
    /// Optimize FSRS parameters from stored revlog and optionally persist them.
    pub fn optimize_fsrs(&mut self, apply: bool) -> Result<OptimizeReport> {
        let revlog = self.store().all_revlog()?;
        let review_count = revlog.len();
        if review_count == 0 {
            return Err(Error::Invalid(
                "no review history yet; answer some cards first".into(),
            ));
        }

        let (items, card_ids) = build_train_set(&revlog, Utc::now());
        if items.is_empty() {
            return Err(Error::Invalid(
                "not enough completed review sequences to optimize".into(),
            ));
        }

        let input = ComputeParametersInput {
            train_set: items.clone(),
            card_ids: Some(card_ids),
            enable_short_term: true,
            ..ComputeParametersInput::default()
        };
        let params = compute_parameters(input)
            .map_err(|e| Error::Scheduler(format!("compute_parameters: {e}")))?;

        // Evaluate improved fit vs defaults when possible.
        let log_loss = {
            use fsrs::FSRS;
            let model = FSRS::new(&params).ok();
            match model {
                Some(m) => m.evaluate(items.clone(), |_| true).ok().map(|e| e.log_loss),
                None => None,
            }
        };

        if apply {
            let encoded = serde_json::to_string(&params)
                .map_err(|e| Error::Other(e.to_string()))?;
            self.store().set_meta(META_FSRS_PARAMS, &encoded)?;
            // Swap live scheduler to new weights.
            self.set_scheduler_params(params.clone())?;
        }

        Ok(OptimizeReport {
            review_count,
            item_count: items.len(),
            card_count: count_cards(&revlog),
            params,
            log_loss,
        })
    }

    pub fn fsrs_params(&self) -> Result<Option<Vec<f32>>> {
        let raw: Option<String> = self.store().get_meta(META_FSRS_PARAMS)?;
        match raw {
            None => Ok(None),
            Some(s) => {
                let v: Vec<f32> = serde_json::from_str(&s)
                    .map_err(|e| Error::Other(format!("corrupt fsrs_params: {e}")))?;
                Ok(Some(v))
            }
        }
    }
}

fn count_cards(revlog: &[crate::model::RevlogEntry]) -> usize {
    let mut ids: Vec<_> = revlog.iter().map(|r| r.card_id.to_string()).collect();
    ids.sort();
    ids.dedup();
    ids.len()
}

/// Expand revlog into FSRS training items (one item per review after the first).
fn build_train_set(
    revlog: &[crate::model::RevlogEntry],
    _now: DateTime<Utc>,
) -> (Vec<FSRSItem>, Vec<i64>) {
    // Group by card preserving time order.
    let mut by_card: std::collections::BTreeMap<String, Vec<&crate::model::RevlogEntry>> =
        std::collections::BTreeMap::new();
    for e in revlog {
        by_card.entry(e.card_id.to_string()).or_default().push(e);
    }

    let mut items = Vec::new();
    let mut card_ids = Vec::new();
    let mut card_idx: i64 = 0;

    for (_id, entries) in by_card {
        if entries.len() < 2 {
            continue;
        }
        card_idx += 1;
        let mut history: Vec<FSRSReview> = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let delta_t = if i == 0 {
                0
            } else {
                let prev = entries[i - 1].reviewed_at;
                let days = (e.reviewed_at - prev).num_seconds().max(0) as u32 / 86_400;
                days.max(1)
            };
            history.push(FSRSReview {
                rating: rating_u32(e.rating),
                delta_t,
            });
            if i == 0 {
                continue;
            }
            // Prefix item: reviews[0..=i]
            let prefix = history.clone();
            items.push(FSRSItem { reviews: prefix });
            card_ids.push(card_idx);
        }
    }

    (items, card_ids)
}

fn rating_u32(r: Rating) -> u32 {
    r.as_u8() as u32
}
