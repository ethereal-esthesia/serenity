# Engine/Main Separation Flow

![Serenity engine/main separation flow diagram](engine-main-separation-flow.png)

Mermaid source: [`engine-main-separation-flow.mmd`](engine-main-separation-flow.mmd)

This flow defines the target architecture:

1. `main` is orchestration-only (pump, forward, tick, render, present).
2. Input state mutation lives outside `main` in engine/input worker paths.
3. Global capture + local SDL alias resolution converge in one atomic state machine.
4. Watchdog/heartbeat enforces fail-open safety when the consumer stalls.
