use crate::api::live_log::LiveLog;
use crate::config::AppConfig;
use crate::infra::deepseek_client::DeepSeekClient;
use crate::infra::pow::PowSolver;
use crate::infra::tokenizer::Tokenizer;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_rusqlite::Connection;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: Connection,
    pub client: DeepSeekClient,
    pub pow_solver: Arc<PowSolver>,
    pub in_flight: Arc<Mutex<HashMap<i64, usize>>>,
    /// One admission gate per quota-limited client key. Serialising a key's
    /// requests makes its rolling-window quota check exact (see
    /// [`AppState::acquire_client_gate`]).
    pub client_gates: Arc<Mutex<HashMap<i64, Arc<Semaphore>>>>,
    pub tera: Arc<Tera>,
    pub live_log: Arc<LiveLog>,
    pub metrics: Arc<crate::infra::metrics::Metrics>,
    pub tokenizer: Arc<Tokenizer>,
}

/// RAII guard for one in-flight token counter entry. Dropping it releases the
/// slot, so a streaming response holds the slot for its whole lifetime.
pub struct CounterGuard {
    map: Arc<Mutex<HashMap<i64, usize>>>,
    key: i64,
}

impl Drop for CounterGuard {
    fn drop(&mut self) {
        // Fast path: release synchronously when the lock is free so tests and
        // simple paths observe the decrement immediately.
        if let Ok(mut guard) = self.map.try_lock() {
            Self::release(&mut guard, self.key);
            return;
        }
        let map = self.map.clone();
        let key = self.key;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut guard = map.lock().await;
                Self::release(&mut guard, key);
            });
        }
    }
}

impl CounterGuard {
    fn release(map: &mut HashMap<i64, usize>, key: i64) {
        if let Some(count) = map.get_mut(&key) {
            if *count > 1 {
                *count -= 1;
                return;
            }
        }
        map.remove(&key);
    }
}

impl AppState {
    /// Register an in-flight upstream request for `token_id`. The returned
    /// guard must be kept alive for as long as the request is in flight.
    pub async fn begin_token_request(&self, token_id: i64) -> CounterGuard {
        {
            let mut map = self.in_flight.lock().await;
            *map.entry(token_id).or_insert(0) += 1;
        }
        CounterGuard {
            map: self.in_flight.clone(),
            key: token_id,
        }
    }

    pub async fn get_in_flight_snapshot(&self) -> HashMap<i64, usize> {
        self.in_flight.lock().await.clone()
    }

    /// Acquire the one-permit admission gate for a quota-limited client key.
    ///
    /// A key's requests run one at a time, so by the time the next request is
    /// admitted the previous one's usage has been recorded — the rolling-window
    /// quota check then sees the real total and cannot be overshot by a burst
    /// of concurrent requests. Returns `None` only if the semaphore is closed,
    /// in which case the caller proceeds without serialisation.
    pub async fn acquire_client_gate(&self, key_id: i64) -> Option<OwnedSemaphorePermit> {
        let sem = {
            let mut map = self.client_gates.lock().await;
            map.entry(key_id)
                .or_insert_with(|| Arc::new(Semaphore::new(1)))
                .clone()
        };
        sem.acquire_owned().await.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The in-flight counter must drop a token's slot exactly when the guard
    /// is dropped, removing the entry only when the last holder is gone. This
    /// is what lets a streaming response keep its slot for the stream's whole
    /// lifetime while `begin_token_request` owns the increment.
    #[tokio::test]
    async fn counter_guard_releases_slot_on_drop() {
        let map: Arc<Mutex<HashMap<i64, usize>>> = Arc::new(Mutex::new(HashMap::new()));

        // Simulate two concurrent `begin_token_request` calls for token 7.
        *map.lock().await.entry(7).or_insert(0) += 1;
        *map.lock().await.entry(7).or_insert(0) += 1;

        let guard = CounterGuard {
            map: map.clone(),
            key: 7,
        };
        assert_eq!(*map.lock().await.get(&7).unwrap(), 2);

        drop(guard);
        assert_eq!(*map.lock().await.get(&7).unwrap(), 1);
    }

    /// Dropping the last guard for a token must clean the entry up rather than
    /// leaving a zero behind.
    #[tokio::test]
    async fn counter_guard_removes_last_entry() {
        let map: Arc<Mutex<HashMap<i64, usize>>> = Arc::new(Mutex::new(HashMap::new()));
        *map.lock().await.entry(3).or_insert(0) += 1;

        let guard = CounterGuard {
            map: map.clone(),
            key: 3,
        };
        drop(guard);

        assert!(map.lock().await.get(&3).is_none());
    }
}
