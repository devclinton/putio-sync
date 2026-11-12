// Install section: platform filter and copy buttons. Without this script every
// option stays visible and the commands can still be selected by hand.
(() => {
  const filter = document.querySelector(".filter");
  const ways = [...document.querySelectorAll(".ways article")];
  if (filter) {
    const buttons = [...filter.querySelectorAll("button")];
    filter.hidden = false;
    filter.addEventListener("click", (event) => {
      const button = event.target.closest("button");
      if (!button) return;
      const show = button.dataset.show;
      for (const b of buttons) b.setAttribute("aria-pressed", String(b === button));
      for (const way of ways) way.hidden = show !== "all" && !way.dataset.for.split(" ").includes(show);
    });
  }

  if (!navigator.clipboard) return;
  // Button text comes from the page, so translated pages get it in their language
  const text = document.querySelector(".ways")?.dataset ?? {};
  const copy = text.copy ?? "Copy text";
  for (const pre of document.querySelectorAll(".ways pre")) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "copy";
    button.textContent = copy;
    button.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(pre.querySelector("code").textContent);
        button.textContent = text.copied ?? "Copied";
      } catch {
        button.textContent = text.copyFailed ?? "Select and copy";
      }
      setTimeout(() => (button.textContent = copy), 2000);
    });
    pre.after(button);
  }
})();
