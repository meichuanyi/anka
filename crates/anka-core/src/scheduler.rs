//! FSRS-backed scheduling.

use chrono::{DateTime, Duration, Utc};
use fsrs::{MemoryState, FSRS};

use crate::error::{Error, Result};
use crate::model::{CardState, Rating};

pub struct Scheduler {
    fsrs: FSRS,
    desired_retention: f32,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(None).expect("default FSRS parameters are valid")
    }
}

impl Scheduler {
    /// `weights`: optional personalized FSRS parameters (empty/None → crate defaults).
    pub fn new(weights: Option<Vec<f32>>) -> Result<Self> {
        let slice: &[f32] = match weights.as_deref() {
            Some(w) if !w.is_empty() => w,
            _ => &[],
        };
        let fsrs = FSRS::new(slice).map_err(|e| Error::Scheduler(format!("fsrs init: {e}")))?;
        Ok(Self {
            fsrs,
            desired_retention: 0.9,
        })
    }

    pub fn with_retention(mut self, r: f32) -> Self {
        self.desired_retention = r.clamp(0.7, 0.97);
        self
    }

    /// Compute the next card state after a rating.
    pub fn next_state(
        &self,
        state: &CardState,
        rating: Rating,
        now: DateTime<Utc>,
    ) -> Result<CardState> {
        // First presentation or no meaningful stability yet — short learning-like intervals.
        if state.reps == 0 || state.stability <= 0.0 {
            return Ok(self.graduate_first(state, rating, now));
        }

        let previous = Some(MemoryState {
            stability: state.stability.max(0.0),
            difficulty: state.difficulty.clamp(1.0, 10.0),
        });
        let elapsed_days = state
            .last_review_at
            .map(|t| {
                let secs = (now - t).num_seconds().max(0) as u32;
                (secs / 86_400).max(1)
            })
            .unwrap_or(1);

        let next = self
            .fsrs
            .next_states(previous, self.desired_retention, elapsed_days)
            .map_err(|e| Error::Scheduler(format!("next_states: {e}")))?;

        let chosen = match rating {
            Rating::Again => next.again,
            Rating::Hard => next.hard,
            Rating::Good => next.good,
            Rating::Easy => next.easy,
        };

        let interval_days = chosen.interval.max(1.0);
        let due_at = now + Duration::days(interval_days.round() as i64);

        let mut out = state.clone();
        out.stability = chosen.memory.stability.max(0.0);
        out.difficulty = chosen.memory.difficulty.clamp(1.0, 10.0);
        out.due_at = due_at;
        out.reps = state.reps.saturating_add(1);
        if rating == Rating::Again {
            out.lapses = state.lapses.saturating_add(1);
        }
        out.last_review_at = Some(now);
        Ok(out)
    }

    fn graduate_first(&self, state: &CardState, rating: Rating, now: DateTime<Utc>) -> CardState {
        let (stability, difficulty, days) = match rating {
            Rating::Again => (0.2, 7.0, 0.0),
            Rating::Hard => (0.4, 6.5, 1.0),
            Rating::Good => (0.7, 6.0, 2.0),
            Rating::Easy => (1.2, 5.0, 4.0),
        };
        let due_at = if days <= 0.0 {
            now + Duration::minutes(10)
        } else {
            now + Duration::days(days as i64)
        };
        CardState {
            due_at,
            stability,
            difficulty,
            reps: state.reps.saturating_add(1),
            lapses: if rating == Rating::Again {
                state.lapses.saturating_add(1)
            } else {
                state.lapses
            },
            last_review_at: Some(now),
        }
    }
}
