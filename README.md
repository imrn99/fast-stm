# rust-stm

This repository is a Rust workspace for Software Transactional Memory (STM) implementations.

STM provides a composable alternative to regular synchronization mechanisms for concurrent
operations; It is based on two primiti`ves: transactions and transactional variables.

Users who wish to familiarize themselves with the mechanism can skim through the following
documents:

- Dedicated STM chapter of [_Real World Haskell_](https://wiki.haskell.org/Real_World_Haskell) for
  a quick intuitive introduction
- [_Software Transactional Memory_, Shavit et al., 1997](https://doi.org/10.1007/s004460050028)
- [_On the correctness of transactional memory_, Guerraoui et al., 2008](https://dl.acm.org/doi/10.1145/1345206.1345233)

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
