import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiProblem } from "@/api/client";
import { renderIntl } from "@/test/utils";
import { EmptyState, ErrorState, LoadingState, ProblemAlert } from "./states";

const p = (status: number, code: string) => new ApiProblem({ type: "", title: "", status, code, request_id: "rid-9" });

describe("UI states", () => {
  it("loading and empty are announced", () => {
    renderIntl(<><LoadingState /><EmptyState /></>);
    expect(screen.getByText("Loading…").closest("[role=status]")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Nothing here yet.")).toBeInTheDocument();
  });

  it("maps problem codes to localized messages with request reference", () => {
    renderIntl(<ProblemAlert error={p(403, "forbidden")} />);
    expect(screen.getByRole("alert")).toHaveTextContent("You do not have permission to do this.");
    expect(screen.getByText("Reference: rid-9")).toBeInTheDocument();
  });

  it("falls back to a generic message for unknown codes and non-API errors", () => {
    renderIntl(<><ProblemAlert error={p(418, "teapot")} /><ProblemAlert error={new Error("boom")} /></>);
    expect(screen.getAllByText("Something went wrong. Please try again.")).toHaveLength(2);
  });

  it("renders a conflict state for 409 that never auto-merges", async () => {
    const reload = vi.fn();
    renderIntl(<ProblemAlert error={p(409, "stale_revision")} onReload={reload} />);
    expect(screen.getByRole("alert")).toHaveAttribute("data-state", "conflict");
    await userEvent.click(screen.getByRole("button", { name: "Load latest version" }));
    expect(reload).toHaveBeenCalledOnce();
  });

  it("error state offers retry", async () => {
    const retry = vi.fn();
    renderIntl(<ErrorState error={p(503, "unavailable")} onRetry={retry} />);
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledOnce();
  });

  it("localizes in Arabic", () => {
    renderIntl(<ProblemAlert error={p(401, "invalid_credentials")} />, "ar");
    expect(screen.getByRole("alert")).toHaveTextContent("البريد الإلكتروني أو كلمة المرور غير صحيحة.");
  });
});
