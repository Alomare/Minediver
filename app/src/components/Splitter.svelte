<script>
  // A drag handle between two panels. `dir` "v" resizes a width (drag sideways), "h" a height. `invert`
  // grows the value when dragging left or up, for a panel on the right or below. Double-click resets.
  let { value = $bindable(), dir = "v", min = 120, max = 2000, invert = false, reset, onend, label = "Resize" } = $props();
  let dragging = $state(false);

  function down(e) {
    if (e.button !== 0) return;
    e.preventDefault();
    const start = dir === "v" ? e.clientX : e.clientY;
    const from = value;
    dragging = true;
    document.body.classList.add(dir === "v" ? "resizing" : "resizing-v");
    const move = (m) => {
      const d = (dir === "v" ? m.clientX : m.clientY) - start;
      value = Math.round(Math.max(min, Math.min(max, from + (invert ? -d : d))));
    };
    const up = () => {
      dragging = false;
      document.body.classList.remove("resizing", "resizing-v");
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
      onend?.();
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
  }

  function key(e) {
    const step = e.shiftKey ? 40 : 10;
    const grow = dir === "v" ? { ArrowRight: 1, ArrowLeft: -1 } : { ArrowDown: 1, ArrowUp: -1 };
    if (e.key in grow) {
      e.preventDefault();
      value = Math.max(min, Math.min(max, value + grow[e.key] * step * (invert ? -1 : 1)));
      onend?.();
    }
  }
</script>

<div
  class="splitter {dir}"
  class:dragging
  role="separator"
  tabindex="0"
  aria-label={label}
  aria-orientation={dir === "v" ? "vertical" : "horizontal"}
  aria-valuenow={value}
  onpointerdown={down}
  onkeydown={key}
  ondblclick={() => { if (reset !== undefined) { value = reset; onend?.(); } }}
></div>
