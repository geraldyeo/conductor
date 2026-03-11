# MVP Test Plan

Covers end-to-end validation of the 10 MVP CLI commands, the orchestrator daemon lifecycle, IPC, and the session lifecycle engine. Unit tests (77 passing) cover internal logic; this plan validates real-world integration with tmux, GitHub, and the filesystem.

## Prerequisites

| Requirement | Verification |
|---|---|
| `rustup` stable toolchain | `rustup show` — active toolchain listed |
| `cargo build --workspace` passes | Binary at `target/debug/ao` |
| `tmux` installed | `tmux -V` |
| `gh` CLI authenticated | `gh auth status` — logged in |
| GitHub repo available for testing | A real repo with at least 1 open issue |
| `~/.agent-orchestrator/` writable | `ls ~/.agent-orchestrator/` or create it |
| No stale orchestrator socket | `ls ~/.agent-orchestrator/orchestrator.sock` — should not exist |

All tests use `target/debug/ao` (or `cargo run -p ao --`) unless the binary is installed in PATH.

## Setup

Run once before all tests:

```
cargo build --workspace
export AO=./target/debug/ao
export TEST_REPO=owner/repo          # replace with real test repo
export TEST_ISSUE=123                # replace with real open issue number
export TEST_PROJECT=my-project       # replace with project ID in config
```

---

## TC-01: `ao init` — Config Generation

**Purpose:** Verify that `ao init` generates a valid config file and the config is discoverable by subsequent commands.

### TC-01-A: Basic init

```
rm -f agent-orchestrator.yaml
$AO init
```

Expected:
- `agent-orchestrator.yaml` created in CWD
- File contains `projects:` key
- No error output

### TC-01-B: Custom output path

```
$AO init -o /tmp/ao-test.yaml
```

Expected:
- `/tmp/ao-test.yaml` created
- No error output

### TC-01-C: Init with --auto

```
rm -f agent-orchestrator.yaml
$AO init --auto
```

Expected:
- Config generated with smart defaults, no interactive prompts
- `agent-orchestrator.yaml` present

### TC-01-D: Init when config already exists

```
$AO init
```

Expected:
- Either: error "config already exists" (exits non-zero)
- Or: prompt to overwrite (should not silently overwrite)

**Cleanup:** `rm -f agent-orchestrator.yaml`

---

## TC-02: `ao start` — Orchestrator Daemon

**Purpose:** Verify that `ao start` starts the poll loop, binds the socket, and is interruptible.

### TC-02-A: Foreground start and Ctrl-C

Terminal 1:
```
$AO start
```

Expected within 5s:
- Logs show startup sequence (config loaded, plugins validated, dirs created, socket bound, poll loop started)
- Socket file exists: `ls ~/.agent-orchestrator/orchestrator.sock`

Press Ctrl-C. Expected:
- Logs show graceful shutdown message
- Socket file removed: `ls ~/.agent-orchestrator/orchestrator.sock` → no such file
- Exit code 0

### TC-02-B: Start while already running

Terminal 1 (keep running):
```
$AO start
```

Terminal 2:
```
$AO start
```

Expected:
- Terminal 2 exits with error: "orchestrator already running" (or similar)
- Exit code non-zero (code 1 or 4)
- Terminal 1's orchestrator unaffected

**Cleanup:** Ctrl-C in Terminal 1

### TC-02-C: Start with missing config

```
cd /tmp && $AO start
```

Expected:
- Error: config not found (exit code 3)

---

## TC-03: `ao stop` — Graceful Shutdown

**Purpose:** Verify IPC `Stop` request causes clean daemon exit.

Setup: Start orchestrator in background tmux pane.

```
tmux new-session -d -s ao-daemon "$AO start"
sleep 2
```

### TC-03-A: Normal stop

```
$AO stop
```

Expected:
- Output: "Orchestrator stopped" (or similar)
- Exit code 0
- Socket file removed
- Daemon tmux pane exits cleanly

