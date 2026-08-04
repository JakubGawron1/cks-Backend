use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{internal, AppError, AppResult};

const BREVO_URL: &str = "https://api.brevo.com/v3/smtp/email";

#[derive(Clone)]
pub struct Mailer {
    enabled: bool,
    api_key: Option<String>,
    from_name: String,
    from_email: String,
    http: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct BrevoSender<'a> {
    name: &'a str,
    email: &'a str,
}

#[derive(Debug, Serialize)]
struct BrevoRecipient<'a> {
    email: &'a str,
}

#[derive(Debug, Serialize)]
struct BrevoPayload<'a> {
    sender: BrevoSender<'a>,
    to: Vec<BrevoRecipient<'a>>,
    subject: &'a str,
    #[serde(rename = "htmlContent")]
    html_content: &'a str,
}

#[derive(Debug, Deserialize)]
struct BrevoErrorBody {
    message: Option<String>,
}

/// Parsuje `EMAIL_FROM`: `Name <addr@x>` albo sam `addr@x`.
pub fn parse_from_header(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let Some((name, rest)) = raw.rsplit_once('<') {
        let email = rest.trim().trim_end_matches('>').trim();
        let name = name.trim().trim_matches('"');
        if !email.is_empty() {
            let display = if name.is_empty() {
                "Slavia".to_string()
            } else {
                name.to_string()
            };
            return (display, email.to_string());
        }
    }
    if raw.contains('@') {
        return ("Slavia".into(), raw.to_string());
    }
    ("Slavia".into(), raw.to_string())
}

impl Mailer {
    pub fn from_config(config: &Config) -> Self {
        let (from_name, from_email) = match config.email_from.as_deref() {
            Some(raw) => parse_from_header(raw),
            // Bez domeny: ustaw EMAIL_FROM na zweryfikowany sender z Brevo (np. Twój Gmail).
            None => ("Slavia".into(), String::new()),
        };
        Self {
            enabled: config.email_enabled,
            api_key: config.brevo_api_key.clone(),
            from_name,
            from_email,
            http: reqwest::Client::new(),
        }
    }

    pub fn primary_frontend_origin(config: &Config) -> &str {
        config
            .frontend_origins
            .first()
            .map(String::as_str)
            .unwrap_or("http://localhost:3000")
    }

    /// Wysyła e-mail. Gdy wyłączone / brak klucza — loguje zamiast wysyłki.
    pub async fn send(&self, to: &str, subject: &str, html: &str) -> AppResult<()> {
        let to = to.trim();
        if to.is_empty() {
            return Err(AppError::BadRequest("Brak adresu e-mail odbiorcy.".into()));
        }

        if !self.enabled {
            tracing::info!(
                to,
                subject,
                "email: EMAIL_ENABLED=false — pominięto wysyłkę (log)"
            );
            tracing::debug!(html_len = html.len(), "email body (dev log)");
            return Ok(());
        }

        let Some(api_key) = self.api_key.as_deref() else {
            tracing::warn!(
                to,
                subject,
                "email: brak BREVO_API_KEY — pominięto wysyłkę (log)"
            );
            return Ok(());
        };

        if self.from_email.is_empty() || !self.from_email.contains('@') {
            return Err(AppError::BadRequest(
                "Brak poprawnego EMAIL_FROM — ustaw zweryfikowany nadawcę z Brevo, \
                 w cudzysłowach gdy nazwa ma spacje, np. EMAIL_FROM=\"CKS Slavia <twoj@email.pl>\"."
                    .into(),
            ));
        }

        let payload = BrevoPayload {
            sender: BrevoSender {
                name: &self.from_name,
                email: &self.from_email,
            },
            to: vec![BrevoRecipient { email: to }],
            subject,
            html_content: html,
        };

        let response = self
            .http
            .post(BREVO_URL)
            .header("api-key", api_key)
            .header("accept", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(internal)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::error!(%status, body = %body, to, "email: Brevo error");

            let brevo_msg = serde_json::from_str::<BrevoErrorBody>(&body)
                .ok()
                .and_then(|b| b.message)
                .unwrap_or_else(|| format!("Brevo {status}"));

            if status.is_client_error() {
                return Err(AppError::BadRequest(format!(
                    "Brevo odrzucił wysyłkę: {brevo_msg}"
                )));
            }
            return Err(internal(format!(
                "Nie udało się wysłać e-maila (Brevo {status})."
            )));
        }

        tracing::info!(to, subject, "email: wysłano przez Brevo");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_from_header;

    #[test]
    fn parses_angle_from() {
        assert_eq!(
            parse_from_header("Slavia <dev@example.com>"),
            ("Slavia".into(), "dev@example.com".into())
        );
    }

    #[test]
    fn parses_bare_email() {
        assert_eq!(
            parse_from_header("dev@example.com"),
            ("Slavia".into(), "dev@example.com".into())
        );
    }
}
