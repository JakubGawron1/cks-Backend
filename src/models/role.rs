use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Role klubowe — można łączyć (np. zawodnik + trener).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(rename_all = "lowercase")]
pub enum Role {
    Zawodnik,
    Trener,
    Admin,
    Superadmin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Zawodnik => "zawodnik",
            Role::Trener => "trener",
            Role::Admin => "admin",
            Role::Superadmin => "superadmin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "zawodnik" => Some(Role::Zawodnik),
            "trener" => Some(Role::Trener),
            "admin" => Some(Role::Admin),
            "superadmin" => Some(Role::Superadmin),
            _ => None,
        }
    }
}

pub fn roles_from_json(raw: &str) -> Result<Vec<Role>, serde_json::Error> {
    let as_strings: Vec<String> = serde_json::from_str(raw)?;
    Ok(as_strings
        .into_iter()
        .filter_map(|s| Role::parse(&s))
        .collect())
}

pub fn roles_to_json(roles: &[Role]) -> String {
    let labels: Vec<&str> = roles.iter().map(|r| r.as_str()).collect();
    serde_json::to_string(&labels).unwrap_or_else(|_| "[]".into())
}

pub fn has_role(roles: &[Role], required: Role) -> bool {
    roles.contains(&required) || roles.contains(&Role::Superadmin)
}

pub fn has_any_role(roles: &[Role], required: &[Role]) -> bool {
    if roles.contains(&Role::Superadmin) {
        return true;
    }
    required.iter().any(|r| roles.contains(r))
}
