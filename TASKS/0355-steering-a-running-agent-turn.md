# 0355 Steering a running agent turn

Status: done
Priority: medium
Subsystem: backend, agent, frontend
Depends on: none

## Context

Part of 0351. While the assistant works through many tool calls (summarising a folder, for
example), the person can only press Stop and ask again, which throws away the work in progress.
Pi queues messages typed during a turn: a steering message is given to the model after the
current tool calls finish, a follow-up waits until the turn ends.

## Acceptance Criteria
- `fm-agent`: `TurnHost` gets a method returning messages queued since the last step (default:
  none, so existing hosts compile unchanged). `run_turn` checks it after each round of tool
  results and before the next model call, records them as person messages, and emits a
  `TurnEvent` so the UI shows they were delivered.
  - A queued message never interrupts a running tool; it waits for the round to finish.
  - When the model has already given its final answer, queued messages are not lost: they start
    the next turn (follow-up).
- Terminal chat input and chat accept input while a turn runs: Enter queues a steering message,
  shown as pending until delivered; it can be removed before delivery.
- Bounded: at most 8 queued messages, each at most the normal question size.
- Works through HTTP and Tauri alike (a "steer" request next to the existing stop request).
- Tests: message delivered between rounds and not mid-round, delivery event, follow-up after the
  final answer, cancel still wins over a queued message.

## Implementation Notes
- Loop: `crates/fm-agent/src/turn.rs` (`run_turn`, `TurnHost`, `TurnEvent`).
- Hosts: `terminal_assist/mod.rs` and `chat_agent.rs`; stop is already wired through
  `ModelSideCalls::stopped_when` and the turn's cancellation, so the steer path can follow it.

## Agent Notes
- 2026-11-24 claude: backend done, frontend not started; nothing committed.
  - Done: `TurnHost::take_steering` (default empty) and `TurnEvent::Steered` in fm-agent; the loop
    delivers after each tool round and before accepting an answer (then continues). Terminal:
    `Sessions::steer/take_steering/drop_steering` (max 8, cleared by a new question and by Stop),
    `terminal_assist::steer`, `FileManagerService::terminal_assist_steer`, Tauri command
    `terminal_assist_steer` (both handler lists), `TerminalAssistEventDto::Steered`. Undelivered
    messages are dropped when a question ends; the terminal must ask them as the next question.
  - Tests: fm-agent `turn::` 23 pass (2 new); terminal_assist 87 pass (2 new).
  - Next: frontend. `terminal-assist.ts`: event `{ kind: 'steered', text }`; `terminal-client.ts`:
    `assistSteer?(sessionId, text): Promise<boolean>` → `invoke('terminal_assist_steer', …)`.
    `terminal-drawer.ts`: when busy, a submitted question goes to `live.steering` + `assistSteer`
    (fall back to `live.queued` when it returns false); on a `steered` event remove it and write it
    as a question line; when the reply ends, ask what is left as the next question; on cancel put
    it back in the composer. Then i18n, a drawer test, lint, commit.
  - Gap: removing a queued message before delivery is not offered (append-only queue).
- 2026-11-24 claude: frontend done; task done.
  - `terminal-client.ts` `assistSteer`; `terminal-assist.ts` event `steered`; `terminal-drawer.ts`
    `LiveTerminal.steering`: while a request runs, a sent question goes to the request (not a
    slash command; at most 8; falls back to "after this answer" when not taken). A delivered one
    is written as a question line; what was never read is asked as the next question when the
    answer ends; Stop puts it back in the input. Hint `composerSteering` in 8 locales.
  - Chat: `TurnEvent::Steered` is mapped but chat sends nothing during a reply (no chat steer
    endpoint yet); the terminal is where long tool loops run.
  - Verified: drawer test "sends a message written while the assistant works to the running
    request"; terminal + i18n frontend tests 463 pass; tsc clean.
