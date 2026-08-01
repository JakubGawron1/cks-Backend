use crate::config::Config;
use crate::db::Database;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub config: Config,
}
