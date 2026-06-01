import {
  AssetAmount,
  Network,
  PaymentPayloadResult,
  PaymentRequirements,
  Price,
  SchemeNetworkClient,
  SchemeNetworkServer,
} from "@x402/core/types";
import { ExactEvmScheme } from "@x402/evm/exact/server";
import { ClientEvmSigner } from "@x402/evm";
import { getAddress, toHex } from "viem";
import {
  PERMIT2_ADDRESS,
  uptoPermit2WitnessTypes,
  x402UptoPermit2ProxyAddress,
} from "@x402/evm";

// FIXME This should be an upto from the upstream x402 packages

export class UptoEvmSchemeServer implements SchemeNetworkServer {
  readonly scheme: string = "upto";
  private readonly exact: ExactEvmScheme;

  constructor() {
    this.exact = new ExactEvmScheme();
  }

  parsePrice(price: Price, network: Network): Promise<AssetAmount> {
    return this.exact.parsePrice(price, network);
  }

  async enhancePaymentRequirements(
    paymentRequirements: PaymentRequirements,
    supportedKind: {
      x402Version: number;
      scheme: string;
      network: Network;
      extra?: Record<string, unknown>;
    },
    _facilitatorExtensions: string[],
  ): Promise<PaymentRequirements> {
    const facilitatorAddress = supportedKind.extra?.facilitatorAddress as
      | string
      | undefined;
    return {
      ...paymentRequirements,
      extra: {
        ...paymentRequirements.extra,
        assetTransferMethod: "permit2",
        ...(facilitatorAddress
          ? { facilitatorAddress: getAddress(facilitatorAddress) }
          : {}),
      },
    };
  }
}

export class UptoEvmSchemeClient implements SchemeNetworkClient {
  readonly scheme: string = "upto";

  private readonly signer: ClientEvmSigner;

  constructor(signer: ClientEvmSigner) {
    this.signer = signer;
  }

  async createPaymentPayload(
    x402Version: number,
    paymentRequirements: PaymentRequirements,
  ): Promise<PaymentPayloadResult> {
    const facilitatorAddress = (
      paymentRequirements.extra as Record<string, unknown> | undefined
    )?.facilitatorAddress as `0x${string}` | undefined;
    if (!facilitatorAddress) {
      throw new Error(
        "upto scheme requires facilitatorAddress in paymentRequirements.extra",
      );
    }

    const caipChainId = paymentRequirements.network;
    const chainId = parseInt(caipChainId.split(":")[1], 10);
    const nonce = createPermit2Nonce();
    const now = Math.floor(Date.now() / 1000);
    const validAfter = (now - 600).toString();
    const deadline = (now + paymentRequirements.maxTimeoutSeconds).toString();

    const permit2Authorization = {
      from: this.signer.address,
      permitted: {
        token: getAddress(paymentRequirements.asset),
        amount: paymentRequirements.amount,
      },
      spender: x402UptoPermit2ProxyAddress,
      nonce,
      deadline,
      witness: {
        to: getAddress(paymentRequirements.payTo),
        facilitator: getAddress(facilitatorAddress),
        validAfter,
      },
    };

    const signature = await this.signer.signTypedData({
      domain: {
        name: "Permit2",
        chainId,
        verifyingContract: PERMIT2_ADDRESS,
      },
      types: uptoPermit2WitnessTypes,
      primaryType: "PermitWitnessTransferFrom",
      message: {
        permitted: {
          token: getAddress(permit2Authorization.permitted.token),
          amount: BigInt(permit2Authorization.permitted.amount),
        },
        spender: getAddress(permit2Authorization.spender),
        nonce: BigInt(permit2Authorization.nonce),
        deadline: BigInt(permit2Authorization.deadline),
        witness: {
          to: getAddress(permit2Authorization.witness.to),
          facilitator: getAddress(permit2Authorization.witness.facilitator),
          validAfter: BigInt(permit2Authorization.witness.validAfter),
        },
      },
    });

    return {
      x402Version,
      payload: { signature, permit2Authorization },
    };
  }
}

/**
 * Creates a random 256-bit nonce for Permit2.
 */
export function createPermit2Nonce(): string {
  const randomBytes = crypto.getRandomValues(new Uint8Array(32));
  return BigInt(toHex(randomBytes)).toString();
}
