import createClient from "openapi-fetch";
import type { components, paths } from "./schema";

export type Problem = components["schemas"]["Problem"];
export type User = components["schemas"]["User"];
export type FieldError = components["schemas"]["FieldError"];

/**
 * The one error type UI code handles. Every non-2xx response becomes an ApiProblem;
 * a response that is not problem+json (proxy down, HTML error page) is normalized
 * so components never branch on transport details.
 */
export class ApiProblem extends Error {
  constructor(public readonly problem: Problem) {
    super(problem.code);
    this.name = "ApiProblem";
  }
  get status() {
    return this.problem.status;
  }
  get code() {
    return this.problem.code;
  }
  get fieldErrors(): FieldError[] {
    return this.problem.errors ?? [];
  }
  get isConflict() {
    return this.problem.status === 409;
  }
}

function isProblem(v: unknown): v is Problem {
  return typeof v === "object" && v !== null && typeof (v as Problem).code === "string" && typeof (v as Problem).status === "number";
}

export function toProblem(error: unknown, status: number): ApiProblem {
  if (isProblem(error)) return new ApiProblem(error);
  const code = status === 0 ? "network_error" : "unexpected_response";
  return new ApiProblem({ type: "about:blank", title: code, status, code });
}

/** Same-origin in the browser (Next rewrites /api/v1 to the API); `Request` needs an absolute URL. */
const defaultBase = () => (typeof window === "undefined" ? "" : window.location.origin);

/** Creates a typed client. */
export function createApi(baseUrl = defaultBase(), fetchImpl: typeof fetch = (...a) => globalThis.fetch(...a)) {
  const client = createClient<paths>({
    baseUrl,
    credentials: "same-origin",
    fetch: async (req: Request) => {
      // Required by the API's CSRF guard on state-changing requests; harmless on GET.
      req.headers.set("x-requested-with", "fetch");
      try {
        const res = await fetchImpl(req);
        // Body-less success (202/204) may lose Content-Length through proxies; mark it empty
        // so it is not parsed as JSON.
        if (res.ok && !res.headers.get("content-type")) {
          const headers = new Headers(res.headers);
          headers.set("content-length", "0");
          return new Response(null, { status: res.status, headers });
        }
        return res;
      } catch {
        return new Response(null, { status: 599 });
      }
    },
  });
  return client;
}

/** Unwraps an openapi-fetch result, throwing ApiProblem on any failure. */
export async function call<T>(
  p: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<T> {
  const { data, error, response } = await p;
  if (response.ok) return data as T;
  throw toProblem(error, response.status === 599 ? 0 : response.status);
}

export const api = createApi();
