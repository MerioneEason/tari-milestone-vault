# Development notes — Milestone Vault, phase 1 (2026-11-08)

Local development only: no accounts registered, nothing posted, no wallet
generated, nothing pushed anywhere.

## Version baseline actually used

| Item | Version | Source |
|---|---|---|
| Rust / cargo | 1.99.1 (stable, rustup) | `rustc --version` |
| WASM target | `wasm32-unknown-unknown` | rustup target add |
| `tari_template_lib` | 0.38 (resolved 0.38.0) | crates.io latest on 2026-11-08 |
| `tari_template_test_tooling` | 0.50 (resolved 0.50.1) | crates.io latest on 2026-11-08 |
| `tari_ootle_template_build` | 0.18 (resolved 0.18.0) | crates.io latest on 2026-11-08 |
| tari-ootle workspace (reference clone) | 0.50.1 | repo `Cargo.toml`, branch `development` |

Official docs/skills were taken from the cloned `tari-project/tari-ootle`
(`docs/skills/`) and `tari-project/wasm-template` repos under `reference/`.

## Toolchain verification

Official example `examples/guessing_game` (wasm-template repo) run as shipped
(its own pins: `tari_template_lib` 0.37, `tari_template_test_tooling` 0.49.2):

```
test it_works ... ok   (1 passed; 0 failed)
```

## Milestone Vault engine tests — 9/9 pass

Command: `cd template && cargo test` → `test result: ok. 9 passed; 0 failed`.

1. `full_flow_lock_submit_approve_release` — lock 300_000 µT across two
   milestones, submit + approve both, freelancer balance +300_000, client
   −300_000, vault empty, statuses Released.
2. `timeout_refunds_unsubmitted_milestone_to_client` — RefundClient policy,
   no submission, epoch advanced past deadline, third party settles, client
   made whole, status Refunded.
3. `timeout_refund_policy_refunds_even_if_submitted` — submitted but policy is
   RefundClient → refund, freelancer unchanged.
4. `timeout_auto_releases_submitted_milestone` — AutoReleaseIfSubmitted
   policy, submitted in time, client silent → freelancer paid on settle.
5. `non_client_cannot_approve` — freelancer and unrelated third party both
   rejected; milestone stays Submitted, funds stay locked.
6. `non_freelancer_cannot_submit` — client submitting on the freelancer's
   behalf is rejected.
7. `double_release_is_rejected` — second approve fails with "Milestone has
   already been settled"; settle-after-release also fails.
8. `settle_before_deadline_is_rejected` — fails with "Milestone deadline has
   not passed yet".
9. `approve_before_submit_is_rejected` — fails with "Milestone has not been
   submitted yet".

Time control in tests: `TemplateTest::set_virtual_substate(CurrentEpoch, …)`,
which is what `Consensus::current_epoch()` reads during execution.

## WASM build

`cargo build --target wasm32-unknown-unknown --release` succeeds:
`milestone_vault.wasm` = 144,412 bytes (~141 KiB) before `wasm-opt`. That is
above the 96 KiB "cheap storage" threshold quoted in the official skills doc;
the wallet publish flow runs `wasm-opt` automatically, and shrinking the
binary is a phase-2 optimisation item, not a correctness issue.

## Gotchas hit during development (for future phases)

- The `#[template]` macro names the template after the **first** struct/enum
  in the module and only accepts an `impl` for that type. The component
  struct must be defined first (see the note in `template/src/lib.rs`).
- In tests: `RistrettoPublicKeyBytes` lives at
  `tari_template_lib::types::crypto::…`; the byte-conversion trait is
  `ToByteType` (`tari_template_test_tooling::byte_type`); `Workspace("name")`
  is written bare inside `args![…]` (the macro supplies it); `Amount::from`
  needs explicitly typed literals (`300_000u64`).
- First-time compile of the test-tooling stack (wasmer/cranelift) takes
  ~12 minutes on this 4-core VM; subsequent runs are ~1 minute.

## Not done in phase 1 (by design)

- No testnet (Esmeralda) publish, no wallet, no GitHub push, no forum post,
  no announcement post. Those need the user's approval per the contest prep
  plan, and the final pre-deadline rebuild against the then-latest toolchain
  is still owed (protocol-V1 version risk).
