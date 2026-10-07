use crate::api::live_log::LiveLog;
use crate::config::AppConfig;
use crate::infra::deepseek_client::DeepSeekClient;
use crate::infra::pow::PowSolver;
use crate::infra::tokenizer::Tokenizer;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::Mutex;
use tokio_rusqlite::Connection;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: Connection,
    pub client: DeepSeekClient,
    pub pow_solver: Arc<PowSolver>,
    pub in_flight: Arc<Mutex<HashMap<i64, usize>>>,
    pub tera: Arc<Tera>,
    pub live_log: Arc<LiveLog>,
    pub metrics: Arc<crate::infra::metrics::Metrics>,
    pub tokenizer: Arc<Tokenizer>,
}

impl AppState {
    pub async fn increment_in_flight(&self, token_id: i64) {
        let mut map = self.in_flight.lock().await;
        *map.entry(token_id).or_insert(0) += 1;
    }

    pub async fn decrement_in_flight(&self, token_id: i64) {
        let mut map = self.in_flight.lock().await;
        if let Some(count) = map.get_mut(&token_id) {
            if *count > 1 {
                *count -= 1;
            } else {
                map.remove(&token_id);
            }
        }
    }

    pub async fn get_in_flight_snapshot(&self) -> HashMap<i64, usize> {
        self.in_flight.lock().await.clone()
    }
}
