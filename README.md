# Milestone Vault — milestone-based escrow payments on Tari Ootle

**An entry to the Tari November Build Contest (theme: MVP).**

- **Code:** https://github.com/MerioneEason/tari-milestone-vault
- **Live demo (simulated, no wallet needed):** https://merioneeason.github.io/tari-milestone-vault/demo/

Freelance and small-service work has a trust problem at both ends: clients
don't want to pay before the work exists, freelancers don't want to work
without a guarantee of payment. Milestone Vault is the escrow middle ground,
implemented as a Tari Ootle template:

1. The **client** locks the *full* payment for a job into the vault up front,
   split into milestones — each with its own amount, description and deadline
   (in epochs).
2. The **freelancer** marks a milestone as submitted, with an on-chain
   delivery note.
3. The **client** approves a submitted milestone, and only then is that
   milestone's amount released to the freelancer's account.
4. If a deadline passes without approval, **anyone** may settle the expired
   milestone — but the outcome is not up to the caller. It follows the timeout
   policy the two parties agreed to at creation, which is locked into the
   component and can never be changed.

The payment is real from block one (the freelancer can verify the vault is
fully funded before starting work — the constructor rejects any payment that
does not exactly equal the sum of the milestone amounts), and neither side
can move money outside the agreed rules.

## Timeout policy: the choice, and why

At creation the parties pick one of two policies (`auto_release_on_timeout`):

| Policy | Submitted in time, client silent past deadline | Never submitted by deadline |
|---|---|---|
| `RefundClient` (default) | Refunded to client | Refunded to client |
| `AutoReleaseIfSubmitted` | **Paid to freelancer** | Refunded to client |

**The default is `RefundClient`.** Rationale: escrow exists first to protect
the payer against non-delivery, and a refund default fails safe — money only
leaves the vault when the client affirmatively approves, or when both parties
explicitly opted into auto-release. `AutoReleaseIfSubmitted` exists for the
opposite, equally real failure mode — a client who receives the work and then
goes silent — and is offered as a deliberate, visible opt-in at creation time,
never as a surprise. Under both policies, unsubmitted work is never paid.

## How the Ootle permission model is used

- **Role-locked methods.** Component access rules bind `submit_milestone` to
  the freelancer's public key and `approve_milestone` to the client's public
  key (`rule!(public_key(...))`). The same checks are repeated in-method
  against `CallerContext::transaction_signer_public_key()` as defence in
  depth.
- **Locked rules.** The component is created with `OwnerRule::None`, so no
  one — not even the client who created it — can change the access rules
  afterwards, and the template itself is locked under the default update
  rule. The deal you verified at creation is the deal that executes.
- **Permissionless settlement, predetermined outcome.** `settle_expired` is
  callable by anyone once a deadline has passed. That is safe precisely
  because the caller cannot influence where the money goes; liveness does not
  depend on either party showing up.
- **Terminal states.** `Released` and `Refunded` are terminal: every mutating
  method rejects a settled milestone, which makes double payment impossible
  by construction rather than by bookkeeping.

## How this differs from Crypt (October contest)

Crypt (an October Build Contest entry) escrows a *secret*: a seller locks
encrypted digital goods and the escrow releases the secret against payment —
a one-shot, two-party exchange of a specific digital item for a price.

Milestone Vault generalises escrow in a different direction — **services
over time**, not goods in a single exchange:

- **N milestones, not one exchange**: staged amounts, staged deadlines,
  partial completion (some milestones can be released while others are
  refunded).
- **Work-verification workflow**: an explicit submit → approve loop with an
  on-chain delivery note per milestone, instead of atomic secret revelation.
- **Timeout policy as a first-class, locked term**: the parties negotiate
  what silence means (refund vs. auto-release) before funds move; Crypt-style
  one-shot escrow has no equivalent concept.
- **Different market**: freelancers, agencies and DAOs paying for phased work
  — an MVP-shaped product with users today — rather than a marketplace for
  secrets.

