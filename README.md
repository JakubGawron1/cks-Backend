---
title: Slavia Backend
emoji: 🏋️
colorFrom: green
colorTo: green
sdk: docker
app_port: 8080
pinned: false
license: mit
---

# CKS Slavia — Backend (Axum)

API: logowanie JWT, role łączone (`zawodnik` | `trener` | `admin` | `superadmin`).

Publiczny URL Space: [https://koliber-cks-slavia.hf.space](https://koliber-cks-slavia.hf.space)

## Zasada: Dev vs hosting

| Środowisko | Jak uruchamiać |
|------------|----------------|
| **Dev (lokalnie)** | wyłącznie `cargo run` |
| **Hosting** | Docker — **Hugging Face Space** (`koliber/cks-slavia`) lub Render |

**Nie używaj Dockera do codziennego developmentu.**

## Dev — lokalnie

```bash
cp .env.example .env
cargo run
```

API: `http://127.0.0.1:8080`  
Frontend lokalnie: `NEXT_PUBLIC_API_URL=http://127.0.0.1:8080`

## Deploy — Hugging Face Space (z GitHuba)

Space: [koliber/cks-slavia](https://huggingface.co/spaces/koliber/cks-slavia)  
Źródło: push na `main` w [cks-Backend](https://github.com/JakubGawron1/cks-Backend) → Actions sync → build Space.

### Jednorazowo: `HF_TOKEN` w GitHubie

Secret Actions z tokenem HF (**write** do Spaces). Szczegóły: [deploy.md](./deploy.md).

### Secrets aplikacji (Space Settings → Variables and secrets)

| Klucz | Wymagane | Opis |
|-------|----------|------|
| `PRODUCTION_MODE` | tak | `production` |
| `DATABASE_URL` | tak | `libsql://YOUR-DB.turso.io` (alias: `TURSO_DATABASE_URL`) |
| `TURSO_AUTH_TOKEN` | tak | Token z Turso Dashboard |
| `JWT_SECRET` | tak | Min. 16 znaków (lepiej 32+) |
| `FRONTEND_ORIGIN` | tak* | `https://slavia.vercel.app` (+ opcjonalnie localhost) |
| `SEED_SUPERADMIN_PASSWORD` | tak | Silne hasło (nie `superadmin123!`) |
| `SEED_SUPERADMIN_EMAIL` | nie | Domyślnie `superadmin@cks-slavia.local` |
| `JWT_EXPIRY_HOURS` | nie | Domyślnie `72` |

\* Alias: `CORS_ALLOWED_ORIGINS` (stara nazwa z poprzedniego Space).

Przykład:

```text
PRODUCTION_MODE=production
DATABASE_URL=libsql://slavia-xxx.turso.io
TURSO_AUTH_TOKEN=…
FRONTEND_ORIGIN=https://slavia.vercel.app,http://localhost:3000
```

### Frontend (Vercel)

```env
NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space
```

Po zmianie — **Redeploy** frontendu.

### Deploy

```bash
git push origin main
```

Workflow: `.github/workflows/sync-to-hf.yml`. Status: [Actions](https://github.com/JakubGawron1/cks-Backend/actions) oraz [Space](https://huggingface.co/spaces/koliber/cks-slavia).

Healthcheck:

```bash
curl https://koliber-cks-slavia.hf.space/api/health
```

Pełna instrukcja: [deploy.md](./deploy.md).

## Baza danych

| Tryb | Env | Baza |
|------|-----|------|
| **Dev** | `PRODUCTION_MODE=dev` | lokalny plik `file:./data/slavia.db` (libSQL/SQLite) |
| **Production** | `PRODUCTION_MODE=production` | **Turso** (`libsql://…` + `TURSO_AUTH_TOKEN`) |

## Endpointy

| Metoda | Ścieżka | Auth | Opis |
|--------|---------|------|------|
| GET | `/` | — | Strona index (link do frontendu) |
| GET | `/api/health` | — | Healthcheck |
| POST | `/api/auth/login` | — | `{ email, password }` → JWT + user |
| GET | `/api/auth/me` | Bearer | Profil zalogowanego |

## Konto seed (tylko superadmin)

Przy pustej bazie tworzone jest **wyłącznie** konto z najwyższymi uprawnieniami (email/hasło z env).

## Docker — tylko hosting

```bash
# nie do codziennego dev
docker build -t slavia-backend .
```
