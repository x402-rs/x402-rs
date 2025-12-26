pub use crate::scheme::v1_eip155_exact::types::ExactScheme;

use crate::chain::eip155::{ChecksummedAddress, TokenAmount};
use crate::proto::v2;
use crate::scheme::v1_eip155_exact::types::{ExactEvmPayload, PaymentRequirementsExtra};

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactEvmPayload>;
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, TokenAmount, ChecksummedAddress, PaymentRequirementsExtra>;
