# ADR-0006: Local File-Based Persistence

## Status

Draft

## Context

Session state — IDs, statuses, PR associations, branch names, and metadata — must survive orchestrator restarts. The expected data volume is tens to low hundreds of sessions per orchestrator instance. The storage mechanism must support atomic writes (crash safety during updates), concurrent reads from the dashboard and CLI, and be inspectable by humans and shell scripts.

The orchestrator is a developer tool, not a production service. Setup simplicity and zero external dependencies matter more than query power or write throughput.

## Considered Options

1. **Flat key=value files** — One metadata file per session stored in `~/.agent-orchestrator/`. Uses a bash-compatible `KEY=VALUE` format, making files human-readable and directly sourceable in shell scripts. Atomic writes are achieved via writing to a temporary file and renaming (rename is atomic on POSIX filesystems). Deleted sessions are archived with timestamps for audit purposes. New session IDs are reserved race-free using the `O_EXCL` flag on file creation. Path traversal is prevented by validating session ID characters.

2. **SQLite** — A single-file embedded database. Provides ACID transactions, SQL query capability, and concurrent access via WAL mode. Richer query support would simplify dashboard data aggregation. However, it adds a binary dependency (libsqlite3 or a bundled version) and the data is not human-readable without tooling.

3. **External database (Postgres/Redis)** — A full database server offering maximum query power and concurrent access. But it requires setup, a running daemon, and connection configuration. This is overkill for a local developer tool where the data volume is small and the user expectation is zero-config.

## Decision

Option 1 — Flat key=value files.

Zero dependencies and zero setup. Files are human-readable, bash-scriptable, and sufficient for the expected data volume. Hash-based directory namespacing (`~/.agent-orchestrator/{sha256-12chars}-{projectId}/`) allows multiple orchestrator instances to coexist without collision, with `.origin` files to detect hash conflicts.

SQLite (Option 2) remains a viable future upgrade if query complexity grows — the metadata format could be migrated without changing the external API.

## Consequences

**Positive:**
- Zero setup, zero dependencies — the orchestrator works immediately after installation.
- Files are human-readable and bash-scriptable (useful for debugging and custom automation).
- Atomic writes via temp file + rename prevent corruption during crashes.
- Archive-on-delete provides an audit trail of past sessions.
- Works on any OS with a POSIX-compatible filesystem.

**Negative:**
- No complex queries — listing all sessions requires scanning the directory, filtering and sorting happen in application code.
- No transactions spanning multiple session files — updates to two sessions are not atomic relative to each other.
- Performance degrades at very high session counts due to filesystem scan overhead (unlikely to be a practical issue).
- The dashboard must aggregate data from individual files on each request, rather than running a single query.
