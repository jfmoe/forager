---
name: rust-code-quality
description: Guide for writing, modifying, and reviewing idiomatic Rust, including production async Rust with Tokio, tasks, channels, cancellation, error handling, and concurrency control. Use whenever creating or changing Rust code, reviewing a Rust diff, branch, or PR, or when another skill needs a Rust code-quality review.
---

## Best Practices Reference

Before writing, modifying, or reviewing Rust, read ALL relevant chapters in the same turn. Reference these files when implementing changes or providing review feedback:

- [Chapter 1 - Coding Styles and Idioms](references/chapter_01.md): Borrowing vs cloning, `Copy`, `Option`/`Result`, allocation, iterators, comments, imports
- [Chapter 2 - Clippy and Linting](references/chapter_02.md): Clippy configuration, important lints, workspace lint setup
- [Chapter 3 - Performance Mindset](references/chapter_03.md): Profiling, avoiding redundant clones, stack vs heap, zero-cost abstractions
- [Chapter 4 - Error Handling](references/chapter_04.md): `Result` vs panic, `thiserror` vs `anyhow`, error hierarchies
- [Chapter 5 - Automated Testing](references/chapter_05.md): Test naming, assertions, doc tests, integration tests, snapshot testing
- [Chapter 6 - Generics and Dispatch](references/chapter_06.md): Static vs dynamic dispatch, trait objects
- [Chapter 7 - Type State Pattern](references/chapter_07.md): Compile-time state safety, when to use it
- [Chapter 8 - Comments vs Documentation](references/chapter_08.md): Comments, doc comments, rustdoc
- [Chapter 9 - Understanding Pointers](references/chapter_09.md): Thread safety, `Send`/`Sync`, pointer types
- [Chapter 10 - Async Rust Patterns](references/chapter_10.md): Tokio tasks, channels, errors, shutdown, async traits, streams, resource management

When reviewing Rust code, examine code quality against every relevant chapter.

## Quick Reference

### Borrowing & Ownership

- Prefer `&T` over `.clone()` unless ownership transfer is required
- Use `&str` over `String`, `&[T]` over `Vec<T>` in function parameters
- Small `Copy` types can be passed by value
- Use `Cow<'_, T>` when ownership is ambiguous

### Error Handling

- Return `Result<T, E>` for fallible operations; avoid `panic!` in production
- Avoid `unwrap()`/`expect()` outside tests unless failure is provably impossible
- Use `thiserror` for library errors and `anyhow` for application-level errors
- Prefer `?` over match chains for error propagation

### Performance

- Measure before optimizing and benchmark release builds
- Avoid cloning in loops and unnecessary intermediate collections
- Prefer iterators when they improve composition and clarity
- Use profiling and benchmarks to validate performance changes

### Linting

Run the project's lint command. When no project command exists, start with:

```shell
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Key lints:

- `redundant_clone`
- `large_enum_variant`
- `needless_collect`
- `clone_on_copy`

Prefer a justified `#[expect(clippy::lint)]` over a broad `#[allow(...)]`.

### Testing

- Name tests descriptively
- Keep each test focused on one behavior
- Test error and boundary cases
- Use doc tests for public API examples
- Use snapshots for complex structured output, not simple values

### Generics & Dispatch

- Prefer generics and static dispatch when concrete types are known
- Use `dyn Trait` when runtime polymorphism or heterogeneous collections are required
- Box at API boundaries rather than prematurely inside implementations

### Type State

Use type-state when making invalid states unrepresentable prevents real bugs. Avoid it when it only adds generic complexity.

### Documentation

- Use `//` comments for non-obvious reasons, safety constraints, and workarounds
- Use `///` and `//!` for public API and module documentation
- Document errors, panics, and safety contracts where relevant
- Link actionable TODOs to tracked issues

### Async Rust

- Use `JoinSet` or collected `JoinHandle`s to own spawned tasks
- Use `tokio::select!` for racing futures and cancellation-aware control flow
- Prefer channels for task communication; choose `mpsc`, `broadcast`, `oneshot`, or `watch` by semantics
- Bound concurrency with semaphores or buffered streams
- Propagate cancellation with `CancellationToken` and define graceful shutdown
- Never block the async runtime or hold blocking locks across `.await`
- Do not spawn unbounded tasks or ignore task and channel errors
- Instrument async operations with `tracing`
