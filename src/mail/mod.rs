//! Wysyłka e-mail (Brevo) + helpery powiadomień.

mod brevo;
mod notify;
pub mod templates;

pub use brevo::Mailer;
pub use notify::{notify_staff_email, notify_user, EmailChannel};

/// Adresy developerskie: domena kończy się na `.dev` lub `.local`.
pub fn is_dev_email(email: &str) -> bool {
    let email = email.trim().to_ascii_lowercase();
    let Some((_, host)) = email.rsplit_once('@') else {
        return false;
    };
    let host = host.trim_end_matches('.');
    host.ends_with(".dev") || host.ends_with(".local")
}

#[cfg(test)]
mod tests {
    use super::is_dev_email;

    #[test]
    fn detects_dev_and_local() {
        assert!(is_dev_email("zawodnik@cks-slavia.local"));
        assert!(is_dev_email("admin@cks-slavia.dev"));
        assert!(is_dev_email("Admin@CKS-Slavia.DEV"));
        assert!(!is_dev_email("user@gmail.com"));
        assert!(!is_dev_email("user@example.com"));
        assert!(!is_dev_email("not-an-email"));
    }
}
