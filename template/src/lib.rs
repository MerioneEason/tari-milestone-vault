//   Copyright 2026 MerioneEason
//   SPDX-License-Identifier: MIT
//
//   Milestone Vault — milestone-based escrow payments on Tari Ootle.
//
//   A client locks the full payment for a job into this component up front.
//   The job is split into milestones, each with its own amount, description
//   and deadline (in epochs). The freelancer marks a milestone as submitted,
//   the client approves it, and only then is that milestone's amount released.
//   If a deadline passes without approval, anyone may call `settle_expired`
//   and the pre-agreed timeout policy — fixed at creation and locked —
//   decides whether the money is refunded to the client or auto-released to
//   a freelancer who did submit in time.
//
//   Built by an AI agent (see README).

use tari_template_lib::prelude::*;

#[template]
mod milestone_vault {
    use super::*;

    /// The escrow component. Holds the locked funds in a single vault; the sum
    /// of all milestone amounts equals the vault balance until settlements
    /// start paying out, and each settlement reduces both by the same amount.
    ///
    /// NOTE: this struct must stay the first type defined in this module — the
    /// `#[template]` macro names the template after the first struct/enum it
    /// finds.
    pub struct MilestoneVault {
        funds: Vault,
        /// The client is whoever signed the creation transaction.
        client: RistrettoPublicKeyBytes,
        freelancer: RistrettoPublicKeyBytes,
        /// Refunds are deposited into this account component.
        client_account: ComponentAddress,
        /// Approved milestone payments are deposited into this account component.
        freelancer_account: ComponentAddress,
        timeout_policy: TimeoutPolicy,
        milestones: Vec<Milestone>,
    }

    pub struct Milestone {
        amount: Amount,
        description: String,
        /// Last epoch at which the milestone may be submitted / approved.
        deadline_epoch: u64,
        status: MilestoneStatus,
        /// Free-text delivery note supplied by the freelancer on submission.
        submission_note: String,
    }

    /// What happens to a milestone's funds once its deadline epoch has passed
    /// without the client approving the milestone. Chosen at creation time,
    /// stored in component state, and impossible to change afterwards because
    /// the component's owner rule is `OwnerRule::None`.
    pub enum TimeoutPolicy {
        /// The milestone amount goes back to the client.
        RefundClient,
        /// A milestone that was submitted before its deadline is paid out to
        /// the freelancer (the client had the whole review window to object).
        /// A milestone that was never submitted is still refunded.
        AutoReleaseIfSubmitted,
    }

    /// Lifecycle of a single milestone. `Released` and `Refunded` are terminal:
    /// every mutating method rejects a milestone that has reached either.
    pub enum MilestoneStatus {
        Pending,
        Submitted,
        Released,
        Refunded,
    }

