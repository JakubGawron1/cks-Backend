# Changelog — Backend

Notatki developerskie (Rust / Axum). Wspólna wersja z `Slavia.toml` przy braku breaking API.

Format sekcji:

```
## [X.Y.Z] - YYYY-MM-DD
### Tytuł wpisu
- punkt
```

Opcjonalnie po dacie: `!breaking` (breaking API).

## [1.0.0.1+1] - 2026-08-03

### `end_date` dla zawodów

- `CalendarEvent.end_date` (włącznie); brak / równy `date` = jednodniowe.
- Walidacja w create/update: treningi bez zakresu; zawody z opcjonalnym zakresem.
- Publiczne / zawodnik DTO zwracają `end_date` gdy zakres > 1 dzień.

## [1.0.0] - 2026-08-03

### Wspólna wersja OpenAPI

- `info.version` w OpenAPI synchronizowane z `Slavia.toml` (`sync-version`).
- Brak breaking API w tej wersji — klienci (web/mobile) dzielą ten sam numer.

## [1.0.0] - 2026-08-01

### Kalendarz, obecność, RBAC

- `GET /api/events/mine` z `attendance_status`; reconcile auto-absent.
- Endpointy obecności / flag / stats pod panelem superadmina.
- libSQL lokalnie (dev) / Turso w produkcji.
