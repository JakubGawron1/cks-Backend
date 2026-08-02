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

## Deploy — Hugging Face Space

Space: [koliber/cks-slavia](https://huggingface.co/spaces/koliber/cks-slavia)

### Secrets (Settings → Variables and secrets)

| Klucz | Wymagane | Opis |
|-------|----------|------|
| `JWT_SECRET` | tak | Min. 16 znaków (lepiej 32+) |
| `FRONTEND_ORIGIN` | tak* | `https://slavia.vercel.app` (+ opcjonalnie localhost) |
| `SEED_SUPERADMIN_PASSWORD` | tak | Silne hasło (nie `superadmin123!`) |
| `SEED_SUPERADMIN_EMAIL` | nie | Domyślnie `superadmin@cks-slavia.local` |
| `JWT_EXPIRY_HOURS` | nie | Domyślnie `72` |

\* Alias: `CORS_ALLOWED_ORIGINS` (stara nazwa z poprzedniego Space).

Przykład:

```text
FRONTEND_ORIGIN=https://slavia.vercel.app,http://localhost:3000
```

### Frontend (Vercel)

```env
NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space
```

Po zmianie — **Redeploy** frontendu.

### Push na Space (zastąpienie starego kodu)

```bash
git remote add hf https://huggingface.co/spaces/koliber/cks-slavia
# lub: git remote set-url hf ...
git push hf main --force
```

Wymaga zalogowania: `hf auth login` (token z write do Spaces).

Healthcheck:

```bash
curl https://koliber-cks-slavia.hf.space/api/health
```

Pełna instrukcja: [deploy.md](./deploy.md).

## Uwaga: baza na Space

`DATABASE_URL=file:/app/data/slavia.redb` — dysk Space jest **efemeryczny**. Po restarcie/rebuild dane mogą zniknąć (seed od nowa).

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
