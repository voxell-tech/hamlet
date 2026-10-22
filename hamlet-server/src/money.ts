export const CURRENCY = "usd";
export const MIN_PAID_CENTS = 50;
export const MAX_PRICE_CENTS = 50_000;

export function isValidPrice(priceCents: number): boolean {
  return (
    Number.isInteger(priceCents) &&
    (priceCents === 0 || (priceCents >= MIN_PAID_CENTS && priceCents <= MAX_PRICE_CENTS))
  );
}

/**
 * The platform's cut, `round(price * percent / 100)`, halves rounded up.
 * Done in integer basis points so 10.1% and the like do not pick up float
 * error before rounding.
 */
export function platformFeeCents(priceCents: number, feePercent: number): number {
  const basisPoints = Math.round(feePercent * 100);
  return Math.floor((priceCents * basisPoints + 5_000) / 10_000);
}
