# sserp-stm

`sserp-stm` is a Rust implementation of
[software transactional memory](https://en.wikipedia.org/wiki/Software_transactional_memory)
using the SSER+ algorithm from
[Boosting Transactional Memory with Stricter Serializability](https://doi.org/10.1007/978-3-319-92408-3_11)
by Sutra, Marlier, Schiavoni, and Trahay.

This crate mirrors the public API of `fast-stm`, while using a different STM
algorithm internally.

With locks, the sequential composition of two threadsafe actions is no longer
threadsafe because other threads may interfere between those actions. Applying a
third lock to protect both may lead to common sources of errors like deadlocks
or race conditions.

Unlike locks, software transactional memory is composable. It is typically
implemented by writing all read and write operations in a log. When the action
has finished and all the used `TVar`s are consistent, the writes are committed
as a single atomic operation. Otherwise the computation repeats. This may lead
to starvation, but avoids common sources of bugs.

Panicking within STM does not poison the `TVar`s. STM ensures consistency by
never committing on panic.

## Features

The following features are available:

- `wait-on-retry` - enabled by default. If `retry` is called explicitly in a
  transaction, the thread waits for one of the variables read in the initial
  transaction to change before attempting the computation again.
- `profiling` - add event counters to transactions and expose
  `profile_atomically` / `profile_atomically_with_err`.
- `bench` - expose manual transaction initialization and commit helpers used by
  the repository's benchmarks.

Only `wait-on-retry` is enabled by default.

## Usage

You should only use the functions that are safe to use.

Do not have side effects except for the atomic variables from this library.
Especially a mutex or other blocking mechanisms inside software transactional
memory is dangerous.

You can run the top-level atomic operation by calling `atomically`.

```rust
use sserp_stm::atomically;

atomically(|_tx| {
    // some action
    // return value as `Result`, for example
    Ok(42)
});
```

Calls to `atomically` should not be nested.

For running an atomic operation inside of another, pass a mutable reference to a
`Transaction` and use `?` on the result. You should not handle the error
yourself, because it breaks consistency.

```rust
use sserp_stm::{atomically, TVar};

let var = TVar::new(0);

let x = atomically(|tx| {
    var.write(tx, 42)?;
    var.read(tx)
});

println!("var = {}", x);
```

## STM safety

Software transactional memory is completely safe in the terms that Rust
considers safe. Still there are multiple rules that you should obey when
dealing with software transactional memory:

- Do not run code with side effects, especially no IO-code, because STM repeats
  the computation when it detects inconsistent state. Return a closure if you
  have to.
- Do not handle the error types yourself, unless you absolutely know what you
  are doing. Use `Transaction::or` to combine alternative paths. Always use `?`
  and never ignore a `StmResult`.
- Do not run `atomically` inside of another. `atomically` is designed to have
  side effects and will therefore break STM's assumptions. Nested calls are
  detected at runtime and handled with panic. When you use STM in the inner of a
  function, express it in the public interface by taking `&mut Transaction` as a
  parameter and returning `StmResult<T>`. Callers can safely compose it into
  larger blocks.
- Do not mix locks and transactions. Your code will easily deadlock or slow
  unpredictably.
- Do not use inner mutability to change the content of a `TVar`.

## Speed

Generally keep your atomic blocks as small as possible, because the more time
you spend, the more likely it is to collide with other threads. Every used
`TVar` increases the chance of collisions. Therefore you should keep the amount
of accessed variables as low as needed.

## License

Licensed under either of:

- Apache License, Version 2.0,
  ([LICENSE-APACHE](https://github.com/imrn99/fast-stm/blob/main/LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license
  ([LICENSE-MIT](https://github.com/imrn99/fast-stm/blob/main/LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
