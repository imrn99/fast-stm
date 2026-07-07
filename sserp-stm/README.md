# sserp-stm

`sserp-stm` is a Rust implementation of
[software transactional memory](https://en.wikipedia.org/wiki/Software_transactional_memory)
using the SSER+ algorithm from
[Boosting Transactional Memory with Stricter Serializability](https://doi.org/10.1007/978-3-319-92408-3_11)
by Sutra, Marlier, Schiavoni, and Trahay.

## STM

Users who wish to familiarize themselves with the mechanism can skim through the following
documents:

- Dedicated STM chapter of [_Real World Haskell_](https://wiki.haskell.org/Real_World_Haskell) for
  a quick intuitive introduction
- [_Software Transactional Memory_, Shavit et al., 1997](https://doi.org/10.1007/s004460050028)
- [_On the correctness of transactional memory_, Guerraoui et al., 2008](https://dl.acm.org/doi/10.1145/1345206.1345233)

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

This crate exposes features that can tweak implementation behavior:

- `wait-on-retry` - enabled by default. If `retry` is called explicitly in a
  transaction, the thread waits for one of the variables read in the initial
  transaction to change before attempting the computation again.
- `hash-registers` - NOT YET IMPLEMENTED

Only `wait-on-retry` is enabled by default.

Two additional features are provided for instrumentation: 
  
- `profiling` - add event counters to transactions and expose
  `profile_atomically` / `profile_atomically_with_err`.
- `bench` - expose manual transaction initialization and commit helpers used by
  the repository's benchmarks.


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
