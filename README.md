# CKS Slavia — Backend (Axum)

API: logowanie JWT, role łączone (`zawodnik` | `trener` | `admin` | `superadmin`).

## Zasada: Dev vs hosting

| Środowisko | Jak uruchamiać |
|------------|----------------|
| **Dev (lokalnie)** | wyłącznie `cargo run` |
| **Hosting (Hugging Face)** | Docker (`Dockerfile`) |

**Nie używaj Dockera do codziennego developmentu.** Docker służy tylko do wdrożenia API na Hugging Face.

## Dev — lokalnie

```bash
cp .env.example .env
cargo run
```

API: `http://127.0.0.1:8080`

Lokalnie: baza plikowa **redb** (`./data/slavia.redb`). Frontend: `NEXT_PUBLIC_API_URL=http://127.0.0.1:8080`.

## Produkcja / Turso

```env
DATABASE_URL=libsql://YOUR-DB.turso.io
TURSO_AUTH_TOKEN=...
```

Docelowo warstwa `Database` przełączy się na klienta libsql. Schema SQL poniżej.

### Docelowy schemat SQL (Turso / SQLite)

```sql
CREATE TABLE users (
  id TEXT PRIMARY KEY NOT NULL,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash TEXT NOT NULL,
  display_name TEXT NOT NULL,
  roles TEXT NOT NULL,          -- JSON: ["zawodnik","trener"]
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

## Endpointy

| Metoda | Ścieżka | Auth | Opis |
|--------|---------|------|------|
| GET | `/api/health` | — | Healthcheck |
| POST | `/api/auth/login` | — | `{ email, password }` → JWT + user |
| GET | `/api/auth/me` | Bearer | Profil zalogowanego |

## Konto seed (tylko superadmin)

Przy pustej bazie tworzone jest **wyłącznie** konto z najwyższymi uprawnieniami:

| E-mail | Hasło | Role |
|--------|-------|------|
| `superadmin@cks-slavia.local` | `superadmin123!` | `superadmin` |

Pozostałe role (`admin`, `trener`, `zawodnik`) nadaje później superadmin.

## Docker — tylko Hugging Face

Obraz jest przeznaczony pod Space / kontener HF, **nie** pod lokalny workflow.

```bash
# budowanie pod deploy na Hugging Face — nie do codziennego dev
docker build -t slavia-backend .
```
