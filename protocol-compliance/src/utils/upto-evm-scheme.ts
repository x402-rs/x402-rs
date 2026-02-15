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
    _supportedKind: {
      x402Version: number;
      scheme: string;
      network: Network;
      extra?: Record<string, unknown>;
    },
    _facilitatorExtensions: string[],
  ): Promise<PaymentRequirements> {
    return paymentRequirements;
  }
}

export class UptoEvmSchemeClient implements SchemeNetworkClient {
  readonly scheme: string = "upto";

  private readonly signer: ClientEvmSigner;

  constructor(signer: ClientEvmSigner) {
    this.signer = signer;
  }

  createPaymentPayload(
    x402Version: number,
    paymentRequirements: PaymentRequirements,
  ): Promise<PaymentPayloadResult> {
    console.log('x.0', x402Version);
    console.log('x.1', paymentRequirements);
    throw new Error("Method not implemented.");
  }
}
