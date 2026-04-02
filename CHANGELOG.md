# CHANGELOG

Breaking changes are **highlighted using bold**.

## 0.7.1

### Fixed

* `TVar::replace` and `TVar::modify` insert a read-write log in transaction registers
  by @imrn99 in https://github.com/imrn99/fast-stm/pull/34

### Misc

* doc: mention (lack of) opacity
  by @imrn99 in https://github.com/imrn99/fast-stm/pull/35

---

## 0.7.0

### New

* (bench) add `RwLock` to read/write time micro-benchmarks
  by @imrn99 in https://github.com/imrn99/fast-stm/pull/30
* (ci) check dock generation on PR, publish it on merge
  by @imrn99 in https://github.com/imrn99/fast-stm/pull/31

### Changed

* **rename `TVar::replace` to `TVar::exchange`**
  by @imrn99 in https://github.com/imrn99/fast-stm/pull/29

---

## 0.6.3

### Changed

* change `TVar::modify`/`TVar::replace` internals to avoid double access
  by @imrn99 in https://github.com/imrn99/fast-stm/pull/27
* (internals) replace `Transaction::new` with a `Default` trait impl

### Fixed

* re-export `TransactionTallies` if `profiling` feature is enabled

---

## 0.6.2

### New

* add a `write_atomic` method to `TVar` to write values without using a transaction
* implement event counters in transaction behind new `profiling` feature
* implement manual transaction initialization and commit behind new `bench` feature

### Changed

* rework benchmarks 
* (repo) update nix flake

---

## 0.6.1

### New

* add `unwrap_or_abort` helper function

### Changed

* (repo) replace `shell.nix` file with flake

---

## 0.6.0

### New

* add feature to enable `wait_for_change` on retries by @imrn99 in https://github.com/imrn99/fast-stm/pull/16
* implement hash-based registers enabled with `hash-registers` feature by @imrn99 in https://github.com/imrn99/fast-stm/pull/17
* implement early read inconsistency check with `early-conflict-detection` feature by @imrn99 in https://github.com/imrn99/fast-stm/pull/18

### Misc

* update criterion requirement from 0.5.1 to 0.7.0 by @dependabot[bot]
* bump actions/checkout from 4 to 5 by @dependabot[bot]

---

## 0.5.0

First release
