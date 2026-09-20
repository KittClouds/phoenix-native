# LT9-LA2-E1Y protocol

E1Y is a diagnostic-only credit-routing stress assay. It does not change
LA2-B, the memory contract, or serving behavior. A fixed event stream creates
overlapping obligations, opposite polarity, capacity pressure, and witnesses
that arrive after a semantic deadline.

The four arms are:

- `trace_only`: route by the largest decaying eligibility trace; there is no
  pending tag or bounded pending queue.
- `binary_pending`: route the oldest live pending obligation for a slot.
- `pending_confidence`: route the highest-confidence live pending obligation,
  preferring the witness polarity when one exists.
- `pending_confidence_expiry`: use the same confidence and polarity policy,
  with the expiry policy below and a seven-obligation pending bound. If no
  live obligation has the witness polarity, it rejects the witness rather than
  falling back to the opposite polarity.

Semantic expiry is evaluated before each event for every live obligation.

1. `deadline`: an obligation expires when `event_tick - created_tick > 32`.
2. `superseded_by_opposite`: a new obligation with the same owner and slot but
   the opposite polarity expires the older live obligation immediately.
3. `capacity_eviction`: when a bounded pending arm is full, it evicts one
   obligation. Binary pending evicts the oldest; confidence arms evict the
   lowest-confidence entry, breaking ties by age. The new entry may itself be
   rejected when it is the lowest-confidence entry.

Expiry is terminal. A late witness for an expired obligation is ignored by the
expiry arm. The other arms retain a shadow semantic-expiry mark so the receipt
can count stale-credit resurrection when they update from that old obligation.
An obligation is eligible for the starvation metric only while it has not
expired and has not already received its correct witness. This keeps late,
semantically invalid witnesses separate from valid routing failures.

The credit-routing gate requires `pending_confidence_expiry` to have zero
wrong-owner updates, polarity errors, stale-credit resurrections, and valid
obligation starvation, while keeping peak unresolved obligations at or below
the seven-entry capacity. The result remains a diagnostic gate; it does not
authorize promotion into LA2-B or serving.
