type Tree = { [key: string]: unknown };

const isTree = (v: unknown): v is Tree => typeof v === "object" && v !== null && !Array.isArray(v);

/**
 * A language's messages laid over English: anything not translated yet shows in English instead of as a
 * missing-key error, so a language can ship with its most-read pages translated and grow from there.
 */
export function withFallback<T extends Tree>(base: T, over: Tree): T {
  const out: Tree = { ...base };
  for (const [k, v] of Object.entries(over)) {
    const b = out[k];
    out[k] = isTree(v) && isTree(b) ? withFallback(b, v) : v;
  }
  return out as T;
}
