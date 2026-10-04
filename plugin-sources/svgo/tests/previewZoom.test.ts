// @vitest-environment jsdom
import m from "mithril";
import { afterEach, expect, it, vi } from "vitest";

const root = document.createElement("div");

afterEach(() => {
  m.render(root, null);
  root.remove();
  delete window.procyonPlugin;
  vi.restoreAllMocks();
});

it("keeps the SVG point under the wheel cursor while zooming, panning and resizing", async () => {
  window.procyonPlugin = { loadToken: "zoom-test", postMessage: vi.fn() };
  vi.resetModules();
  const { optimizer } = await import("../src/optimizer");
  const { App, initializeGlobalHandlers } = await import("../src/ui");
  optimizer.loadSvgString('<svg xmlns="http://www.w3.org/2000/svg" width="400" height="240"><rect width="400" height="240"/></svg>');
  document.body.append(root);
  m.render(root, m(App));
  initializeGlobalHandlers();

  const container = root.querySelector<HTMLElement>(".preview-container")!;
  const svg = container.querySelector<SVGSVGElement>("svg")!;
  let baseX = 120;
  let baseY = 80;
  let scrollX = 0;
  let scrollY = 0;
  let fitScale = 1;
  let pendingFrame: FrameRequestCallback | null = null;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    pendingFrame = callback;
    return 1;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {
    pendingFrame = null;
  });
  const matrix = () => {
    const transform = svg.style.transform;
    const scale = Number(transform.match(/scale\(([^)]+)\)/)?.[1] ?? 1) * fitScale;
    const panX = Number(transform.match(/translate\(([^p]+)px/)?.[1] ?? 0);
    const panY = Number(transform.match(/translate\([^,]+, ([^p]+)px/)?.[1] ?? 0);
    return {
      a: scale, d: scale,
      e: baseX - scrollX + panX,
      f: baseY - scrollY + panY,
      inverse() {
        return { a: 1 / scale, d: 1 / scale, e: -this.e / scale, f: -this.f / scale };
      },
    };
  };
  Object.defineProperty(svg, "getScreenCTM", { value: matrix });
  const makePoint = (x: number, y: number) => ({
    x,
    y,
    matrixTransform(transform: { a: number; d: number; e: number; f: number }) {
      return makePoint(this.x * transform.a + transform.e, this.y * transform.d + transform.f);
    },
  });
  Object.defineProperty(svg, "createSVGPoint", {
    value: () => makePoint(0, 0),
  });

  const wheel = (x: number, y: number, deltaY: number, afterLayout?: () => void) => {
    const before = matrix();
    const modelX = (x - before.e) / before.a;
    const modelY = (y - before.f) / before.d;
    const event = new WheelEvent("wheel", {
      bubbles: true, cancelable: true, clientX: x, clientY: y, deltaY,
    });
    container.dispatchEvent(event);
    afterLayout?.();
    if (pendingFrame) {
      const frame = pendingFrame;
      pendingFrame = null;
      frame(0);
    }
    const after = matrix();
    expect(event.defaultPrevented).toBe(true);
    expect(after.a * modelX + after.e).toBeCloseTo(x, 4);
    expect(after.d * modelY + after.f).toBeCloseTo(y, 4);
    return Number(svg.style.transform.match(/scale\(([^)]+)\)/)?.[1] ?? 1);
  };

  expect(wheel(370, 180, -100, () => {
    fitScale = 0.94;
    baseX += 8;
  })).toBeCloseTo(1.1);
  expect(wheel(370, 180, -100)).toBeCloseTo(1.21);
  expect(wheel(370, 180, 100)).toBeCloseTo(1.089);

  container.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: 280, clientY: 150 }));
  container.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: 310, clientY: 170 }));
  container.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  baseX += 40;
  baseY -= 20;
  scrollX = 110;
  scrollY = 30;
  expect(wheel(410, 205, -100)).toBeCloseTo(1.1979);
  expect(wheel(410, 205, 100)).toBeCloseTo(1.07811);

  for (let i = 0; i < 100; i++) wheel(410, 205, -100);
  expect(svg.style.transform).toContain("scale(10)");
  for (let i = 0; i < 150; i++) wheel(410, 205, 100);
  expect(svg.style.transform).toContain("scale(0.1)");

  const controls = root.querySelectorAll<HTMLButtonElement>(".preview-control-btn");
  controls[3].click();
  expect(svg.style.transform).toBe("translate(0px, 0px) scale(1)");
  controls[1].click();
  expect(svg.style.transform).toContain("scale(1.2)");
  controls[2].click();
  expect(svg.style.transform).toContain("scale(0.96)");

  svg.remove();
  const emptyWheel = new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -100 });
  container.dispatchEvent(emptyWheel);
  expect(emptyWheel.defaultPrevented).toBe(true);
});
