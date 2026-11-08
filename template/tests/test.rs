//   Copyright 2026 MerioneEason
//   SPDX-License-Identifier: MIT
//
//   Engine tests for the Milestone Vault template, run entirely in-process
//   with tari_template_test_tooling (no network). Time is controlled through
//   the CurrentEpoch virtual substate.

use tari_template_lib::types::{
    Amount, ComponentAddress, NonFungibleAddress, constants::TARI_TOKEN,
    crypto::RistrettoPublicKeyBytes,
};
use tari_template_test_tooling::{
    TemplateTest,
    byte_type::ToByteType,
    crypto::RistrettoSecretKey,
    engine_types::virtual_substate::{VirtualSubstate, VirtualSubstateId},
    support::assert_error::assert_reject_reason,
    transaction::args,
};

const TEMPLATE_NAME: &str = "MilestoneVault";

// Milestone status codes returned by the template's `milestone_status` method.
const PENDING: u8 = 0;
const SUBMITTED: u8 = 1;
const RELEASED: u8 = 2;
const REFUNDED: u8 = 3;

struct Actors {
    client_account: ComponentAddress,
    client_proof: NonFungibleAddress,
    client_secret: RistrettoSecretKey,
    freelancer_account: ComponentAddress,
    freelancer_proof: NonFungibleAddress,
    freelancer_secret: RistrettoSecretKey,
    freelancer_pk_bytes: RistrettoPublicKeyBytes,
}

fn make_actors(test: &mut TemplateTest) -> Actors {
    let (client_account, client_proof, client_secret, _client_pk) =
        test.create_funded_account_with_keypair();
    let (freelancer_account, freelancer_proof, freelancer_secret, freelancer_pk) =
        test.create_funded_account_with_keypair();
    Actors {
        client_account,
        client_proof,
        client_secret,
        freelancer_account,
        freelancer_proof,
        freelancer_secret,
        freelancer_pk_bytes: freelancer_pk.to_byte_type(),
    }
}

/// Creates a vault funded by the client and returns its component address.
fn create_vault(
    test: &mut TemplateTest,
    actors: &Actors,
    amounts: &[u64],
    deadlines: &[u64],
    auto_release_on_timeout: bool,
) -> ComponentAddress {
    let template = test.get_template_address(TEMPLATE_NAME);
    let total: u64 = amounts.iter().sum();
    let descriptions: Vec<String> = amounts
        .iter()
        .enumerate()
        .map(|(i, _)| format!("Milestone {}", i + 1))
        .collect();
    let amount_args: Vec<Amount> = amounts.iter().map(|a| Amount::from(*a)).collect();

    test.execute_expect_success(
        test.transaction()
            .call_method(actors.client_account, "withdraw", args![TARI_TOKEN, total])
            .put_last_instruction_output_on_workspace("payment")
            .call_function(
                template,
                "new",
                args![
                    Workspace("payment"),
                    actors.freelancer_pk_bytes,
                    actors.freelancer_account,
                    actors.client_account,
                    amount_args,
                    descriptions,
                    deadlines.to_vec(),
                    auto_release_on_timeout
                ],
            )
            .build_and_seal(&actors.client_secret),
        vec![actors.client_proof.clone()],
    );

    let (vault, _) = test
        .read_only_state_store()
        .get_components_by_template_address(template)
        .unwrap()
        .into_iter()
        .last()
        .unwrap();
    vault
}

fn tari_balance(
    test: &mut TemplateTest,
    account: ComponentAddress,
    secret: &RistrettoSecretKey,
    proof: NonFungibleAddress,
) -> Amount {
    test.execute_expect_success(
        test.transaction()
            .call_method(account, "balance", args![TARI_TOKEN])
            .build_and_seal(secret),
        vec![proof],
    );
    test.expect_return::<Amount>(0)
}

fn milestone_status(test: &mut TemplateTest, vault: ComponentAddress, index: u32) -> u8 {
    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "milestone_status", args![index])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    test.expect_return::<u8>(0)
}

fn vault_balance(test: &mut TemplateTest, vault: ComponentAddress) -> Amount {
    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "vault_balance", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    test.expect_return::<Amount>(0)
}

