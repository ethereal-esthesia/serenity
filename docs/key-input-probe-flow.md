# Key Input Probe Flow

![Serenity key input probe flow](key-input-probe-flow.png)

Mermaid source: [`key-input-probe-flow.mmd`](key-input-probe-flow.mmd)
Diagram render script: [`../scripts/docs/render_diagrams.sh`](../scripts/docs/render_diagrams.sh)

## Notes

- Non-mod, non-functional keys are candidates for probe passthrough.
- Unresolved probe keys are withheld from returned app events/state until probe lock.
- Probe lock is driven by SDL keydown alias feedback (`try_lock_probe_alias`).
- Probe timeout is 4ms; on timeout the unresolved probe key is dropped and can reprobe on next occurrence.
- Probe lifecycle debug logs:
  - `probe_start`
  - `probe_locked`
  - `probe_timeout_drop`
