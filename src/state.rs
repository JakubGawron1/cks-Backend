use crate::config::Config;
use crate::db::Database;
use crate::mail::Mailer;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub config: Config,
    pub mailer: Mailer,
}