fn set_epoch(test: &mut TemplateTest, epoch: u64) {
    test.set_virtual_substate(
        VirtualSubstateId::CurrentEpoch,
        VirtualSubstate::CurrentEpoch(epoch),
    );
}

#[test]
fn full_flow_lock_submit_approve_release() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);

    let client_before = tari_balance(
        &mut test,
        actors.client_account,
        &actors.client_secret,
        actors.client_proof.clone(),
    );
    let freelancer_before = tari_balance(
        &mut test,
        actors.freelancer_account,
        &actors.freelancer_secret,
        actors.freelancer_proof.clone(),
    );

    let vault = create_vault(&mut test, &actors, &[100_000, 200_000], &[10, 10], false);
    assert_eq!(vault_balance(&mut test, vault), Amount::from(300_000u64));
    assert_eq!(milestone_status(&mut test, vault, 0), PENDING);
    assert_eq!(milestone_status(&mut test, vault, 1), PENDING);

    // Freelancer submits both milestones.
    for index in [0u32, 1u32] {
        test.execute_expect_success(
            test.transaction()
                .call_method(
                    vault,
                    "submit_milestone",
                    args![index, format!("Delivery for milestone {}", index + 1)],
                )
                .build_and_seal(&actors.freelancer_secret),
            vec![],
        );
        assert_eq!(milestone_status(&mut test, vault, index), SUBMITTED);
    }

    // Client approves both milestones.
    for index in [0u32, 1u32] {
        test.execute_expect_success(
            test.transaction()
                .call_method(vault, "approve_milestone", args![index])
                .build_and_seal(&actors.client_secret),
            vec![],
        );
        assert_eq!(milestone_status(&mut test, vault, index), RELEASED);
    }

    assert_eq!(vault_balance(&mut test, vault), Amount::zero());

    let client_after = tari_balance(
        &mut test,
        actors.client_account,
        &actors.client_secret,
        actors.client_proof.clone(),
    );
    let freelancer_after = tari_balance(
        &mut test,
        actors.freelancer_account,
        &actors.freelancer_secret,
        actors.freelancer_proof.clone(),
    );
    assert_eq!(client_before - client_after, Amount::from(300_000u64));
    assert_eq!(freelancer_after - freelancer_before, Amount::from(300_000u64));
}

#[test]
fn timeout_refunds_unsubmitted_milestone_to_client() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);

    let client_before = tari_balance(
        &mut test,
        actors.client_account,
        &actors.client_secret,
        actors.client_proof.clone(),
    );

    // RefundClient policy (auto_release_on_timeout = false).
    let vault = create_vault(&mut test, &actors, &[150_000], &[10], false);

    // Deadline passes with no submission. A neutral third party (the test's
    // default signer, neither client nor freelancer) triggers settlement.
    set_epoch(&mut test, 11);
    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "settle_expired", args![0u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_eq!(milestone_status(&mut test, vault, 0), REFUNDED);
    assert_eq!(vault_balance(&mut test, vault), Amount::zero());

    let client_after = tari_balance(
        &mut test,
        actors.client_account,
        &actors.client_secret,
        actors.client_proof.clone(),
    );
    assert_eq!(client_after, client_before);
}

#[test]
fn timeout_refund_policy_refunds_even_if_submitted() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);

    let client_before = tari_balance(
        &mut test,
        actors.client_account,
        &actors.client_secret,
        actors.client_proof.clone(),
    );
    let freelancer_before = tari_balance(
        &mut test,
        actors.freelancer_account,
        &actors.freelancer_secret,
        actors.freelancer_proof.clone(),
    );

    let vault = create_vault(&mut test, &actors, &[150_000], &[10], false);

    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "submit_milestone", args![0u32, "Work delivered".to_string()])
            .build_and_seal(&actors.freelancer_secret),
        vec![],
    );

    set_epoch(&mut test, 11);
    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "settle_expired", args![0u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_eq!(milestone_status(&mut test, vault, 0), REFUNDED);
    let client_after = tari_balance(
        &mut test,
        actors.client_account,
        &actors.client_secret,
        actors.client_proof.clone(),
    );
    let freelancer_after = tari_balance(
        &mut test,
        actors.freelancer_account,
        &actors.freelancer_secret,
        actors.freelancer_proof.clone(),
    );
    assert_eq!(client_after, client_before);
    assert_eq!(freelancer_after, freelancer_before);
}

