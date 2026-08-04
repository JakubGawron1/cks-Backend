# Changelog — Backend

Notatki developerskie (Rust / Axum). Wspólna wersja z `Slavia.toml` przy braku breaking API.

Format sekcji:

```
## [X.Y.Z] - YYYY-MM-DD
### Tytuł wpisu
- punkt
```

Opcjonalnie po dacie: `!breaking` (breaking API).

## [1.0.0.2+6] - 2026-08-04

### Powiadomienia: usuwanie

- `DELETE /api/notifications/{id}` — właściciel może usunąć swoje powiadomienie.

## [1.0.0.2+5] - 2026-08-04

### Mail: Resend → Brevo

- Provider e-mail: Brevo (`BREVO_API_KEY`, `EMAIL_FROM` jako zweryfikowany sender).
- Usunięto `RESEND_API_KEY` / klienta Resend.

## [1.0.0.2+4] - 2026-08-04

### DevTools: testowy e-mail

- `POST /api/admin/debug/send-test-email` (superadmin) — wysyłka testowa przez Resend / log w dev.

## [1.0.0.2+3] - 2026-08-03

### E-mail (Resend): weryfikacja, reset, powiadomienia

- Moduł `mail` — Resend HTTPS (`RESEND_API_KEY`, `EMAIL_FROM`, `EMAIL_ENABLED`); w dev log zamiast wysyłki.
- Pola użytkownika: `email_verified`, `pending_email`, `notification_prefs`; KV `email_tokens`.
- Auto-weryfikacja adresów z domeną `.dev` / `.local`.
- Endpointy: `POST /api/auth/email/request-verification`, `confirm`, `forgot-password`, `reset-password`.
- E-mail + in-app: skład zawodów (w tym wypisanie), plany treningowe, kontakt do kadry; potwierdzenie formularza do nadawcy.

## [1.0.0.2+3] - 2026-08-03

### Push FCM + device tokens

- `POST/DELETE /api/devices` — rejestracja tokenów FCM per użytkownik (KV `device_tokens`).
- Przy `notify_user` wysyłka FCM (legacy HTTP) gdy ustawione `FCM_SERVER_KEY`; invalid tokeny usuwane.
- OpenAPI: schemat `DeviceToken`, tag `devices`.

## [1.0.0.1+1] - 2026-08-03

### `end_date` dla zawodów

- `CalendarEvent.end_date` (włącznie); brak / równy `date` = jednodniowe.
- Walidacja w create/update: treningi bez zakresu; zawody z opcjonalnym zakresem.
- Publiczne / zawodnik DTO zwracają `end_date` gdy zakres > 1 dzień.

### Fix: wydajność `/api/events/mine`

- Jednorazowe `list_profiles` + `list_attendance` przy budowie widoku zawodnika (wcześniej per event).
- `reconcile_past_training_attendance_since_days` — batch + limit dni (mine: 21, attendance: 62).
- Widoczność: `club_assigned` **lub** `all_athletes` **lub** skład; treningi bez rozdmuchanej listy `assigned_athletes`.

## [1.0.0] - 2026-08-03

### Wspólna wersja OpenAPI

- `info.version` w OpenAPI synchronizowane z `Slavia.toml` (`sync-version`).
- Brak breaking API w tej wersji — klienci (web/mobile) dzielą ten sam numer.

## [1.0.0] - 2026-08-01

### Kalendarz, obecność, RBAC

- `GET /api/events/mine` z `attendance_status`; reconcile auto-absent.
- Endpointy obecności / flag / stats pod panelem superadmina.
- libSQL lokalnie (dev) / Turso w produkcji.
