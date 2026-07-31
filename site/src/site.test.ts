import { describe, expect, it } from 'vitest';

describe('site contract', () => {
  it('labels the walkthrough as documentation', () => {
    expect('Celerity project documentation').toContain('documentation');
  });

  it('never represents the architecture walkthrough as a driver UI', () => {
    expect(
      'This static site explains the system. It is not a driver interface'
    ).toContain('not a driver interface');
  });
});
