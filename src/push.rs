//! Push FCM (legacy HTTP) — no-op gdy brak `FCM_SERVER_KEY`.

use serde_json::json;

use crate::models::club::Notification;
use crate::state::AppState;

pub async fn deliver_push(state: &AppState, notification: &Notification) {
    let Some(server_key) = state.config.fcm_server_key.as_deref() else {
        return;
    };

    let Ok(devices) = state.db.list_devices_for_user(&notification.user_id).await else {
        return;
    };
    if devices.is_empty() {
        return;
    }

    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(_) => return,
    };

    for device in devices {
        let payload = json!({
            "to": device.token,
            "notification": {
                "title": notification.title,
                "body": notification.body,
            },
            "data": {
                "notification_id": notification.id,
                "kind": notification.kind,
                "href": notification.href.clone().unwrap_or_default(),
            },
            "priority": "high",
        });

        let res = client
            .post("https://fcm.googleapis.com/fcm/send")
            .header("Authorization", format!("key={server_key}"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await;

        match res {
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 200 {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let results = body.get("results").and_then(|v| v.as_array());
                        if let Some(arr) = results {
                            for item in arr {
                                if item.get("error").and_then(|e| e.as_str())
                                    == Some("NotRegistered")
                                    || item.get("error").and_then(|e| e.as_str())
                                        == Some("InvalidRegistration")
                                {
                                    let _ = state.db.delete_device_token(&device.token).await;
                                }
                            }
                        }
                    }
                } else {
                    tracing::warn!(
                        status = %status,
                        token_prefix = %device.token.chars().take(12).collect::<String>(),
                        "FCM send failed"
                    );
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "FCM request error");
            }
        }
    }
}
