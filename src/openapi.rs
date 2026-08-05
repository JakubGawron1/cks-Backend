use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::models::club::*;
use crate::models::role::Role;
use crate::handlers::debug::{SendTestEmailBody, SendTestEmailResponse};
use crate::models::user::{ErrorBody, OkResponse, NotificationPrefs, PublicUser};
use crate::images::{DeleteImageResponse, ImageProvider, UploadImageResponse};

/// Schemat bezpieczeństwa Bearer JWT.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );
        }
    }
}

/// Bazowy dokument OpenAPI — ścieżki i pozostałe schematy zbiera `OpenApiRouter`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "CKS Slavia API",
        version = "1.1.0.5+2",
        description = "API panelu klubowego CKS Slavia Ruda Śląska"
    ),
    modifiers(&SecurityAddon),
    components(schemas(
        PublicUser,
        OkResponse,
        ErrorBody,
        NotificationPrefs,
        Role,
        AthleteProfile,
        FeatureFlag,
        PublicFlag,
        FlagKind,
        FlagRolloutStatus,
        CompetitionResult,
        ResultStatus,
        AttendanceSession,
        AttendanceRecord,
        CalendarEvent,
        EventWithdrawal,
        WithdrawalStatus,
        TrainingScheduleDefaults,
        PublicCalendarEvent,
        AssignedAthleteBrief,
        AthleteCalendarEvent,
        PlanExercise,
        TrainingPlan,
        PlanProgressEntry,
        TrainingPlanProgress,
        AthleteStats,
        CmsStatus,
        CmsBlock,
        CmsPage,
        LogLevel,
        SystemLog,
        SiteStats,
        HealthResponse,
        ContactMessage,
        Notification,
        UnreadCountResponse,
        DeviceToken,
        SendTestEmailBody,
        SendTestEmailResponse,
        ImageProvider,
        UploadImageResponse,
        DeleteImageResponse,
    )),
    tags(
        (name = "auth", description = "Logowanie i sesja"),
        (name = "users", description = "Konta użytkowników"),
        (name = "flags", description = "Feature flags"),
        (name = "admin", description = "Narzędzia superadmin"),
        (name = "contact", description = "Formularz kontaktowy i skrzynka"),
        (name = "notifications", description = "Powiadomienia in-app"),
        (name = "devices", description = "Tokeny push (FCM)"),
        (name = "uploads", description = "Upload obrazów (ImageKit)"),
        (name = "public", description = "Publiczne endpointy strony klubu"),
    )
)]
pub struct ApiDoc;
