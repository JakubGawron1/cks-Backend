use crate::mail::templates;
use crate::mail::Mailer;
use crate::models::user::NotificationPrefs;
use crate::state::AppState;

/// Który przełącznik e-mail sprawdzić (None = bez e-maila, tylko in-app).
#[derive(Debug, Clone, Copy)]
pub enum EmailChannel {
    None,
    Squad,
    TrainingPlan,
    Contact,
}

fn pref_allows(prefs: &NotificationPrefs, channel: EmailChannel) -> bool {
    match channel {
        EmailChannel::None => false,
        EmailChannel::Squad => prefs.email_squad,
        EmailChannel::TrainingPlan => prefs.email_training_plans,
        EmailChannel::Contact => prefs.email_contact,
    }
}

/// In-app zawsze; push FCM (gdy skonfigurowane); e-mail gdy verified + pref + mailer.
pub async fn notify_user(
    state: &AppState,
    user_id: &str,
    title: &str,
    body: &str,
    kind: &str,
    href: Option<&str>,
    email: EmailChannel,
) {
    match state
        .db
        .create_notification(user_id, title, body, kind, href)
        .await
    {
        Ok(notification) => {
            crate::push::deliver_push(state, &notification).await;
        }
        Err(err) => {
            tracing::warn!(error = %err, user_id, "notify_user: in-app failed");
            return;
        }
    }

    if matches!(email, EmailChannel::None) {
        return;
    }

    if !state
        .db
        .is_flag_enabled("experimental_notification_emails")
        .await
    {
        return;
    }

    let Ok(Some(user)) = state.db.find_user_by_id(user_id).await else {
        return;
    };
    if !user.email_verified || !pref_allows(&user.notification_prefs, email) {
        return;
    }

    let origin = Mailer::primary_frontend_origin(&state.config);
    let (subject, html) = templates::notification(title, body, href, origin);
    if let Err(err) = state.mailer.send(&user.email, &subject, &html).await {
        tracing::warn!(error = %err, user_id, "notify_user: email failed");
    }
}

/// In-app do kadry + e-mail wg `email_contact`.
pub async fn notify_staff_email(
    state: &AppState,
    title: &str,
    body: &str,
    kind: &str,
    href: Option<&str>,
    exclude_user_id: Option<&str>,
    email: EmailChannel,
) {
    let staff_roles = [
        crate::models::role::Role::Trener,
        crate::models::role::Role::Admin,
        crate::models::role::Role::Superadmin,
    ];
    let Ok(users) = state.db.list_users().await else {
        return;
    };

    for user in users {
        if !user.is_active {
            continue;
        }
        if let Some(exclude) = exclude_user_id {
            if user.id == exclude {
                continue;
            }
        }
        let is_staff = user.roles.iter().any(|r| staff_roles.contains(r));
        if !is_staff {
            continue;
        }

        notify_user(
            state,
            &user.id,
            title,
            body,
            kind,
            href,
            email,
        )
        .await;
    }
}
