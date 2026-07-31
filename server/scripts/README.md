# Server scripts

| Script | Purpose |
|--------|---------|
| `test-services.ps1` | Safe `go test ./services -tags nodonut` (avoids AV on Donut) |
| `build-frontend.ps1` | `frontend-v2` → `dist/` (go:embed) |
| `build-inject-module.ps1` | Build L2 inject DLL → `storage/modules/inject.bin` |

## Frontend (M-011)

Embed path is **only** `server/dist`. Do not use `server/ui`.

```powershell
powershell -File scripts/build-frontend.ps1
```

## `test-services.ps1` (recommended)

AV often quarantines `services.test.exe` because package `services` used to always
link **go-donut** (shellcode generator). Daily tests should use:

```powershell
cd server
powershell -File scripts/test-services.ps1
```

This runs:

```text
go test ./services/ -tags nodonut -count=1
```

With `-tags nodonut`, `ToShellcodeFromBytes` is a stub (`donut_service_stub.go`), so the
test binary does not embed Donut.

### Full Donut / fileless conversion tests

```powershell
powershell -File scripts/test-services.ps1 -WithDonut
# or compile only under TEMP:
powershell -File scripts/test-services.ps1 -WithDonut -Compile
```

If AV still kills the binary, exclude the folder or run on a lab VM without real-time scan on `%TEMP%`.

### Never do this on a scanned volume

```powershell
go test -c ./services/          # writes services.test.exe into server/ → often deleted
```

Production `go build` is unchanged (Donut included; no `nodonut` tag).
