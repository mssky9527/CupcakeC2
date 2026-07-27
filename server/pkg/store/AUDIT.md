# Store layer SQL safety audit (2026-07-24)

## Method
- Grepped `server/pkg/store/*.go` and `server/**/*.go` for `Raw(`, `Exec(`, and string-built SQL.
- Reviewed all `Where`/`Delete`/`Update` call sites.

## Findings

| File | Pattern | Result |
|------|---------|--------|
| `agent_store.go` | `Where("uuid = ?", uuid)` | Parameterized ✅ |
| `command_store.go` | `Where("req_id = ?", …)` / agent_uuid | Parameterized ✅ |
| `config_store.go` | `Where("key = ?", key)` | Parameterized ✅ |
| `listener_store.go` | `Where("id = ?", id)` | Parameterized ✅ |
| `tunnel_store.go` | `Where("port = ?", port)` | Parameterized ✅ |
| `user_store.go` | `Where("username = ?", …)` | Parameterized ✅ |
| `db.go` | settings key counts | Parameterized ✅ |
| Whole server | `Raw(`/`Exec(` SQL | **None** found for SQL (only unrelated names) |

## Residual risks (not SQLi)
- Path construction `fmt.Sprintf("task_%s.txt", reqID)` — ensure reqID charset validated at API edge.
- Mass assignment via GORM `Save` on models — controller binding should restrict fields (product concern).

## Conclusion
**No string-concatenated SQL injection vectors in store layer.** All queries use GORM placeholders.
