# CKS Slavia — Backend (Axum)

API: logowanie JWT, role łączone (`zawodnik` | `trener` | `admin` | `superadmin`).

## Zasada: Dev vs hosting

| Środowisko | Jak uruchamiać |
|------------|----------------|
| **Dev (lokalnie)** | wyłącznie `cargo run` |
| **Hosting** | Docker (`Dockerfile` + `render.yaml`) na **Render Free** |

**Nie używaj Dockera do codziennego developmentu.**  
**Nie hostujemy na Hugging Face Docker Spaces** — utworzenie wymaga płatnego HF PRO. Instrukcja: [deploy.md](./deploy.md).

## Dev — lokalnie

```bash
cp .env.example .env
cargo run
```

API: `http://127.0.0.1:8080`

Lokalnie: baza plikowa **redb** (`./data/slavia.redb`). Frontend: `NEXT_PUBLIC_API_URL=http://127.0.0.1:8080`.

## Deploy (Render Free)

Szybki start:

1. Push repo na GitHub
2. Render → **New** → **Blueprint** → wybierz to repo (`render.yaml`)
3. Ustaw `FRONTEND_ORIGIN`, `SEED_SUPERADMIN_EMAIL`, `SEED_SUPERADMIN_PASSWORD`
4. `curl https://TWOJ-SERWIS.onrender.com/api/health`

Pełna instrukcja: [deploy.md](./deploy.md).

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

## Docker — tylko hosting (Render)

Obraz pod deploy kontenera na Render. **Nie** pod lokalny workflow i **nie** pod HF Docker Spaces.

```bash
# budowanie pod deploy — nie do codziennego dev
docker build -t slavia-backend .
```

Pliki: `Dockerfile`, `.dockerignore`, `render.yaml`. Instrukcja: [deploy.md](./deploy.md).