#[test]
fn timeout_auto_releases_submitted_milestone() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);

    let freelancer_before = tari_balance(
        &mut test,
        actors.freelancer_account,
        &actors.freelancer_secret,
        actors.freelancer_proof.clone(),
    );

    // AutoReleaseIfSubmitted policy.
    let vault = create_vault(&mut test, &actors, &[150_000], &[10], true);

    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "submit_milestone", args![0u32, "Work delivered".to_string()])
            .build_and_seal(&actors.freelancer_secret),
        vec![],
    );

    // Client stays silent past the deadline; anyone may settle.
    set_epoch(&mut test, 11);
    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "settle_expired", args![0u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_eq!(milestone_status(&mut test, vault, 0), RELEASED);
    assert_eq!(vault_balance(&mut test, vault), Amount::zero());
    let freelancer_after = tari_balance(
        &mut test,
        actors.freelancer_account,
        &actors.freelancer_secret,
        actors.freelancer_proof.clone(),
    );
    assert_eq!(freelancer_after - freelancer_before, Amount::from(150_000u64));
}

#[test]
fn non_client_cannot_approve() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);
    let vault = create_vault(&mut test, &actors, &[100_000], &[10], false);

    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "submit_milestone", args![0u32, "Done".to_string()])
            .build_and_seal(&actors.freelancer_secret),
        vec![],
    );

    // The freelancer tries to approve their own milestone.
    test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "approve_milestone", args![0u32])
            .build_and_seal(&actors.freelancer_secret),
        vec![],
    );

    // An unrelated third party tries as well.
    test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "approve_milestone", args![0u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_eq!(milestone_status(&mut test, vault, 0), SUBMITTED);
    assert_eq!(vault_balance(&mut test, vault), Amount::from(100_000u64));
}

#[test]
fn non_freelancer_cannot_submit() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);
    let vault = create_vault(&mut test, &actors, &[100_000], &[10], false);

    // The client tries to submit on the freelancer's behalf.
    test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "submit_milestone", args![0u32, "Fake delivery".to_string()])
            .build_and_seal(&actors.client_secret),
        vec![],
    );

    assert_eq!(milestone_status(&mut test, vault, 0), PENDING);
}

#[test]
fn double_release_is_rejected() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);
    let vault = create_vault(&mut test, &actors, &[100_000], &[10], false);

    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "submit_milestone", args![0u32, "Done".to_string()])
            .build_and_seal(&actors.freelancer_secret),
        vec![],
    );
    test.execute_expect_success(
        test.transaction()
            .call_method(vault, "approve_milestone", args![0u32])
            .build_and_seal(&actors.client_secret),
        vec![],
    );
    assert_eq!(milestone_status(&mut test, vault, 0), RELEASED);

    // Second approval must fail: the milestone is already settled.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "approve_milestone", args![0u32])
            .build_and_seal(&actors.client_secret),
        vec![],
    );
    assert_reject_reason(reason, "Milestone has already been settled");

    // Timeout settlement after release must fail too.
    set_epoch(&mut test, 11);
    test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "settle_expired", args![0u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn settle_before_deadline_is_rejected() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);
    let vault = create_vault(&mut test, &actors, &[100_000], &[10], false);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "settle_expired", args![0u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(reason, "Milestone deadline has not passed yet");
    assert_eq!(milestone_status(&mut test, vault, 0), PENDING);
}

#[test]
fn approve_before_submit_is_rejected() {
    let mut test = TemplateTest::my_crate();
    let actors = make_actors(&mut test);
    let vault = create_vault(&mut test, &actors, &[100_000], &[10], false);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(vault, "approve_milestone", args![0u32])
            .build_and_seal(&actors.client_secret),
        vec![],
    );
    assert_reject_reason(reason, "Milestone has not been submitted yet");
}
