# rust-stm

This repository is a Rust workspace for Software Transactional Memory (STM) implementations.

STM lets code compose concurrent operations by running them inside transactions. The transaction
records reads and writes to `TVar`s, commits all writes atomically when the observed state is still
valid, and retries otherwise.

## Workspace structure

- [`fast-stm`](fast-stm/) - performance-focused STM implementation forked from Marthog's original
  [`stm` crate](https://github.com/Marthog/rust-stm).
- [`sserp-stm`](sserp-stm/) - STM implementation using the SSER+ algorithm from _Boosting
  transactional memory with stricter serializability_.
- [`benches`](benches/) - internal Criterion benchmark harness used to compare STM implementations
  and synchronization primitives.

Each published crate has its own README:

- [`fast-stm/README.md`](fast-stm/README.md)
- [`sserp-stm/README.md`](sserp-stm/README.md)

## Development

Run the published crates' test suites:

```sh
cargo test -p fast-stm -p sserp-stm
```

Build package documentation:

```sh
cargo doc --workspace --no-deps
```

Run benchmarks for one STM implementation at a time:

```sh
cargo bench --features fast-stm
cargo bench --features sserp-stm
```

The benchmark crate requires exactly one of the `fast-stm` or `sserp-stm` features.

## License

Licensed under either of:

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option.

## Contribution

Contributions are welcome and accepted as pull requests on [GitHub][GH]. Feel free to use issues to
report bugs, missing documentation or suggest improvements of the project.

[GH]: https://github.com/imrn99/fast-stm
