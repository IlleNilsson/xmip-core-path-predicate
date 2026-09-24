# xmip-core-path-predicate

Predicate path technology: a boolean expression over paths another reader
answers — comparisons, exists, starts-with, contains, and, or, not — so a
route or a Subscription reads one truth from content, for route and process.
A technology of [xmip-core-path](https://github.com/IlleNilsson/xmip-core-path).

A predicate is read through `xmip-core-library-codec`'s character reader:
any Unicode whitespace separates tokens, a word may hold any letter, and a
character that begins no token is refused by its offset, never a panic.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
