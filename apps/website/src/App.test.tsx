import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./App";

describe("P11 public website", () => {
  it("presents the locked hero promise and truthful calls to action", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Your AI can say it finished. Relintor makes it prove it." })).toBeTruthy();
    expect(screen.getByText("Don’t trust done. Prove it.")).toBeTruthy();
    const download = screen.getByRole("link", { name: "Download for Windows" });
    expect(download.getAttribute("href")).toBe("/downloads/Relintor_0.1.0_x64-setup.exe");
    expect(download.getAttribute("download")).toBe("Relintor_0.1.0_x64-setup.exe");
    expect(screen.getByText("Windows x64 · Closed Beta")).toBeTruthy();
    expect(screen.getAllByRole("link", { name: "Take over a project" }).some((link) => link.getAttribute("href") === "#takeover")).toBe(true);
  });

  it("explains the four-step authority boundary without claiming release certification", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "From intent to an accountable mission." })).toBeTruthy();
    expect(screen.getByText(/External billing, provider availability, signing/)).toBeTruthy();
    expect(screen.getByText("No API keys")).toBeTruthy();
  });
});
