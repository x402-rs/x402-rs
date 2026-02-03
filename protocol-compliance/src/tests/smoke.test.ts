import { describe, it, expect } from 'vitest';
import { config } from '../utils/config.js';

describe('Smoke Tests', () => {
  describe('Configuration', () => {
    it('should have facilitator URL configured', () => {
      expect(config.facilitator.url).toBeDefined();
      expect(config.facilitator.url).not.toBe('');
    });

    it('should have server port configured', () => {
      expect(config.server.port).toBeGreaterThan(0);
    });

    it('should have chain configurations', () => {
      expect(config.chains.eip155).toBeDefined();
      expect(config.chains.solana).toBeDefined();
      expect(config.chains.aptos).toBeDefined();
    });
  });

  describe('Wallets', () => {
    it('should have wallet configurations (may be empty for optional)', () => {
      // Wallets are optional - they may be empty in .env
      expect(config.wallets).toBeDefined();
      expect(config.wallets.payer).toBeDefined();
      expect(config.wallets.payee).toBeDefined();
    });
  });
});
