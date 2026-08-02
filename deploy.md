# Deploy backendu (Docker)

## Zasada

| Środowisko | Jak |
|------------|-----|
| **Dev** | wyłącznie `cargo run` |
| **Hosting** | Docker — **Hugging Face Space** lub Render Free |

Aktualny produkcyjny target: [koliber/cks-slavia](https://huggingface.co/spaces/koliber/cks-slavia)  
URL API: `https://koliber-cks-slavia.hf.space`

---

## A) Hugging Face Space (główny)

### Co jest gotowe

| Plik | Rola |
|------|------|
| `Dockerfile` | multi-stage, `CARGO_BUILD_JOBS=2`, port **8080** (`app_port`) |
| `README.md` | YAML frontmatter HF (`sdk: docker`) |
| `GET /api/health` | healthcheck |
| `GET /` | strona index → link do Vercel |

Na Space (`SPACE_ID` ustawione) wymagane: `JWT_SECRET`, `FRONTEND_ORIGIN` (lub `CORS_ALLOWED_ORIGINS`), `SEED_SUPERADMIN_PASSWORD` (nie domyślne).

### 1. Secrets w Space

[Settings → Variables and secrets](https://huggingface.co/spaces/koliber/cks-slavia/settings):

| Zmienna | Przykład |
|---------|----------|
| `JWT_SECRET` | `openssl rand -hex 32` |
| `FRONTEND_ORIGIN` | `https://slavia.vercel.app,http://localhost:3000` |
| `SEED_SUPERADMIN_EMAIL` | Twój email |
| `SEED_SUPERADMIN_PASSWORD` | silne hasło |

Usuń / zignoruj stare klucze starego monorepo (`TURSO_*`, `GROQ_*`, itd.), jeśli nie są używane.

### 2. Zastąpienie starego kodu (force push)

```bash
cd slavia-backend
hf auth login   # token z uprawnieniem write do Spaces
git remote add hf https://huggingface.co/spaces/koliber/cks-slavia
# jeśli remote już jest:
# git remote set-url hf https://huggingface.co/spaces/koliber/cks-slavia
git push hf main --force
```

HF zbuduje obraz z `Dockerfile` (pierwszy build Rust: długo).  
Status: [Space](https://huggingface.co/spaces/koliber/cks-slavia) → Building → Running.

### 3. Weryfikacja

```bash
curl https://koliber-cks-slavia.hf.space/api/health
curl https://koliber-cks-slavia.hf.space/
```

### 4. Frontend (Vercel)

```env
NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space
```

**Redeploy** frontendu po zmianie `NEXT_PUBLIC_*`.

### Baza na Space

Dysk efemeryczny — `file:/app/data/slavia.redb` może znikać po restarcie. Seed utworzy superadmina od nowa.

---

## B) Render Free (alternatywa)

Szczegóły poniżej — ten sam `Dockerfile`, `render.yaml`.

Backend czyta `PORT`. Na Renderze (`RENDER=true`) te same wymagane sekrety co na HF.

### Blueprint

1. [dashboard.render.com](https://dashboard.render.com) → **New** → **Blueprint**
2. Podłącz repo → `render.yaml`
3. Ustaw `FRONTEND_ORIGIN`, `SEED_SUPERADMIN_*`

### Ręcznie

**New** → **Web Service** → Docker → Free → Health Check `/api/health`

---

## Checklist (HF)

- [ ] Secrets: `JWT_SECRET`, `FRONTEND_ORIGIN`, `SEED_SUPERADMIN_PASSWORD`
- [ ] Force push obecnego `main` na Space
- [ ] `curl …/api/health` → ok
- [ ] Vercel: `NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space` + redeploy
- [ ] Login z frontendu działa (CORS = origin Vercel)

---

## Typowe problemy

| Objaw | Co zrobić |
|-------|-----------|
| Crash przy starcie: brak env | Ustaw secrets w Space Settings |
| Build OOM / timeout | Redeploy; `CARGO_BUILD_JOBS=2` już w Dockerfile |
| CORS / Failed to fetch | `FRONTEND_ORIGIN=https://slavia.vercel.app` + poprawne `NEXT_PUBLIC_API_URL` |
| Stary kod na Space | `git push hf main --force` z tego repo |
| Utrata danych | Efemeryczny dysk — oczekiwane przy redb |

---

## Skrót HF

```text
1. Secrets w Space (JWT, FRONTEND_ORIGIN, SEED password)
2. hf auth login && git push hf main --force
3. curl https://koliber-cks-slavia.hf.space/api/health
4. Vercel: NEXT_PUBLIC_API_URL=https://koliber-cks-slavia.hf.space
```