### TC-03-B: Stop when not running

Ensure no orchestrator running, then:
```
$AO stop
```

Expected:
- Output: "Orchestrator not running" (or similar)
- Exit code 0 (idempotent)

---

## TC-04: `ao status` — Session Table

**Purpose:** Verify status reads SessionStore directly without requiring the orchestrator.

### TC-04-A: Status with no sessions

(Orchestrator not running, no sessions in store)

```
$AO status
```

Expected:
- Empty table or "No sessions" message
- Exit code 0

### TC-04-B: Status while orchestrator running (no sessions yet)

```
tmux new-session -d -s ao-daemon "$AO start"
sleep 2
$AO status
```

Expected:
- Empty table or "No sessions"
- Exit code 0

### TC-04-C: Status with JSON output

```
$AO status --json
```

Expected:
- Valid JSON on stdout (parseable with `jq`)
- Schema: array of session objects (even if empty array)

**Cleanup:** `$AO stop`

---

## TC-05: `ao spawn` — Single Session Spawn

**Purpose:** Verify that spawning creates a tmux session, worktree, and session metadata.

Setup: Orchestrator running.

### TC-05-A: Happy path spawn

```
tmux new-session -d -s ao-daemon "$AO start"
sleep 2
$AO spawn $TEST_ISSUE
```

Expected:
- Output: session ID printed (e.g., `my-project-123-1`)
- Exit code 0
- `$AO status` shows session in `spawning` or `working` state within 30s
- Worktree created: `ls ~/.agent-orchestrator/*/worktrees/` — contains a directory
- Session metadata file: `ls ~/.agent-orchestrator/*/sessions/*/` — contains `metadata` file
- tmux session exists: `tmux list-sessions` — shows session

### TC-05-B: Spawn with specific agent override

```
$AO spawn $TEST_ISSUE --agent claude-code
```

Expected:
- Session spawned (if `--agent` flag is supported)
- Or: error with clear message if agent not configured

### TC-05-C: Spawn without orchestrator running

(No orchestrator running)

```
$AO spawn $TEST_ISSUE
```

Expected:
- Error: "Orchestrator not running" (exit code 4)

### TC-05-D: Spawn with invalid issue ID

```
$AO spawn 9999999
```

Expected:
- Error from tracker: issue not found or not in active state (exit code 1)
- No worktree or session created

### TC-05-E: Spawn duplicate (same issue already running)

```
$AO spawn $TEST_ISSUE
sleep 2
$AO spawn $TEST_ISSUE
```

Expected:
- Second spawn: error or skip with message "session already exists for this issue"

**Cleanup:** `$AO session kill <session-id>` for spawned sessions, then `$AO stop`

---

## TC-06: `ao batch-spawn` — Multiple Session Spawn

**Purpose:** Verify batch spawn with deduplication and 500ms delay between spawns.

Setup: Orchestrator running. Requires 2–3 open test issues.

### TC-06-A: Batch spawn multiple issues

```
$AO batch-spawn 123 124 125
```

Expected:
- Output: table showing each issue → `spawned`, `skipped`, or `failed`
- Exit code 0 (even if some skipped)
- Spawned sessions appear in `$AO status`

### TC-06-B: Batch spawn with duplicates in the same batch

```
$AO batch-spawn 123 123
```

Expected:
- Second 123 marked as skipped (duplicate detection within batch)
- Only one session created

### TC-06-C: Batch spawn where one issue already has a running session

(Session for 123 already running from TC-05)

```
$AO batch-spawn 123 124
```

Expected:
- 123 skipped (existing session), 124 spawned (or skipped if not in active state)

**Cleanup:** Kill all spawned sessions, `$AO stop`

---

## TC-07: `ao send` — Message Delivery

**Purpose:** Verify message delivery to a running agent session.

