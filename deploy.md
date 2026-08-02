# Deploy na Render Free (Docker)

## Zasada

| Środowisko | Jak |
|------------|-----|
| **Dev** | wyłącznie `cargo run` |
| **Hosting** | Docker na **Render Free** (`Dockerfile` + `render.yaml`) |

**Nie** używamy Hugging Face Docker Spaces (paywall PRO).

---

## Co jest gotowe w repo

| Plik | Rola |
|------|------|
| `Dockerfile` | multi-stage, `CARGO_BUILD_JOBS=2`, non-root, `PORT`/`HOST` |
| `.dockerignore` | mniejszy kontekst builda |
| `render.yaml` | Blueprint: Free, Frankfurt, healthcheck `/api/health` |
| `GET /api/health` | healthcheck Rendera |

Backend czyta `PORT` (Render wstrzykuje). Na Renderze (`RENDER=true`) wymagane są: `JWT_SECRET`, `FRONTEND_ORIGIN`, `SEED_SUPERADMIN_PASSWORD` (nie domyślne).

---

## 1. Repo na GitHubie

Push `slavia-backend` (osobne repo). **Nie** commituj `.env`, `data/`, `target/`.

---

## 2. Deploy — Blueprint (zalecane)

1. [dashboard.render.com](https://dashboard.render.com) → **New** → **Blueprint**
2. Podłącz repo `slavia-backend`
3. Render wczyta `render.yaml`
4. Uzupełnij zmienne z `sync: false`:

| Zmienna | Przykład |
|---------|----------|
| `FRONTEND_ORIGIN` | `https://twoja-apka.vercel.app` (można listę po przecinku + `http://localhost:3000`) |
| `SEED_SUPERADMIN_EMAIL` | Twój email admina |
| `SEED_SUPERADMIN_PASSWORD` | silne hasło (nie `superadmin123!`) |

`JWT_SECRET` generuje Render automatycznie (`generateValue`).

5. Deploy — pierwszy build Rust: kilka–kilkanaście minut.

Publiczny URL: `https://slavia-backend.onrender.com` (lub podobny).

```bash
curl https://TWOJ-SERWIS.onrender.com/api/health
```

---

## 3. Deploy — ręcznie (bez Blueprint)

1. **New** → **Web Service** → podłącz repo  
2. **Runtime** → **Docker**  
3. **Instance type** → **Free**  
4. **Health Check Path** → `/api/health`  
5. Region: Frankfurt (lub bliżej użytkowników)  
6. Environment — jak w tabeli powyżej + opcjonalnie:

```text
DATABASE_URL=file:/app/data/slavia.redb
JWT_EXPIRY_HOURS=72
RUST_LOG=slavia_backend=info,tower_http=info,axum=info
```

---

## 4. Frontend (Vercel)

```env
NEXT_PUBLIC_API_URL=https://TWOJ-SERWIS.onrender.com
```

Na backendzie `FRONTEND_ORIGIN` = dokładny origin Vercel (scheme + host, bez `/` na końcu).

---

## 5. Baza na Free

`DATABASE_URL=file:/app/data/slavia.redb` — dysk **efemeryczny**: po redeploy / długim śnie dane mogą zniknąć (seed utworzy się ponownie).

Na dłużej: Turso (gdy warstwa `Database` wspiera libsql):

```env
DATABASE_URL=libsql://YOUR-DB.turso.io
TURSO_AUTH_TOKEN=...
```

---

## 6. Limity Free — czego się spodziewać

| Temat | Zachowanie |
|-------|------------|
| Cold start | Sleep po ~15 min bez ruchu; pierwszy request ~30–60 s |
| RAM runtime | 512 MB — binarka Axum wystarczy |
| Build | Czasem wolny / OOM; Dockerfile ma `CARGO_BUILD_JOBS=2` i cache deps — spróbuj ponownie |
| HTTPS | Wbudowane |

---

## 7. Checklist

- [ ] Repo na GitHubie, `.env` poza gitem
- [ ] Blueprint / Web Service Free + Docker
- [ ] `FRONTEND_ORIGIN` = URL Vercel
- [ ] `SEED_SUPERADMIN_PASSWORD` zmienione
- [ ] `curl …/api/health` → `{"status":"ok",…}`
- [ ] Login seed działa
- [ ] Vercel: `NEXT_PUBLIC_API_URL` = URL Render

---

## 8. Typowe problemy

| Objaw | Co zrobić |
|-------|-----------|
| Build OOM / timeout | Redeploy; cache warstw deps powinno pomóc przy kolejnych |
| Crash przy starcie: brak `JWT_SECRET` / `FRONTEND_ORIGIN` | Ustaw env w dashboardzie |
| CORS | Dokładny origin w `FRONTEND_ORIGIN` |
| Utrata danych | Efemeryczny dysk Free — seed od nowa / Turso później |
| Cold start | Normalne na Free |

---

## Skrót

```text
1. Push repo → Render Blueprint (render.yaml) lub Web Service Docker Free
2. Env: FRONTEND_ORIGIN, SEED_SUPERADMIN_PASSWORD (+ JWT_SECRET auto)
3. curl …/api/health
4. Vercel: NEXT_PUBLIC_API_URL=https://….onrender.com
```
