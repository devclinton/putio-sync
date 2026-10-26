// Settings page. Placeholder until its user story lands; navigation works meanwhile.

export function mount(el) {
  const title = document.createElement("h1");
  title.textContent = "Settings";
  const note = document.createElement("p");
  note.className = "notice";
  note.textContent = "This page is coming in a later update.";
  el.append(title, note);
}

export function unmount() {}
