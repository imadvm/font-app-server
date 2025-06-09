use std::{ path::PathBuf, time::SystemTime };

use axum::{
  extract::{ ws::{ Message, WebSocket, WebSocketUpgrade }, State },
  response::IntoResponse,
};
use futures::{ SinkExt, StreamExt };
use log::{ error, info };
use serde::{ Deserialize, Serialize };
use uuid::Uuid;
use crate::{ app_state::{ AppState, SyncClient }, auth::AuthUser };

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SyncMessage {
  Init {
    client_id: Uuid,
  },
  FileCreated {
    path: PathBuf,
    source: SyncSource,
    client_id: Uuid,
    user_id: Uuid,
  },
  FolderCreated {
    path: PathBuf,
    client_id: Uuid,
    user_id: Uuid,
  },
  FileChanged {
    path: PathBuf,
    timestamp: SystemTime,
    source: SyncSource,
    client_id: Uuid,
    user_id: Uuid,
  },
  FileDeleted {
    path: PathBuf,
    source: SyncSource,
    client_id: Uuid,
    user_id: Uuid,
  },
  FolderDeleted {
    path: PathBuf,
    client_id: Uuid,
    user_id: Uuid,
  },
  ObjectCreated {
    path: PathBuf,
    source: SyncSource,
    client_id: Uuid,
    user_id: Uuid,
  },
  ObjectDeleted {
    path: PathBuf,
    source: SyncSource,
    client_id: Uuid,
    user_id: Uuid,
  },
  Ping,
  Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncSource {
  Client,
  Server,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncEnvelope {
  pub sender_id: Uuid,
  pub message: SyncMessage,
}

pub async fn ws_handler(
  user: AuthUser,
  ws: WebSocketUpgrade,
  State(state): State<AppState>
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| handle_socket(user, socket, state))
}

async fn handle_socket(user: AuthUser, socket: WebSocket, state: AppState) {
  info!("WebSocket connected for user: {}", user.email);

  let (mut sender, mut receiver) = socket.split();
  let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

  let client_id = Uuid::new_v4();
  info!("Assigned session ID {} to user {}", client_id, user.email);

  {
    let mut clients_lock = state.sync_clients.lock().await;
    let sync_client = SyncClient {
      client_id,
      user_id: user.user_id.clone(),
      sender: tx.clone(),
    };
    clients_lock.push(sync_client);

    let init_message = SyncEnvelope {
      sender_id: state.server_id,
      message: SyncMessage::Init { client_id },
    };

    if let Ok(json) = serde_json::to_string(&init_message) {
      let _ = tx.send(Message::Text(json.into()));
    }
  }

  let send_task = tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
      if sender.send(msg).await.is_err() {
        break;
      }
    }
  });

  while let Some(Ok(msg)) = receiver.next().await {
    if let Message::Text(text) = msg {
      match serde_json::from_str::<SyncEnvelope>(&text) {
        Ok(envelope) => {
          info!("Received from client: {:?}", envelope);

          let clients_guard = state.sync_clients.lock().await;
          for sync_client in clients_guard.iter() {
            if
              sync_client.client_id != envelope.sender_id &&
              sync_client.client_id != state.server_id &&
              sync_client.user_id == user.user_id
            {
              let _ = sync_client.sender.send(Message::Text(text.clone()));
            }
          }
        }
        Err(e) => error!("Failed to deserialize server message: {}", e),
      }
    }
  }

  info!("WebSocket closed for user {} with client {}", user.email, client_id);
  send_task.abort();

  {
    let mut clients_lock = state.sync_clients.lock().await;
    clients_lock.retain(|sync_client| sync_client.client_id != client_id);
  }
}