## Repository layout

- `template/` — the Rust Ootle template (`tari_template_lib`) and its engine
  tests (`tari_template_test_tooling`, fully in-process, no network).
- `demo/` — a static, browser-only **simulated** demo of the flow.
- `reference/` — local clones of the official `tari-ootle` and `wasm-template`
  repositories used during development (not part of the deliverable; excluded
  from git).

## Tests

Engine tests (run with `cargo test` in `template/`) cover:

- full flow: lock → submit → approve → funds land in the freelancer's account;
- timeout refund of an unsubmitted milestone (RefundClient policy);
- timeout refund of a *submitted* milestone under RefundClient;
- timeout auto-release of a submitted milestone under AutoReleaseIfSubmitted;
- approval by a non-client is rejected (freelancer and third party);
- submission by a non-freelancer is rejected;
- double release is rejected (second approve, and settle-after-release);
- settling before the deadline is rejected;
- approving before submission is rejected.

_Status (2026-11-12): **9/9 engine tests pass** against `tari_template_lib`
0.40.1 / `tari_template_test_tooling` 0.53.3 (rebuilt 2026-11-12 after the
Ootle mainnet launch; previous baseline was lib 0.38 / test_tooling 0.50).
The template also compiles to WASM
(`wasm32-unknown-unknown`, release). It has not been published to any network
yet._

## Demo — please read

The demo in `demo/` (open `demo/index.html`, no build step) is a
**simulation**: the template is **not published to mainnet**, the page
connects to no wallet and no chain, and no real funds move. It reproduces the
template's state machine and guards faithfully so reviewers can walk the flow
from both the client's and the freelancer's point of view. On-chain behaviour
is what the engine tests verify.

## Threat model & honest limitations

- **No dispute arbitration.** If the client claims submitted work is bad and
  the freelancer disagrees, the template cannot judge quality. The timeout
  policy is the whole recourse: pick `RefundClient` and a dissatisfied client
  can wait out the deadline; pick `AutoReleaseIfSubmitted` and the freelancer
  is protected against silent clients. There is deliberately no third-party
  arbiter role in v1 — adding one reintroduces a trusted party, and is listed
  as future work rather than silently implied.
- **Deadlines are epochs, not wall-clock time.** Epoch length is a network
  parameter; clients should choose deadline epochs with the current epoch
  length in mind.
- **Amounts are public.** This template uses a public fungible resource;
  milestone amounts and payments are visible on-chain. Confidential amounts
  are future work.
- **The delivery note is a statement, not proof of work.** It records what
  the freelancer claims to have delivered (e.g. a link or hash); verifying
  the actual work happens off-chain, as with any escrow for services.
- **Not audited, not on mainnet.** This is a contest MVP: engine-tested, but
  not security-audited and not yet published to any live network.

## Building

```bash
rustup target add wasm32-unknown-unknown
cd template
cargo test                                        # engine tests (in-process)
cargo build --target wasm32-unknown-unknown --release   # template WASM
```

Development baseline (rebuilt 2026-11-12): Rust stable 1.99.1,
`tari_template_lib` 0.40.1, `tari_template_test_tooling` 0.53.3,
`tari_ootle_template_build` 0.20.0 — the latest crates.io releases at the time
of the rebuild, matching the tari-ootle workspace version 0.53.0 (walletd
v0.53.0). Because the
Ootle toolchain moves fast (templates built against old library versions
cannot load after protocol V1 activation), the template will be rebuilt and
re-tested against the then-latest versions in the final week before the
contest deadline.

## Authorship

**Built by an AI agent** (Muse, operated by MerioneEason), in line with the
contest organisers' stated position that agent-built entries are welcome and
judged on code quality. Design decisions, threat model and limitations above
are stated as-is; nothing in this repository claims human authorship or a
mainnet deployment that has not happened.

## License

MIT — see `LICENSE`.
