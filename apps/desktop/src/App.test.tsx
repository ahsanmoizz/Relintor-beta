import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App";

describe("desktop shell foundation", () => {
  beforeEach(() => {
    window.location.hash = "#home";
    localStorage.clear();
  });

  it("exposes exactly four primary destinations and a truthful new-project launcher", () => {
    render(<App />);

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    expect(navigation).toBeTruthy();

    const destinationNames = ["Home", "Projects", "Activity", "Account"];
    const destinations = destinationNames.map((name) =>
      screen.getByRole("button", { name }),
    );

    expect(destinations).toHaveLength(4);
    for (const destination of destinations) {
      expect(destination.tabIndex).not.toBe(-1);
    }

    expect(screen.getByRole("link", { name: "Skip to content" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /New project/ })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    expect(screen.getByRole("heading", { name: "Make the idea legible." })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "What are you building?" })).toBeTruthy();
  });

  it("changes the active view and exposes aria-current", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Projects" }));
    expect(screen.getByRole("heading", { name: "Continue a project." })).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Projects" }).getAttribute("aria-current"),
    ).toBe("page");
  });

  it("runs the browser-safe investigator preview without exposing provider configuration", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), {
      target: { value: "A private project tracker" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Important questions")).toBeTruthy();
    expect(screen.queryByText(/provider|api key|raw prompt/i)).toBeNull();
  });

  it("keeps browser preview outside sealing and verification", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), {
      target: { value: "A private project tracker" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Important questions")).toBeTruthy();
    expect(await screen.findByText("Choose a real project workspace before standards review.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Seal mission" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Verify work" })).toBeNull();
  });

  it("keeps takeover identity through the reality report and goal step", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /Take over existing project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Project folder" }), { target: { value: "C:\\Projects\\existing-app" } });
    fireEvent.click(screen.getByRole("button", { name: "Scan project" }));
    expect(await screen.findByText("REALITY REPORT")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Continue" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { name: "Set the next project goal." })).toBeTruthy();
    expect(screen.getByText("C:\\Projects\\existing-app")).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "What do you want Relintor to change?" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Back to reality report" })).toBeTruthy();
  });

  it("makes a selected investigation answer visually and semantically explicit", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), { target: { value: "A private project tracker" } });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Important questions")).toBeTruthy();
    const option = screen.getByRole("button", { name: "Single role" });
    fireEvent.click(option);
    await waitFor(() => expect(option.getAttribute("aria-pressed")).toBe("true"));
    expect(option.className).toContain("selected");
  });

  it("reports browser preview health as unavailable instead of checking forever", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(
      await screen.findByText("Browser preview has no Rust authority process."),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Browser preview cannot verify bundled specification resources.",
      ),
    ).toBeTruthy();
  });

  it("explains executor setup without exposing terminal instructions", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(await screen.findByRole("heading", { name: "Antigravity setup required" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Set up Antigravity" })).toBeTruthy();
    expect(await screen.findByText(/Antigravity setup runs only from the installed desktop app/)).toBeTruthy();
    expect(screen.getByText(/At least 5 GB/)).toBeTruthy();
    expect(screen.queryByText(/PowerShell|manual CLI/i)).toBeNull();
  });

  it("persists an explicit theme selection with aria-pressed state", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    const dark = screen.getByRole("button", { name: "Dark" });

    fireEvent.click(dark);

    expect(localStorage.getItem("relintor-theme")).toBe("dark");
    expect(dark.getAttribute("aria-pressed")).toBe("true");
  });

  it("shows a truthful signed-out account and unavailable entitlement state", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(await screen.findByRole("heading", { name: "Signed out" })).toBeTruthy();
    expect(screen.getByText("Not confirmed")).toBeTruthy();
    expect(screen.getByText(/Local project work remains available/)).toBeTruthy();
  });

  it("explains the no-terminal first-run path and remembers dismissal", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "A calm path from idea to proof." })).toBeTruthy();
    expect(screen.getByText(/No terminal or documentation reading is required/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue to workspace" }));
    expect(screen.queryByRole("heading", { name: "A calm path from idea to proof." })).toBeNull();
    expect(localStorage.getItem("relintor-onboarding-seen")).toBe("true");
  });

  it("shows investigator risks and the authority sealing boundary", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), { target: { value: "A private project tracker" } });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Risks")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "What the authority will lock" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Continue to standards review" })).toBeTruthy();
  });

  it("keeps an empty activity view explicit instead of inventing events", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    expect(screen.getByTestId("activity-empty")).toBeTruthy();
    expect(screen.getByText(/There is no frontend-only activity/)).toBeTruthy();
  });

  it("exposes privacy controls and safe diagnostics without sensitive fields", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    expect(await screen.findByRole("heading", { name: "Choose what leaves this device." })).toBeTruthy();
    expect(screen.getByLabelText(/Keep project evidence local/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Export safe diagnostics" })).toBeTruthy();
    expect(screen.queryByText(/database_path/i)).toBeNull();
    fireEvent.click(screen.getByLabelText(/Allow cloud account\/team state/));
    await waitFor(() => expect(localStorage.getItem("relintor-privacy-cloud")).toBe("true"));
  });
});
