const ICON_NS = "http://www.w3.org/2000/svg";

export const createIcon = (name, size = 16, weight = "regular") => {
  const svg = document.createElementNS(ICON_NS, "svg");
  svg.setAttribute("class", `icon icon-${weight}`);
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("viewBox", "0 0 256 256");
  const use = document.createElementNS(ICON_NS, "use");
  use.setAttribute("href", `assets/icons/sprite.svg#${name}`);
  svg.appendChild(use);
  return svg;
};

export const hydrateIcons = () => {
  document.querySelectorAll("[data-icon]").forEach((node) => {
    const name = node.getAttribute("data-icon");
    if (!name) {
      return;
    }
    const size = Number.parseInt(node.getAttribute("data-icon-size") || "16", 10);
    const weight = node.getAttribute("data-icon-weight") || "regular";
    node.textContent = "";
    node.appendChild(createIcon(name, Number.isFinite(size) ? size : 16, weight));
  });
};
