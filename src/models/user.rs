use serde::{Deserialize, Serialize};

use super::role::Role;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&UserRecord> for PublicUser {
    fn from(user: &UserRecord) -> Self {
        Self {
            id: user.id.clone(),
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            roles: user.roles.clone(),
            is_active: user.is_active,
        }
    }
}