Setup: Orchestrator running, at least one session in `working` state.

### TC-07-A: Send short message to running session

```
SESSION_ID=$(...)    # session ID from TC-05 output
$AO send $SESSION_ID "Please summarize what you have done so far."
```

Expected:
- Output: "Delivered" (or delivery confirmation)
- Exit code 0
- Message appears in tmux session (attach to verify: `tmux attach -t $SESSION_ID`)

### TC-07-B: Send from file

```
echo "Please check the test suite is passing." > /tmp/ao-msg.txt
$AO send $SESSION_ID -f /tmp/ao-msg.txt
```

Expected:
- Delivery confirmed
- Exit code 0

### TC-07-C: Send with --no-wait flag

```
$AO send $SESSION_ID "A quick note." --no-wait
```

Expected:
- Immediate return (does not wait for idle)
- Exit code 0

### TC-07-D: Send to non-existent session

```
$AO send nonexistent-session-id "Hello"
```

Expected:
- Error: session not found (exit code 1)

### TC-07-E: Send without orchestrator (direct fallback)

(Stop orchestrator, then try to send)

```
$AO stop
$AO send $SESSION_ID "Direct delivery test." --no-wait
```

Expected:
- Warning: "Orchestrator not running, delivering without busy detection"
- Message delivered directly via tmux send-keys
- Exit code 0

---

## TC-08: `ao session ls` — List Sessions

**Purpose:** Verify `session ls` is an alias for `ao status`.

### TC-08-A: List sessions

```
$AO session ls
```

Expected:
- Same output as `$AO status`
- Exit code 0

### TC-08-B: Filter by project

```
$AO session ls -p $TEST_PROJECT
```

Expected:
- Only sessions for `$TEST_PROJECT` shown (or all if only one project)

---

## TC-09: `ao session kill` — Kill Session

**Purpose:** Verify kill sets the manualKill flag and transitions session to `killed`.

Setup: Orchestrator running, one session in `working` state.

### TC-09-A: Kill running session

```
$AO session kill $SESSION_ID
```

Expected:
- Output: "Session killed" or "Kill scheduled, will complete within 30s"
- Exit code 0
- Within 30s: `$AO status` shows session in `killed` state
- tmux session no longer exists: `tmux list-sessions` — session removed
- Worktree removed or session archived

### TC-09-B: Kill already-terminal session

```
$AO session kill $SESSION_ID   # (already killed from TC-09-A)
```

Expected:
- Error or warning: "Session already in terminal state"
- Exit code 1 or 0 with message

### TC-09-C: Kill non-existent session

```
$AO session kill nonexistent-session
```

Expected:
- Error: session not found (exit code 1)

---

## TC-10: `ao session cleanup` — Clean Up Terminal-Tracker Sessions

**Purpose:** Verify cleanup detects tracker-terminal sessions and kills them.

Setup: Orchestrator running, one session whose tracker issue has been closed (or move one to closed state on GitHub).

### TC-10-A: Cleanup with dry-run

```
$AO session cleanup --dry-run
```

Expected:
- Output: list of sessions that would be killed
- No sessions actually killed
- Exit code 0

### TC-10-B: Cleanup active (no dry-run)

```
$AO session cleanup
```

Expected:
- Sessions with closed/terminal tracker issues → `cleanup` state
- Sessions with active tracker issues → skipped
- Output: table showing killed vs skipped

### TC-10-C: Cleanup with no terminal-tracker sessions

```
$AO session cleanup
```

Expected:
- "Nothing to clean up" or empty killed list
- Exit code 0

---

## TC-11: Full Session Lifecycle Integration

**Purpose:** Validate the complete `spawning → working → pr_open → done` path end-to-end.

This is the most important integration test. It requires a real agent run.

### TC-11-A: Full lifecycle (manual monitoring)

