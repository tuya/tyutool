import { nextTick, onBeforeUnmount, ref, watch, type Ref } from "vue";

export interface PanelAnchor {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** Shared positioning for teleported dropdowns, popovers and context menus. */
export function useFloatingPanel(
  panel: Ref<HTMLElement | null>,
  open: () => boolean,
  anchor: () => PanelAnchor | null,
) {
  const style = ref({
    top: "0px",
    left: "0px",
    minWidth: "",
    maxWidth: "0px",
    maxHeight: "0px",
  });
  let generation = 0;
  let positionFrame: number | null = null;
  function schedulePosition(event: Event): void {
    // Scrolling the panel's contents does not move its anchor.
    if (
      event.type === "scroll" &&
      event.target instanceof Node &&
      panel.value?.contains(event.target)
    )
      return;
    if (positionFrame !== null || !open()) return;
    positionFrame = requestAnimationFrame(() => {
      positionFrame = null;
      position();
    });
  }
  function stopPositioning(): void {
    window.removeEventListener("resize", schedulePosition);
    window.removeEventListener("scroll", schedulePosition, true);
    if (positionFrame !== null) cancelAnimationFrame(positionFrame);
    positionFrame = null;
  }
  function position(): void {
    const rect = anchor();
    const el = panel.value;
    if (!open() || !rect || !el) return;
    const margin = 8;
    const width = Math.min(
      window.innerWidth - margin * 2,
      Math.max(rect.right - rect.left, el.offsetWidth),
    );
    const below = Math.max(0, window.innerHeight - margin - rect.bottom - 4);
    const above = Math.max(0, rect.top - margin - 4);
    // scrollHeight excludes borders; reserve the full outer height above an anchor.
    const outerHeight = el.scrollHeight + el.offsetHeight - el.clientHeight;
    const down = outerHeight <= below || below >= above;
    const available = Math.max(0, down ? below : above);
    const height = Math.min(outerHeight, available);
    const top = down ? rect.bottom + 4 : rect.top - height - 4;
    style.value = {
      top: `${Math.max(margin, Math.min(top, window.innerHeight - margin - height))}px`,
      left: `${Math.max(margin, Math.min(rect.left, window.innerWidth - margin - width))}px`,
      minWidth:
        rect.right > rect.left
          ? `${Math.min(rect.right - rect.left, window.innerWidth - margin * 2)}px`
          : "",
      maxWidth: `${window.innerWidth - margin * 2}px`,
      maxHeight: `${available}px`,
    };
  }
  watch(
    open,
    async (visible) => {
      const request = ++generation;
      if (visible) {
        // Clear the previous measurement constraint before measuring a new panel.
        style.value.maxHeight = `${window.innerHeight - 16}px`;
        style.value.maxWidth = `${window.innerWidth - 16}px`;
        await nextTick();
        if (!open() || request !== generation) return;
        position();
        window.addEventListener("resize", schedulePosition);
        window.addEventListener("scroll", schedulePosition, {
          capture: true,
          passive: true,
        });
      } else {
        stopPositioning();
      }
    },
    { immediate: true },
  );
  onBeforeUnmount(() => {
    generation++;
    stopPositioning();
  });
  return { style, position };
}
