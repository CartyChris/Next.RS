// A vanilla module that exports mount(el): it gets the page's root element.
export function mount(el) {
  const canvas = document.createElement("canvas");
  canvas.width = 200;
  canvas.height = 120;
  canvas.id = "sketch";
  el.append(canvas);
  const g = canvas.getContext("2d");
  g.fillStyle = "#41a6f6";
  g.fillRect(20, 20, 160, 80);
  window.__mounted = el.id;
}
