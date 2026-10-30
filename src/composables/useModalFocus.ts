import { nextTick, onBeforeUnmount, watch, type Ref } from "vue";

interface ModalFocus {
  dialog: Ref<HTMLElement | null>;
  opener: HTMLElement | null;
}

const modals: ModalFocus[] = [];
const inertElements = new Map<HTMLElement, string | null>();
let observer: MutationObserver | null = null;
const focusableSelector = "button,input,select,textarea,a[href],[tabindex]";

function topModal(): ModalFocus | undefined {
  return modals[modals.length - 1];
}

function modalRoots(modal: ModalFocus): HTMLElement[] {
  const dialog = modal.dialog.value;
  if (!dialog) return [];
  const roots = [dialog];
  // TySelect teleports its list to body; aria-controls retains ownership.
  for (const trigger of dialog.querySelectorAll(
    '[aria-expanded="true"][aria-controls]',
  )) {
    const popup = document.getElementById(
      trigger.getAttribute("aria-controls")!,
    );
    if (popup) roots.push(popup);
  }
  if (roots.length > 1) {
    roots.push(
      ...document.querySelectorAll<HTMLElement>(".ty-select-floating-tip"),
    );
  }
  return roots;
}

function containsFocus(modal: ModalFocus, target: Node | null): boolean {
  return modalRoots(modal).some((root) => root.contains(target));
}

function refreshInert(): void {
  const top = topModal();
  const roots = top ? modalRoots(top) : [];
  const blocked = new Set<HTMLElement>();
  for (const root of roots) {
    for (
      let node: HTMLElement | null = root;
      node && node !== document.body;
      node = node.parentElement
    ) {
      for (const sibling of node.parentElement?.children ?? []) {
        if (
          sibling instanceof HTMLElement &&
          !roots.some((allowed) => sibling.contains(allowed))
        ) {
          blocked.add(sibling);
        }
      }
    }
  }
  for (const [element, previous] of inertElements) {
    if (!blocked.has(element)) {
      if (previous === null) element.removeAttribute("inert");
      else element.setAttribute("inert", previous);
      inertElements.delete(element);
    }
  }
  for (const element of blocked) {
    if (!inertElements.has(element)) {
      inertElements.set(element, element.getAttribute("inert"));
      element.setAttribute("inert", "");
    }
  }
}

function canFocus(element: HTMLElement): boolean {
  if (element.matches(":disabled") || element.closest("[hidden],[inert]"))
    return false;
  for (
    let node: HTMLElement | null = element;
    node;
    node = node.parentElement
  ) {
    const style = getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden") return false;
  }
  return true;
}

function focusables(modal: ModalFocus): HTMLElement[] {
  return Array.from(
    modal.dialog.value?.querySelectorAll<HTMLElement>(focusableSelector) ?? [],
  ).filter((element) => element.tabIndex >= 0 && canFocus(element));
}

/** Shared modal lifecycle, including conditional mounting and nested dialogs. */
export function useModalFocus(
  dialog: Ref<HTMLElement | null>,
  open: () => boolean,
  onClose: () => void,
  initialFocus?: () => HTMLElement | null,
): void {
  let modal: ModalFocus | null = null;
  let generation = 0;

  function focusEntry(): void {
    if (!modal || topModal() !== modal) return;
    const preferred = initialFocus?.();
    const target =
      preferred && canFocus(preferred)
        ? preferred
        : (focusables(modal)[0] ?? dialog.value);
    target?.focus();
  }

  function onFocusin(event: FocusEvent): void {
    if (
      modal &&
      topModal() === modal &&
      !containsFocus(modal, event.target as Node)
    )
      focusEntry();
  }

  function onKeydown(event: KeyboardEvent): void {
    if (!modal || topModal() !== modal || event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    } else if (event.key === "Tab") {
      const elements = focusables(modal);
      const first = elements[0];
      const last = elements[elements.length - 1];
      const active = document.activeElement;
      if (
        !first ||
        !elements.includes(active as HTMLElement) ||
        (event.shiftKey ? active === first : active === last)
      ) {
        event.preventDefault();
        (event.shiftKey ? last : first)?.focus();
        if (!first) dialog.value?.focus();
      }
    }
  }

  function stop(): void {
    const closing = modal;
    if (!closing) return;
    const wasTop = topModal() === closing;
    // If a parent is removed before its child, keep the page opener reachable.
    for (const other of modals) {
      if (other !== closing && closing.dialog.value?.contains(other.opener))
        other.opener = closing.opener;
    }
    modals.splice(modals.indexOf(closing), 1);
    modal = null;
    const closedGeneration = ++generation;
    document.removeEventListener("keydown", onKeydown);
    document.removeEventListener("focusin", onFocusin);
    refreshInert();
    if (!modals.length) {
      observer?.disconnect();
      observer = null;
    }
    if (wasTop)
      void nextTick(() => {
        if (generation !== closedGeneration) return;
        const top = topModal();
        const opener = closing.opener;
        if (
          opener?.isConnected &&
          canFocus(opener) &&
          (!top || containsFocus(top, opener))
        )
          opener.focus();
        else if (top) (focusables(top)[0] ?? top.dialog.value)?.focus();
      });
  }

  watch(
    open,
    (visible) => {
      if (!visible) {
        stop();
        return;
      }
      const openedGeneration = ++generation;
      modal = {
        dialog,
        opener:
          document.activeElement instanceof HTMLElement
            ? document.activeElement
            : null,
      };
      modals.push(modal);
      document.addEventListener("keydown", onKeydown);
      document.addEventListener("focusin", onFocusin);
      void nextTick(() => {
        if (generation !== openedGeneration) return;
        if (!observer) {
          observer = new MutationObserver(refreshInert);
          observer.observe(document.body, {
            childList: true,
            subtree: true,
            attributes: true,
            attributeFilter: ["aria-controls", "aria-expanded"],
          });
        }
        refreshInert();
        focusEntry();
      });
    },
    { immediate: true },
  );
  onBeforeUnmount(stop);
}
