//! Offline tests for payer-signature checks on the V1 Solana exact facilitator.

use super::*;
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::Instruction;

fn two_signer_transaction(fee_payer: &Keypair, payer: &Keypair) -> VersionedTransaction {
    let ix = Instruction::new_with_bytes(
        Pubkey::new_unique(),
        &[],
        vec![solana_transaction::AccountMeta::new(payer.pubkey(), true)],
    );
    let message = Message::new(&[ix], Some(&fee_payer.pubkey()));
    let num_required = message.header.num_required_signatures as usize;
    VersionedTransaction {
        signatures: vec![Signature::default(); num_required],
        message: VersionedMessage::Legacy(message),
    }
}

fn sign_slot(tx: &mut VersionedTransaction, signer: &Keypair) {
    let msg_bytes = tx.message.serialize();
    let signature = signer.sign_message(msg_bytes.as_slice());
    let keys = tx.message.static_account_keys();
    let num_required = tx.message.header().num_required_signatures as usize;
    let pos = keys[..num_required]
        .iter()
        .position(|k| *k == signer.pubkey())
        .expect("signer is a required account");
    tx.signatures[pos] = signature;
}

#[test]
fn accepts_valid_payer_signature_with_unsigned_fee_payer() {
    let fee_payer = Keypair::new();
    let payer = Keypair::new();
    let mut tx = two_signer_transaction(&fee_payer, &payer);
    sign_slot(&mut tx, &payer);
    assert!(assert_non_fee_payer_signatures_valid(&tx, &fee_payer.pubkey()).is_ok());
}

#[test]
fn rejects_default_payer_signature() {
    let fee_payer = Keypair::new();
    let payer = Keypair::new();
    let tx = two_signer_transaction(&fee_payer, &payer);
    let err = assert_non_fee_payer_signatures_valid(&tx, &fee_payer.pubkey()).unwrap_err();
    assert!(matches!(err, SolanaExactError::MissingPayerSignature));
}

#[test]
fn rejects_forged_payer_signature() {
    let fee_payer = Keypair::new();
    let payer = Keypair::new();
    let mut tx = two_signer_transaction(&fee_payer, &payer);
    let keys = tx.message.static_account_keys();
    let num_required = tx.message.header().num_required_signatures as usize;
    let pos = keys[..num_required]
        .iter()
        .position(|k| *k == payer.pubkey())
        .expect("payer is a required account");
    tx.signatures[pos] = Signature::from([1u8; 64]);
    let err = assert_non_fee_payer_signatures_valid(&tx, &fee_payer.pubkey()).unwrap_err();
    assert!(matches!(err, SolanaExactError::InvalidPayerSignature));
}
