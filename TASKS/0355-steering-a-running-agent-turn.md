# 0355 Steering a running agent turn

Status: open
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