    impl MilestoneVault {
        /// Creates a new escrow vault and locks `payment` inside it.
        ///
        /// The three milestone vectors are parallel: entry `i` of each one
        /// describes milestone `i`. The payment bucket amount must equal the
        /// sum of `milestone_amounts` exactly, so the vault can never be
        /// under- or over-funded relative to its terms.
        ///
        /// Callable by: anyone (the transaction signer becomes the client).
        ///
        /// # Panics
        ///
        /// Panics if the milestone lists are empty or mismatched, if any
        /// amount is zero, if any deadline is not in the future, if the
        /// freelancer is the client, or if the payment does not equal the sum
        /// of the milestone amounts.
        pub fn new(
            payment: Bucket,
            freelancer: RistrettoPublicKeyBytes,
            freelancer_account: ComponentAddress,
            client_account: ComponentAddress,
            milestone_amounts: Vec<Amount>,
            milestone_descriptions: Vec<String>,
            milestone_deadline_epochs: Vec<u64>,
            auto_release_on_timeout: bool,
        ) -> Component<Self> {
            let client = CallerContext::transaction_signer_public_key();
            assert!(freelancer != client, "Freelancer must differ from the client");
            assert!(
                !milestone_amounts.is_empty(),
                "At least one milestone is required"
            );
            assert!(
                milestone_amounts.len() == milestone_descriptions.len()
                    && milestone_amounts.len() == milestone_deadline_epochs.len(),
                "Milestone amounts, descriptions and deadlines must have the same length"
            );

            let now = Consensus::current_epoch();
            let mut total = Amount::zero();
            let mut milestones = Vec::with_capacity(milestone_amounts.len());
            for ((amount, description), deadline_epoch) in milestone_amounts
                .into_iter()
                .zip(milestone_descriptions)
                .zip(milestone_deadline_epochs)
            {
                assert!(!amount.is_zero(), "Milestone amounts must be positive");
                assert!(
                    deadline_epoch > now,
                    "Milestone deadlines must be in the future"
                );
                total += amount;
                milestones.push(Milestone {
                    amount,
                    description,
                    deadline_epoch,
                    status: MilestoneStatus::Pending,
                    submission_note: String::new(),
                });
            }
            assert!(
                payment.amount() == total,
                "Payment must equal the sum of the milestone amounts"
            );

            let timeout_policy = if auto_release_on_timeout {
                TimeoutPolicy::AutoReleaseIfSubmitted
            } else {
                TimeoutPolicy::RefundClient
            };

            // Role-locked access rules: only the freelancer's key may submit,
            // only the client's key may approve (i.e. release funds). The
            // timeout settlement is callable by anyone because its outcome is
            // fully determined by the locked policy — the caller cannot
            // influence where the money goes.
            let access_rules = ComponentAccessRules::new()
                .method("submit_milestone", rule!(public_key(freelancer)))
                .method("approve_milestone", rule!(public_key(client)))
                .method("settle_expired", rule!(allow_all))
                .method("milestone_count", rule!(allow_all))
                .method("milestone_status", rule!(allow_all))
                .method("milestone_amount", rule!(allow_all))
                .method("milestone_description", rule!(allow_all))
                .method("milestone_deadline_epoch", rule!(allow_all))
                .method("milestone_submission_note", rule!(allow_all))
                .method("vault_balance", rule!(allow_all))
                .method("client_public_key", rule!(allow_all))
                .method("freelancer_public_key", rule!(allow_all))
                .default(rule!(deny_all));

            Component::new(Self {
                funds: Vault::from_bucket(payment),
                client,
                freelancer,
                client_account,
                freelancer_account,
                timeout_policy,
                milestones,
            })
            .with_access_rules(access_rules)
            // No owner: nobody can ever change the access rules above, so the
            // role assignments and the settlement behaviour stay as agreed at
            // creation. The component's template is likewise locked by the
            // default update rule.
            .with_owner_rule(OwnerRule::None)
            .create()
        }

        /// Marks a milestone as delivered. The `note` is stored on-chain as the
        /// freelancer's delivery statement (e.g. a link or summary of the work).
        ///
        /// Callable by: the freelancer only (access rule + signer check).
        ///
        /// # Panics
        ///
        /// Panics if the milestone does not exist, was already submitted or
        /// settled, or its deadline has passed.
        pub fn submit_milestone(&mut self, index: u32, note: String) {
            assert!(
                CallerContext::transaction_signer_public_key() == self.freelancer,
                "Only the freelancer can submit a milestone"
            );
            let milestone = self.get_milestone_mut(index);
            assert!(
                matches!(milestone.status, MilestoneStatus::Pending),
                "Milestone has already been submitted or settled"
            );
            assert!(
                Consensus::current_epoch() <= milestone.deadline_epoch,
                "Milestone deadline has passed"
            );
            milestone.status = MilestoneStatus::Submitted;
            milestone.submission_note = note;
            emit_event(
                "MilestoneSubmitted",
                metadata![
                    "index" => index.to_string(),
                    "deadline_epoch" => milestone.deadline_epoch.to_string(),
                ],
            );
        }

        /// Approves a submitted milestone and releases its amount to the
        /// freelancer's account.
        ///
        /// Callable by: the client only (access rule + signer check).
        ///
        /// # Panics
        ///
        /// Panics if the milestone does not exist, has not been submitted yet,
        /// or has already been settled (which also prevents double payment).
        pub fn approve_milestone(&mut self, index: u32) {
            assert!(
                CallerContext::transaction_signer_public_key() == self.client,
                "Only the client can approve a milestone"
            );
            let amount = {
                let milestone = self.get_milestone_mut(index);
                assert!(
                    !matches!(
                        milestone.status,
                        MilestoneStatus::Released | MilestoneStatus::Refunded
                    ),
                    "Milestone has already been settled"
                );
                assert!(
                    matches!(milestone.status, MilestoneStatus::Submitted),
                    "Milestone has not been submitted yet"
                );
                milestone.status = MilestoneStatus::Released;
                milestone.amount
            };
            let payment = self.funds.withdraw(amount);
            ComponentManager::get(self.freelancer_account).invoke("deposit", args![payment]);
            emit_event(
                "MilestoneReleased",
                metadata![
                    "index" => index.to_string(),
                    "amount" => amount.to_string(),
                    "auto" => "false".to_string(),
                ],
            );
        }

