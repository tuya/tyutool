import { describe, expect, it } from "vitest";
import { buildFeaturePortIndicators, resolvePortIndicatorColor } from "./model";

describe("buildFeaturePortIndicators", () => {
  it("derives single-port and toolbox count indicators from active ports", () => {
    const indicators = buildFeaturePortIndicators({
      enabled: true,
      flash: { connected: true, port: "COM3" },
      serialDebug: { open: true, port: "COM5" },
      toolboxSlots: [
        { port: "COM9", status: "reading" },
        { port: "COM1", status: "failed" },
        { port: "COM7", status: "authorizing" },
        { port: "COM9", status: "flashing" },
      ],
    });

    expect(indicators.flash).toEqual({
      enabled: true,
      active: true,
      ports: ["COM3"],
      count: 1,
      displayMode: "single-port",
    });
    expect(indicators["serial-debug"]).toEqual({
      enabled: true,
      active: true,
      ports: ["COM5"],
      count: 1,
      displayMode: "single-port",
    });
    expect(indicators.toolbox).toEqual({
      enabled: true,
      active: true,
      ports: ["COM7", "COM9"],
      count: 2,
      displayMode: "count",
    });
  });

  it("ignores idle and terminal toolbox slots", () => {
    const indicators = buildFeaturePortIndicators({
      enabled: true,
      flash: { connected: false, port: "COM3" },
      serialDebug: { open: false, port: "COM5" },
      toolboxSlots: [
        { port: "COM1", status: "idle" },
        { port: "COM2", status: "done" },
        { port: "COM3", status: "failed" },
        { port: "COM4", status: "skipped" },
        { port: "COM5", status: "no_code" },
      ],
    });

    expect(indicators.toolbox).toEqual({
      enabled: true,
      active: false,
      ports: [],
      count: 0,
      displayMode: "count",
    });
  });

  it("returns hidden inactive indicators when the feature is disabled", () => {
    const indicators = buildFeaturePortIndicators({
      enabled: false,
      flash: { connected: true, port: "COM3" },
      serialDebug: { open: true, port: "COM5" },
      toolboxSlots: [{ port: "COM9", status: "reading" }],
    });

    expect(indicators.flash.enabled).toBe(false);
    expect(indicators.flash.active).toBe(false);
    expect(indicators.flash.ports).toEqual([]);
    expect(indicators["serial-debug"].enabled).toBe(false);
    expect(indicators.toolbox.enabled).toBe(false);
    expect(indicators.toolbox.count).toBe(0);
  });
});

describe("resolvePortIndicatorColor", () => {
  it("keeps the same port stable within a theme and changes palettes between themes", () => {
    const activePorts = ["COM3", "COM5"];
    const lightColor = resolvePortIndicatorColor("COM3", activePorts, "light");

    expect(resolvePortIndicatorColor("COM3", activePorts, "light")).toBe(
      lightColor,
    );
    expect(resolvePortIndicatorColor("COM3", activePorts, "dark")).not.toBe(
      lightColor,
    );
  });

  it("distributes multiple ports across more than one palette slot", () => {
    const colors = new Set(
      ["COM1", "COM2", "COM3", "COM4", "COM5", "COM6"].map((port) =>
        resolvePortIndicatorColor(
          port,
          ["COM1", "COM2", "COM3", "COM4", "COM5", "COM6"],
          "light",
        ),
      ),
    );

    expect(colors.size).toBeGreaterThan(1);
  });

  it("assigns distinct colors based on the current active COM set", () => {
    const ports = ["COM30", "COM41", "COM52", "COM63"];
    const colors = ports.map((port) =>
      resolvePortIndicatorColor(port, ports, "light"),
    );

    expect(new Set(colors).size).toBe(ports.length);
  });

  it("maps high-numbered COMs by relative active order, not absolute COM number", () => {
    const lowPorts = ["COM1", "COM2", "COM3"];
    const highPorts = ["COM30", "COM41", "COM52"];

    expect(resolvePortIndicatorColor("COM1", lowPorts, "light")).toBe(
      resolvePortIndicatorColor("COM30", highPorts, "light"),
    );
    expect(resolvePortIndicatorColor("COM2", lowPorts, "light")).toBe(
      resolvePortIndicatorColor("COM41", highPorts, "light"),
    );
    expect(resolvePortIndicatorColor("COM3", lowPorts, "light")).toBe(
      resolvePortIndicatorColor("COM52", highPorts, "light"),
    );
  });
});
