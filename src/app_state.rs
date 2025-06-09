use std::sync::Arc;

use axum::extract::ws::Message;
use tokio::sync::{ mpsc::{ self, Sender, UnboundedSender }, Mutex };
use uuid::Uuid;

use crate::{
  database::connect_db,
  storage::connect_s3,
  sync_engine::{ SyncEnvelope, SyncMessage },
};

#[derive(Debug, Clone)]
pub struct SyncClient {
  pub client_id: Uuid,
  pub user_id: Uuid,
  pub sender: UnboundedSender<axum::extract::ws::Message>,
}

pub type AppState = Arc<AppStateInner>;

pub struct AppStateInner {
  pub s3_client: aws_sdk_s3::Client,
  pub db_client: tokio_postgres::Client,
  pub sync_clients: Arc<Mutex<Vec<SyncClient>>>,
  pub notify_tx: Sender<SyncMessage>,
  pub server_id: Uuid,
}

pub async fn create_app_state() -> Result<Arc<AppStateInner>, Box<dyn std::error::Error>> {
  let s3_client = connect_s3().await?;
  let db_client = connect_db().await?;
  let sync_clients: Arc<Mutex<Vec<SyncClient>>> = Arc::new(Mutex::new(Vec::new()));
  let (notify_tx, mut notify_rx) = mpsc::channel::<SyncMessage>(100);
  let sync_clients_clone = sync_clients.clone();
  let server_id = Uuid::new_v4();

  tokio::spawn(async move {
    while let Some(message) = notify_rx.recv().await {
      let mut clients = sync_clients_clone.lock().await;

      let envelope = SyncEnvelope {
        sender_id: server_id,
        message,
      };

      let json_msg = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".into());

      clients.retain(|sync_client| {
        match sync_client.sender.send(Message::Text(json_msg.clone().into())) {
          Ok(_) => true,
          Err(e) => {
            eprintln!(
              "Failed to send WebSocket message to client {}: {}",
              sync_client.client_id,
              e
            );
            false
          }
        }
      });
    }
  });

  Ok(
    Arc::new(AppStateInner {
      s3_client,
      db_client,
      sync_clients,
      notify_tx,
      server_id,
    })
  )
}