        /// Settles a milestone whose deadline has passed without approval,
        /// applying the timeout policy fixed at creation:
        /// - never submitted → refunded to the client under both policies;
        /// - submitted in time → refunded under `RefundClient`, paid to the
        ///   freelancer under `AutoReleaseIfSubmitted`.
        ///
        /// Callable by: anyone. The caller cannot influence the outcome.
        ///
        /// # Panics
        ///
        /// Panics if the milestone does not exist, its deadline has not passed
        /// yet, or it has already been settled.
        pub fn settle_expired(&mut self, index: u32) {
            let (amount, was_submitted) = {
                let milestone = self.get_milestone_mut(index);
                assert!(
                    !matches!(
                        milestone.status,
                        MilestoneStatus::Released | MilestoneStatus::Refunded
                    ),
                    "Milestone has already been settled"
                );
                assert!(
                    Consensus::current_epoch() > milestone.deadline_epoch,
                    "Milestone deadline has not passed yet"
                );
                let was_submitted = matches!(milestone.status, MilestoneStatus::Submitted);
                (milestone.amount, was_submitted)
            };

            let pay_freelancer = was_submitted
                && matches!(self.timeout_policy, TimeoutPolicy::AutoReleaseIfSubmitted);

            let payout = self.funds.withdraw(amount);
            if pay_freelancer {
                self.get_milestone_mut(index).status = MilestoneStatus::Released;
                ComponentManager::get(self.freelancer_account).invoke("deposit", args![payout]);
                emit_event(
                    "MilestoneReleased",
                    metadata![
                        "index" => index.to_string(),
                        "amount" => amount.to_string(),
                        "auto" => "true".to_string(),
                    ],
                );
            } else {
                self.get_milestone_mut(index).status = MilestoneStatus::Refunded;
                ComponentManager::get(self.client_account).invoke("deposit", args![payout]);
                emit_event(
                    "MilestoneRefunded",
                    metadata![
                        "index" => index.to_string(),
                        "amount" => amount.to_string(),
                        "was_submitted" => was_submitted.to_string(),
                    ],
                );
            }
        }

        /// Number of milestones in this vault.
        pub fn milestone_count(&self) -> u32 {
            self.milestones.len() as u32
        }

        /// Status code of a milestone: 0 = Pending, 1 = Submitted,
        /// 2 = Released, 3 = Refunded.
        pub fn milestone_status(&self, index: u32) -> u8 {
            match self.milestones[index as usize].status {
                MilestoneStatus::Pending => 0,
                MilestoneStatus::Submitted => 1,
                MilestoneStatus::Released => 2,
                MilestoneStatus::Refunded => 3,
            }
        }

        pub fn milestone_amount(&self, index: u32) -> Amount {
            self.milestones[index as usize].amount
        }

        pub fn milestone_description(&self, index: u32) -> String {
            self.milestones[index as usize].description.clone()
        }

        pub fn milestone_deadline_epoch(&self, index: u32) -> u64 {
            self.milestones[index as usize].deadline_epoch
        }

        pub fn milestone_submission_note(&self, index: u32) -> String {
            self.milestones[index as usize].submission_note.clone()
        }

        /// Funds still locked in the vault.
        pub fn vault_balance(&self) -> Amount {
            self.funds.balance()
        }

        pub fn client_public_key(&self) -> RistrettoPublicKeyBytes {
            self.client
        }

        pub fn freelancer_public_key(&self) -> RistrettoPublicKeyBytes {
            self.freelancer
        }

        fn get_milestone_mut(&mut self, index: u32) -> &mut Milestone {
            let index = index as usize;
            assert!(index < self.milestones.len(), "Milestone index out of range");
            &mut self.milestones[index]
        }
    }
}
