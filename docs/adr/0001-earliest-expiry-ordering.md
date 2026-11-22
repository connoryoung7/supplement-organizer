# Bottles are taken in earliest-expiry order; the app never adjusts doses

The next bottle opened is always the new bottle with the earliest expiry date, and the app's only response to a bottle that won't be finished in time is to flag it. We chose this for predictability: the app does exactly what you would do by hand, and it never tells you to take more of something.

## Considered Options

- **Minimum-expired ordering (Moore–Hodgson).** Provably minimises the number of expired bottles, but sometimes tells you to skip an earlier-expiring bottle and take it last, already expired. Rejected as less predictable; revisit if expired-bottle counts stay high.
- **Dose suggestions within a per-product maximum.** The only lever that can save a single bottle that's already behind, but risky for fat-soluble vitamins and minerals. Rejected; the app also deliberately does not show the rate needed to finish on time.
