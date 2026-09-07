const MAX_SAFE_MS = Number.MAX_SAFE_INTEGER;

export function unsignedInteger(input: unknown, name: string): number {
  if (
    typeof input !== "number" ||
    !Number.isSafeInteger(input) ||
    input < 0 ||
    input > MAX_SAFE_MS
  ) {
    throw new Error(`${name} must be an unsigned safe integer`);
  }
  return input;
}

export function nullableUnsignedInteger(
  input: unknown,
  name: string,
): number | null {
  return input === null ? null : unsignedInteger(input, name);
}

export function stringUnion<const Values extends readonly string[]>(
  input: unknown,
  values: Values,
  name: string,
): Values[number] {
  if (typeof input !== "string" || !values.includes(input)) {
    throw new Error(`${name} is outside the contract`);
  }
  return input;
}

export function exactKeys(
  input: Record<string, unknown>,
  expected: string[],
): void {
  const actual = Object.keys(input).sort();
  const wanted = [...expected].sort();
  if (
    actual.length !== wanted.length ||
    actual.some((key, index) => key !== wanted[index])
  ) {
    throw new Error("status object fields are outside the contract");
  }
}

export function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === "object" && input !== null && !Array.isArray(input);
}

export function saturatingSum(values: number[]): number {
  return values.reduce(
    (total, value) => Math.min(MAX_SAFE_MS, total + value),
    0,
  );
}

export function assertNever(value: never): never {
  throw new Error(`unhandled status value: ${String(value)}`);
}
