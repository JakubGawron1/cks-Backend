# Deploy backendu (Docker)

## Zasada

| Środowisko | Jak |
|------------|-----|
| **Dev** | wyłącznie `cargo run` (`PRODUCTION_MODE=dev`, lokalny plik SQLite) |
| **Hosting** | Docker na **Hugging Face Space** — deploy z **GitHuba** (Actions) + **Turso** |

Aktualny produkcyjny target: [koliber/cks-slavia](https://huggingface.co/spaces/koliber/cks-slavia)  
URL API: `https://koliber-cks-slavia.hf.space`  
Repo źródłowe: [JakubGawron1/cks-Backend](https://github.com/JakubGawron1/cks-Backend)

---

## A) Hugging Face Space przez GitHub (główny)

Push na `main` w GitHubie → workflow **Sync to Hugging Face Space** → mirror na Space → build Dockera.

| Plik | Rola |
|------|------|
| `.github/workflows/sync-to-hf.yml` | sync GitHub → HF (`huggingface/hub-sync`) |
| `Dockerfile` | multi-stage, `CARGO_BUILD_JOBS=2`, port **8080** |
| `README.md` | YAML frontmatter HF (`sdk: docker`) |

### 1. Jednorazowo: secret `HF_TOKEN` w GitHubie

1. Token HF z uprawnieniem **write** do Spaces: [huggingface.co/settings/tokens](https://huggingface.co/settings/tokens)
2. W repo GitHub: **Settings → Secrets and variables → Actions → New repository secret**
   - Name: `HF_TOKEN`
   - Value: token z kroku 1

Albo CLI:

```bash
cd slavia-backend
gh secret set HF_TOKEN
# wklej token HF
```

### 2. Secrets aplikacji w Space

[Settings → Variables and secrets](https://huggingface.co/spaces/koliber/cks-slavia/settings):

| Zmienna | Przykład |
|---------|----------|
| `PRODUCTION_MODE` | `production` |
| `DATABASE_URL` | `libsql://YOUR-DB-NAME-ORG.turso.io` |
| `TURSO_AUTH_TOKEN` | token z Turso Dashboard |
| `JWT_SECRET` | `openssl rand -hex 32` |
| `FRONTEND_ORIGIN` | `https://slavia.vercel.app,http://localhost:3000` |
| `SEED_SUPERADMIN_EMAIL` | Twój email |
| `SEED_SUPERADMIN_PASSWORD` | silne hasło |
| `BREVO_API_KEY` | klucz API [Brevo](https://app.brevo.com/settings/keys/api) (opcjonalnie) |
| `EMAIL_FROM` | np. `Slavia <twoj@email.pl>` — adres zweryfikowany w Brevo → Senders |
| `EMAIL_ENABLED` | `true` / `false` (bez zmiennej: w prod włączone gdy jest klucz) |

**Brevo bez własnej domeny:** utwórz API key, zweryfikuj sender (zwykle e-mail konta Brevo) i ustaw `EMAIL_FROM` na ten adres. Pełna wysyłka z `noreply@domena.pl` wymaga domeny w Brevo → Domains.

Alias URL: `TURSO_DATABASE_URL` (jeśli ustawisz zamiast `DATABASE_URL`).

### Turso — szybki start

1. Utwórz bazę w [Turso](https://turso.tech/).
2. Skopiuj URL (`libsql://…`) i auth token.
3. Wklej jako secrets Space (powyżej).

### 3. Deploy

```bash
cd slavia-backend
git push origin main
```

Albo ręcznie: GitHub → **Actions** → **Sync to Hugging Face Space** → **Run workflow**.

Status: [Space](https://huggingface.co/spaces/koliber/cks-slavia) → Building → Running.  
Workflow: [Actions](https://github.com/JakubGawron1/cks-Backend/actions).

### 4. Weryfikacja

```bash
curl https://koliber-cks-slavia.hf.space/api/health
curl https://koliber-cks-slavia.hf.space/
```

### 5. Frontend (Vercel)

```env
NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space
```

**Redeploy** frontendu po zmianie `NEXT_PUBLIC_*`.

### Baza na Space

Trwałe dane: **Turso**. Lokalny plik w kontenerze nie jest używany przy `PRODUCTION_MODE=production`.

### Awaryjnie (bez GitHuba)

Tylko gdy Actions nie działa — nie używaj na co dzień:

```bash
git push hf main --force
```

---

## B) Render Free (alternatywa)

Ten sam `Dockerfile`, `render.yaml`. Te same sekrety co na HF (w tym Turso).

### Blueprint

1. [dashboard.render.com](https://dashboard.render.com) → **New** → **Blueprint**
2. Podłącz repo → `render.yaml`
3. Ustaw `DATABASE_URL`, `TURSO_AUTH_TOKEN`, `FRONTEND_ORIGIN`, `SEED_SUPERADMIN_*`

---

## Checklist (HF)

- [x] GitHub secret `HF_TOKEN` (write do Spaces)
- [x] Space secrets: `PRODUCTION_MODE=production`, `DATABASE_URL`, `TURSO_AUTH_TOKEN`, `JWT_SECRET`, `FRONTEND_ORIGIN`, `SEED_SUPERADMIN_PASSWORD`
- [x] `git push origin main` → zielony workflow Actions
- [x] `curl …/api/health` → ok
- [x] Vercel: `NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space` + redeploy
- [x] Login z frontendu działa (CORS = origin Vercel)

---

## Typowe problemy

| Objaw | Co zrobić |
|-------|-----------|
| Actions: auth / 401 | Sprawdź `HF_TOKEN` (write) i dostęp do `koliber/cks-slavia` |
| Crash przy starcie: brak env | Ustaw secrets w Space Settings (w tym Turso) |
| Build OOM / timeout | Redeploy; `CARGO_BUILD_JOBS=2` już w Dockerfile |
| CORS / Failed to fetch | `FRONTEND_ORIGIN=https://slavia.vercel.app` + poprawne `NEXT_PUBLIC_API_URL` |
| Stary kod na Space | Push na `main` albo **Run workflow**; sprawdź logi Actions |
| Błąd Turso / auth | Sprawdź `DATABASE_URL` (`libsql://`) i `TURSO_AUTH_TOKEN` |

---

## OpenAPI / typy frontendu

Źródło prawdy: adnotacje `utoipa` w backendzie (`OpenApiRouter`).

| URL na Space | Opis |
|--------------|------|
| `/api/openapi.json` | Surowy OpenAPI 3 |
| `/api/docs` | **Scalar** |
| `/api/swagger` | **Swagger UI** |

### Regeneracja typów (Orval + React Query)

```text
cd slavia-backend
cargo test export_openapi -- --ignored

cd ../slavia-frontend
pnpm gen:api
```

Commituj w parze: `slavia-frontend/openapi/openapi.json` oraz `slavia-frontend/lib/api/generated/**`.

Feature flags: katalog (klucz, kind, opis, rollout_status) żyje w backendzie (`sync_flag_catalog` przy starcie). DevTools: `GET /api/admin/flags` (superadmin). Publiczne: `GET /api/flags/public` (bez auth) — steruje m.in. `/blog`, `/ogloszenia`, motywami paneli.

---

## Skrót

```text
1. gh secret set HF_TOKEN  (jednorazowo)
2. Secrets Space: Turso + JWT + FRONTEND_ORIGIN + SEED
3. git push origin main
4. curl https://koliber-cks-slavia.hf.space/api/health
5. Vercel: NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space
```
