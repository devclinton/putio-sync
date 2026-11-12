// Sign-in page: pages/signin.js, loaded the first time it shows (feature 019)

let page = null;
let shown = 0;

export function mount(el, params) {
  const mine = ++shown;
  import("./signin.js").then((m) => {
    if (mine !== shown) return;
    page = m;
    m.mount(el, params);
  });
}

export function unmount() {
  shown++;
  if (page) page.unmount();
  page = null;
}
