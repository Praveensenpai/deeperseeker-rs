use crate::domain::telegram::{TelegramNotification, TelegramSettings};
use crate::infra::telegram_client::TelegramClient;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

#[derive(Clone)]
pub struct TelegramNotifier {
    settings: Arc<RwLock<TelegramSettings>>,
    sender: mpsc::UnboundedSender<TelegramNotification>,
    last_empty_alert: Arc<RwLock<Option<Instant>>>,
    suppressed_empty_hits: Arc<AtomicUsize>,
}

impl TelegramNotifier {
    pub fn new(settings: TelegramSettings) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let settings = Arc::new(RwLock::new(settings));
        let client = TelegramClient::new();
        let last_empty_alert = Arc::new(RwLock::new(None));
        let suppressed_empty_hits = Arc::new(AtomicUsize::new(0));

        let notifier = Self {
            settings: settings.clone(),
            sender: tx,
            last_empty_alert,
            suppressed_empty_hits,
        };

        tokio::spawn(run_notification_worker(settings, client, rx));
        notifier
    }

    pub async fn update_settings(&self, new_settings: TelegramSettings) {
        let mut w = self.settings.write().await;
        *w = new_settings;
    }

    pub async fn current_settings(&self) -> TelegramSettings {
        self.settings.read().await.clone()
    }

    pub fn notify(&self, notification: TelegramNotification) {
        let _ = self.sender.send(notification);
    }

    pub async fn notify_no_valid_tokens(&self, endpoint: &str, model: &str, total: usize) {
        let now = Instant::now();
        let mut last_alert = self.last_empty_alert.write().await;

        let should_send = match *last_alert {
            Some(prev) => now.duration_since(prev) >= Duration::from_secs(15),
            None => true,
        };

        if should_send {
            *last_alert = Some(now);
            let hits = self.suppressed_empty_hits.swap(0, Ordering::Relaxed);
            let mut notif = TelegramNotification::NoValidTokens {
                endpoint: endpoint.to_string(),
                model: model.to_string(),
                total_tokens: total,
            };
            if hits > 0 {
                if let TelegramNotification::NoValidTokens {
                    ref mut endpoint, ..
                } = notif
                {
                    endpoint.push_str(&format!(" (+{hits} repeated hits throttled)"));
                }
            }
            let _ = self.sender.send(notif);
        } else {
            self.suppressed_empty_hits.fetch_add(1, Ordering::Relaxed);
        }
    }
}

async fn run_notification_worker(
    settings: Arc<RwLock<TelegramSettings>>,
    client: TelegramClient,
    mut rx: mpsc::UnboundedReceiver<TelegramNotification>,
) {
    let mut last_sent = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);

    while let Some(notif) = rx.recv().await {
        let cfg = settings.read().await.clone();
        if !cfg.is_configured() {
            continue;
        }

        // Telegram rate limit guard: enforce >= 1.5s between messages
        let elapsed = Instant::now().duration_since(last_sent);
        if elapsed < Duration::from_millis(1500) {
            tokio::time::sleep(Duration::from_millis(1500) - elapsed).await;
        }

        let msg = notif.format_markdown();
        match client
            .send_message(&cfg.bot_token, &cfg.chat_id, &msg)
            .await
        {
            Ok(()) => {
                last_sent = Instant::now();
            }
            Err(e) => {
                tracing::warn!("Telegram dispatch failed: {e:#}");
                // Back off on error to prevent cascading rate-limit loops
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}
