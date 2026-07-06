# SSER+ Algorithm Mapping

This crate implements Algorithm 1 from:

Pierre Sutra, Patrick Marlier, Valerio Schiavoni, and François Trahay. 2018.
Boosting Transactional Memory with Stricter Serializability.
In Coordination Models and Languages: 20th IFIP WG 6.1 International Conference, COORDINATION 2018, Held as Part of the 13th International Federated Conference on Distributed Computing Techniques, DisCoTec 2018, Madrid, Spain, June 18-21, 2018.
Proceedings. Springer-Verlag, Berlin, Heidelberg, 231–251. https://doi.org/10.1007/978-3-319-92408-3_11

## Variant

The initial implementation uses the disjoint-access parallel variant discussed in
Section 3.5. Process clocks are not read or advanced; transaction-local clocks are still
used to avoid validating the snapshot on every read.

## Pseudocode Mapping

- `loc(x)` maps to `VarControlBlock::state`, a mutex-protected `(value, timestamp)`.
- `lock(x)`, `isLocked(x)`, and `unlock(x)` map to `VarControlBlock::owner`, an atomic
  transaction id.
- `clock(T)` maps to `Transaction::clock`.
- `rs(T)` maps to `Transaction::reads`, keyed by `TVar` identity and storing observed
  timestamps.
- `ws(T)` maps to `Transaction::writes`, keyed by `TVar` identity and storing deferred
  type-erased values.
- `extend(T, t)` maps to `Transaction::extend`, which validates non-obsolete reads and
  advances the transaction clock.
- `commit(T)` maps to `Transaction::commit`, which validates, advances the transaction
  clock for write transactions, publishes all deferred writes with one timestamp, wakes
  retry waiters, and unlocks written variables.

`Transaction::or` marks reads from a retried first branch as obsolete. They are retained
for `wait-on-retry` wakeups but skipped by snapshot validation, matching the existing
`fast-stm` branch-composition semantics.
