//! Proste szablony HTML (PL) dla maili transakcyjnych.

fn wrap(title: &str, body_html: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="pl">
<head><meta charset="utf-8"><title>{title}</title></head>
<body style="font-family:Segoe UI,system-ui,sans-serif;line-height:1.5;color:#0f172a;background:#f8fafc;padding:24px;">
  <div style="max-width:560px;margin:0 auto;background:#fff;border:1px solid #e2e8f0;border-radius:8px;padding:28px;">
    <p style="margin:0 0 8px;font-size:12px;letter-spacing:0.08em;text-transform:uppercase;color:#64748b;">CKS Slavia</p>
    <h1 style="margin:0 0 16px;font-size:22px;">{title}</h1>
    {body_html}
    <p style="margin:28px 0 0;font-size:12px;color:#94a3b8;">Wiadomość systemowa platformy CKS Slavia. Nie odpowiadaj na ten adres.</p>
  </div>
</body>
</html>"#
    )
}

fn cta(url: &str, label: &str) -> String {
    format!(
        r#"<p style="margin:24px 0;"><a href="{url}" style="display:inline-block;padding:12px 20px;background:#0f766e;color:#fff;text-decoration:none;border-radius:6px;font-weight:600;">{label}</a></p>
<p style="font-size:13px;color:#64748b;">Jeśli przycisk nie działa, skopiuj link:<br><a href="{url}" style="color:#0f766e;word-break:break-all;">{url}</a></p>"#
    )
}

pub fn verify_email(display_name: &str, verify_url: &str) -> (String, String) {
    let subject = "Potwierdź adres e-mail — CKS Slavia".to_string();
    let html = wrap(
        "Potwierdź adres e-mail",
        &format!(
            "<p>Cześć {display_name},</p><p>Aby otrzymywać powiadomienia z klubu, potwierdź swój adres e-mail.</p>{}",
            cta(verify_url, "Potwierdź e-mail")
        ),
    );
    (subject, html)
}

pub fn reset_password(display_name: &str, reset_url: &str) -> (String, String) {
    let subject = "Reset hasła — CKS Slavia".to_string();
    let html = wrap(
        "Reset hasła",
        &format!(
            "<p>Cześć {display_name},</p><p>Otrzymaliśmy prośbę o zresetowanie hasła. Link jest ważny przez godzinę.</p>{}<p>Jeśli to nie Ty — zignoruj tę wiadomość.</p>",
            cta(reset_url, "Ustaw nowe hasło")
        ),
    );
    (subject, html)
}

pub fn notification(title: &str, body: &str, href: Option<&str>, frontend_origin: &str) -> (String, String) {
    let subject = format!("CKS Slavia — {title}");
    let link = href.map(|h| {
        if h.starts_with("http") {
            h.to_string()
        } else {
            format!("{}{}", frontend_origin.trim_end_matches('/'), h)
        }
    });
    let cta_html = link
        .as_deref()
        .map(|url| cta(url, "Otwórz w aplikacji"))
        .unwrap_or_default();
    let html = wrap(
        title,
        &format!("<p>{body}</p>{cta_html}"),
    );
    (subject, html)
}

pub fn contact_confirmation(name: &str, subject_line: &str) -> (String, String) {
    let subject = "Otrzymaliśmy Twoją wiadomość — CKS Slavia".to_string();
    let html = wrap(
        "Dziękujemy za kontakt",
        &format!(
            "<p>Cześć {name},</p><p>Potwierdzamy przyjęcie wiadomości: <strong>{subject_line}</strong>.</p><p>Kadra klubu odpowie tak szybko, jak to możliwe.</p>"
        ),
    );
    (subject, html)
}

pub fn debug_test(from_admin: &str, to: &str) -> (String, String) {
    let subject = "Test e-mail — CKS Slavia DevTools".to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let html = wrap(
        "Testowy e-mail",
        &format!(
            "<p>To jest wiadomość testowa z DevTools.</p>\
             <p><strong>Nadawca (admin):</strong> {from_admin}<br>\
             <strong>Odbiorca:</strong> {to}<br>\
             <strong>Czas (UTC):</strong> {now}</p>\
             <p>Jeśli widzisz tę wiadomość, Resend / mailer działa poprawnie.</p>"
        ),
    );
    (subject, html)
}
