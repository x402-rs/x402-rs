use crate::chain::solana::Address;
use crate::proto::util::U64String;
use crate::proto::v2;
use crate::scheme::v1_solana_exact::types::{ExactSolanaPayload, SupportedPaymentKindExtra};

pub use crate::scheme::v1_eip155_exact::types::ExactScheme;

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactSolanaPayload>;
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, U64String, Address, SupportedPaymentKindExtra>;