1. Start orchestrator: `$AO start` (in a tmux pane)
2. Spawn session: `$AO spawn $TEST_ISSUE`
3. Monitor status every 30s: `watch -n 30 $AO status`
4. Expected state transitions over time:
   - `spawning` → `working` (within 2 poll ticks)
   - `working` → `pr_open` (when agent opens a PR)
   - `pr_open` → `review_pending` (when CI passes)
   - `review_pending` → `approved` (when PR approved)
   - `approved` → `mergeable` (when CI green + no conflicts)
5. Verify at each step: `$AO status --json | jq '.[0].status'`

### TC-11-B: Crash recovery

1. Start orchestrator with active sessions running
2. Kill the orchestrator process (`Ctrl-C` or `kill`)
3. Restart: `$AO start`
4. Expected:
   - Orchestrator reloads existing sessions from SessionStore
   - Runs one poll tick immediately
   - Sessions return to correct state without manual intervention
   - No duplicate sessions spawned

---

## TC-12: Error Handling and Exit Codes

| Scenario | Expected Exit Code |
|---|---|
| Config not found | 3 |
| Unknown project ID with `-p` | 3 |
| Orchestrator not running (mutating command) | 4 |
| Invalid issue ID | 1 |
| Duplicate spawn | 1 |
| Invalid args / missing required arg | 2 |
| Successful command | 0 |
| `ao stop` when not running | 0 |

### TC-12-A: Verify exit codes

For each scenario above, run the command and check `echo $?` immediately after.

---

## TC-13: IPC Round-Trip

**Purpose:** Verify IPC channel handles concurrent requests correctly.

### TC-13-A: Concurrent spawn requests

```
$AO spawn $TEST_ISSUE &
$AO spawn $TEST_ISSUE &
wait
```

Expected:
- One spawned, one skipped (duplicate detection)
- No crashes or data corruption
- `$AO status` shows exactly one session

### TC-13-B: Send + status concurrently

```
$AO send $SESSION_ID "Hello" &
$AO status &
wait
```

Expected:
- Both complete without error
- No interleaved corrupt output

---

## TC-14: Config Validation

### TC-14-A: Invalid config format

```
echo "not: valid: yaml:" > agent-orchestrator.yaml
$AO status
```

Expected:
- Error: config validation failed (exit code 3)
- Clear message indicating parse error

### TC-14-B: Config with missing required fields

```
echo "projects: {}" > agent-orchestrator.yaml
$AO status
```

Expected:
- Error: config validation failed with field-level detail (exit code 3)

---

## Known Limitations (MVP)

The following are intentional MVP gaps, not bugs:

- `ao start` occupies a terminal (no `--daemon` flag). Use tmux to background it.
- `ao session kill` may take up to one poll interval (default 30s) to complete.
- `ao send` fallback (without orchestrator) skips busy detection — message may interrupt agent mid-turn.
- `ao status` PR/CI/review columns are not shown (post-MVP: requires persisting PollContext fields).
- `ao session restore`, `ao review-check`, `ao dashboard`, `ao open` commands are not implemented.
- `ao init --smart` flag is a no-op (prints "coming in a future release").
- `ao start <url>` (one-command onboarding) is not implemented.

---

## Test Execution Checklist

Run in order. Each TC builds on the environment state of the previous.

- [ ] TC-01: `ao init`
- [ ] TC-02: `ao start`
- [ ] TC-03: `ao stop`
- [ ] TC-04: `ao status`
- [ ] TC-05: `ao spawn`
- [ ] TC-06: `ao batch-spawn`
- [ ] TC-07: `ao send`
- [ ] TC-08: `ao session ls`
- [ ] TC-09: `ao session kill`
- [ ] TC-10: `ao session cleanup`
- [ ] TC-11: Full lifecycle integration
- [ ] TC-12: Exit codes
- [ ] TC-13: IPC round-trip
- [ ] TC-14: Config validation

Record any failures with: command run, actual output, expected output, and exit code observed.
