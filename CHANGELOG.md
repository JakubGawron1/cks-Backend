# Changelog — Backend

Notatki developerskie (Rust / Axum). Wspólna wersja z `Slavia.toml` przy braku breaking API.

Format sekcji:

```
## [X.Y.Z] - YYYY-MM-DD
### Tytuł wpisu
- punkt
```

Opcjonalnie po dacie: `!breaking` (breaking API).

## [1.1.0.5+1] - 2026-08-05

### Fix: poprawka zaakceptowanego wyniku przez zawodnika

- Zawodnik może edytować też wynik `accepted`; po zapisie status → `pending` + powiadomienie kadry.

## [1.0.0.3+23] - 2026-08-05

### Feature: edycja wyników (kadra + zawodnik)

- `PATCH /api/results/{id}`: opcjonalne pola wyniku (nazwa, data, ciężary, masa, miejsce) + opcjonalny status.
- Kadra: edycja `pending` / `needs_edit` / `accepted` (poprawki); po zapisie zaakceptowanego — sync kategorii w profilu.
- Zawodnik: edycja własnych `pending` / `needs_edit` → wraca do `pending`.

## [1.0.0.3+22] - 2026-08-05

### Feature: auto kategoria przy zapisie profilu

- `POST/PATCH /api/profiles`: gdy są masa, data urodzenia i płeć — `category` wyliczana z tabel 2026 (nadpisuje ręczną wartość).

## [1.0.0.3+21] - 2026-08-05

### Feature: kategoria z zawodów → profil

- Po akceptacji wyniku z zawodów (weryfikacja albo wpis kadry) `category` + `bodyweight_kg` trafiają do `AthleteProfile`.
- Statystyki panelu i listy kont biorą kategorię z profilu — wahania wagi przy krawędzi kategorii rozwiązane ważeniem na zawodach.

## [1.0.0.3+20] - 2026-08-05

### Feature: `event_date` w wynikach

- `CompetitionResult.event_date` (YYYY-MM-DD) + wymagane w `POST /api/results` (zawody i trening).
- Kategoria wagowa nadal z podanej masy ciała + tabel JSON i wieku/płci z profilu — data wydarzenia jej nie zmienia.

## [1.0.0.3+19] - 2026-08-05

### Feature: auto kategoria wagowa (2026)

- `POST /api/results` (zawody): kategoria z profilu (`birth_date`, `sex`) + `bodyweight_kg` wg tabel U15–Senior.
- Opcjonalne `profile_id` (staff); masa ciała wymagana; ręczne `category` ignorowane dla zawodów.
- Moduł `weightlifting_categories` + testy jednostkowe.

## [1.0.0.3+18] - 2026-08-05

### Flagi e-mail

- Katalog: `experimental_notification_emails` (OFF) — gate w `notify_user` dla kanałów Squad/TrainingPlan/Contact.
- Stable ON: `email_password_reset`, `email_verification`, `email_contact_confirmation`, `email_test` + `Database::is_flag_enabled`.
- Public flags: ekspozycja `experimental_notification_emails`.

## [1.0.0.3+16] - 2026-08-05

### Wyniki: trening bez wymaganej nazwy

- `POST /api/results` przy `kind=training`: puste `event_name` → domyślnie „Trening”; nazwa wymagana tylko dla zawodów.

## [1.0.0.3+15] - 2026-08-05

### Fix: CORS dla X-View-As-User

- `CorsLayer.allow_headers` obejmuje `x-view-as-user` — bez tego przeglądarka nie wysyłała nagłówka podglądu.

## [1.0.0.3+14] - 2026-08-04

### Podgląd kont — View-As (read-only)

- `AuthUser.view_as` + nagłówek `X-View-As-User` (tylko superadmin).
- `GET /api/auth/me` i scoped reads (`plans`, `notifications`, `athlete/stats`, `results?mine`, `my-events`, attendance) używają `effective_id`.
- Mutacje przy aktywnym View-As → 403 (wyjątek: `preview/start|stop`).
- `preview/start` odrzuca nieaktywne / własne konto.

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
